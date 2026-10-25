//! Tela cheia no X11, pelas propriedades EWMH que o gerenciador de janelas
//! publica: `_NET_CLIENT_LIST` na raiz e `_NET_WM_STATE` em cada janela.
//!
//! Conta como tela cheia uma janela com `_NET_WM_STATE_FULLSCREEN`, que não
//! esteja minimizada e que esteja na área de trabalho atual. Jogos que
//! cobrem a tela sem pedir tela cheia ao gerenciador não são detectados.
//! Sem gerenciador compatível com EWMH, o sinal fica como não detectado.

use std::collections::HashSet;
use std::ffi::{CString, c_int, c_uchar, c_ulong};
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};

use x11_dl::xlib::{self, Xlib};

use super::{Latest, wait_readable};

const ALL_DESKTOPS: c_ulong = 0xFFFF_FFFF;

struct Atoms {
    client_list: xlib::Atom,
    current_desktop: xlib::Atom,
    wm_desktop: xlib::Atom,
    wm_state: xlib::Atom,
    fullscreen: xlib::Atom,
    hidden: xlib::Atom,
}

struct Connection {
    xlib: Xlib,
    display: *mut xlib::Display,
    root: xlib::Window,
    atoms: Atoms,
}

/// Erros do X (uma janela que fechou entre a lista e a leitura) não podem
/// derrubar o daemon, que é o que o tratador padrão do Xlib faz.
unsafe extern "C" fn ignore_errors(
    _display: *mut xlib::Display,
    _event: *mut xlib::XErrorEvent,
) -> c_int {
    0
}

impl Connection {
    fn open() -> Result<Self, String> {
        let xlib = Xlib::open().map_err(|e| format!("não foi possível carregar o libX11: {e}"))?;
        // SAFETY: XInitThreads vem antes de qualquer outra chamada do Xlib
        // no processo (a superfície X11 também chama, e repetir é seguro).
        unsafe {
            (xlib.XInitThreads)();
            (xlib.XSetErrorHandler)(Some(ignore_errors));
        }
        // SAFETY: nulo abre o display de $DISPLAY.
        let display = unsafe { (xlib.XOpenDisplay)(ptr::null()) };
        if display.is_null() {
            return Err("sem conexão com o servidor X".into());
        }
        // SAFETY: display válido daqui até o Drop.
        let root = unsafe { (xlib.XDefaultRootWindow)(display) };
        let intern = |name: &str| {
            let name = CString::new(name).expect("nome de átomo sem nulo");
            // SAFETY: display válido, nome terminado em nulo.
            unsafe { (xlib.XInternAtom)(display, name.as_ptr(), xlib::False) }
        };
        let atoms = Atoms {
            client_list: intern("_NET_CLIENT_LIST"),
            current_desktop: intern("_NET_CURRENT_DESKTOP"),
            wm_desktop: intern("_NET_WM_DESKTOP"),
            wm_state: intern("_NET_WM_STATE"),
            fullscreen: intern("_NET_WM_STATE_FULLSCREEN"),
            hidden: intern("_NET_WM_STATE_HIDDEN"),
        };
        Ok(Self {
            xlib,
            display,
            root,
            atoms,
        })
    }

    /// Lê uma propriedade de 32 bits (que o Xlib entrega como `long`).
    fn longs(&self, window: xlib::Window, property: xlib::Atom) -> Option<Vec<c_ulong>> {
        let mut actual_type = MaybeUninit::<xlib::Atom>::uninit();
        let mut format = MaybeUninit::<c_int>::uninit();
        let mut count = MaybeUninit::<c_ulong>::uninit();
        let mut after = MaybeUninit::<c_ulong>::uninit();
        let mut data = MaybeUninit::<*mut c_uchar>::uninit();
        // SAFETY: display válido; o Xlib preenche todas as saídas quando
        // devolve Success, e os dados são liberados com XFree.
        unsafe {
            let status = (self.xlib.XGetWindowProperty)(
                self.display,
                window,
                property,
                0,
                4096,
                xlib::False,
                xlib::AnyPropertyType as c_ulong,
                actual_type.as_mut_ptr(),
                format.as_mut_ptr(),
                count.as_mut_ptr(),
                after.as_mut_ptr(),
                data.as_mut_ptr(),
            );
            if status != xlib::Success as c_int {
                return None;
            }
            let data = data.assume_init();
            if data.is_null() {
                return None;
            }
            let values = if format.assume_init() == 32 {
                let items = data.cast::<c_ulong>();
                Some(
                    (0..count.assume_init() as usize)
                        .map(|i| *items.add(i))
                        .collect(),
                )
            } else {
                None
            };
            (self.xlib.XFree)(data.cast());
            values
        }
    }

