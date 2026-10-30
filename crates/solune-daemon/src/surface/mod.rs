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
#[cfg(windows)]
mod workerw;
#[cfg(target_os = "linux")]
mod x11;

use std::sync::Arc;
#[cfg(any(target_os = "linux", windows))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(any(target_os = "linux", windows))]
use std::sync::mpsc;
#[cfg(any(target_os = "linux", windows))]
use std::thread::{self, JoinHandle};
#[cfg(any(target_os = "linux", windows))]
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
    /// Janela filha do WorkerW, atrás dos ícones da área de trabalho do Windows.
    #[cfg(windows)]
    WorkerW,
}

impl Kind {
    /// A superfície da sessão atual, pelo ambiente. `None` quando não há
    /// nenhuma suportada.
    pub fn detect() -> Option<Self> {
        #[cfg(windows)]
        return Some(Self::WorkerW);
        #[cfg(target_os = "linux")]
        {
            if std::env::var_os("WAYLAND_DISPLAY").is_some() {
                return Some(Self::LayerShell);
            }
            if std::env::var_os("DISPLAY").is_some() {
                return Some(Self::X11);
            }
        }
        #[cfg(not(windows))]
        None
    }

    pub fn name(self) -> &'static str {
        match self {
            #[cfg(target_os = "linux")]
            Self::LayerShell => "layer-shell",
            #[cfg(target_os = "linux")]
            Self::X11 => "x11",
            #[cfg(windows)]
            Self::WorkerW => "workerw",
        }
    }
}

/// Avisa o daemon quando uma superfície termina por conta própria (o
/// compositor fechou a camada, a conexão com o X caiu).
pub type Ended = tokio::sync::mpsc::UnboundedSender<Result<(), String>>;

/// Resposta de um pedido à thread da superfície.
#[cfg(any(target_os = "linux", windows))]
type Reply = mpsc::Sender<Result<(), String>>;

/// Pedidos do player à thread da superfície.
#[cfg(any(target_os = "linux", windows))]
#[derive(Debug)]
enum Control {
    Stop,
    /// Guarda o quadro atual na tela; os quadros do libmpv param de aparecer.
    Freeze(Reply),
    /// Desliga o render do libmpv (congelado e com o arquivo parado).
    Release(Reply),
    /// Liga o render do libmpv de novo.
    Thaw(Reply),
}

#[cfg(target_os = "linux")]
type ControlSender = smithay_client_toolkit::reexports::calloop::channel::Sender<Control>;
#[cfg(windows)]
use workerw::Sender as ControlSender;

/// Superfície rodando.
#[derive(Debug)]
pub struct Surface {
    #[cfg(any(target_os = "linux", windows))]
    control: Option<ControlSender>,
    #[cfg(any(target_os = "linux", windows))]
    join: Option<JoinHandle<()>>,
    /// A thread avisa aqui quando termina, para `shutdown` não esperar sem
    /// limite por uma thread presa.
    #[cfg(any(target_os = "linux", windows))]
    done: Option<mpsc::Receiver<()>>,
}

/// Quanto esperar a superfície ficar pronta (EGL e render criados).
#[cfg(any(target_os = "linux", windows))]
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// Quanto esperar a superfície congelar ou descongelar o render.
#[cfg(any(target_os = "linux", windows))]
const CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// Quanto esperar a thread da superfície terminar depois do pedido de parar.
/// Presa no compositor ou no driver, ela fica para trás em vez de prender o
/// daemon.
#[cfg(any(target_os = "linux", windows))]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

/// Avisa uma vez se a superfície ficou pronta ou falhou ao começar. As
/// cópias dividem o mesmo aviso: só o primeiro `ok` ou `finish` vale.
#[cfg(any(target_os = "linux", windows))]
#[derive(Debug, Clone)]
struct Ready {
    tx: mpsc::Sender<Result<(), String>>,
    sent: Arc<AtomicBool>,
}

