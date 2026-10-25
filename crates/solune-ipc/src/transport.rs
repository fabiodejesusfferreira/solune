//! Socket local entre o app e o daemon.
//!
//! No Linux é um Unix socket em `$XDG_RUNTIME_DIR/solune/daemon.sock`,
//! pasta que só o usuário acessa. No Windows é um named pipe com o nome do
//! usuário. `SOLUNE_SOCKET` troca o caminho, para testes e desenvolvimento.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use interprocess::local_socket::tokio::{Listener as RawListener, Stream};
use interprocess::local_socket::traits::tokio::{Listener as _, Stream as _};
use interprocess::local_socket::{GenericFilePath, ListenerOptions, Name, ToFsName};
use tokio::io::{BufReader, ReadHalf, WriteHalf};
use tokio::sync::{broadcast, mpsc, oneshot};

use crate::codec::{CodecError, read_message, write_message};
use crate::protocol::{
    ClientMessage, Command, ErrorCode, Event, Outcome, PROTOCOL_VERSION, Reply, ServerMessage,
};

/// Variável de ambiente que troca o endereço do socket.
pub const SOCKET_ENV: &str = "SOLUNE_SOCKET";

/// Quanto o cliente espera pelo `welcome` antes de desistir.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);

/// Endereço do socket do daemon para o usuário atual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    /// Caminho de um Unix socket (ou de um named pipe, quando começa com
    /// `\\.\pipe\` no Windows).
    Path(PathBuf),
    /// Named pipe do Windows, pelo nome.
    #[cfg(windows)]
    Pipe(String),
}

impl Address {
    /// O endereço padrão, respeitando `SOLUNE_SOCKET`.
    pub fn default_for_user() -> io::Result<Self> {
        if let Some(path) = std::env::var_os(SOCKET_ENV) {
            return Ok(Self::Path(path.into()));
        }
        Self::platform_default()
    }

    #[cfg(unix)]
    fn platform_default() -> io::Result<Self> {
        let runtime = std::env::var_os("XDG_RUNTIME_DIR").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "XDG_RUNTIME_DIR não está definido; defina SOLUNE_SOCKET",
            )
        })?;
        let mut path = PathBuf::from(runtime);
        path.push("solune");
        path.push("daemon.sock");
        Ok(Self::Path(path))
    }

    #[cfg(windows)]
    fn platform_default() -> io::Result<Self> {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
        Ok(Self::Pipe(format!("solune-daemon-{user}")))
    }

    fn name(&self) -> io::Result<Name<'_>> {
        match self {
            Self::Path(path) => path.as_os_str().to_fs_name::<GenericFilePath>(),
            #[cfg(windows)]
            Self::Pipe(pipe) => {
                use interprocess::local_socket::{GenericNamespaced, ToNsName};
                pipe.as_str().to_ns_name::<GenericNamespaced>()
            }
        }
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(path) => write!(f, "{}", path.display()),
            #[cfg(windows)]
            Self::Pipe(pipe) => write!(f, r"\\.\pipe\{pipe}"),
        }
    }
}

impl From<OsString> for Address {
    fn from(value: OsString) -> Self {
        Self::Path(value.into())
    }
}

// ---------------------------------------------------------------- servidor

/// Socket do daemon, à espera de clientes.
#[derive(Debug)]
pub struct Listener {
    inner: RawListener,
}

impl Listener {
    /// Abre o socket. No Linux cria a pasta com permissão só do usuário e
    /// recupera um socket que sobrou de um daemon que caiu.
    pub fn bind(address: &Address) -> io::Result<Self> {
        #[cfg(unix)]
        if let Address::Path(path) = address
            && let Some(dir) = path.parent()
        {
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?;
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        }
        let inner = ListenerOptions::new()
            .name(address.name()?)
            .reclaim_name(true)
            .try_overwrite(true)
            .create_tokio()?;
        Ok(Self { inner })
    }

    /// Aceita o próximo cliente e faz o handshake. Um cliente de outra
    /// versão recebe `rejected` e a conexão volta como erro.
    pub async fn accept(&self, daemon_version: &str) -> Result<ServerConnection, ServerError> {
        let stream = self.inner.accept().await?;
        let (read, mut write) = tokio::io::split(stream);
        let mut read = BufReader::new(read);
        let hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_message(&mut read))
            .await
            .map_err(|_| ServerError::HandshakeTimeout)??;
        match hello {
            Some(ClientMessage::Hello { protocol }) if protocol == PROTOCOL_VERSION => {
                let welcome = ServerMessage::Welcome {
                    protocol: PROTOCOL_VERSION,
                    daemon_version: daemon_version.to_owned(),
                };
                write_message(&mut write, &welcome).await?;
                Ok(ServerConnection { read, write })
            }
            Some(ClientMessage::Hello { protocol }) => {
                let rejected = ServerMessage::Rejected {
                    protocol: PROTOCOL_VERSION,
                    error: ErrorCode::Incompatible,
                };
                write_message(&mut write, &rejected).await?;
                Err(ServerError::Incompatible { client: protocol })
            }
            Some(ClientMessage::Request { .. }) | None => Err(ServerError::NoHello),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error("o cliente não mandou hello a tempo")]
    HandshakeTimeout,
    #[error("o cliente não começou com hello")]
    NoHello,
    #[error("o cliente fala o protocolo {client}, o daemon fala o {PROTOCOL_VERSION}")]
    Incompatible { client: u32 },
}

