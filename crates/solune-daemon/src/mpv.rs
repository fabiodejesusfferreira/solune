//! Bindings mínimos da API de cliente do libmpv.
//!
//! Escritos à mão a partir de `mpv/client.h` (licença ISC) para manter o
//! grafo de crates livre de copyleft. Só o que o daemon usa está aqui.

use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::fmt;
use std::ptr;

#[repr(C)]
pub(crate) struct MpvHandle {
    _private: [u8; 0],
}

#[repr(C)]
struct RawEvent {
    event_id: c_int,
    error: c_int,
    reply_userdata: u64,
    data: *mut c_void,
}

#[repr(C)]
struct RawEndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

unsafe extern "C" {
    fn mpv_client_api_version() -> std::ffi::c_ulong;
    fn mpv_error_string(error: c_int) -> *const c_char;
    fn mpv_free(data: *mut c_void);
    fn mpv_create() -> *mut MpvHandle;
    fn mpv_initialize(ctx: *mut MpvHandle) -> c_int;
    fn mpv_terminate_destroy(ctx: *mut MpvHandle);
    fn mpv_set_option_string(
        ctx: *mut MpvHandle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    fn mpv_command(ctx: *mut MpvHandle, args: *mut *const c_char) -> c_int;
    fn mpv_get_property_string(ctx: *mut MpvHandle, name: *const c_char) -> *mut c_char;
    fn mpv_set_property_string(
        ctx: *mut MpvHandle,
        name: *const c_char,
        data: *const c_char,
    ) -> c_int;
    fn mpv_wait_event(ctx: *mut MpvHandle, timeout: c_double) -> *mut RawEvent;
}

const EVENT_NONE: c_int = 0;
const EVENT_SHUTDOWN: c_int = 1;
const EVENT_END_FILE: c_int = 7;
const EVENT_FILE_LOADED: c_int = 8;
const EVENT_PLAYBACK_RESTART: c_int = 21;

const END_FILE_REASON_ERROR: c_int = 4;

/// Erro devolvido pelo libmpv, com a mensagem dele.
#[derive(Debug)]
pub struct Error {
    pub code: i32,
    pub context: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, Self::describe(self.code))
    }
}

impl std::error::Error for Error {}

impl Error {
    /// A mensagem do libmpv para um código de erro.
    pub fn describe(code: i32) -> String {
        // SAFETY: mpv_error_string devolve uma string estática para qualquer código.
        let msg = unsafe { CStr::from_ptr(mpv_error_string(code)) };
        msg.to_string_lossy().into_owned()
    }
}

pub(crate) fn check(code: c_int, context: impl Into<String>) -> Result<(), Error> {
    if code < 0 {
        Err(Error {
            code,
            context: context.into(),
        })
    } else {
        Ok(())
    }
}

fn cstring(s: &str) -> CString {
    CString::new(s).expect("string sem byte nulo")
}

/// Versão da API de cliente do libmpv carregado, como (major, minor).
pub fn client_api_version() -> (u32, u32) {
    // SAFETY: função sem argumentos e sem estado.
    let v = unsafe { mpv_client_api_version() } as u64;
    ((v >> 16) as u32, (v & 0xffff) as u32)
}

/// Eventos que o spike observa.
#[derive(Debug)]
pub enum Event {
    FileLoaded,
    PlaybackRestart,
    EndFile { error: Option<i32> },
    Shutdown,
    Other,
}

/// Uma instância do libmpv.
#[derive(Debug)]
pub struct Mpv {
    handle: *mut MpvHandle,
}

