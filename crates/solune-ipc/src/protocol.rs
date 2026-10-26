//! Mensagens trocadas entre o app (cliente) e o daemon (servidor).
//!
//! Cada mensagem é um objeto JSON numa linha. O cliente abre com `hello`,
//! o daemon responde `welcome` e, daí em diante, o cliente manda pedidos
//! com `id` e o daemon devolve uma resposta com o mesmo `id`. Eventos do
//! daemon chegam a qualquer momento, sem `id`.
//!
//! O daemon não tem texto: estados, motivos de pausa e erros são códigos, e
//! quem traduz é a interface (`pause_reason.*` e `error.*` no catálogo).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::{Schedule, Theme};

/// Versão do protocolo. Muda quando uma mensagem muda de forma
/// incompatível; cliente e daemon de versões diferentes não conversam.
pub const PROTOCOL_VERSION: u32 = 3;

/// Mensagem do cliente para o daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Hello { protocol: u32 },
    Request { id: u64, command: Command },
}

/// O que o cliente pede ao daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum Command {
    /// Toca o arquivo como papel de parede, em loop.
    Play {
        path: PathBuf,
    },
    /// Toca uma lista: um item só, ou um carrossel. Cada item pode ter um
    /// par para o tema escuro.
    PlayList {
        list: Playlist,
    },
    /// Pula para o próximo item do carrossel.
    Next,
    /// Pausa por pedido do usuário.
    Pause,
    /// Retoma depois de uma pausa do usuário.
    Resume,
    /// Para e devolve a tela ao papel de parede do sistema.
    Stop,
    Status,
    /// Preferências das pausas e o que o daemon detecta do sistema agora.
    GetPolicy,
    /// Troca as preferências das pausas. O daemon salva e aplica na hora.
    SetSettings {
        settings: Settings,
    },
    /// Quanto o daemon está usando de CPU, GPU e memória.
    Usage,
    /// Encerra o daemon.
    Shutdown,
}

/// O que o daemon toca. Com mais de um item, troca a cada `every_minutes`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Playlist {
    pub items: Vec<Item>,
    /// Minutos entre as trocas. Zero troca só quando o daemon inicia (no login).
    #[serde(default)]
    pub every_minutes: u32,
    #[serde(default)]
    pub shuffle: bool,
}

impl Playlist {
    /// Um arquivo só, igual nos dois temas.
    pub fn single(path: PathBuf) -> Self {
        Self {
            items: vec![Item { path, dark: None }],
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// O arquivo do tema claro, ou dos dois temas quando não há `dark`.
    pub path: PathBuf,
    #[serde(default)]
    pub dark: Option<PathBuf>,
}

impl Item {
    /// O arquivo que vale para o tema.
    pub fn for_theme(&self, theme: Theme) -> &std::path::Path {
        match (theme, &self.dark) {
            (Theme::Dark, Some(dark)) => dark,
            _ => &self.path,
        }
    }
}

/// Mensagem do daemon para o cliente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Welcome {
        protocol: u32,
        daemon_version: String,
    },
    /// O daemon não fala a versão do cliente; a conexão é fechada em seguida.
    Rejected {
        protocol: u32,
        error: ErrorCode,
    },
    Response {
        id: u64,
        outcome: Outcome,
    },
    Event {
        event: Event,
    },
}

/// Resultado de um pedido.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok(Reply),
    Error(ErrorCode),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Reply {
    Done,
    Status(Status),
    Policy(Policy),
    Usage(Usage),
}

/// Estado do player, como a interface mostra.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub state: PlayerState,
    /// Arquivo carregado, se houver.
    pub path: Option<PathBuf>,
    /// Por que está pausado, quando `state` é `paused`.
    pub pause_reason: Option<PauseReason>,
    /// Decodificação por hardware em uso, como o mpv informa (`vaapi`, `d3d11va`, `no`).
    pub hwdec: Option<String>,
    /// O tema que vale agora, pela agenda das preferências.
    #[serde(default)]
    pub theme: Theme,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerState {
    #[default]
    Stopped,
    Playing,
    Paused,
}

