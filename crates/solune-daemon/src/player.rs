//! O player: uma instância do libmpv, a superfície e o estado que o app vê.
//!
//! Tudo passa por uma tarefa só (o "ator"), que recebe os pedidos do app,
//! os avisos do libmpv e o fim da superfície, nessa ordem de chegada. Assim
//! o estado nunca é mexido de dois lugares ao mesmo tempo.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use solune_ipc::{Command, ErrorCode, Event, Outcome, PauseReason, PlayerState, Reply, Status};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::mpv::{self, Mpv};
use crate::surface::{Kind, Surface};

/// Onde o vídeo aparece.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Na superfície de papel de parede da sessão.
    Wallpaper,
    /// Em lugar nenhum (`vo=null`): testes e CI sem tela.
    Headless,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub output: Output,
    /// Modo de decodificação por hardware do mpv, como `auto-safe` ou `no`.
    pub hwdec: String,
}

/// Mensagens que o ator recebe.
#[derive(Debug)]
pub enum Msg {
    Command(Command, oneshot::Sender<Outcome>),
    Mpv(Note),
    SurfaceEnded(Result<(), String>),
    /// Encerra o ator. A caixa de entrada não fecha sozinha: o próprio
    /// player guarda remetentes para a superfície e a thread de eventos.
    Quit,
}

/// O que a thread de eventos do libmpv repassa.
#[derive(Debug)]
pub enum Note {
    /// O primeiro quadro depois de carregar ou de voltar ao início.
    Playing { hwdec: Option<String> },
    /// O arquivo não pôde ser tocado; o código é do libmpv.
    Failed(i32),
}

/// Erros do libmpv (client.h) que significam arquivo que não toca.
const MPV_ERROR_LOADING_FAILED: i32 = -13;
const MPV_ERROR_VO_INIT_FAILED: i32 = -14;
const MPV_ERROR_NOTHING_TO_PLAY: i32 = -16;
const MPV_ERROR_UNKNOWN_FORMAT: i32 = -17;

fn playback_error(code: i32) -> ErrorCode {
    match code {
        MPV_ERROR_LOADING_FAILED | MPV_ERROR_NOTHING_TO_PLAY | MPV_ERROR_UNKNOWN_FORMAT => {
            ErrorCode::UnsupportedFormat
        }
        MPV_ERROR_VO_INIT_FAILED => ErrorCode::SurfaceUnavailable,
        _ => ErrorCode::Internal,
    }
}

pub struct Player {
    mpv: Arc<Mpv>,
    config: Config,
    surface: Option<Surface>,
    status: Status,
    events: broadcast::Sender<Event>,
    inbox: mpsc::UnboundedSender<Msg>,
    watcher: Option<Watcher>,
}

impl std::fmt::Debug for Player {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Player")
            .field("config", &self.config)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Player {
    /// Cria o libmpv. A superfície só nasce no primeiro `play`.
    pub fn new(
        config: Config,
        events: broadcast::Sender<Event>,
        inbox: mpsc::UnboundedSender<Msg>,
    ) -> Result<Self, mpv::Error> {
        let (major, minor) = mpv::client_api_version();
        tracing::info!("libmpv: API de cliente {major}.{minor}");
        let vo = match config.output {
            Output::Wallpaper => "libmpv",
            Output::Headless => "null",
        };
        let mpv = Arc::new(Mpv::new(&[
            ("terminal", "yes"),
            ("msg-level", "all=warn"),
            ("idle", "yes"),
            ("vo", vo),
            ("hwdec", &config.hwdec),
            ("loop-file", "inf"),
            ("aid", "no"),
        ])?);
        let watcher = Watcher::spawn(Arc::clone(&mpv), inbox.clone());
        Ok(Self {
            mpv,
            config,
            surface: None,
            status: Status::default(),
            events,
            inbox,
            watcher: Some(watcher),
        })
    }

    /// Roda o ator até receber `Quit`.
    pub async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Msg>) {
        while let Some(msg) = inbox.recv().await {
            match msg {
                Msg::Command(command, reply) => {
                    let outcome = self.handle(command);
                    let _ = reply.send(outcome);
                }
                Msg::Mpv(note) => self.on_note(note),
                Msg::SurfaceEnded(result) => self.on_surface_ended(result),
                Msg::Quit => break,
            }
        }
    }

