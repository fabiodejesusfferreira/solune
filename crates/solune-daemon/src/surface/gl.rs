//! Contexto EGL e render do libmpv, comum às superfícies do Linux.
//!
//! Cada superfície entrega um display nativo (Wayland ou X11) e uma janela
//! nativa; daqui sai um contexto OpenGL atual com o render do libmpv ligado.
//!
//! Depois de uma pausa longa, o render do libmpv pode ser desligado: o
//! último quadro fica guardado num framebuffer nosso, que continua na tela,
//! e o libmpv solta o decodificador, as texturas e o interop com a GPU.

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

/// Funções do OpenGL 3.0 para guardar e mostrar o último quadro. O libmpv
/// carrega as dele; estas são só nossas.
#[derive(Debug, Clone, Copy)]
struct Fns {
    gen_framebuffers: unsafe extern "system" fn(i32, *mut u32),
    delete_framebuffers: unsafe extern "system" fn(i32, *const u32),
    bind_framebuffer: unsafe extern "system" fn(u32, u32),
    gen_renderbuffers: unsafe extern "system" fn(i32, *mut u32),
    delete_renderbuffers: unsafe extern "system" fn(i32, *const u32),
    bind_renderbuffer: unsafe extern "system" fn(u32, u32),
    renderbuffer_storage: unsafe extern "system" fn(u32, u32, i32, i32),
    framebuffer_renderbuffer: unsafe extern "system" fn(u32, u32, u32, u32),
    check_framebuffer_status: unsafe extern "system" fn(u32) -> u32,
    blit_framebuffer: unsafe extern "system" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32),
}

const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_RGBA8: u32 = 0x8058;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_COLOR_BUFFER_BIT: u32 = 0x4000;
const GL_LINEAR: u32 = 0x2601;

impl Fns {
    fn load() -> Result<Self, String> {
        let egl = egl()?;
        // SAFETY: cada campo de Fns tem a assinatura do OpenGL 3.0 para a
        // função com o nome pedido.
        unsafe {
            Ok(Self {
                gen_framebuffers: gl_fn(egl, "glGenFramebuffers")?,
                delete_framebuffers: gl_fn(egl, "glDeleteFramebuffers")?,
                bind_framebuffer: gl_fn(egl, "glBindFramebuffer")?,
                gen_renderbuffers: gl_fn(egl, "glGenRenderbuffers")?,
                delete_renderbuffers: gl_fn(egl, "glDeleteRenderbuffers")?,
                bind_renderbuffer: gl_fn(egl, "glBindRenderbuffer")?,
                renderbuffer_storage: gl_fn(egl, "glRenderbufferStorage")?,
                framebuffer_renderbuffer: gl_fn(egl, "glFramebufferRenderbuffer")?,
                check_framebuffer_status: gl_fn(egl, "glCheckFramebufferStatus")?,
                blit_framebuffer: gl_fn(egl, "glBlitFramebuffer")?,
            })
        }
    }
}

/// Uma função do OpenGL pelo nome, com o tipo `F`.
///
/// # Safety
///
/// `F` precisa ser um ponteiro de função com a assinatura real de `name`.
unsafe fn gl_fn<F: Copy>(egl: &Egl, name: &str) -> Result<F, String> {
    let f = egl
        .get_proc_address(name)
        .ok_or_else(|| format!("OpenGL sem {name}"))?;
    assert_eq!(size_of::<F>(), size_of_val(&f));
    // SAFETY: mesmo tamanho; o chamador garante a assinatura.
    Ok(unsafe { std::mem::transmute_copy(&f) })
}

/// O último quadro do vídeo, guardado num framebuffer do tamanho da tela.
#[derive(Debug)]
struct Still {
    fns: Fns,
    fbo: u32,
    renderbuffer: u32,
    size: (i32, i32),
}

impl Still {
    /// Cria o framebuffer. Precisa do contexto OpenGL atual.
    fn new(width: i32, height: i32) -> Result<Self, String> {
        let fns = Fns::load()?;
        let (mut fbo, mut renderbuffer) = (0, 0);
        // SAFETY: contexto atual; os nomes gerados são deste contexto.
        let status = unsafe {
            (fns.gen_framebuffers)(1, &mut fbo);
            (fns.gen_renderbuffers)(1, &mut renderbuffer);
            (fns.bind_renderbuffer)(GL_RENDERBUFFER, renderbuffer);
            (fns.renderbuffer_storage)(GL_RENDERBUFFER, GL_RGBA8, width, height);
            (fns.bind_framebuffer)(GL_FRAMEBUFFER, fbo);
            (fns.framebuffer_renderbuffer)(
                GL_FRAMEBUFFER,
                GL_COLOR_ATTACHMENT0,
                GL_RENDERBUFFER,
                renderbuffer,
            );
            let status = (fns.check_framebuffer_status)(GL_FRAMEBUFFER);
            (fns.bind_framebuffer)(GL_FRAMEBUFFER, 0);
            status
        };
        let still = Self {
            fns,
            fbo,
            renderbuffer,
            size: (width, height),
        };
        if status != GL_FRAMEBUFFER_COMPLETE {
            return Err(format!("framebuffer incompleto: {status:#x}"));
        }
        Ok(still)
    }