/// Conexão de um cliente já apresentado.
#[derive(Debug)]
pub struct ServerConnection {
    read: BufReader<ReadHalf<Stream>>,
    write: WriteHalf<Stream>,
}

impl ServerConnection {
    /// Separa leitura e escrita, para responder e mandar eventos de tarefas
    /// diferentes.
    pub fn into_split(self) -> (ServerReader, ServerWriter) {
        (
            ServerReader { read: self.read },
            ServerWriter { write: self.write },
        )
    }
}

#[derive(Debug)]
pub struct ServerReader {
    read: BufReader<ReadHalf<Stream>>,
}

impl ServerReader {
    /// O próximo pedido do cliente. `None` quando ele desconecta.
    pub async fn next_request(&mut self) -> Result<Option<(u64, Command)>, CodecError> {
        loop {
            match read_message(&mut self.read).await? {
                Some(ClientMessage::Request { id, command }) => return Ok(Some((id, command))),
                // Um hello repetido não muda nada.
                Some(ClientMessage::Hello { .. }) => {}
                None => return Ok(None),
            }
        }
    }
}

#[derive(Debug)]
pub struct ServerWriter {
    write: WriteHalf<Stream>,
}

impl ServerWriter {
    pub async fn send(&mut self, message: &ServerMessage) -> Result<(), CodecError> {
        write_message(&mut self.write, message).await
    }
}

// ----------------------------------------------------------------- cliente

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("o daemon não está rodando ({address}): {source}")]
    NotRunning { address: String, source: io::Error },
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Codec(#[from] CodecError),
    #[error("o daemon fala o protocolo {daemon}, o app fala o {PROTOCOL_VERSION}")]
    Incompatible { daemon: u32 },
    #[error("o daemon não respondeu ao hello a tempo")]
    HandshakeTimeout,
    #[error("resposta inesperada do daemon")]
    Unexpected,
    #[error("a conexão com o daemon caiu")]
    Disconnected,
    #[error("o daemon recusou o pedido: {0}")]
    Daemon(ErrorCode),
}

type Pending = oneshot::Sender<Outcome>;

/// Cliente do daemon: pedidos com resposta e assinatura de eventos.
///
/// Uma tarefa em segundo plano cuida do socket; quando a conexão cai, os
/// pedidos em andamento falham com `Disconnected`.
#[derive(Debug, Clone)]
pub struct Client {
    requests: mpsc::Sender<(Command, Pending)>,
    events: broadcast::Sender<Event>,
    daemon_version: String,
}

impl Client {
    /// Conecta ao daemon e faz o handshake. Precisa de um runtime tokio.
    pub async fn connect(address: &Address) -> Result<Self, ClientError> {
        let stream =
            Stream::connect(address.name()?)
                .await
                .map_err(|source| ClientError::NotRunning {
                    address: address.to_string(),
                    source,
                })?;
        let (read, mut write) = tokio::io::split(stream);
        let mut read = BufReader::new(read);

        write_message(
            &mut write,
            &ClientMessage::Hello {
                protocol: PROTOCOL_VERSION,
            },
        )
        .await?;
        let reply = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_message(&mut read))
            .await
            .map_err(|_| ClientError::HandshakeTimeout)??;
        let daemon_version = match reply {
            Some(ServerMessage::Welcome {
                protocol: PROTOCOL_VERSION,
                daemon_version,
            }) => daemon_version,
            Some(
                ServerMessage::Welcome { protocol, .. } | ServerMessage::Rejected { protocol, .. },
            ) => {
                return Err(ClientError::Incompatible { daemon: protocol });
            }
            Some(_) => return Err(ClientError::Unexpected),
            None => return Err(ClientError::Disconnected),
        };

        let (requests, inbox) = mpsc::channel(32);
        let (events, _) = broadcast::channel(64);
        tokio::spawn(run_client(read, write, inbox, events.clone()));
        Ok(Self {
            requests,
            events,
            daemon_version,
        })
    }

    pub fn daemon_version(&self) -> &str {
        &self.daemon_version
    }

    /// Manda um pedido e espera a resposta.
    pub async fn request(&self, command: Command) -> Result<Reply, ClientError> {
        let (tx, rx) = oneshot::channel();
        self.requests
            .send((command, tx))
            .await
            .map_err(|_| ClientError::Disconnected)?;
        match rx.await.map_err(|_| ClientError::Disconnected)? {
            Outcome::Ok(reply) => Ok(reply),
            Outcome::Error(code) => Err(ClientError::Daemon(code)),
        }
    }

    /// Recebe os eventos do daemon a partir de agora.
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
}

