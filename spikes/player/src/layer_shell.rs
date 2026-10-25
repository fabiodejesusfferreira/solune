//! Superfície layer-shell: o vídeo na camada de fundo do Wayland.
//!
//! Funciona em compositores com `zwlr_layer_shell_v1`, como o KDE Plasma 6 e
//! os baseados em wlroots (Sway, Hyprland). O libmpv desenha pela render API
//! num contexto EGL ligado à superfície.

use std::ffi::{CStr, c_char, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use khronos_egl as egl;
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            EventLoop,
            channel::{self, Sender},
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

use crate::mpv::{self, Mpv};
use crate::render::{RenderContext, Wakeup};
use crate::{Flow, past, poll};

type Egl = egl::DynamicInstance<egl::EGL1_4>;

static EGL: OnceLock<Egl> = OnceLock::new();

/// Endereços das funções do OpenGL para o libmpv.
unsafe extern "C" fn get_proc_address(_ctx: *mut c_void, name: *const c_char) -> *mut c_void {
    // SAFETY: o libmpv passa um nome terminado em nulo.
    let name = unsafe { CStr::from_ptr(name) };
    let (Some(egl), Ok(name)) = (EGL.get(), name.to_str()) else {
        return std::ptr::null_mut();
    };
    egl.get_proc_address(name)
        .map_or(std::ptr::null_mut(), |f| f as *mut c_void)
}

/// Acorda o laço de eventos quando o libmpv tem um quadro novo.
#[derive(Debug)]
struct PingWaker(Ping);

impl Wakeup for PingWaker {
    fn wake(&self) {
        self.0.ping();
    }
}

/// Estado EGL de uma superfície configurada.
struct Gl {
    display: egl::Display,
    surface: egl::Surface,
    window: WlEglSurface,
    render: RenderContext<PingWaker>,
}

struct App {
    registry_state: RegistryState,
    output_state: OutputState,
    conn: Connection,
    layer: LayerSurface,
    player: Arc<Mpv>,
    hwdec_off: bool,
    waker: Option<Ping>,
    /// Avisa a thread de eventos que o contexto de render existe e o
    /// arquivo pode ser carregado.
    ready: Option<mpsc::Sender<()>>,
    gl: Option<Gl>,
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

    // O render.h pede que a thread de render não chame a API comum do
    // libmpv (comandos, propriedades, eventos): ela pode esperar pelo próprio
    // render e travar. Por isso os eventos ficam numa thread separada.
    let player = Arc::new(player);
    let stop = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready_rx) = mpsc::channel();
    let (outcome_tx, outcome_rx) = channel::channel();
    let events = thread::spawn({
        let player = Arc::clone(&player);
        let stop = Arc::clone(&stop);
        let video = video.to_owned();
        move || events_thread(&player, &video, deadline, &stop, &ready_rx, &outcome_tx)
    });
    handle
        .insert_source(outcome_rx, |event, _, app: &mut App| match event {
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
        ready: Some(ready_tx),
        gl: None,
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
    stop.store(true, Ordering::Relaxed);
    // O contexto de render e o EGL saem antes do libmpv e da superfície.
    drop(app.gl.take());
    let _ = events.join();
    match app.error.take() {
        Some(e) => Err(e),
        None => result,
    }
}

/// Carrega o arquivo quando a superfície fica pronta e trata os eventos do
/// libmpv até o fim, o prazo ou o pedido de parada.
fn events_thread(
    player: &Mpv,
    video: &str,
    deadline: Option<Instant>,
    stop: &AtomicBool,
    ready: &mpsc::Receiver<()>,
    outcome: &Sender<Result<(), String>>,
) {
    let run = || -> Result<(), mpv::Error> {
        loop {
            if stop.load(Ordering::Relaxed) || past(deadline) {
                return Ok(());
            }
            match ready.recv_timeout(Duration::from_millis(100)) {
                Ok(()) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            }
        }
        player.command(&["loadfile", video])?;
        let mut reported = false;
        while !stop.load(Ordering::Relaxed) && !past(deadline) {
            if poll(player, video, 0.25, &mut reported)? == Flow::Stop {
                break;
            }
        }
        Ok(())
    };
    let _ = outcome.send(run().map_err(|e| e.to_string()));
}

impl App {
    fn fail(&mut self, error: Box<dyn std::error::Error>) {
        self.error = Some(error);
        self.stop = true;
    }

    /// Cria o EGL e o contexto de render na primeira configuração da superfície.
    fn setup_gl(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (width, height) = self.size;
        let egl = match EGL.get() {
            Some(egl) => egl,
            None => {
                // SAFETY: carrega o libEGL.so.1 uma vez; a instância vive até o fim do processo.
                let loaded = unsafe { Egl::load_required() }
                    .map_err(|e| format!("não foi possível carregar o libEGL: {e}"))?;
                EGL.get_or_init(|| loaded)
            }
        };

        let wl_display = self.conn.backend().display_ptr().cast::<c_void>();
        // SAFETY: wl_display é a conexão viva deste processo.
        let display = unsafe { egl.get_display(wl_display) }.ok_or("EGL sem display Wayland")?;
        let (major, minor) = egl.initialize(display)?;
        println!("EGL {major}.{minor}");

        let config_attrs = [
            egl::SURFACE_TYPE,
            egl::WINDOW_BIT,
            egl::RENDERABLE_TYPE,
            egl::OPENGL_BIT,
            egl::RED_SIZE,
            8,
            egl::GREEN_SIZE,
            8,
            egl::BLUE_SIZE,
            8,
            egl::NONE,
        ];
        // RGB de 8 bits, sem alfa. O EGL ordena as configurações pela
        // profundidade de cor, e a primeira pode ser de 10 bits (XR30) ou ter
        // alfa; num Sway com renderer pixman, as duas aparecem pretas.
        let mut configs = Vec::with_capacity(64);
        egl.choose_config(display, &config_attrs, &mut configs)?;
        let attrib = |c, a| egl.get_config_attrib(display, c, a).ok();
        let config = configs
            .into_iter()
            .find(|&c| {
                attrib(c, egl::RED_SIZE) == Some(8)
                    && attrib(c, egl::GREEN_SIZE) == Some(8)
                    && attrib(c, egl::BLUE_SIZE) == Some(8)
                    && attrib(c, egl::ALPHA_SIZE) == Some(0)
            })
            .ok_or("nenhuma configuração EGL RGB de 8 bits com OpenGL")?;
        egl.bind_api(egl::OPENGL_API)?;
        let context = egl.create_context(display, config, None, &[egl::NONE])?;

        let window = WlEglSurface::new(self.layer.wl_surface().id(), width, height)?;
        // SAFETY: a janela EGL do Wayland vive em Gl junto com a superfície EGL.
        let surface = unsafe {
            egl.create_window_surface(display, config, window.ptr() as egl::NativeWindowType, None)
        }?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))?;
        // Sem vsync no swap: quem dita o ritmo é o libmpv, e um swap que
        // bloqueia esperando o compositor travaria o laço de eventos.
        egl.swap_interval(display, 0)?;

        // Com o wl_display, o libmpv inicializa o VA-API já na criação do
        // contexto, mesmo com hwdec=no. Sem GPU isso derruba o processo dentro
        // do libva-wayland (o mpv de linha de comando cai do mesmo jeito), então
        // com hwdec=no o display não é passado.
        let interop_display = if self.hwdec_off {
            std::ptr::null_mut()
        } else {
            wl_display
        };

        let waker = PingWaker(self.waker.take().ok_or("wakeup já usado")?);
        // SAFETY: o contexto acabou de ficar atual; wl_display vive mais que App.gl.
        let render = unsafe {
            RenderContext::new(
                Arc::clone(&self.player),
                get_proc_address,
                interop_display,
                waker,
            )
        }?;
        self.gl = Some(Gl {
            display,
            surface,
            window,
            render,
        });
        if let Some(ready) = self.ready.take() {
            let _ = ready.send(());
        }
        Ok(())
    }

    fn draw_if_needed(&mut self) {
        let Some(gl) = &self.gl else { return };
        if !gl.render.needs_frame() {
            return;
        }
        let (width, height) = self.size;
        let result = gl
            .render
            .render(width, height)
            .map_err(Into::into)
            .and_then(|()| {
                let egl = EGL.get().expect("EGL carregado em setup_gl");
                egl.swap_buffers(gl.display, gl.surface).map_err(Into::into)
            });
        match result {
            Ok(()) => gl.render.report_swap(),
            Err(e) => self.fail(e),
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
        match &self.gl {
            Some(gl) => gl.window.resize(self.size.0, self.size.1, 0, 0),
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