/// Motivo de uma pausa. Cada um tem texto em `pause_reason.<código>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseReason {
    User,
    Fullscreen,
    Battery,
    Locked,
}

impl PauseReason {
    /// O código como vai no protocolo e no catálogo (`pause_reason.<código>`).
    pub fn code(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Fullscreen => "fullscreen",
            Self::Battery => "battery",
            Self::Locked => "locked",
        }
    }
}

/// Preferências das pausas automáticas, salvas pelo daemon. Um campo que
/// falta no arquivo ou na mensagem vale o padrão, para versões novas
/// acrescentarem preferências sem quebrar as antigas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub pause_on_fullscreen: bool,
    pub pause_on_battery: bool,
    pub pause_on_lock: bool,
    /// Depois de 5 minutos pausado, troca o vídeo pela última imagem e
    /// libera o decodificador e a memória de vídeo.
    pub release_when_paused: bool,
    /// Quando usar o arquivo do tema claro ou do escuro.
    pub schedule: Schedule,
    /// Toca o áudio do vídeo. Desligado, o decodificador de áudio nem abre.
    pub sound: bool,
    /// Mostra os ícones da área de trabalho com o wallpaper ativo. Desligado,
    /// o daemon os esconde enquanto toca e os devolve ao parar.
    pub show_desktop_icons: bool,
}

impl Default for Settings {
    /// Tudo ligado: o carro-chefe é o baixo consumo.
    fn default() -> Self {
        Self {
            pause_on_fullscreen: true,
            pause_on_battery: true,
            pause_on_lock: true,
            release_when_paused: true,
            schedule: Schedule::System,
            sound: false,
            show_desktop_icons: true,
        }
    }
}

/// O que o daemon detecta do sistema agora. `None` quando o sinal não pode
/// ser detectado nesta sessão (sem bateria, compositor sem o protocolo...).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conditions {
    /// Algum app em tela cheia cobre o papel de parede.
    pub fullscreen: Option<bool>,
    pub on_battery: Option<bool>,
    pub screen_locked: Option<bool>,
    /// O sistema está no tema escuro.
    pub dark: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub settings: Settings,
    pub conditions: Conditions,
}

/// Uso de recursos do processo do daemon. `None` quando o sistema não
/// informa. Porcentagens vão em milésimos para o protocolo não ter números
/// de ponto flutuante: 25 é 2,5%.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// CPU desde a leitura anterior, em milésimos de um núcleo.
    pub cpu_permille: Option<u32>,
    /// Fração do tempo em que a GPU trabalhou para o daemon, em milésimos.
    pub gpu_permille: Option<u32>,
    /// Memória residente do processo.
    pub memory_bytes: Option<u64>,
    /// Memória que a GPU reservou para o daemon (na integrada, sai da RAM).
    pub gpu_memory_bytes: Option<u64>,
    /// Quadros por segundo que o vídeo está mostrando agora.
    pub fps: Option<u32>,
    /// O decodificador foi liberado depois de muito tempo em pausa.
    pub released: bool,
}

/// Erro com código estável. Cada um tem texto em `error.<código>`, exceto
/// `incompatible`, que a interface mostra como `daemon.incompatible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    #[error("arquivo não encontrado")]
    FileNotFound,
    #[error("formato não suportado")]
    UnsupportedFormat,
    #[error("superfície de papel de parede indisponível nesta sessão")]
    SurfaceUnavailable,
    #[error("versão de protocolo incompatível")]
    Incompatible,
    #[error("erro interno do daemon")]
    Internal,
}

impl ErrorCode {
    /// O código como vai no protocolo e no catálogo (`error.<código>`).
    pub fn code(self) -> &'static str {
        match self {
            Self::FileNotFound => "file_not_found",
            Self::UnsupportedFormat => "unsupported_format",
            Self::SurfaceUnavailable => "surface_unavailable",
            Self::Incompatible => "incompatible",
            Self::Internal => "internal",
        }
    }
}

