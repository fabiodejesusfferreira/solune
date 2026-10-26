//! Sinais do sistema: app em tela cheia, bateria e tela bloqueada, para a
//! política de pausa, e o tema claro ou escuro, para a agenda de temas.
//!
//! Cada sinal tem o seu observador, que avisa o ator só quando o valor
//! muda. `None` quer dizer que o sinal não pode ser detectado nesta sessão;
//! a política conta isso como ausente.

#[cfg(target_os = "linux")]
mod battery;
#[cfg(target_os = "linux")]
mod fullscreen_wayland;
#[cfg(target_os = "linux")]
mod fullscreen_x11;
#[cfg(target_os = "linux")]
mod locked;
#[cfg(target_os = "linux")]
mod theme;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

use crate::surface::Kind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Fullscreen(Option<bool>),
    Battery(Option<bool>),
    Locked(Option<bool>),
    /// O sistema está no tema escuro.
    Dark(Option<bool>),
}

/// Quem recebe as mudanças: o ator do player.
pub type Notify = Arc<dyn Fn(Change) + Send + Sync>;

/// Observadores rodando. Param quando o valor sai de escopo.
pub struct Watchers {
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl std::fmt::Debug for Watchers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watchers")
            .field("threads", &self.threads.len())
            .field("tasks", &self.tasks.len())
            .finish()
    }
}

impl Watchers {
    /// Começa a observar. A tela cheia depende da superfície da sessão; sem
    /// superfície, ela fica como não detectada. Precisa de um runtime tokio.
    pub fn start(kind: Option<Kind>, notify: Notify) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let mut watchers = Self {
            stop,
            threads: Vec::new(),
            tasks: Vec::new(),
        };
        watchers.start_platform(kind, notify);
        watchers
    }

    /// Os sinais ainda não existem fora do Linux: ficam como não detectados.
    #[cfg(not(target_os = "linux"))]
    fn start_platform(&mut self, _kind: Option<Kind>, notify: Notify) {
        notify(Change::Fullscreen(None));
        notify(Change::Battery(None));
        notify(Change::Locked(None));
        notify(Change::Dark(None));
    }

    #[cfg(target_os = "linux")]
    fn start_platform(&mut self, kind: Option<Kind>, notify: Notify) {
        let fullscreen: Box<dyn FnOnce(Arc<AtomicBool>, Notify) + Send> = match kind {
            Some(Kind::X11) => Box::new(|stop, notify| {
                fullscreen_x11::run(&stop, &|v| notify(Change::Fullscreen(v)));
            }),
            Some(Kind::LayerShell) => Box::new(|stop, notify| {
                fullscreen_wayland::run(&stop, &|v| notify(Change::Fullscreen(v)));
            }),
            None => Box::new(|_, notify| notify(Change::Fullscreen(None))),
        };
        let (stop, n) = (Arc::clone(&self.stop), Arc::clone(&notify));
        match std::thread::Builder::new()
            .name("signal-fullscreen".into())
            .spawn(move || fullscreen(stop, n))
        {
            Ok(join) => self.threads.push(join),
            Err(e) => {
                tracing::warn!("não foi possível observar a tela cheia: {e}");
                notify(Change::Fullscreen(None));
            }
        }

        let n = Arc::clone(&notify);
        self.tasks.push(tokio::spawn(battery::watch(move |v| {
            n(Change::Battery(v));
        })));
        let n = Arc::clone(&notify);
        self.tasks.push(tokio::spawn(locked::watch(move |v| {
            n(Change::Locked(v));
        })));
        let n = Arc::clone(&notify);
        self.tasks.push(tokio::spawn(theme::watch(move |v| {
            n(Change::Dark(v));
        })));
    }
}

impl Drop for Watchers {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for task in &self.tasks {
            task.abort();
        }
        for join in self.threads.drain(..) {
            let _ = join.join();
        }
    }
}

/// Avisa só quando o valor muda.
#[derive(Debug, Default)]
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct Latest<T>(Option<T>);

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
impl<T: PartialEq + Copy> Latest<T> {
    /// Guarda o valor e devolve `true` se ele mudou (ou é o primeiro).
    pub(crate) fn update(&mut self, value: T) -> bool {
        if self.0 == Some(value) {
            return false;
        }
        self.0 = Some(value);
        true
    }
}

/// Espera o descritor ficar legível por até `timeout_ms`. `true` se ficou.
#[cfg(target_os = "linux")]
pub(crate) fn wait_readable(fd: std::os::fd::RawFd, timeout_ms: i32) -> bool {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: um pollfd válido, contado como 1.
    let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
    ready > 0 && pollfd.revents & libc::POLLIN != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_so_avisa_mudanca() {
        let mut latest = Latest::default();
        assert!(latest.update(Some(false)));
        assert!(!latest.update(Some(false)));
        assert!(latest.update(Some(true)));
        assert!(latest.update(None));
    }
}
