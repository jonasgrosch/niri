use std::cell::RefCell;
use std::collections::HashMap;

use smithay::reexports::wayland_server::backend::ClientId;
use smithay::reexports::wayland_server::protocol::wl_surface::WlSurface;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::compositor::with_states;
use smithay::wayland::shell::wlr_layer::LayerSurface;
use smithay::wayland::shell::xdg::XdgToplevelSurfaceData;

use super::raw::ext_background_effect::v1::server::{
    ext_background_effect_manager_v1, ext_background_effect_v1,
};
use ext_background_effect_manager_v1::{Error, ExtBackgroundEffectManagerV1};
use ext_background_effect_v1::ExtBackgroundEffectV1;

const VERSION: u32 = 1;

pub struct BackgroundEffectManagerState {
    // Track which surfaces have a background effect object
    surface_effects: HashMap<WlSurface, ExtBackgroundEffectV1>,
}

pub struct BackgroundEffectManagerGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

pub trait BackgroundEffectHandler {
    fn background_effect_manager_state(&mut self) -> &mut BackgroundEffectManagerState;
}

/// State stored with each background effect object
pub struct BackgroundEffectState {
    surface: WlSurface,
}

/// State stored in surface compositor data
#[derive(Debug, Default)]
pub struct SurfaceBackgroundEffectState {
    /// The current background effect type (e.g., "blur", "none")
    pub effect_type: RefCell<Option<String>>,
}

impl BackgroundEffectManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<ExtBackgroundEffectManagerV1, BackgroundEffectManagerGlobalData>,
        D: Dispatch<ExtBackgroundEffectManagerV1, ()>,
        D: Dispatch<ExtBackgroundEffectV1, BackgroundEffectState>,
        D: BackgroundEffectHandler,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = BackgroundEffectManagerGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, ExtBackgroundEffectManagerV1, _>(VERSION, global_data);

        Self {
            surface_effects: HashMap::new(),
        }
    }

    /// Check if a surface is valid for background effects (xdg_surface, layer_surface, or lock_surface)
    fn is_valid_surface(surface: &WlSurface) -> bool {
        with_states(surface, |states| {
            // Check if it's an xdg_toplevel
            if states.data_map.get::<XdgToplevelSurfaceData>().is_some() {
                return true;
            }

            // Check if it's a layer surface
            if LayerSurface::from_wl_surface(surface).is_some() {
                return true;
            }

            // Check if it's a lock surface
            // Lock surfaces use ext_session_lock_surface_v1 which is tracked separately
            // We'll check for the session lock role through compositor states
            if states
                .data_map
                .get::<smithay::wayland::session_lock::SessionLockSurfaceCachedState>()
                .is_some()
            {
                return true;
            }

            false
        })
    }

    pub fn surface_destroyed(&mut self, surface: &WlSurface) {
        self.surface_effects.remove(surface);
    }
}

impl<D> GlobalDispatch<ExtBackgroundEffectManagerV1, BackgroundEffectManagerGlobalData, D>
    for BackgroundEffectManagerState
where
    D: GlobalDispatch<ExtBackgroundEffectManagerV1, BackgroundEffectManagerGlobalData>,
    D: Dispatch<ExtBackgroundEffectManagerV1, ()>,
    D: Dispatch<ExtBackgroundEffectV1, BackgroundEffectState>,
    D: BackgroundEffectHandler,
    D: 'static,
{
    fn bind(
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<ExtBackgroundEffectManagerV1>,
        _manager_state: &BackgroundEffectManagerGlobalData,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(manager, ());
    }

    fn can_view(client: Client, global_data: &BackgroundEffectManagerGlobalData) -> bool {
        (global_data.filter)(&client)
    }
}

