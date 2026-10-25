//! Superfície X11: o vídeo numa janela do tipo desktop.
//!
//! É o caminho para sessões X11, como o Cinnamon do Linux Mint. A janela
//! cobre a tela inteira com `_NET_WM_WINDOW_TYPE_DESKTOP`, que o gerenciador
//! de janelas põe abaixo de todas as outras, e pede para ficar no fundo da
//! pilha para que os ícones da área de trabalho continuem por cima. O libmpv
//! desenha pela render API, como no layer-shell.

use std::ffi::{CString, c_int, c_long, c_uchar, c_void};
use std::mem::MaybeUninit;
use std::os::fd::BorrowedFd;
use std::ptr;
use std::sync::Arc;

use khronos_egl as egl;
use smithay_client_toolkit::reexports::calloop::{
    EventLoop, Interest, Mode, PostAction, channel, generic::Generic, ping::make_ping,
};
use x11_dl::xlib::{self, Xlib};

use super::Ready;
use super::gl::{Gl, PingWaker};
use crate::mpv::Mpv;
use crate::render::Interop;

/// Conexão com o servidor X e a janela do papel de parede.
struct Desktop {
    xlib: Xlib,
    display: *mut xlib::Display,
    root: xlib::Window,
    window: xlib::Window,
}

impl Desktop {
    fn open() -> Result<Self, Box<dyn std::error::Error>> {
        let xlib = Xlib::open().map_err(|e| format!("não foi possível carregar o libX11: {e}"))?;
        // SAFETY: XInitThreads vem antes de qualquer outra chamada do Xlib; o
        // libmpv pode usar o display de outra thread na decodificação por
        // hardware.
        unsafe { (xlib.XInitThreads)() };
        // SAFETY: nulo abre o display de $DISPLAY.
        let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
        if display.is_null() {
            return Err("sem conexão com o servidor X (confira $DISPLAY)".into());
        }
        // SAFETY: display válido daqui até o Drop.
        let (root, width, height) = unsafe {
            let screen = (xlib.XDefaultScreen)(display);
            (
                (xlib.XRootWindow)(display, screen),
                (xlib.XDisplayWidth)(display, screen),
                (xlib.XDisplayHeight)(display, screen),
            )
        };
        // SAFETY: display e root válidos; a janela é destruída no Drop.
        let window = unsafe {
            let screen = (xlib.XDefaultScreen)(display);
            let black = (xlib.XBlackPixel)(display, screen);
            (xlib.XCreateSimpleWindow)(
                display,
                root,
                0,
                0,
                width as u32,
                height as u32,
                0,
                black,
                black,
            )
        };
        let desktop = Self {
            xlib,
            display,
            root,
            window,
        };
        desktop.set_hints();
        // SAFETY: display e janelas válidos.
        unsafe {
            // A raiz avisa quando a resolução muda; a janela, quando é redimensionada.
            (desktop.xlib.XSelectInput)(display, root, xlib::StructureNotifyMask);
            (desktop.xlib.XSelectInput)(display, window, xlib::StructureNotifyMask);
            (desktop.xlib.XMapWindow)(display, window);
            (desktop.xlib.XLowerWindow)(display, window);
            (desktop.xlib.XFlush)(display);
        }
        Ok(desktop)
    }

    fn atom(&self, name: &str) -> xlib::Atom {
        let name = CString::new(name).expect("sem byte nulo");
        // SAFETY: display válido, nome terminado em nulo.
        unsafe { (self.xlib.XInternAtom)(self.display, name.as_ptr(), xlib::False) }
    }

    fn set_atoms(&self, property: &str, kind: xlib::Atom, values: &[c_long]) {
        let property = self.atom(property);
        // SAFETY: formato 32 com `values.len()` itens de c_long, como o Xlib espera.
        unsafe {
            (self.xlib.XChangeProperty)(
                self.display,
                self.window,
                property,
                kind,
                32,
                xlib::PropModeReplace,
                values.as_ptr().cast::<c_uchar>(),
                values.len() as c_int,
            );
        }
    }

    /// Diz ao gerenciador de janelas que esta é uma janela de área de
    /// trabalho: abaixo de tudo, em todos os espaços de trabalho, fora da
    /// barra de tarefas e do alt-tab.
    fn set_hints(&self) {
        let atom = self.atom("ATOM");
        let desktop = self.atom("_NET_WM_WINDOW_TYPE_DESKTOP") as c_long;
        self.set_atoms("_NET_WM_WINDOW_TYPE", atom, &[desktop]);
        let state = [
            "_NET_WM_STATE_BELOW",
            "_NET_WM_STATE_STICKY",
            "_NET_WM_STATE_SKIP_TASKBAR",
            "_NET_WM_STATE_SKIP_PAGER",
        ]
        .map(|name| self.atom(name) as c_long);
        self.set_atoms("_NET_WM_STATE", atom, &state);
        // 0xFFFFFFFF: todos os espaços de trabalho.
        self.set_atoms("_NET_WM_DESKTOP", self.atom("CARDINAL"), &[0xFFFF_FFFF]);

        let name = CString::new("solune").expect("sem byte nulo");
        let mut hint = xlib::XClassHint {
            res_name: name.as_ptr().cast_mut(),
            res_class: name.as_ptr().cast_mut(),
        };
        // SAFETY: display e janela válidos; as strings vivem durante as chamadas.
        unsafe {
            (self.xlib.XStoreName)(self.display, self.window, name.as_ptr());
            (self.xlib.XSetClassHint)(self.display, self.window, &mut hint);
        }
    }

