//! O player: uma instância do libmpv, a superfície e o estado que o app vê.
//!
//! Tudo passa por uma tarefa só (o "ator"), que recebe os pedidos do app,
//! os avisos do libmpv e o fim da superfície, nessa ordem de chegada. Assim
//! o estado nunca é mexido de dois lugares ao mesmo tempo.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use solune_core::policy::{self, Decision, Pauses, RELEASE_AFTER};
use solune_ipc::{
    Command, Conditions, ErrorCode, Event, Outcome, PlayerState, Policy, Reply, Settings, Status,
    Usage,
};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::config::{State, Store};
use crate::mpv::{self, Mpv};
use crate::signals::{Change, Watchers};
use crate::surface::{Kind, Surface};
use crate::usage::Sampler;

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
    /// Onde ficam as preferências e o wallpaper em uso.
    pub store: Store,
    /// Observar tela cheia, bateria e tela bloqueada. Desligado sem tela.
    pub signals: bool,
}

/// Variável que encurta os 5 minutos até liberar o decodificador, para
/// testes e medições.
const RELEASE_AFTER_ENV: &str = "SOLUNE_RELEASE_AFTER_SECS";

fn release_after() -> Duration {
    std::env::var(RELEASE_AFTER_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(RELEASE_AFTER, Duration::from_secs)
}

/// Onde fica a última imagem do vídeo enquanto o decodificador está liberado.
fn frame_path() -> PathBuf {
    let dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(|d| PathBuf::from(d).join("solune"))
        .unwrap_or_else(std::env::temp_dir);
    dir.join(format!("frame-{}.png", std::process::id()))
}

/// Mensagens que o ator recebe.
#[derive(Debug)]
pub enum Msg {
    Command(Command, oneshot::Sender<Outcome>),
    Mpv(Note),
    SurfaceEnded(Result<(), String>),
    Signal(Change),
    /// Passou o tempo de liberar o decodificador da pausa com este número.
    ReleaseTimer(u64),
    /// Encerra o ator. A caixa de entrada não fecha sozinha: o próprio
    /// player guarda remetentes para a superfície e a thread de eventos.
    Quit,
}

/// O que a thread de eventos do libmpv repassa.
#[derive(Debug)]
pub enum Note {
    /// O arquivo abriu; dá para pular para uma posição.
    Loaded,
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

/// O vídeo foi trocado pela última imagem para liberar o decodificador.
#[derive(Debug)]
struct Released {
    /// Onde o vídeo estava, para voltar do mesmo ponto.
    position: Option<String>,
}

/// Pular para `position` quando `path` terminar de abrir.
#[derive(Debug)]
struct Seek {
    path: String,
    position: String,
}

pub struct Player {
    mpv: Arc<Mpv>,
    config: Config,
    surface: Option<Surface>,
    status: Status,
    events: broadcast::Sender<Event>,
    inbox: mpsc::UnboundedSender<Msg>,
    watcher: Option<Watcher>,
    signals: Option<Watchers>,
    pauses: Pauses,
    conditions: Conditions,
    paused_since: Option<Instant>,
    /// Muda a cada pausa, para um timer velho não liberar a pausa nova.
    pause_generation: u64,
    released: Option<Released>,
    seek: Option<Seek>,
    sampler: Sampler,
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
            // A última imagem, quando o decodificador é liberado, fica na tela.
            ("image-display-duration", "inf"),
        ])?);
        let watcher = Watcher::spawn(Arc::clone(&mpv), inbox.clone());
        let pauses = Pauses::new(config.store.settings());
        let signals = config.signals.then(|| {
            let inbox = inbox.clone();
            let kind = match config.output {
                Output::Wallpaper => Kind::detect(),
                Output::Headless => None,
            };
            Watchers::start(
                kind,
                Arc::new(move |change| {
                    let _ = inbox.send(Msg::Signal(change));
                }),
            )
        });
        Ok(Self {
            mpv,
            config,
            surface: None,
            status: Status::default(),
            events,
            inbox,
            watcher: Some(watcher),
            signals,
            pauses,
            conditions: Conditions::default(),
            paused_since: None,
            pause_generation: 0,
            released: None,
            seek: None,
            sampler: Sampler::default(),
        })
    }

    /// Roda o ator até receber `Quit`. Antes, volta a tocar o wallpaper que
    /// estava em uso quando o daemon parou.
    pub async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Msg>) {
        self.restore();
        while let Some(msg) = inbox.recv().await {
            match msg {
                Msg::Command(command, reply) => {
                    let outcome = self.handle(command);
                    let _ = reply.send(outcome);
                }
                Msg::Mpv(note) => self.on_note(note),
                Msg::SurfaceEnded(result) => self.on_surface_ended(result),
                Msg::Signal(change) => self.on_signal(change),
                Msg::ReleaseTimer(generation) => self.on_release_timer(generation),
                Msg::Quit => break,
            }
        }
        drop(self.signals.take());
    }

    fn restore(&mut self) {
        let Some(path) = self.config.store.state().path else {
            return;
        };
        tracing::info!("voltando ao wallpaper em uso: {}", path.display());
        if let Err(code) = self.play(&path) {
            tracing::warn!("não deu para voltar ao wallpaper: {}", code.code());
            if code == ErrorCode::FileNotFound {
                self.config.store.save_state(&State::default());
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
                self.config.store.save_state(&State::default());
                Ok(())
            }
            Command::Status => return Outcome::Ok(Reply::Status(self.status.clone())),
            Command::GetPolicy => return Outcome::Ok(Reply::Policy(self.policy())),
            Command::SetSettings { settings } => self.set_settings(settings),
            Command::Usage => return Outcome::Ok(Reply::Usage(self.usage())),
            // Quem encerra é o servidor, depois de responder.
            Command::Shutdown => Ok(()),
        };
        match result {
            Ok(()) => Outcome::Ok(Reply::Done),
            Err(code) => Outcome::Error(code),
        }
    }

    fn policy(&self) -> Policy {
        Policy {
            settings: self.pauses.settings(),
            conditions: self.conditions,
        }
    }

    fn publish_policy(&self) {
        let _ = self.events.send(Event::PolicyChanged {
            policy: self.policy(),
        });
    }

    fn set_settings(&mut self, settings: Settings) -> Result<(), ErrorCode> {
        let before = self.pauses.settings();
        self.pauses.set_settings(settings);
        self.config.store.save_settings(&settings);
        if settings.release_when_paused && !before.release_when_paused {
            self.arm_release_timer();
        }
        self.publish_policy();
        self.apply_policy()
    }

    fn usage(&mut self) -> Usage {
        let reading = self.sampler.read();
        let fps = match self.status.state {
            PlayerState::Stopped => None,
            PlayerState::Paused => Some(0),
            PlayerState::Playing => self
                .mpv
                .property("estimated-vf-fps")
                .and_then(|v| v.parse::<f64>().ok())
                .map(|fps| fps.round() as u32),
        };
        Usage {
            cpu_permille: reading.cpu_permille,
            gpu_permille: reading.gpu_permille,
            memory_bytes: reading.memory_bytes,
            gpu_memory_bytes: reading.gpu_memory_bytes,
            fps,
            released: self.released.is_some(),
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
        self.forget_release();
        self.mpv
            .command(&["loadfile", file])
            .and_then(|()| self.mpv.set_property("pause", "no"))
            .map_err(|e| {
                tracing::error!("{e}");
                ErrorCode::Internal
            })?;
        self.paused_since = None;
        self.set_status(Status {
            state: PlayerState::Playing,
            path: Some(path.to_owned()),
            pause_reason: None,
            hwdec: None,
        });
        self.config.store.save_state(&State {
            path: Some(path.to_owned()),
        });
        // Um wallpaper novo já nasce pausado se, por exemplo, há um app em
        // tela cheia.
        self.pauses.reset();
        self.apply_policy()
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
        if self.status.state == PlayerState::Stopped {
            return Ok(());
        }
        self.pauses.pause_by_user();
        self.apply_policy()
    }

    fn resume(&mut self) -> Result<(), ErrorCode> {
        if self.status.state == PlayerState::Stopped {
            return Ok(());
        }
        self.pauses.resume_by_user();
        self.apply_policy()
    }

    /// Põe o player no estado que a política pede: toca, ou pausa com o
    /// motivo que vale agora.
    fn apply_policy(&mut self) -> Result<(), ErrorCode> {
        if self.status.state == PlayerState::Stopped {
            return Ok(());
        }
        match self.pauses.decision() {
            Decision::Play => {
                if self.status.state == PlayerState::Paused {
                    self.unrelease()?;
                    self.set_pause(false)?;
                    self.paused_since = None;
                    self.set_status(Status {
                        state: PlayerState::Playing,
                        pause_reason: None,
                        ..self.status.clone()
                    });
                }
            }
            Decision::Pause(reason) => {
                if self.status.state == PlayerState::Playing {
                    self.set_pause(true)?;
                    self.paused_since = Some(Instant::now());
                    self.arm_release_timer();
                }
                self.set_status(Status {
                    state: PlayerState::Paused,
                    pause_reason: Some(reason),
                    ..self.status.clone()
                });
            }
        }
        Ok(())
    }

    fn on_signal(&mut self, change: Change) {
        let before = self.conditions;
        match change {
            Change::Fullscreen(v) => self.conditions.fullscreen = v,
            Change::Battery(v) => self.conditions.on_battery = v,
            Change::Locked(v) => self.conditions.screen_locked = v,
        }
        if self.conditions == before {
            return;
        }
        self.pauses.set_signals(self.conditions.into());
        self.publish_policy();
        if let Err(code) = self.apply_policy() {
            tracing::warn!("a política não pôde ser aplicada: {}", code.code());
        }
    }

    /// Agenda a liberação do decodificador para quando a pausa atual fizer
    /// o tempo combinado.
    fn arm_release_timer(&mut self) {
        self.pause_generation += 1;
        let Some(since) = self.paused_since else {
            return;
        };
        let generation = self.pause_generation;
        let wait = release_after().saturating_sub(since.elapsed());
        let inbox = self.inbox.clone();
        tokio::spawn(async move {
            tokio::time::sleep(wait).await;
            let _ = inbox.send(Msg::ReleaseTimer(generation));
        });
    }

    fn on_release_timer(&mut self, generation: u64) {
        let Some(since) = self.paused_since else {
            return;
        };
        let due = policy::should_release(since.elapsed(), release_after(), self.pauses.settings());
        if generation == self.pause_generation
            && self.status.state == PlayerState::Paused
            && self.released.is_none()
            && due
        {
            self.release();
        }
    }

    /// Troca o vídeo pausado pela imagem do quadro atual. O libmpv fecha o
    /// decodificador e a memória de vídeo dele; na tela, nada muda.
    fn release(&mut self) {
        if self.config.output == Output::Headless {
            return;
        }
        let position = self.mpv.property("time-pos");
        let frame = frame_path();
        let Some(frame_str) = frame.to_str() else {
            return;
        };
        if let Some(dir) = frame.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let result = self
            .mpv
            .command(&["screenshot-to-file", frame_str, "video"])
            .and_then(|()| self.mpv.command(&["loadfile", frame_str]));
        match result {
            Ok(()) => {
                tracing::info!("decodificador liberado depois da pausa longa");
                self.released = Some(Released { position });
            }
            Err(e) => tracing::warn!("não deu para liberar o decodificador: {e}"),
        }
    }

    /// Volta ao vídeo, do ponto onde estava.
    fn unrelease(&mut self) -> Result<(), ErrorCode> {
        let Some(released) = self.released.take() else {
            return Ok(());
        };
        let _ = std::fs::remove_file(frame_path());
        let Some(file) = self.status.path.as_deref().and_then(Path::to_str) else {
            return Err(ErrorCode::Internal);
        };
        self.seek = released.position.map(|position| Seek {
            path: file.to_owned(),
            position,
        });
        self.mpv.command(&["loadfile", file]).map_err(|e| {
            tracing::error!("{e}");
            ErrorCode::Internal
        })?;
        tracing::info!("decodificador de volta");
        Ok(())
    }

    /// Esquece a imagem e o ponto de volta (o vídeo vai ser trocado ou parado).
    fn forget_release(&mut self) {
        if self.released.take().is_some() {
            let _ = std::fs::remove_file(frame_path());
        }
        self.seek = None;
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
        self.forget_release();
        self.paused_since = None;
        self.pauses.reset();
        if let Some(mut surface) = self.surface.take() {
            tokio::task::block_in_place(|| surface.shutdown());
        }
        self.set_status(Status::default());
    }

    fn on_note(&mut self, note: Note) {
        match note {
            Note::Loaded => {
                let Some(seek) = self.seek.take() else {
                    return;
                };
                if self.mpv.property("path").as_deref() != Some(seek.path.as_str()) {
                    // Ainda é a imagem que abriu; o vídeo vem a seguir.
                    self.seek = Some(seek);
                    return;
                }
                if let Err(e) = self
                    .mpv
                    .command(&["seek", &seek.position, "absolute", "exact"])
                {
                    tracing::warn!("{e}");
                }
            }
            // A imagem que fica no lugar do vídeo não muda a decodificação.
            Note::Playing { .. } if self.released.is_some() => {}
            Note::Playing { hwdec } => {
                if self.status.state != PlayerState::Stopped && self.status.hwdec != hwdec {
                    tracing::info!("decodificação: {}", hwdec.as_deref().unwrap_or("?"));
                    self.set_status(Status {
                        hwdec,
                        ..self.status.clone()
                    });
                }
            }
            Note::Failed(code) if self.released.is_some() => {
                tracing::warn!(
                    "a imagem do quadro não abriu: {}; voltando ao vídeo",
                    mpv::Error::describe(code)
                );
                self.released = None;
                let _ = std::fs::remove_file(frame_path());
                if let Some(file) = self.status.path.as_deref().and_then(Path::to_str) {
                    let _ = self.mpv.command(&["loadfile", file]);
                }
            }
            Note::Failed(code) => {
                let error = playback_error(code);
                tracing::warn!(
                    "a reprodução falhou: {} ({code})",
                    mpv::Error::describe(code)
                );
                self.stop();
                // Não adianta tentar de novo ao reiniciar.
                self.config.store.save_state(&State::default());
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
        drop(self.signals.take());
        drop(self.surface.take());
        drop(self.watcher.take());
        if self.released.is_some() {
            let _ = std::fs::remove_file(frame_path());
        }
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
            mpv::Event::FileLoaded => Note::Loaded,
            mpv::Event::PlaybackRestart => Note::Playing {
                hwdec: player.property("hwdec-current"),
            },
            mpv::Event::EndFile { error: Some(code) } => Note::Failed(code),
            mpv::Event::Shutdown => break,
            mpv::Event::EndFile { error: None } | mpv::Event::Other => continue,
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
