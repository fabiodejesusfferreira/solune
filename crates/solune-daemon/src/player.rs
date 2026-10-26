//! O player: uma instância do libmpv, a superfície e o estado que o app vê.
//!
//! Tudo passa por uma tarefa só (o "ator"), que recebe os pedidos do app,
//! os avisos do libmpv e o fim da superfície, nessa ordem de chegada. Assim
//! o estado nunca é mexido de dois lugares ao mesmo tempo.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use solune_core::policy::{self, Decision, Pauses, RELEASE_AFTER};
use solune_ipc::{
    Command, Conditions, ErrorCode, Event, Outcome, PlayerState, Playlist, Policy, Reply, Settings,
    Status, Theme, Usage,
};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::config::{State, Store};
use crate::icons::Icons;
use crate::mpv::{self, Mpv};
use crate::show::{Show, local_time};
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

/// De quanto em quanto tempo o ator olha o relógio: carrossel e agenda de temas.
const TICK: Duration = Duration::from_secs(30);

/// Quanto esperar o tema do sistema antes de voltar ao wallpaper salvo. Sem
/// ele, um par claro e escuro abriria o claro e trocaria logo em seguida.
const THEME_WAIT: Duration = Duration::from_secs(2);

/// Variável que encurta os 5 minutos até liberar o decodificador, para
/// testes e medições.
const RELEASE_AFTER_ENV: &str = "SOLUNE_RELEASE_AFTER_SECS";

fn release_after() -> Duration {
    std::env::var(RELEASE_AFTER_ENV)
        .ok()
        .and_then(|s| s.parse().ok())
        .map_or(RELEASE_AFTER, Duration::from_secs)
}

/// Devolve ao sistema a memória que o processo soltou. O malloc do glibc
/// guarda o que foi liberado para usar de novo, e sem isso a RAM do daemon
/// não desce depois de liberar o decodificador.
fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: malloc_trim não tem pré-condições.
    unsafe {
        libc::malloc_trim(0);
    }
}

/// A faixa de áudio do mpv: `no` nem abre o decodificador de áudio.
fn audio_track(sound: bool) -> &'static str {
    if sound { "auto" } else { "no" }
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
    /// Acabou a espera pelo tema do sistema; volta ao wallpaper salvo.
    RestoreTimer,
    /// Hora de conferir o carrossel e a agenda de temas.
    Tick,
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

/// Erros do libmpv (client.h) que significam arquivo que não toca. O -14 é
/// `MPV_ERROR_AO_INIT_FAILED` (áudio) e cai em `Internal`.
const MPV_ERROR_LOADING_FAILED: i32 = -13;
const MPV_ERROR_VO_INIT_FAILED: i32 = -15;
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