    fn connection_fd(&self) -> c_int {
        // SAFETY: display válido.
        unsafe { (self.xlib.XConnectionNumber)(self.display) }
    }

    /// Lê os eventos já recebidos, sem bloquear.
    fn drain(&self, mut on_event: impl FnMut(&xlib::XEvent)) {
        // SAFETY: display válido; XNextEvent só é chamado com evento pendente.
        unsafe {
            while (self.xlib.XPending)(self.display) > 0 {
                let mut event = MaybeUninit::<xlib::XEvent>::uninit();
                (self.xlib.XNextEvent)(self.display, event.as_mut_ptr());
                on_event(&event.assume_init());
            }
        }
    }

    fn resize(&self, width: i32, height: i32) {
        // SAFETY: display e janela válidos.
        unsafe {
            (self.xlib.XMoveResizeWindow)(
                self.display,
                self.window,
                0,
                0,
                width as u32,
                height as u32,
            );
            (self.xlib.XFlush)(self.display);
        }
    }
}

impl Drop for Desktop {
    fn drop(&mut self) {
        // SAFETY: display e janela válidos, liberados uma única vez.
        unsafe {
            (self.xlib.XDestroyWindow)(self.display, self.window);
            (self.xlib.XCloseDisplay)(self.display);
        }
    }
}

struct App {
    // O Gl sai antes do Desktop: o contexto EGL usa a janela e o display.
    gl: Option<Gl>,
    desktop: Desktop,
    size: (i32, i32),
    error: Option<Box<dyn std::error::Error>>,
    stop: bool,
}

impl App {
    fn fail(&mut self, error: Box<dyn std::error::Error>) {
        self.error = Some(error);
        self.stop = true;
    }

    fn handle_x(&mut self) {
        let (root, window) = (self.desktop.root, self.desktop.window);
        let mut root_size = None;
        self.desktop.drain(|event| {
            if event.get_type() != xlib::ConfigureNotify {
                return;
            }
            let configure = xlib::XConfigureEvent::from(*event);
            if configure.window == root {
                root_size = Some((configure.width, configure.height));
            } else if configure.window == window {
                self.size = (configure.width.max(1), configure.height.max(1));
            }
        });
        if let Some((w, h)) = root_size
            && (w, h) != self.size
        {
            tracing::info!("resolução da tela mudou: {w}x{h}");
            self.desktop.resize(w, h);
        }
    }

    fn draw_if_needed(&mut self) {
        let Some(gl) = &self.gl else { return };
        let (width, height) = self.size;
        if let Err(e) = gl.draw_if_needed(width, height) {
            self.fail(e);
        }
    }
}

/// Roda a superfície até `stop` receber um aviso ou ser fechado.
pub(super) fn run(
    player: Arc<Mpv>,
    hwdec_off: bool,
    ready: Ready,
    stop: channel::Channel<()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let desktop = Desktop::open()?;
    let mut attrs = MaybeUninit::<xlib::XWindowAttributes>::uninit();
    // SAFETY: display e janela válidos; o Xlib preenche a estrutura.
    let attrs = unsafe {
        (desktop.xlib.XGetWindowAttributes)(desktop.display, desktop.window, attrs.as_mut_ptr());
        attrs.assume_init()
    };
    let size = (attrs.width, attrs.height);
    tracing::info!("janela de desktop: {}x{}", size.0, size.1);

    let mut event_loop: EventLoop<App> = EventLoop::try_new()?;
    let handle = event_loop.handle();

    // SAFETY: o descritor é da conexão com o X, que vive em App.desktop até
    // depois do laço de eventos.
    let fd = unsafe { BorrowedFd::borrow_raw(desktop.connection_fd()) };
    handle
        .insert_source(
            Generic::new(fd, Interest::READ, Mode::Level),
            |_, _, app: &mut App| {
                app.handle_x();
                Ok(PostAction::Continue)
            },
        )
        .map_err(|e| format!("falha ao ligar o X11 ao laço de eventos: {e}"))?;

    let (ping, ping_source) = make_ping()?;
    handle
        .insert_source(ping_source, |_, _, app: &mut App| app.draw_if_needed())
        .map_err(|e| format!("falha ao registrar o wakeup: {e}"))?;

    handle
        .insert_source(stop, |_, _, app: &mut App| app.stop = true)
        .map_err(|e| format!("falha ao registrar o canal de parada: {e}"))?;

    // Como no layer-shell, com hwdec=no o display não vai para o libmpv:
    // assim ele não inicializa o VA-API na criação do contexto.
    let interop = if hwdec_off {
        Interop::None
    } else {
        Interop::X11(desktop.display.cast::<c_void>())
    };
    // SAFETY: display e janela vivem em App.desktop, que sai depois do Gl.
    let gl = unsafe {
        Gl::new(
            desktop.display.cast::<c_void>(),
            desktop.window as usize as egl::NativeWindowType,
            interop,
            player,
            PingWaker(ping),
        )
    }?;
    ready.ok();

    let mut app = App {
        gl: Some(gl),
        desktop,
        size,
        error: None,
        stop: false,
    };

    let result = loop {
        // O EGL também lê da conexão com o X; eventos que ele já tirou do
        // socket não acordam o laço, então são lidos antes de esperar.
        app.handle_x();
        if app.stop {
            break Ok(());
        }
        if let Err(e) = event_loop.dispatch(None, &mut app) {
            break Err(e.into());
        }
    };
    // O contexto de render e o EGL saem antes do libmpv e da janela.
    drop(app.gl.take());
    match app.error.take() {
        Some(e) => Err(e),
        None => result,
    }
}
