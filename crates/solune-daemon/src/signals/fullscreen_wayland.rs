//! Tela cheia no Wayland, pelo protocolo wlr-foreign-toplevel-management,
//! que o Sway, o Hyprland e outros compositores wlroots oferecem.
//!
//! Conta como tela cheia uma janela com o estado `fullscreen` e sem
//! `minimized`. O KDE Plasma não oferece esse protocolo, então lá o sinal
//! fica como não detectado.

use std::collections::HashMap;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};

use wayland_client::backend::ObjectId;
use wayland_client::globals::{GlobalListContents, registry_queue_init};
use wayland_client::protocol::wl_registry;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, event_created_child};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::{self, ZwlrForeignToplevelHandleV1},
    zwlr_foreign_toplevel_manager_v1::{self, ZwlrForeignToplevelManagerV1},
};

use super::{Latest, wait_readable};

/// Estados do protocolo (`zwlr_foreign_toplevel_handle_v1.state`).
const STATE_MINIMIZED: u32 = 1;
const STATE_FULLSCREEN: u32 = 3;

#[derive(Debug, Default, Clone, Copy)]
struct Flags {
    fullscreen: bool,
    minimized: bool,
}

impl Flags {
    /// O array do protocolo: valores u32 na ordem de bytes da máquina.
    fn parse(bytes: &[u8]) -> Self {
        let states: Vec<u32> = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|c| u32::from_ne_bytes(*c))
            .collect();
        Self {
            fullscreen: states.contains(&STATE_FULLSCREEN),
            minimized: states.contains(&STATE_MINIMIZED),
        }
    }
}

#[derive(Debug, Default)]
struct Toplevel {
    pending: Flags,
    current: Flags,
}

#[derive(Debug, Default)]
struct Toplevels {
    windows: HashMap<ObjectId, Toplevel>,
    finished: bool,
}

impl Toplevels {
    fn any_fullscreen(&self) -> bool {
        self.windows
            .values()
            .any(|w| w.current.fullscreen && !w.current.minimized)
    }
}

impl Dispatch<wl_registry::WlRegistry, GlobalListContents> for Toplevels {
    fn event(
        _: &mut Self,
        _: &wl_registry::WlRegistry,
        _: wl_registry::Event,
        _: &GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<ZwlrForeignToplevelManagerV1, ()> for Toplevels {
    fn event(
        state: &mut Self,
        _: &ZwlrForeignToplevelManagerV1,
        event: zwlr_foreign_toplevel_manager_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_manager_v1::Event::Toplevel { toplevel } => {
                state.windows.insert(toplevel.id(), Toplevel::default());
            }
            zwlr_foreign_toplevel_manager_v1::Event::Finished => state.finished = true,
            _ => {}
        }
    }

    event_created_child!(Toplevels, ZwlrForeignToplevelManagerV1, [
        zwlr_foreign_toplevel_manager_v1::EVT_TOPLEVEL_OPCODE => (ZwlrForeignToplevelHandleV1, ())
    ]);
}

impl Dispatch<ZwlrForeignToplevelHandleV1, ()> for Toplevels {
    fn event(
        state: &mut Self,
        handle: &ZwlrForeignToplevelHandleV1,
        event: zwlr_foreign_toplevel_handle_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            zwlr_foreign_toplevel_handle_v1::Event::State { state: bytes } => {
                if let Some(window) = state.windows.get_mut(&handle.id()) {
                    window.pending = Flags::parse(&bytes);
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Done => {
                if let Some(window) = state.windows.get_mut(&handle.id()) {
                    window.current = window.pending;
                }
            }
            zwlr_foreign_toplevel_handle_v1::Event::Closed => {
                state.windows.remove(&handle.id());
                handle.destroy();
            }
            _ => {}
        }
    }
}

/// Observa até `stop`. Avisa `None` se não der para detectar.
pub fn run(stop: &AtomicBool, notify: &dyn Fn(Option<bool>)) {
    if let Err(e) = watch(stop, notify) {
        tracing::warn!("tela cheia não detectada: {e}");
        notify(None);
    }
}

fn watch(
    stop: &AtomicBool,
    notify: &dyn Fn(Option<bool>),
) -> Result<(), Box<dyn std::error::Error>> {
    let connection = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<Toplevels>(&connection)?;
    let qh = queue.handle();
    let manager: ZwlrForeignToplevelManagerV1 = globals
        .bind(&qh, 1..=3, ())
        .map_err(|_| "o compositor não oferece wlr-foreign-toplevel-management")?;
    let mut state = Toplevels::default();
    // A primeira volta traz as janelas; a segunda, os estados delas.
    queue.roundtrip(&mut state)?;
    queue.roundtrip(&mut state)?;

    let mut latest = Latest::default();
    while !stop.load(Ordering::Relaxed) && !state.finished {
        let value = Some(state.any_fullscreen());
        if latest.update(value) {
            tracing::info!("tela cheia: {value:?}");
            notify(value);
        }
        queue.flush()?;
        if let Some(guard) = queue.prepare_read()
            && wait_readable(guard.connection_fd().as_raw_fd(), 500)
        {
            guard.read()?;
        }
        queue.dispatch_pending(&mut state)?;
    }
    manager.stop();
    let _ = queue.flush();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_os_estados_do_protocolo() {
        let bytes: Vec<u8> = [2u32, 3].iter().flat_map(|v| v.to_ne_bytes()).collect();
        let flags = Flags::parse(&bytes);
        assert!(flags.fullscreen && !flags.minimized);
        let bytes: Vec<u8> = [1u32, 3].iter().flat_map(|v| v.to_ne_bytes()).collect();
        let flags = Flags::parse(&bytes);
        assert!(flags.fullscreen && flags.minimized);
    }
}