    /// Copia o quadro para a janela, esticando se o tamanho mudou.
    fn draw(&self, width: i32, height: i32) {
        let (w, h) = self.size;
        // SAFETY: contexto atual; o framebuffer é deste contexto.
        unsafe {
            (self.fns.bind_framebuffer)(GL_READ_FRAMEBUFFER, self.fbo);
            (self.fns.bind_framebuffer)(GL_DRAW_FRAMEBUFFER, 0);
            (self.fns.blit_framebuffer)(
                0,
                0,
                w,
                h,
                0,
                0,
                width,
                height,
                GL_COLOR_BUFFER_BIT,
                GL_LINEAR,
            );
            (self.fns.bind_framebuffer)(GL_FRAMEBUFFER, 0);
        }
    }
}

impl Drop for Still {
    fn drop(&mut self) {
        // SAFETY: o Still só é solto pelo Gl, com o contexto atual.
        unsafe {
            (self.fns.delete_framebuffers)(1, &self.fbo);
            (self.fns.delete_renderbuffers)(1, &self.renderbuffer);
        }
    }
}

/// Contexto OpenGL atual com o render do libmpv.
#[derive(Debug)]
pub struct Gl {
    display: egl::Display,
    surface: egl::Surface,
    context: egl::Context,
    render: Option<RenderContext<PingWaker>>,
    /// O quadro que fica na tela com o render do libmpv congelado ou
    /// desligado, e até o primeiro quadro novo depois de religar.
    still: Option<Still>,
    /// Congelado: os quadros do libmpv não vão para a tela.
    frozen: bool,
    // O que é preciso para religar o render.
    player: Arc<Mpv>,
    interop: Interop,
    ping: Ping,
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
        ping: Ping,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let egl = egl()?;
        // SAFETY: o chamador garante que o display nativo é válido.
        let display = unsafe { egl.get_display(native_display) }.ok_or("EGL sem display")?;
        let (major, minor) = egl.initialize(display)?;
        tracing::debug!("EGL {major}.{minor}");

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

        let mut gl = Self {
            display,
            surface,
            context,
            render: None,
            still: None,
            frozen: false,
            player,
            interop,
            ping,
        };
        gl.thaw()?;
        Ok(gl)
    }

    /// Desenha e apresenta um quadro, se o libmpv tiver um novo.
    pub fn draw_if_needed(
        &mut self,
        width: i32,
        height: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let Some(render) = &self.render else {
            return Ok(());
        };
        if self.frozen || !render.needs_frame() {
            return Ok(());
        }
        render.render(0, width, height)?;
        egl()?.swap_buffers(self.display, self.surface)?;
        render.report_swap();
        // O vídeo voltou: o quadro guardado não precisa mais ficar.
        self.still = None;
        Ok(())
    }

    /// Guarda o quadro atual e deixa ele na tela; os quadros do libmpv param
    /// de aparecer. Assim o player pode parar o arquivo sem a tela mudar.
    pub fn freeze(&mut self, width: i32, height: i32) -> Result<(), Box<dyn std::error::Error>> {
        if self.frozen {
            return Ok(());
        }
        let Some(render) = &self.render else {
            return Err("sem render do libmpv".into());
        };
        let still = Still::new(width, height)?;
        render.render(still.fbo, width, height)?;
        still.draw(width, height);
        egl()?.swap_buffers(self.display, self.surface)?;
        self.still = Some(still);
        self.frozen = true;
        Ok(())
    }

    /// Desliga o render do libmpv, que solta texturas, framebuffers e o
    /// interop com a GPU. Só congelado e com o arquivo já parado: com vídeo
    /// ativo, o libmpv encerra o arquivo com erro.
    pub fn release(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if !self.frozen {
            return Err("o render precisa estar congelado".into());
        }
        // Com o contexto atual, como o render.h pede.
        self.render = None;
        Ok(())
    }

    /// Liga o render do libmpv de novo. O quadro guardado continua na tela
    /// até o libmpv desenhar o primeiro quadro do vídeo.
    pub fn thaw(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.frozen = false;
        if self.render.is_some() {
            return Ok(());
        }
        let waker = PingWaker(self.ping.clone());
        // SAFETY: o contexto é atual nesta thread desde o `new`; o display de
        // interop vive mais que o Gl.
        let render = unsafe {
            RenderContext::new(
                Arc::clone(&self.player),
                get_proc_address,
                self.interop,
                waker,
            )
        }?;
        self.render = Some(render);
        Ok(())
    }

    /// Mostra o quadro guardado de novo, se ele está valendo (a janela mudou
    /// de tamanho com o render congelado, por exemplo).
    pub fn redraw_still(&self, width: i32, height: i32) -> Result<(), Box<dyn std::error::Error>> {
        if (self.frozen || self.render.is_none())
            && let Some(still) = &self.still
        {
            still.draw(width, height);
            egl()?.swap_buffers(self.display, self.surface)?;
        }
        Ok(())
    }
}

impl Drop for Gl {
    fn drop(&mut self) {
        // O render do libmpv e o quadro guardado saem com o contexto ainda
        // atual; depois, o EGL.
        drop(self.render.take());
        drop(self.still.take());
        if let Some(egl) = EGL.get() {
            let _ = egl.make_current(self.display, None, None, None);
            let _ = egl.destroy_surface(self.display, self.surface);
            let _ = egl.destroy_context(self.display, self.context);
        }
    }
}