    fn handle(&mut self, command: Command) -> Outcome {
        let result = match command {
            Command::Play { path } => self.play(&path),
            Command::Pause => self.pause(),
            Command::Resume => self.resume(),
            Command::Stop => {
                self.stop();
                Ok(())
            }
            Command::Status => return Outcome::Ok(Reply::Status(self.status.clone())),
            // Quem encerra é o servidor, depois de responder.
            Command::Shutdown => Ok(()),
        };
        match result {
            Ok(()) => Outcome::Ok(Reply::Done),
            Err(code) => Outcome::Error(code),
        }
    }

    fn play(&mut self, path: &Path) -> Result<(), ErrorCode> {
        if !path.is_file() {
            return Err(ErrorCode::FileNotFound);
        }
        let Some(file) = path.to_str() else {
            tracing::warn!("caminho fora de UTF-8: {}", path.display());
            return Err(ErrorCode::FileNotFound);
        };
        self.ensure_surface()?;
        self.mpv
            .command(&["loadfile", file])
            .and_then(|()| self.mpv.set_property("pause", "no"))
            .map_err(|e| {
                tracing::error!("{e}");
                ErrorCode::Internal
            })?;
        self.set_status(Status {
            state: PlayerState::Playing,
            path: Some(path.to_owned()),
            pause_reason: None,
            hwdec: None,
        });
        Ok(())
    }

    /// Cria a superfície se ainda não houver uma. O libmpv precisa do
    /// contexto de render antes de carregar o arquivo.
    fn ensure_surface(&mut self) -> Result<(), ErrorCode> {
        if self.config.output == Output::Headless || self.surface.is_some() {
            return Ok(());
        }
        let Some(kind) = Kind::detect() else {
            tracing::warn!("nenhuma superfície de papel de parede para esta sessão");
            return Err(ErrorCode::SurfaceUnavailable);
        };
        let hwdec_off = self.config.hwdec == "no";
        let (ended_tx, mut ended_rx) = mpsc::unbounded_channel();
        let player = Arc::clone(&self.mpv);
        // Criar a superfície bloqueia até o EGL ficar pronto (poucos
        // milissegundos, no máximo 10 s); o runtime tem outras threads.
        let surface =
            tokio::task::block_in_place(|| Surface::start(kind, player, hwdec_off, ended_tx))
                .map_err(|e| {
                    tracing::error!("superfície {}: {e}", kind.name());
                    ErrorCode::SurfaceUnavailable
                })?;
        tracing::info!("superfície {} pronta", kind.name());
        let inbox = self.inbox.clone();
        tokio::spawn(async move {
            if let Some(result) = ended_rx.recv().await {
                let _ = inbox.send(Msg::SurfaceEnded(result));
            }
        });
        self.surface = Some(surface);
        Ok(())
    }

    fn pause(&mut self) -> Result<(), ErrorCode> {
        if self.status.state != PlayerState::Playing {
            return Ok(());
        }
        self.set_pause(true)?;
        self.set_status(Status {
            state: PlayerState::Paused,
            pause_reason: Some(PauseReason::User),
            ..self.status.clone()
        });
        Ok(())
    }

    fn resume(&mut self) -> Result<(), ErrorCode> {
        if self.status.state != PlayerState::Paused {
            return Ok(());
        }
        self.set_pause(false)?;
        self.set_status(Status {
            state: PlayerState::Playing,
            pause_reason: None,
            ..self.status.clone()
        });
        Ok(())
    }