#[cfg(any(target_os = "linux", windows))]
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
        #[cfg(any(target_os = "linux", windows))]
        {
            #[cfg(target_os = "linux")]
            let (control_tx, control_rx) =
                smithay_client_toolkit::reexports::calloop::channel::channel::<Control>();
            #[cfg(windows)]
            let (control_tx, control_rx) = workerw::channel();
            let (ready_tx, ready_rx) = mpsc::channel();
            let (done_tx, done_rx) = mpsc::channel();
            let join = thread::Builder::new()
                .name(format!("surface-{}", kind.name()))
                .spawn(move || {
                    let ready = Ready {
                        tx: ready_tx,
                        sent: Arc::new(AtomicBool::new(false)),
                    };
                    let result = match kind {
                        #[cfg(target_os = "linux")]
                        Kind::LayerShell => {
                            layer_shell::run(player, hwdec_off, ready.clone(), control_rx)
                                .map_err(|e| e.to_string())
                        }
                        #[cfg(target_os = "linux")]
                        Kind::X11 => x11::run(player, hwdec_off, ready.clone(), control_rx)
                            .map_err(|e| e.to_string()),
                        #[cfg(windows)]
                        Kind::WorkerW => {
                            // Sem interop de decodificação: o libmpv escolhe sozinho.
                            let _ = hwdec_off;
                            workerw::run(player, ready.clone(), control_rx)
                        }
                    };
                    ready.finish(result, &ended);
                    let _ = done_tx.send(());
                })
                .map_err(|e| format!("não foi possível criar a thread da superfície: {e}"))?;

            let mut surface = Self {
                control: Some(control_tx),
                join: Some(join),
                done: Some(done_rx),
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
        #[cfg(not(any(target_os = "linux", windows)))]
        {
            let _ = (player, hwdec_off, ended);
            match kind {}
        }
    }

    /// Guarda o quadro atual na tela; os quadros do libmpv param de
    /// aparecer. Espera a superfície responder.
    pub fn freeze(&self) -> Result<(), String> {
        #[cfg(any(target_os = "linux", windows))]
        return self.request(Control::Freeze);
        #[cfg(not(any(target_os = "linux", windows)))]
        Ok(())
    }

    /// Desliga o render do libmpv, que solta o que tinha na GPU. Só depois
    /// de `freeze` e com o arquivo parado.
    pub fn release(&self) -> Result<(), String> {
        #[cfg(any(target_os = "linux", windows))]
        return self.request(Control::Release);
        #[cfg(not(any(target_os = "linux", windows)))]
        Ok(())
    }

    /// Liga o render do libmpv de novo. O quadro guardado fica na tela até
    /// o primeiro quadro do vídeo.
    pub fn thaw(&self) -> Result<(), String> {
        #[cfg(any(target_os = "linux", windows))]
        return self.request(Control::Thaw);
        #[cfg(not(any(target_os = "linux", windows)))]
        Ok(())
    }

    #[cfg(any(target_os = "linux", windows))]
    fn request(&self, control: fn(Reply) -> Control) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        self.control
            .as_ref()
            .ok_or("a superfície já parou")?
            .send(control(tx))
            .map_err(|_| "a superfície já parou")?;
        rx.recv_timeout(CONTROL_TIMEOUT)
            .map_err(|_| "a superfície não respondeu".to_owned())?
    }

    /// Para a superfície e espera a thread terminar, por até `STOP_TIMEOUT`.
    /// O papel de parede do sistema volta a aparecer.
    pub fn shutdown(&mut self) {
        #[cfg(any(target_os = "linux", windows))]
        {
            if let Some(control) = self.control.take() {
                let _ = control.send(Control::Stop);
            }
            let (Some(join), Some(done)) = (self.join.take(), self.done.take()) else {
                return;
            };
            // `Disconnected` é a thread que terminou sem avisar (um pânico):
            // o join volta na hora. Só o tempo esgotado deixa a thread solta.
            match done.recv_timeout(STOP_TIMEOUT) {
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    tracing::error!("a thread da superfície não parou; seguindo sem ela");
                }
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let _ = join.join();
                }
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        self.shutdown();
    }
}
