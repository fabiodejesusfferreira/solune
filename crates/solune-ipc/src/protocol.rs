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

/// Versão do protocolo. Muda quando uma mensagem muda de forma
/// incompatível; cliente e daemon de versões diferentes não conversam.
pub const PROTOCOL_VERSION: u32 = 1;

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
    /// Pausa por pedido do usuário.
    Pause,
    /// Retoma depois de uma pausa do usuário.
    Resume,
    /// Para e devolve a tela ao papel de parede do sistema.
    Stop,
    Status,
    /// Encerra o daemon.
    Shutdown,
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
    fn status_e_eventos_vao_e_voltam() {
        let status = Status {
            state: PlayerState::Paused,
            path: Some("/videos/praia.mp4".into()),
            pause_reason: Some(PauseReason::Battery),
            hwdec: Some("vaapi".into()),
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
