//! Superfície layer-shell: o vídeo na camada de fundo do Wayland.
//!
//! Funciona em compositores com `zwlr_layer_shell_v1`, como o KDE Plasma 6 e
//! os baseados em wlroots (Sway, Hyprland). O libmpv desenha pela render API
//! num contexto EGL ligado à superfície.

use std::ffi::c_void;
use std::sync::Arc;
use std::time::Instant;

use khronos_egl as egl;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop, channel,
            ping::{Ping, make_ping},
        },
        calloop_wayland_source::WaylandSource,
        client::{
            Connection, Proxy, QueueHandle,
            globals::registry_queue_init,
            protocol::{wl_output, wl_surface},
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        WaylandSurface,
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
    },
};
use wayland_egl::WlEglSurface;

use crate::events::Events;
use crate::gl::{Gl, PingWaker};
use crate::mpv::Mpv;
use crate::render::Interop;

/// Contexto EGL e a janela EGL do Wayland em que ele desenha. Os campos saem
/// na ordem da declaração: o contexto antes da janela.
struct Target {
    gl: Gl,
    window: WlEglSurface,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    conn: Connection,
    layer: LayerSurface,
    player: Arc<Mpv>,
    hwdec_off: bool,
    waker: Option<Ping>,
    events: Events,
    target: Option<Target>,
    size: (i32, i32),
    error: Option<Box<dyn std::error::Error>>,
    stop: bool,
}

pub fn run(
    player: Mpv,
    video: &str,
    hwdec: &str,
    deadline: Option<Instant>,
) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()
        .map_err(|e| format!("sem conexão com o compositor Wayland: {e}"))?;
    let (globals, event_queue) = registry_queue_init(&conn)?;
    let qh = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)?;
    let layer_shell = LayerShell::bind(&globals, &qh)
        .map_err(|_| "o compositor não oferece layer-shell (zwlr_layer_shell_v1)")?;

    let surface = compositor.create_surface(&qh);
    let layer =
        layer_shell.create_layer_surface(&qh, surface, Layer::Background, Some("solune"), None);
    layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);
    layer.set_size(0, 0);
    layer.commit();

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();
    WaylandSource::new(conn.clone(), event_queue)
        .insert(handle.clone())
        .map_err(|e| format!("falha ao ligar o Wayland ao laço de eventos: {e}"))?;

    let (ping, ping_source) = make_ping()?;
    handle
        .insert_source(ping_source, |_, _, app: &mut App| app.draw_if_needed())
        .map_err(|e| format!("falha ao registrar o wakeup: {e}"))?;

    let player = Arc::new(player);
    let (events, outcome) = Events::spawn(Arc::clone(&player), video, deadline);
    handle
        .insert_source(outcome, |event, _, app: &mut App| match event {
            channel::Event::Msg(Err(e)) => app.fail(e.into()),
            channel::Event::Msg(Ok(())) | channel::Event::Closed => app.stop = true,
        })
        .map_err(|e| format!("falha ao registrar o canal de eventos: {e}"))?;

    let mut app = App {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        conn,
        layer,
        player,
        hwdec_off: hwdec == "no",
        waker: Some(ping),
        events,
        target: None,
        size: (0, 0),
        error: None,
        stop: false,
    };

    let result = loop {
        if app.stop {
            break Ok(());
        }
        if let Err(e) = event_loop.dispatch(None, &mut app) {
            break Err(e.into());
        }
    };
    // O contexto de render e o EGL saem antes do libmpv e da superfície.
    drop(app.target.take());
    app.events.shutdown();
    match app.error.take() {
        Some(e) => Err(e),
        None => result,
    }
}

impl App {
    fn fail(&mut self, error: Box<dyn std::error::Error>) {
        self.error = Some(error);
        self.stop = true;
    }

    /// Cria o EGL e o contexto de render na primeira configuração da superfície.
    fn setup_gl(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (width, height) = self.size;
        let wl_display = self.conn.backend().display_ptr().cast::<c_void>();
        let window = WlEglSurface::new(self.layer.wl_surface().id(), width, height)?;

        // Com o wl_display, o libmpv inicializa o VA-API já na criação do
        // contexto, mesmo com hwdec=no. Sem GPU isso derruba o processo dentro
        // do libva-wayland (o mpv de linha de comando cai do mesmo jeito), então
        // com hwdec=no o display não é passado.
        let interop = if self.hwdec_off {
            Interop::None
        } else {
            Interop::Wayland(wl_display)
        };

        let waker = PingWaker(self.waker.take().ok_or("wakeup já usado")?);
        // SAFETY: wl_display é a conexão viva deste processo e a janela EGL
        // fica em Target junto com o Gl, que sai primeiro.
        let gl = unsafe {
            Gl::new(
                wl_display,
                window.ptr() as egl::NativeWindowType,
                interop,
                Arc::clone(&self.player),
                waker,
            )
        }?;
        self.target = Some(Target { gl, window });
        self.events.ready();
        Ok(())
    }

    fn draw_if_needed(&mut self) {
        let Some(target) = &self.target else { return };
        let (width, height) = self.size;
        if let Err(e) = target.gl.draw_if_needed(width, height) {
            self.fail(e);
        }
    }
}

impl LayerShellHandler for App {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &LayerSurface) {
        println!("o compositor fechou a superfície");
        self.stop = true;
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _: u32,
    ) {
        let (w, h) = configure.new_size;
        self.size = (w.max(1) as i32, h.max(1) as i32);
        println!("superfície configurada: {}x{}", self.size.0, self.size.1);
        match &self.target {
            Some(target) => target.window.resize(self.size.0, self.size.1, 0, 0),
            None => {
                if let Err(e) = self.setup_gl() {
                    self.fail(e);
                }
            }
        }
    }
}

impl CompositorHandler for App {
    fn scale_factor_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: i32,
    ) {
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for App {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}

    fn output_destroyed(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {}
}

delegate_registry!(App);

impl ProvidesRegistryState for App {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(App);