/// Aviso espontâneo do daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// O estado mudou: tocando, pausado (com motivo) ou parado.
    StatusChanged { status: Status },
    /// A reprodução falhou depois de aceita, como um arquivo corrompido no meio.
    PlaybackFailed { error: ErrorCode },
    /// Mudaram as preferências ou o que o daemon detecta do sistema.
    PolicyChanged { policy: Policy },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip<T>(value: &T) -> String
    where
        T: Serialize + for<'de> Deserialize<'de> + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serializa");
        let back: T = serde_json::from_str(&json).expect("desserializa");
        assert_eq!(&back, value);
        json
    }

    #[test]
    fn formato_do_pedido_fica_estavel() {
        let json = roundtrip(&ClientMessage::Request {
            id: 7,
            command: Command::Play {
                path: "/videos/praia.mp4".into(),
            },
        });
        assert_eq!(
            json,
            r#"{"type":"request","id":7,"command":{"name":"play","path":"/videos/praia.mp4"}}"#
        );
        assert_eq!(
            roundtrip(&ClientMessage::Hello { protocol: 1 }),
            r#"{"type":"hello","protocol":1}"#
        );
    }

    #[test]
    fn erros_e_motivos_viram_as_chaves_do_catalogo() {
        let json = roundtrip(&ServerMessage::Response {
            id: 3,
            outcome: Outcome::Error(ErrorCode::FileNotFound),
        });
        assert_eq!(
            json,
            r#"{"type":"response","id":3,"outcome":{"error":"file_not_found"}}"#
        );
        assert_eq!(
            serde_json::to_string(&PauseReason::Fullscreen).unwrap(),
            r#""fullscreen""#
        );
    }

    #[test]
    fn code_bate_com_o_serde() {
        use ErrorCode::*;
        for code in [
            FileNotFound,
            UnsupportedFormat,
            SurfaceUnavailable,
            Incompatible,
            Internal,
        ] {
            assert_eq!(
                serde_json::to_string(&code).unwrap(),
                format!("\"{}\"", code.code())
            );
        }
        use PauseReason::*;
        for reason in [User, Fullscreen, Battery, Locked] {
            assert_eq!(
                serde_json::to_string(&reason).unwrap(),
                format!("\"{}\"", reason.code())
            );
        }
    }

    #[test]
    fn preferencia_que_falta_vale_o_padrao() {
        let settings: Settings = serde_json::from_str(r#"{"pause_on_battery":false}"#).unwrap();
        assert_eq!(
            settings,
            Settings {
                pause_on_battery: false,
                ..Settings::default()
            }
        );
    }

    #[test]
    fn item_escolhe_o_arquivo_do_tema() {
        let item = Item {
            path: "/claro.png".into(),
            dark: Some("/escuro.mp4".into()),
        };
        assert_eq!(
            item.for_theme(Theme::Light),
            std::path::Path::new("/claro.png")
        );
        assert_eq!(
            item.for_theme(Theme::Dark),
            std::path::Path::new("/escuro.mp4")
        );
        let only = Playlist::single("/so.mp4".into());
        assert_eq!(
            only.items[0].for_theme(Theme::Dark),
            std::path::Path::new("/so.mp4")
        );
        let list: Playlist = serde_json::from_str(r#"{"items":[{"path":"/a.mp4"}]}"#).unwrap();
        assert_eq!(list, Playlist::single("/a.mp4".into()));
    }

    #[test]
    fn status_e_eventos_vao_e_voltam() {
        let status = Status {
            state: PlayerState::Paused,
            path: Some("/videos/praia.mp4".into()),
            pause_reason: Some(PauseReason::Battery),
            hwdec: Some("vaapi".into()),
            theme: Theme::Dark,
        };
        roundtrip(&ServerMessage::Response {
            id: 1,
            outcome: Outcome::Ok(Reply::Status(status.clone())),
        });
        roundtrip(&ServerMessage::Event {
            event: Event::StatusChanged { status },
        });
        roundtrip(&ServerMessage::Event {
            event: Event::PlaybackFailed {
                error: ErrorCode::UnsupportedFormat,
            },
        });
    }
}