    fn set_pause(&self, paused: bool) -> Result<(), ErrorCode> {
        let value = if paused { "yes" } else { "no" };
        self.mpv.set_property("pause", value).map_err(|e| {
            tracing::error!("{e}");
            ErrorCode::Internal
        })
    }

    /// Para o vídeo e desfaz a superfície: o papel de parede do sistema volta.
    fn stop(&mut self) {
        if let Err(e) = self.mpv.command(&["stop"]) {
            tracing::warn!("{e}");
        }
        if let Some(mut surface) = self.surface.take() {
            tokio::task::block_in_place(|| surface.shutdown());
        }
        self.set_status(Status::default());
    }

    fn on_note(&mut self, note: Note) {
        match note {
            Note::Playing { hwdec } => {
                if self.status.state != PlayerState::Stopped && self.status.hwdec != hwdec {
                    tracing::info!("decodificação: {}", hwdec.as_deref().unwrap_or("?"));
                    self.set_status(Status {
                        hwdec,
                        ..self.status.clone()
                    });
                }
            }
            Note::Failed(code) => {
                let error = playback_error(code);
                tracing::warn!(
                    "a reprodução falhou: {} ({code})",
                    mpv::Error::describe(code)
                );
                self.stop();
                let _ = self.events.send(Event::PlaybackFailed { error });
            }
        }
    }

    fn on_surface_ended(&mut self, result: Result<(), String>) {
        // Uma superfície que já foi desfeita por `stop` não chega aqui com
        // estado a perder; as outras terminaram sozinhas.
        if self.surface.is_none() {
            return;
        }
        match &result {
            Ok(()) => tracing::warn!("a superfície terminou sem pedido"),
            Err(e) => tracing::error!("a superfície caiu: {e}"),
        }
        self.stop();
        if result.is_err() {
            let _ = self.events.send(Event::PlaybackFailed {
                error: ErrorCode::SurfaceUnavailable,
            });
        }
    }

    fn set_status(&mut self, status: Status) {
        if status != self.status {
            self.status = status;
            let _ = self.events.send(Event::StatusChanged {
                status: self.status.clone(),
            });
        }
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        // A superfície (e o contexto de render) sai antes do libmpv, e a
        // thread de eventos para antes de o libmpv ser destruído.
        drop(self.surface.take());
        drop(self.watcher.take());
    }
}

/// Thread que espera os eventos do libmpv e os repassa ao ator.
struct Watcher {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Watcher {
    fn spawn(player: Arc<Mpv>, inbox: mpsc::UnboundedSender<Msg>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let join = thread::Builder::new()
            .name("mpv-events".into())
            .spawn({
                let stop = Arc::clone(&stop);
                move || watch(&player, &stop, &inbox)
            })
            .expect("criar a thread de eventos do libmpv");
        Self {
            stop,
            join: Some(join),
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn watch(player: &Mpv, stop: &AtomicBool, inbox: &mpsc::UnboundedSender<Msg>) {
    while !stop.load(Ordering::Relaxed) {
        let Some(event) = player.wait_event(0.25) else {
            continue;
        };
        let note = match event {
            mpv::Event::PlaybackRestart => Note::Playing {
                hwdec: player.property("hwdec-current"),
            },
            mpv::Event::EndFile { error: Some(code) } => Note::Failed(code),
            mpv::Event::Shutdown => break,
            mpv::Event::FileLoaded | mpv::Event::EndFile { error: None } | mpv::Event::Other => {
                continue;
            }
        };
        if inbox.send(Msg::Mpv(note)).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erros_do_libmpv_viram_codigos_do_protocolo() {
        assert_eq!(playback_error(-13), ErrorCode::UnsupportedFormat);
        assert_eq!(playback_error(-17), ErrorCode::UnsupportedFormat);
        assert_eq!(playback_error(-14), ErrorCode::SurfaceUnavailable);
        assert_eq!(playback_error(-1), ErrorCode::Internal);
    }
}