pub struct Player {
    mpv: Arc<Mpv>,
    config: Config,
    surface: Option<Surface>,
    icons: Icons,
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
    /// O vídeo foi recarregado com a opção `start`, que precisa voltar ao
    /// padrão depois que o arquivo abrir (senão vale para o próximo).
    start_pending: bool,
    /// Quando o vídeo começou a voltar da liberação; o log mostra quanto
    /// levou até o primeiro quadro.
    reloading_since: Option<Instant>,
    sampler: Sampler,
    /// A lista em uso, se há uma.
    show: Option<Show>,
    /// O tema que vale agora.
    theme: Theme,
    /// Quantos itens seguidos do carrossel não tocaram. Zera no primeiro
    /// quadro de um item que toca.
    failures: usize,
    /// O wallpaper salvo ainda não voltou: espera o tema do sistema.
    restore_pending: bool,
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
        let settings = config.store.settings();
        let mpv = Arc::new(Mpv::new(&[
            ("terminal", "yes"),
            ("msg-level", "all=warn"),
            ("idle", "yes"),
            ("vo", vo),
            ("hwdec", &config.hwdec),
            ("loop-file", "inf"),
            // Uma imagem fica na tela até trocar, sem reabrir a cada segundo.
            ("image-display-duration", "inf"),
            ("aid", audio_track(settings.sound)),
        ])?);
        let watcher = Watcher::spawn(Arc::clone(&mpv), inbox.clone());
        let pauses = Pauses::new(settings);
        let restore_pending = config.signals;
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
            icons: Icons::default(),
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
            start_pending: false,
            reloading_since: None,
            sampler: Sampler::default(),
            show: None,
            theme: settings.schedule.theme_at(local_time(), Theme::Light),
            failures: 0,
            restore_pending,
        })
    }

    /// Roda o ator até receber `Quit`. Antes, volta a tocar o wallpaper que
    /// estava em uso quando o daemon parou.
    pub async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Msg>) {
        if self.restore_pending {
            // Com os sinais ligados, o wallpaper volta quando o tema do
            // sistema chegar (`on_signal`); o prazo evita esperar sem fim.
            let wait = self.inbox.clone();
            tokio::spawn(async move {
                tokio::time::sleep(THEME_WAIT).await;
                let _ = wait.send(Msg::RestoreTimer);
            });
        } else {
            self.restore();
        }
        let ticks = self.inbox.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(TICK);
            interval.tick().await;
            loop {
                interval.tick().await;
                if ticks.send(Msg::Tick).is_err() {
                    break;
                }
            }
        });
        while let Some(msg) = inbox.recv().await {
            match msg {
                Msg::Command(command, reply) => {
                    // Quem escolhe o que toca vale mais que o wallpaper salvo.
                    if matches!(
                        command,
                        Command::Play { .. } | Command::PlayList { .. } | Command::Stop
                    ) {
                        self.restore_pending = false;
                    }
                    let outcome = self.handle(command);
                    let _ = reply.send(outcome);
                }
                Msg::Mpv(note) => self.on_note(note),
                Msg::SurfaceEnded(result) => self.on_surface_ended(result),
                Msg::Signal(change) => self.on_signal(change),
                Msg::ReleaseTimer(generation) => self.on_release_timer(generation),
                Msg::RestoreTimer => self.restore_if_pending(),
                Msg::Tick => self.on_tick(),
                Msg::Quit => break,
            }
        }
        drop(self.signals.take());
    }

    fn restore_if_pending(&mut self) {
        if std::mem::take(&mut self.restore_pending) {
            self.restore();
        }
    }

    /// Volta ao wallpaper salvo. Um arquivo que sumiu (um disco que ainda
    /// não montou, por exemplo) não apaga a lista: o daemon pula para o
    /// próximo item e, se nenhum existe, deixa o arquivo salvo como está
    /// para o próximo login.
    fn restore(&mut self) {
        let state = self.config.store.state();
        let Some(list) = state.playlist() else {
            return;
        };
        // Carrossel sem intervalo: troca de item a cada login.
        let position = state.position + usize::from(list.every_minutes == 0);
        tracing::info!("voltando ao wallpaper em uso ({} itens)", list.items.len());
        let mut show = Show::new(list, position);
        for _ in 0..show.len() {
            let path = show.item().for_theme(self.theme).to_owned();
            match self.play(&path) {
                Ok(()) => {
                    self.failures = 0;
                    self.show = Some(show);
                    self.save_show();
                    return;
                }
                Err(ErrorCode::FileNotFound) => show.advance(),
                Err(code) => {
                    tracing::warn!("não deu para voltar ao wallpaper: {}", code.code());
                    return;
                }
            }
        }
        tracing::warn!("nenhum arquivo do wallpaper salvo está disponível");
    }

    fn handle(&mut self, command: Command) -> Outcome {
        let result = match command {
            Command::Play { path } => self.play_list(Playlist::single(path), 0),
            Command::PlayList { list } => self.play_list(list, 0),
            Command::Next => self.next(),
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
        if settings.sound != before.sound
            && let Err(e) = self.mpv.set_property("aid", audio_track(settings.sound))
        {
            tracing::warn!("{e}");
        }
        self.sync_icons();
        self.publish_policy();
        self.apply_policy()?;
        self.update_theme();
        Ok(())
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

    /// Toca a lista a partir do item `position`.
    fn play_list(&mut self, list: Playlist, position: usize) -> Result<(), ErrorCode> {
        if list.items.is_empty() {
            return Err(ErrorCode::FileNotFound);
        }
        let show = Show::new(list, position);
        let path = show.item().for_theme(self.theme).to_owned();
        self.play(&path)?;
        self.failures = 0;
        self.show = Some(show);
        self.save_show();
        Ok(())
    }

    /// O item que acabou de falhar não derruba o carrossel: tenta os outros,
    /// cada um uma vez. `false` quando não há para onde ir.
    fn skip_broken(&mut self) -> bool {
        loop {
            let Some(show) = &mut self.show else {
                return false;
            };
            if self.failures + 1 >= show.len() {
                return false;
            }
            self.failures += 1;
            show.advance();
            self.save_show();
            let failed = self.status.path.clone();
            match self.show_current() {
                // Um item repetido, igual ao que falhou, não abre nada novo.
                Ok(()) if self.status.path != failed => return true,
                Ok(()) => {}
                Err(code) => tracing::warn!("o próximo item também não toca: {}", code.code()),
            }
        }
    }

    fn save_show(&self) {
        let state = self
            .show
            .as_ref()
            .map_or_else(State::default, |show| State {
                path: None,
                list: Some(show.list.clone()),
                position: show.position,
            });
        self.config.store.save_state(&state);
    }

    /// Pula para o próximo item do carrossel.
    fn next(&mut self) -> Result<(), ErrorCode> {
        let Some(show) = &mut self.show else {
            return Ok(());
        };
        show.advance();
        self.save_show();
        self.show_current()
    }

    /// Troca o arquivo pelo que vale agora (item do carrossel e tema), sem
    /// mexer nas pausas: pausado continua pausado.
    fn show_current(&mut self) -> Result<(), ErrorCode> {
        let Some(path) = self
            .show
            .as_ref()
            .map(|s| s.item().for_theme(self.theme).to_owned())
        else {
            return Ok(());
        };
        if self.status.state == PlayerState::Stopped || self.status.path.as_ref() == Some(&path) {
            return Ok(());
        }
        if !path.is_file() {
            return Err(ErrorCode::FileNotFound);
        }
        if let Some(released) = &mut self.released {
            // Retomar já abre o arquivo novo, do começo.
            released.position = None;
        } else {
            let Some(file) = path.to_str() else {
                return Err(ErrorCode::FileNotFound);
            };
            self.reloading_since = None;
            self.clear_start();
            self.mpv.command(&["loadfile", file]).map_err(|e| {
                tracing::error!("{e}");
                ErrorCode::Internal
            })?;
        }
        tracing::info!("trocando para {}", path.display());
        self.set_status(Status {
            path: Some(path),
            hwdec: None,
            ..self.status.clone()
        });
        Ok(())
    }

    /// Recalcula o tema pela agenda e pelo sistema; se mudou, troca o arquivo.
    fn update_theme(&mut self) {
        let system = if self.conditions.dark == Some(true) {
            Theme::Dark
        } else {
            Theme::Light
        };
        let theme = self
            .pauses
            .settings()
            .schedule
            .theme_at(local_time(), system);
        if theme == self.theme {
            return;
        }
        tracing::info!("tema agora: {theme:?}");
        self.theme = theme;
        self.set_status(Status {
            theme,
            ..self.status.clone()
        });
        if let Err(code) = self.show_current() {
            tracing::warn!("não deu para trocar de tema: {}", code.code());
        }
    }

    fn on_tick(&mut self) {
        self.update_theme();
        // Pausado ninguém vê a troca: o carrossel espera voltar a tocar.
        if self.status.state == PlayerState::Playing
            && self.show.as_ref().is_some_and(Show::due)
            && let Err(code) = self.next()
        {
            tracing::warn!("o carrossel não trocou: {}", code.code());
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
        self.forget_release()?;
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
            theme: self.theme,
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
        self.sync_icons();
        Ok(())
    }

    /// Os ícones ficam escondidos só enquanto há wallpaper e o usuário pediu.
    fn sync_icons(&mut self) {
        let hidden = self.surface.is_some() && !self.pauses.settings().show_desktop_icons;
        self.icons.set_hidden(hidden);
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
        self.apply_signal(change);
        // Com o tema do sistema conhecido, o wallpaper salvo abre já no
        // arquivo certo do par claro e escuro.
        if matches!(change, Change::Dark(_)) {
            self.restore_if_pending();
        }
    }

    fn apply_signal(&mut self, change: Change) {
        let before = self.conditions;
        match change {
            Change::Fullscreen(v) => self.conditions.fullscreen = v,
            Change::Battery(v) => self.conditions.on_battery = v,
            Change::Locked(v) => self.conditions.screen_locked = v,
            Change::Dark(v) => self.conditions.dark = v,
        }
        if self.conditions == before {
            return;
        }
        self.pauses.set_signals(self.conditions.into());
        self.publish_policy();
        if let Err(code) = self.apply_policy() {
            tracing::warn!("a política não pôde ser aplicada: {}", code.code());
        }
        self.update_theme();
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

    /// Libera o decodificador numa pausa longa. A superfície guarda o
    /// quadro atual na tela; o arquivo para, e com ele o demuxer e o
    /// decodificador; por fim a superfície desliga o render do libmpv, que
    /// solta as texturas e o interop com a GPU. Na tela, nada muda.
    fn release(&mut self) {
        let Some(surface) = &self.surface else {
            return;
        };
        let position = self.mpv.property("time-pos");
        if let Err(e) = tokio::task::block_in_place(|| surface.freeze()) {
            tracing::warn!("não deu para liberar o decodificador: {e}");
            return;
        }
        // Daqui em diante o ponto de volta vale, mesmo se um passo falhar:
        // retomar religa o render e recarrega o vídeo.
        self.released = Some(Released { position });
        if let Err(e) = self.mpv.command(&["stop"]) {
            tracing::warn!("{e}");
        }
        if let Err(e) = tokio::task::block_in_place(|| surface.release()) {
            tracing::warn!("o render do libmpv não foi desligado: {e}");
        }
        trim_heap();
        tracing::info!("decodificador liberado depois da pausa longa");
    }

    /// Volta ao vídeo, do ponto onde estava.
    fn unrelease(&mut self) -> Result<(), ErrorCode> {
        let Some(released) = self.released.take() else {
            return Ok(());
        };
        let Some(file) = self.status.path.clone() else {
            return Err(ErrorCode::Internal);
        };
        let Some(file) = file.to_str() else {
            return Err(ErrorCode::Internal);
        };
        self.thaw_surface()?;
        if let Some(position) = &released.position {
            // Abrir já na posição evita mostrar o começo do vídeo antes.
            match self.mpv.set_property("start", position) {
                Ok(()) => self.start_pending = true,
                Err(e) => tracing::warn!("{e}"),
            }
        }
        self.mpv.command(&["loadfile", file]).map_err(|e| {
            tracing::error!("{e}");
            ErrorCode::Internal
        })?;
        tracing::info!("decodificador de volta");
        self.reloading_since = Some(Instant::now());
        Ok(())
    }

    /// Liga o render do libmpv de novo, se a superfície existe.
    fn thaw_surface(&self) -> Result<(), ErrorCode> {
        let Some(surface) = &self.surface else {
            return Ok(());
        };
        tokio::task::block_in_place(|| surface.thaw()).map_err(|e| {
            tracing::error!("não deu para religar o render: {e}");
            ErrorCode::Internal
        })
    }

    /// Esquece o ponto de volta antes de trocar de vídeo, religando o render.
    fn forget_release(&mut self) -> Result<(), ErrorCode> {
        self.reloading_since = None;
        self.clear_start();
        if self.released.take().is_some() {
            self.thaw_surface()?;
        }
        Ok(())
    }

    /// Volta a opção `start` ao padrão: o próximo arquivo abre do começo.
    fn clear_start(&mut self) {
        if std::mem::take(&mut self.start_pending)
            && let Err(e) = self.mpv.set_property("start", "none")
        {
            tracing::warn!("{e}");
        }
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
        // A superfície vai embora, então o render não precisa voltar.
        self.released = None;
        self.reloading_since = None;
        self.clear_start();
        self.paused_since = None;
        self.pauses.reset();
        self.show = None;
        if let Some(mut surface) = self.surface.take() {
            tokio::task::block_in_place(|| surface.shutdown());
        }
        self.sync_icons();
        self.set_status(Status {
            theme: self.theme,
            ..Status::default()
        });
    }

    fn on_note(&mut self, note: Note) {
        match note {
            Note::Loaded => self.clear_start(),
            Note::Playing { hwdec } => {
                self.failures = 0;
                if let Some(since) = self.reloading_since.take() {
                    tracing::info!("vídeo de volta em {} ms", since.elapsed().as_millis());
                }
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
                if !self.skip_broken() {
                    // Só um arquivo, ou nenhum da lista toca. Um arquivo
                    // sozinho que não toca não adianta tentar de novo ao
                    // reiniciar; uma lista fica salva, para quando os
                    // arquivos voltarem.
                    let single = self.show.as_ref().is_none_or(|show| show.len() < 2);
                    self.stop();
                    if single {
                        self.config.store.save_state(&State::default());
                    }
                }
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
    }
}

/// Thread que espera os eventos do libmpv e os repassa ao ator.
struct Watcher {
    stop: Arc<AtomicBool>,
    /// Para acordar a thread quando ela precisa parar.
    mpv: Arc<Mpv>,
    join: Option<JoinHandle<()>>,
}

impl Watcher {
    fn spawn(player: Arc<Mpv>, inbox: mpsc::UnboundedSender<Msg>) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let join = thread::Builder::new()
            .name("mpv-events".into())
            .spawn({
                let stop = Arc::clone(&stop);
                let player = Arc::clone(&player);
                move || watch(&player, &stop, &inbox)
            })
            .expect("criar a thread de eventos do libmpv");
        Self {
            stop,
            mpv: player,
            join: Some(join),
        }
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.mpv.wakeup();
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

fn watch(player: &Mpv, stop: &AtomicBool, inbox: &mpsc::UnboundedSender<Msg>) {
    while !stop.load(Ordering::Relaxed) {
        // Sem timeout: parada, a thread dorme até o libmpv ter um evento, em
        // vez de acordar quatro vezes por segundo. `Watcher::drop` a acorda.
        let Some(event) = player.wait_event(-1.0) else {
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
        assert_eq!(playback_error(-15), ErrorCode::SurfaceUnavailable);
        // Falha ao abrir o áudio não é falha da superfície.
        assert_eq!(playback_error(-14), ErrorCode::Internal);
        assert_eq!(playback_error(-1), ErrorCode::Internal);
    }
}
