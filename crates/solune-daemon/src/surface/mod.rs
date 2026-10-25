//! Superfícies: onde o vídeo aparece como papel de parede.
//!
//! Cada superfície roda o próprio laço de eventos numa thread, desenha os
//! quadros do libmpv pela render API e para quando o daemon pede. A thread
//! de render nunca chama a API comum do libmpv (render.h): carregar arquivo,
//! pausar e ler propriedades ficam com o player, em outras threads.

#[cfg(target_os = "linux")]
mod gl;
#[cfg(target_os = "linux")]
mod layer_shell;
#[cfg(target_os = "linux")]
mod x11;

use std::sync::Arc;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(target_os = "linux")]
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::thread::{self, JoinHandle};
#[cfg(target_os = "linux")]
use std::time::Duration;

use crate::mpv::Mpv;

/// Tipos de superfície que este sistema tem.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Camada de fundo do Wayland: KDE Plasma 6 e wlroots.
    #[cfg(target_os = "linux")]
    LayerShell,
    /// Janela do tipo desktop em sessões X11, como o Cinnamon.
    #[cfg(target_os = "linux")]
    X11,
}

impl Kind {
    /// A superfície da sessão atual, pelo ambiente. `None` quando não há
    /// nenhuma suportada (no Windows, até o WorkerW entrar).
    pub fn detect() -> Option<Self> {
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return Some(Self::LayerShell);
            }
            if std::env::var_os("DISPLAY").is_some() {
                return Some(Self::X11);
            }
        }
        None
    }

    pub fn name(self) -> &'static str {
        match self {
            #[cfg(target_os = "linux")]
            Self::LayerShell => "layer-shell",
            #[cfg(target_os = "linux")]
            Self::X11 => "x11",
        }
    }
}

/// Avisa o daemon quando uma superfície termina por conta própria (o
/// compositor fechou a camada, a conexão com o X caiu).
pub type Ended = tokio::sync::mpsc::UnboundedSender<Result<(), String>>;

/// Superfície rodando.
#[derive(Debug)]
pub struct Surface {
    #[cfg(target_os = "linux")]
    stop: Option<smithay_client_toolkit::reexports::calloop::channel::Sender<()>>,
    #[cfg(target_os = "linux")]
    join: Option<JoinHandle<()>>,
}

/// Quanto esperar a superfície ficar pronta (EGL e render criados).
#[cfg(target_os = "linux")]
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Avisa uma vez se a superfície ficou pronta ou falhou ao começar. As
/// cópias dividem o mesmo aviso: só o primeiro `ok` ou `finish` vale.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone)]
struct Ready {
    tx: mpsc::Sender<Result<(), String>>,
    sent: Arc<AtomicBool>,
}

#[cfg(target_os = "linux")]
impl Ready {
    fn ok(&self) {
        if !self.sent.swap(true, Ordering::AcqRel) {
            let _ = self.tx.send(Ok(()));
        }
    }

    /// Entrega o resultado final. Se a superfície nem chegou a ficar pronta,
    /// ele vai para quem esperava em `start`; senão, para `ended`.
    fn finish(self, result: Result<(), String>, ended: &Ended) {
        if self.sent.swap(true, Ordering::AcqRel) {
            let _ = ended.send(result);
        } else {
            let early = result.and(Err("a superfície terminou antes de ficar pronta".into()));
            let _ = self.tx.send(early);
        }
    }
}

impl Surface {
    /// Cria a superfície e espera ela ficar pronta para receber quadros.
    /// `hwdec_off` evita passar o display ao libmpv (veja layer_shell.rs).
    pub fn start(
        kind: Kind,
        player: Arc<Mpv>,
        hwdec_off: bool,
        ended: Ended,
    ) -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            use smithay_client_toolkit::reexports::calloop::channel;

            let (stop_tx, stop_rx) = channel::channel::<()>();
            let (ready_tx, ready_rx) = mpsc::channel();
            let join = thread::Builder::new()
                .name(format!("surface-{}", kind.name()))
                .spawn(move || {
                    let ready = Ready {
                        tx: ready_tx,
                        sent: Arc::new(AtomicBool::new(false)),
                    };
                    let result = match kind {
                        Kind::LayerShell => {
                            layer_shell::run(player, hwdec_off, ready.clone(), stop_rx)
                        }
                        Kind::X11 => x11::run(player, hwdec_off, ready.clone(), stop_rx),
                    };
                    ready.finish(result.map_err(|e| e.to_string()), &ended);
                })
                .map_err(|e| format!("não foi possível criar a thread da superfície: {e}"))?;

            let mut surface = Self {
                stop: Some(stop_tx),
                join: Some(join),
            };
            match ready_rx.recv_timeout(READY_TIMEOUT) {
                Ok(Ok(())) => Ok(surface),
                Ok(Err(e)) => {
                    surface.shutdown();
                    Err(e)
                }
                Err(_) => {
                    surface.shutdown();
                    Err(format!(
                        "a superfície {} não ficou pronta a tempo",
                        kind.name()
                    ))
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (player, hwdec_off, ended);
            match kind {}
        }
    }

    /// Para a superfície e espera a thread terminar. O papel de parede do
    /// sistema volta a aparecer.
    pub fn shutdown(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if let Some(stop) = self.stop.take() {
                let _ = stop.send(());
            }
            if let Some(join) = self.join.take() {
                let _ = join.join();
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.shutdown();
    }
}