async fn run_client(
    mut read: BufReader<ReadHalf<Stream>>,
    mut write: WriteHalf<Stream>,
    mut inbox: mpsc::Receiver<(Command, Pending)>,
    events: broadcast::Sender<Event>,
) {
    let mut pending: HashMap<u64, Pending> = HashMap::new();
    let mut next_id: u64 = 1;
    loop {
        tokio::select! {
            request = inbox.recv() => {
                let Some((command, reply)) = request else { break };
                let id = next_id;
                next_id += 1;
                let message = ClientMessage::Request { id, command };
                if write_message(&mut write, &message).await.is_err() {
                    break;
                }
                pending.insert(id, reply);
            }
            message = read_message::<ServerMessage, _>(&mut read) => {
                match message {
                    Ok(Some(ServerMessage::Response { id, outcome })) => {
                        if let Some(reply) = pending.remove(&id) {
                            let _ = reply.send(outcome);
                        }
                    }
                    Ok(Some(ServerMessage::Event { event })) => {
                        // Sem assinantes, o evento simplesmente se perde.
                        let _ = events.send(event);
                    }
                    Ok(Some(ServerMessage::Welcome { .. } | ServerMessage::Rejected { .. })) => {}
                    Ok(None) | Err(_) => break,
                }
            }
        }
    }
    // Os pedidos pendentes caem com Disconnected quando os senders saem.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{PlayerState, Status};

    fn temp_address(dir: &tempfile::TempDir) -> Address {
        Address::Path(dir.path().join("sub").join("daemon.sock"))
    }

    /// Daemon de mentira: responde status e ecoa um evento a cada pedido.
    async fn fake_daemon(listener: Listener) {
        let connection = listener.accept("0.0.0-test").await.expect("handshake");
        let (mut reader, mut writer) = connection.into_split();
        while let Ok(Some((id, command))) = reader.next_request().await {
            let outcome = match command {
                Command::Status => Outcome::Ok(Reply::Status(Status::default())),
                Command::Play { .. } => Outcome::Error(ErrorCode::FileNotFound),
                _ => Outcome::Ok(Reply::Done),
            };
            writer
                .send(&ServerMessage::Event {
                    event: Event::StatusChanged {
                        status: Status {
                            state: PlayerState::Paused,
                            ..Status::default()
                        },
                    },
                })
                .await
                .unwrap();
            writer
                .send(&ServerMessage::Response { id, outcome })
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn pedido_resposta_e_evento_pelo_socket() {
        let dir = tempfile::tempdir().unwrap();
        let address = temp_address(&dir);
        let listener = Listener::bind(&address).unwrap();
        let daemon = tokio::spawn(fake_daemon(listener));

        let client = Client::connect(&address).await.unwrap();
        assert_eq!(client.daemon_version(), "0.0.0-test");
        let mut events = client.subscribe();

        let reply = client.request(Command::Status).await.unwrap();
        assert_eq!(reply, Reply::Status(Status::default()));
        let event = events.recv().await.unwrap();
        assert!(matches!(event, Event::StatusChanged { .. }));

        let err = client
            .request(Command::Play {
                path: "/nao/existe.mp4".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Daemon(ErrorCode::FileNotFound)));

        drop(client);
        daemon.await.unwrap();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn pasta_do_socket_fica_so_com_o_usuario() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let address = temp_address(&dir);
        let _listener = Listener::bind(&address).unwrap();
        let mode = std::fs::metadata(dir.path().join("sub"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
    }

    #[tokio::test]
    async fn daemon_ausente_vira_not_running() {
        let dir = tempfile::tempdir().unwrap();
        let err = Client::connect(&temp_address(&dir)).await.unwrap_err();
        assert!(matches!(err, ClientError::NotRunning { .. }));
    }

    #[tokio::test]
    async fn versao_diferente_e_recusada() {
        let dir = tempfile::tempdir().unwrap();
        let address = temp_address(&dir);
        let listener = Listener::bind(&address).unwrap();
        let daemon = tokio::spawn(async move { listener.accept("x").await });

        let stream = Stream::connect(address.name().unwrap()).await.unwrap();
        let (read, mut write) = tokio::io::split(stream);
        let mut read = BufReader::new(read);
        write_message(&mut write, &ClientMessage::Hello { protocol: 999 })
            .await
            .unwrap();
        let reply: Option<ServerMessage> = read_message(&mut read).await.unwrap();
        assert_eq!(
            reply,
            Some(ServerMessage::Rejected {
                protocol: PROTOCOL_VERSION,
                error: ErrorCode::Incompatible
            })
        );
        assert!(matches!(
            daemon.await.unwrap(),
            Err(ServerError::Incompatible { client: 999 })
        ));
    }
}