    fn select_properties(&self, window: xlib::Window) {
        // SAFETY: display válido; uma janela que já fechou só gera um erro,
        // que é ignorado.
        unsafe { (self.xlib.XSelectInput)(self.display, window, xlib::PropertyChangeMask) };
    }

    /// Se alguma janela da lista está em tela cheia na área de trabalho atual.
    fn any_fullscreen(&self, clients: &[c_ulong]) -> bool {
        let current = self
            .longs(self.root, self.atoms.current_desktop)
            .and_then(|v| v.first().copied());
        clients.iter().any(|&window| {
            let Some(state) = self.longs(window, self.atoms.wm_state) else {
                return false;
            };
            let fullscreen = state.contains(&self.atoms.fullscreen);
            let hidden = state.contains(&self.atoms.hidden);
            let desktop = self
                .longs(window, self.atoms.wm_desktop)
                .and_then(|v| v.first().copied());
            let here = match (desktop, current) {
                (Some(ALL_DESKTOPS), _) | (None, _) | (_, None) => true,
                (Some(d), Some(c)) => d == c,
            };
            fullscreen && !hidden && here
        })
    }

    fn interesting(&self, atom: xlib::Atom) -> bool {
        let a = &self.atoms;
        [a.client_list, a.current_desktop, a.wm_desktop, a.wm_state].contains(&atom)
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        // SAFETY: display aberto em `open` e fechado uma vez.
        unsafe { (self.xlib.XCloseDisplay)(self.display) };
    }
}

/// Observa até `stop`. Avisa `None` se não der para detectar.
pub fn run(stop: &AtomicBool, notify: &dyn Fn(Option<bool>)) {
    let x = match Connection::open() {
        Ok(x) => x,
        Err(e) => {
            tracing::warn!("tela cheia não detectada: {e}");
            notify(None);
            return;
        }
    };
    x.select_properties(x.root);
    // SAFETY: display válido.
    let fd = unsafe { (x.xlib.XConnectionNumber)(x.display) };
    let mut latest = Latest::default();
    let mut watched = HashSet::new();
    let mut dirty = true;
    while !stop.load(Ordering::Relaxed) {
        if dirty {
            let value = x.longs(x.root, x.atoms.client_list).map(|clients| {
                for &window in &clients {
                    if watched.insert(window) {
                        x.select_properties(window);
                    }
                }
                watched.retain(|w| clients.contains(w));
                x.any_fullscreen(&clients)
            });
            if latest.update(value) {
                tracing::info!("tela cheia: {value:?}");
                notify(value);
            }
            dirty = false;
        }
        // SAFETY: display válido.
        if unsafe { (x.xlib.XPending)(x.display) } == 0 && !wait_readable(fd, 500) {
            continue;
        }
        // SAFETY: display válido.
        while unsafe { (x.xlib.XPending)(x.display) } > 0 {
            let mut event = MaybeUninit::<xlib::XEvent>::uninit();
            // SAFETY: há evento na fila, então XNextEvent não bloqueia e
            // preenche o evento inteiro.
            let event = unsafe {
                (x.xlib.XNextEvent)(x.display, event.as_mut_ptr());
                event.assume_init()
            };
            if event.get_type() == xlib::PropertyNotify {
                let property = xlib::XPropertyEvent::from(event);
                dirty |= x.interesting(property.atom);
            }
        }
    }
}