impl Mpv {
    /// Cria a instância e aplica as opções antes de inicializar.
    pub fn new(options: &[(&str, &str)]) -> Result<Self, Error> {
        // SAFETY: mpv_create não recebe argumentos; um ponteiro nulo indica falha.
        let handle = unsafe { mpv_create() };
        if handle.is_null() {
            return Err(Error {
                code: -1,
                context: "mpv_create".into(),
            });
        }
        let mpv = Self { handle };
        for (name, value) in options {
            let (n, v) = (cstring(name), cstring(value));
            // SAFETY: handle válido, strings terminadas em nulo vivas durante a chamada.
            let code = unsafe { mpv_set_option_string(mpv.handle, n.as_ptr(), v.as_ptr()) };
            check(code, format!("opção {name}={value}"))?;
        }
        // SAFETY: handle válido e ainda não inicializado.
        check(unsafe { mpv_initialize(mpv.handle) }, "mpv_initialize")?;
        Ok(mpv)
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn raw(&self) -> *mut MpvHandle {
        self.handle
    }

    /// Roda um comando, como `["loadfile", "video.mp4"]`.
    pub fn command(&self, args: &[&str]) -> Result<(), Error> {
        let owned: Vec<CString> = args.iter().map(|a| cstring(a)).collect();
        let mut ptrs: Vec<*const c_char> = owned.iter().map(|a| a.as_ptr()).collect();
        ptrs.push(ptr::null());
        // SAFETY: lista terminada em nulo; as strings vivem até o fim da chamada.
        let code = unsafe { mpv_command(self.handle, ptrs.as_mut_ptr()) };
        check(code, format!("comando {}", args.join(" ")))
    }

    /// Lê uma propriedade como texto. `None` se ela não estiver disponível.
    pub fn property(&self, name: &str) -> Option<String> {
        let n = cstring(name);
        // SAFETY: handle válido; o retorno é nulo ou uma string que liberamos com mpv_free.
        unsafe {
            let raw = mpv_get_property_string(self.handle, n.as_ptr());
            if raw.is_null() {
                return None;
            }
            let value = CStr::from_ptr(raw).to_string_lossy().into_owned();
            mpv_free(raw.cast());
            Some(value)
        }
    }

    /// Muda uma propriedade a partir de texto, como `pause` = `yes`.
    pub fn set_property(&self, name: &str, value: &str) -> Result<(), Error> {
        let (n, v) = (cstring(name), cstring(value));
        // SAFETY: handle válido, strings terminadas em nulo vivas durante a chamada.
        let code = unsafe { mpv_set_property_string(self.handle, n.as_ptr(), v.as_ptr()) };
        check(code, format!("propriedade {name}={value}"))
    }

    /// Espera o próximo evento. `None` quando o tempo acaba sem evento.
    pub fn wait_event(&self, timeout_secs: f64) -> Option<Event> {
        // SAFETY: handle válido; o evento pertence ao mpv e só é lido até a próxima chamada.
        let event = unsafe { &*mpv_wait_event(self.handle, timeout_secs) };
        Some(match event.event_id {
            EVENT_NONE => return None,
            EVENT_SHUTDOWN => Event::Shutdown,
            EVENT_FILE_LOADED => Event::FileLoaded,
            EVENT_PLAYBACK_RESTART => Event::PlaybackRestart,
            EVENT_END_FILE => {
                // SAFETY: para MPV_EVENT_END_FILE, data aponta para mpv_event_end_file.
                let end = unsafe { &*(event.data as *const RawEndFile) };
                let error = (end.reason == END_FILE_REASON_ERROR).then_some(end.error);
                Event::EndFile { error }
            }
            _ => Event::Other,
        })
    }
}

// SAFETY: a API de cliente do libmpv é thread-safe ("generally fully
// thread-safe", client.h); o único requisito, um só chamador de
// mpv_wait_event por vez, é cumprido por quem usa a instância.
unsafe impl Send for Mpv {}
// SAFETY: veja acima.
unsafe impl Sync for Mpv {}

impl Drop for Mpv {
    fn drop(&mut self) {
        // SAFETY: handle válido, destruído uma única vez.
        unsafe { mpv_terminate_destroy(self.handle) };
    }
}