impl<D> Dispatch<ExtBackgroundEffectManagerV1, (), D> for BackgroundEffectManagerState
where
    D: Dispatch<ExtBackgroundEffectManagerV1, ()>,
    D: Dispatch<ExtBackgroundEffectV1, BackgroundEffectState>,
    D: BackgroundEffectHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        resource: &ExtBackgroundEffectManagerV1,
        request: <ExtBackgroundEffectManagerV1 as Resource>::Request,
        _data: &(),
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            ext_background_effect_manager_v1::Request::GetBackgroundEffect { id, surface } => {
                let manager_state = state.background_effect_manager_state();

                // Check if the surface already has a background effect
                if manager_state.surface_effects.contains_key(&surface) {
                    resource.post_error(
                        Error::BackgroundEffectExists,
                        "surface already has a background effect object".to_string(),
                    );
                    return;
                }

                // Check if the surface is valid
                if !BackgroundEffectManagerState::is_valid_surface(&surface) {
                    resource.post_error(
                        Error::InvalidSurface,
                        "surface is not a valid surface for background effects".to_string(),
                    );
                    return;
                }

                // Track the effect for this surface
                let manager_state = state.background_effect_manager_state();
                manager_state.surface_effects.insert(surface.clone(), effect.clone());
            }
            ext_background_effect_manager_v1::Request::Destroy => (),
            _ => unreachable!(),
        }
    }
}

impl<D> Dispatch<ExtBackgroundEffectV1, BackgroundEffectState, D> for BackgroundEffectManagerState
where
    D: Dispatch<ExtBackgroundEffectV1, BackgroundEffectState>,
    D: BackgroundEffectHandler,
    D: 'static,
{
    fn request(
        state: &mut D,
        _client: &Client,
        _resource: &ExtBackgroundEffectV1,
        request: <ExtBackgroundEffectV1 as Resource>::Request,
        data: &BackgroundEffectState,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            ext_background_effect_v1::Request::SetEffect { r#type } => {
                // Store the effect type in the surface's compositor data
                with_states(&data.surface, |states| {
                    let effect_state = states
                        .data_map
                        .get_or_insert(SurfaceBackgroundEffectState::default);
                    
                    // Update the effect type (or remove it if it's "none")
                    if r#type == "none" {
                        *effect_state.effect_type.borrow_mut() = None;
                    } else {
                        *effect_state.effect_type.borrow_mut() = Some(r#type);
                    }
                });
            }
            ext_background_effect_v1::Request::Destroy => (),
            _ => unreachable!(),
        }
    }

    fn destroyed(
        state: &mut D,
        _client: ClientId,
        resource: &ExtBackgroundEffectV1,
        data: &BackgroundEffectState,
    ) {
        let manager_state = state.background_effect_manager_state();
        
        // Remove the effect from tracking
        if let Some(tracked_effect) = manager_state.surface_effects.get(&data.surface) {
            if tracked_effect == resource {
                manager_state.surface_effects.remove(&data.surface);
            }
        }

        // Clear the effect from the surface state
        with_states(&data.surface, |states| {
            if let Some(effect_state) = states.data_map.get::<SurfaceBackgroundEffectState>() {
                *effect_state.effect_type.borrow_mut() = None;
            }
        });
    }
}

#[macro_export]
macro_rules! delegate_background_effect {
    ($(@<$( $lt:tt $( : $clt:tt $(+ $dlt:tt )* )? ),+>)? $ty: ty) => {
        smithay::reexports::wayland_server::delegate_global_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            $crate::protocols::raw::ext_background_effect::v1::server::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1: $crate::protocols::background_effect::BackgroundEffectManagerGlobalData
        ] => $crate::protocols::background_effect::BackgroundEffectManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            $crate::protocols::raw::ext_background_effect::v1::server::ext_background_effect_manager_v1::ExtBackgroundEffectManagerV1: ()
        ] => $crate::protocols::background_effect::BackgroundEffectManagerState);

        smithay::reexports::wayland_server::delegate_dispatch!($(@< $( $lt $( : $clt $(+ $dlt )* )? ),+ >)? $ty: [
            $crate::protocols::raw::ext_background_effect::v1::server::ext_background_effect_v1::ExtBackgroundEffectV1: $crate::protocols::background_effect::BackgroundEffectState
        ] => $crate::protocols::background_effect::BackgroundEffectManagerState);
    };
}
