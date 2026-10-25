//! Render API do libmpv com OpenGL.
//!
//! Escrito à mão a partir de `mpv/render.h` e `mpv/render_gl.h` (licença
//! ISC). O libmpv desenha cada quadro num framebuffer OpenGL nosso, e quem
//! decide onde esse framebuffer aparece é a superfície.

use std::ffi::{CString, c_char, c_int, c_void};
use std::marker::PhantomData;
use std::ptr;
use std::sync::Arc;

use crate::mpv::{self, Mpv, MpvHandle};

#[repr(C)]
struct RawRenderContext {
    _private: [u8; 0],
}

#[repr(C)]
struct RenderParam {
    kind: c_int,
    data: *mut c_void,
}

#[repr(C)]
struct OpenGlInitParams {
    get_proc_address: GetProcAddress,
    get_proc_address_ctx: *mut c_void,
}

#[repr(C)]
struct OpenGlFbo {
    fbo: c_int,
    w: c_int,
    h: c_int,
    internal_format: c_int,
}

/// Função que devolve o endereço de uma função do OpenGL pelo nome.
pub type GetProcAddress =
    unsafe extern "C" fn(ctx: *mut c_void, name: *const c_char) -> *mut c_void;

type UpdateCallback = unsafe extern "C" fn(ctx: *mut c_void);

unsafe extern "C" {
    fn mpv_render_context_create(
        res: *mut *mut RawRenderContext,
        mpv: *mut MpvHandle,
        params: *mut RenderParam,
    ) -> c_int;
    fn mpv_render_context_set_update_callback(
        ctx: *mut RawRenderContext,
        callback: Option<UpdateCallback>,
        callback_ctx: *mut c_void,
    );
    fn mpv_render_context_update(ctx: *mut RawRenderContext) -> u64;
    fn mpv_render_context_render(ctx: *mut RawRenderContext, params: *mut RenderParam) -> c_int;
    fn mpv_render_context_report_swap(ctx: *mut RawRenderContext);
    fn mpv_render_context_free(ctx: *mut RawRenderContext);
}

const PARAM_INVALID: c_int = 0;
const PARAM_API_TYPE: c_int = 1;
const PARAM_OPENGL_INIT_PARAMS: c_int = 2;
const PARAM_OPENGL_FBO: c_int = 3;
const PARAM_FLIP_Y: c_int = 4;
const PARAM_WL_DISPLAY: c_int = 9;

const UPDATE_FRAME: u64 = 1 << 0;

/// Chamado pelo libmpv, de outra thread, quando há algo novo para desenhar.
pub trait Wakeup: Send + 'static {
    fn wake(&self);
}

unsafe extern "C" fn on_update<W: Wakeup>(ctx: *mut c_void) {
    // SAFETY: ctx é o waker vazado em RenderContext::new, que nunca é liberado.
    let waker = unsafe { &*(ctx as *const W) };
    waker.wake();
}

/// Contexto de render ligado a uma instância do libmpv.
///
/// Precisa ser criado e usado com o contexto OpenGL atual. Guarda uma
/// referência à instância para que ela só seja destruída depois do contexto.
#[derive(Debug)]
pub struct RenderContext<W: Wakeup> {
    raw: *mut RawRenderContext,
    _mpv: Arc<Mpv>,
    _waker: PhantomData<W>,
}

impl<W: Wakeup> RenderContext<W> {
    /// Cria o contexto. `wl_display` permite ao libmpv usar a decodificação
    /// por hardware com interop EGL no Wayland.
    ///
    /// # Safety
    ///
    /// O contexto OpenGL precisa estar atual nesta thread, e `get_proc_address`
    /// precisa ser válido para ele. `wl_display`, se não nulo, precisa viver
    /// mais que o contexto.
    pub unsafe fn new(
        mpv: Arc<Mpv>,
        get_proc_address: GetProcAddress,
        wl_display: *mut c_void,
        waker: W,
    ) -> Result<Self, mpv::Error> {
        let api = CString::new("opengl").expect("sem byte nulo");
        let mut init = OpenGlInitParams {
            get_proc_address,
            get_proc_address_ctx: ptr::null_mut(),
        };
        let mut params = vec![
            RenderParam {
                kind: PARAM_API_TYPE,
                data: api.as_ptr() as *mut c_void,
            },
            RenderParam {
                kind: PARAM_OPENGL_INIT_PARAMS,
                data: (&raw mut init).cast(),
            },
        ];
        if !wl_display.is_null() {
            params.push(RenderParam {
                kind: PARAM_WL_DISPLAY,
                data: wl_display,
            });
        }
        params.push(RenderParam {
            kind: PARAM_INVALID,
            data: ptr::null_mut(),
        });

        let mut raw = ptr::null_mut();
        // SAFETY: lista terminada em PARAM_INVALID; os dados vivem durante a chamada.
        let code = unsafe { mpv_render_context_create(&mut raw, mpv.raw(), params.as_mut_ptr()) };
        mpv::check(code, "mpv_render_context_create")?;

        // O waker é vazado de propósito: o render.h não garante que um
        // callback em andamento terminou quando o contexto é liberado, e um
        // waker por contexto é pouca memória.
        let waker: *mut W = Box::leak(Box::new(waker));
        // SAFETY: raw é válido e o waker nunca é liberado.
        unsafe {
            mpv_render_context_set_update_callback(raw, Some(on_update::<W>), waker.cast());
        }
        Ok(Self {
            raw,
            _mpv: mpv,
            _waker: PhantomData,
        })
    }

    /// Diz se há um quadro novo para desenhar. Chame depois de cada wakeup.
    pub fn needs_frame(&self) -> bool {
        // SAFETY: contexto válido.
        unsafe { mpv_render_context_update(self.raw) & UPDATE_FRAME != 0 }
    }

    /// Desenha o quadro atual no framebuffer padrão, com o tamanho dado.
    pub fn render(&self, width: i32, height: i32) -> Result<(), mpv::Error> {
        let mut fbo = OpenGlFbo {
            fbo: 0,
            w: width,
            h: height,
            internal_format: 0,
        };
        let mut flip_y: c_int = 1;
        let mut params = [
            RenderParam {
                kind: PARAM_OPENGL_FBO,
                data: (&raw mut fbo).cast(),
            },
            RenderParam {
                kind: PARAM_FLIP_Y,
                data: (&raw mut flip_y).cast(),
            },
            RenderParam {
                kind: PARAM_INVALID,
                data: ptr::null_mut(),
            },
        ];
        // SAFETY: contexto válido; parâmetros vivem durante a chamada.
        let code = unsafe { mpv_render_context_render(self.raw, params.as_mut_ptr()) };
        mpv::check(code, "mpv_render_context_render")
    }

    /// Avisa o libmpv que o quadro foi apresentado (depois do swap).
    pub fn report_swap(&self) {
        // SAFETY: contexto válido.
        unsafe { mpv_render_context_report_swap(self.raw) };
    }
}

impl<W: Wakeup> Drop for RenderContext<W> {
    fn drop(&mut self) {
        // SAFETY: contexto válido, liberado uma única vez.
        unsafe {
            mpv_render_context_set_update_callback(self.raw, None, ptr::null_mut());
            mpv_render_context_free(self.raw);
        }
    }
}
