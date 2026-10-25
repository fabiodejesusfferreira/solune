//! Contexto EGL e render do libmpv, comum às superfícies do Linux.
//!
//! Cada superfície entrega um display nativo (Wayland ou X11) e uma janela
//! nativa; daqui sai um contexto OpenGL atual com o render do libmpv ligado.

use std::ffi::{CStr, c_char, c_void};
use std::sync::{Arc, OnceLock};

use khronos_egl as egl;
use smithay_client_toolkit::reexports::calloop::ping::Ping;

use crate::mpv::Mpv;
use crate::render::{Interop, RenderContext, Wakeup};

type Egl = egl::DynamicInstance<egl::EGL1_4>;

static EGL: OnceLock<Egl> = OnceLock::new();

fn egl() -> Result<&'static Egl, String> {
    if let Some(egl) = EGL.get() {
        return Ok(egl);
    }
    // SAFETY: carrega o libEGL.so.1 uma vez; a instância vive até o fim do processo.
    let loaded = unsafe { Egl::load_required() }
        .map_err(|e| format!("não foi possível carregar o libEGL: {e}"))?;
    Ok(EGL.get_or_init(|| loaded))
}

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

/// Acorda o laço de eventos da superfície quando o libmpv tem um quadro novo.
#[derive(Debug)]
pub struct PingWaker(pub Ping);

impl Wakeup for PingWaker {
    fn wake(&self) {
        self.0.ping();
    }
}

/// Contexto OpenGL atual com o render do libmpv.
#[derive(Debug)]
pub struct Gl {
    display: egl::Display,
    surface: egl::Surface,
    context: egl::Context,
    render: Option<RenderContext<PingWaker>>,
}

impl Gl {
    /// Cria o contexto na janela nativa e liga o render do libmpv.
    ///
    /// # Safety
    ///
    /// `native_display` e `native_window` precisam ser válidos e viver mais
    /// que o `Gl`, assim como o display passado em `interop`.
    pub unsafe fn new(
        native_display: *mut c_void,
        native_window: egl::NativeWindowType,
        interop: Interop,
        player: Arc<Mpv>,
        waker: PingWaker,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let egl = egl()?;
        // SAFETY: o chamador garante que o display nativo é válido.
        let display = unsafe { egl.get_display(native_display) }.ok_or("EGL sem display")?;
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

        // SAFETY: o chamador garante que a janela nativa é válida.
        let surface = unsafe { egl.create_window_surface(display, config, native_window, None) }?;
        egl.make_current(display, Some(surface), Some(surface), Some(context))?;
        // Sem vsync no swap: quem dita o ritmo é o libmpv, e um swap que
        // bloqueia esperando o compositor travaria o laço de eventos.
        egl.swap_interval(display, 0)?;

        // SAFETY: o contexto acabou de ficar atual; o display de interop vive mais que o Gl.
        let render = unsafe { RenderContext::new(player, get_proc_address, interop, waker) }?;
        Ok(Self {
            display,
            surface,
            context,
            render: Some(render),
        })
    }

    /// Desenha e apresenta um quadro, se o libmpv tiver um novo.
    pub fn draw_if_needed(
        &self,
        width: i32,
        height: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(render) = &self.render else {
            return Ok(());
        };
        if !render.needs_frame() {
            return Ok(());
        }
        render.render(width, height)?;
        egl()?.swap_buffers(self.display, self.surface)?;
        render.report_swap();
        Ok(())
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        // O render do libmpv sai com o contexto ainda atual; depois, o EGL.
        drop(self.render.take());
        if let Some(egl) = EGL.get() {
            let _ = egl.make_current(self.display, None, None, None);
            let _ = egl.destroy_surface(self.display, self.surface);
            let _ = egl.destroy_context(self.display, self.context);
        }
    }
}
