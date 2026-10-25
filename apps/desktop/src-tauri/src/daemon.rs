//! Ponte entre a interface e o daemon.
//!
//! A interface chama os comandos `daemon_*`; cada um vira um pedido pelo
//! socket. Se o daemon não está rodando, o app inicia o daemon e tenta de
//! novo. Os eventos do daemon chegam à interface como `daemon://event`, e a
//! queda da conexão como `daemon://disconnected`.

use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde::Serialize;
use solune_ipc::transport::{Address, Client, ClientError};
use solune_ipc::{Command, Reply, Status};
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{Mutex, broadcast};

/// Evento com um `solune_ipc::Event` dentro.
pub const EVENT: &str = "daemon://event";
/// A conexão com o daemon caiu; o próximo comando reconecta.
pub const DISCONNECTED: &str = "daemon://disconnected";

/// Variável que aponta outro executável do daemon, para desenvolvimento.
const DAEMON_ENV: &str = "SOLUNE_DAEMON";
/// Quanto esperar o daemon recém-iniciado abrir o socket.
const START_TIMEOUT: Duration = Duration::from_secs(5);
/// Quanto esperar o daemon confirmar que vai encerrar.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// A conexão com o daemon, aberta no primeiro comando.
#[derive(Debug, Default)]
pub struct Daemon {
    client: Mutex<Option<Client>>,
    /// O app está saindo: nenhum comando inicia o daemon de novo.
    quitting: AtomicBool,
}

/// Erro como a interface recebe. O código tem texto em `error.<código>`,
/// exceto `offline` e `incompatible`, que ficam em `daemon.*`.
#[derive(Debug, Clone, Serialize)]
pub struct CommandError {
    code: &'static str,
}

impl From<ClientError> for CommandError {
    fn from(error: ClientError) -> Self {
        let code = match &error {
            ClientError::Daemon(code) => code.code(),
            ClientError::Incompatible { .. } => "incompatible",
            ClientError::NotRunning { .. }
            | ClientError::Io(_)
            | ClientError::HandshakeTimeout
            | ClientError::Disconnected => "offline",
            ClientError::Codec(_) | ClientError::Unexpected => "internal",
        };
        tracing::warn!("pedido ao daemon falhou: {error}");
        Self { code }
    }
}

#[tauri::command]
pub async fn daemon_status(
    app: AppHandle,
    daemon: State<'_, Daemon>,
) -> Result<Status, CommandError> {
    match daemon.request(&app, Command::Status).await? {
        Reply::Status(status) => Ok(status),
        Reply::Done => Err(ClientError::Unexpected.into()),
    }
}

#[tauri::command]
pub async fn daemon_play(
    app: AppHandle,
    daemon: State<'_, Daemon>,
    path: PathBuf,
) -> Result<(), CommandError> {
    daemon.done(&app, Command::Play { path }).await
}

#[tauri::command]
pub async fn daemon_pause(app: AppHandle, daemon: State<'_, Daemon>) -> Result<(), CommandError> {
    daemon.done(&app, Command::Pause).await
}

#[tauri::command]
pub async fn daemon_resume(app: AppHandle, daemon: State<'_, Daemon>) -> Result<(), CommandError> {
    daemon.done(&app, Command::Resume).await
}

#[tauri::command]
pub async fn daemon_stop(app: AppHandle, daemon: State<'_, Daemon>) -> Result<(), CommandError> {
    daemon.done(&app, Command::Stop).await
}

impl Daemon {
    async fn done(&self, app: &AppHandle, command: Command) -> Result<(), CommandError> {
        self.request(app, command).await?;
        Ok(())
    }

    /// Manda o pedido. Se a conexão antiga tinha caído (o daemon reiniciou,
    /// por exemplo), reconecta uma vez e repete.
    async fn request(&self, app: &AppHandle, command: Command) -> Result<Reply, ClientError> {
        let client = self.client(app).await?;
        match client.request(command.clone()).await {
            Err(ClientError::Disconnected) => {
                self.client.lock().await.take();
                self.client(app).await?.request(command).await
            }
            other => other,
        }
    }

    /// Encerra o daemon, se ele estiver rodando: o vídeo para e o papel de
    /// parede do sistema volta. Usado pelo Sair da bandeja.
    pub async fn shutdown(&self) {
        self.quitting.store(true, Ordering::SeqCst);
        self.client.lock().await.take();
        let Ok(address) = Address::default_for_user() else {
            return;
        };
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, shutdown_daemon(&address)).await {
            Ok(Ok(_)) => tracing::info!("daemon encerrado"),
            Ok(Err(ClientError::NotRunning { .. })) => {}
            Ok(Err(e)) => tracing::warn!("não consegui encerrar o daemon: {e}"),
            Err(_) => tracing::warn!("o daemon não respondeu ao pedido para encerrar"),
        }
    }

    async fn client(&self, app: &AppHandle) -> Result<Client, ClientError> {
        if self.quitting.load(Ordering::SeqCst) {
            return Err(ClientError::Disconnected);
        }
        let mut slot = self.client.lock().await;
        if let Some(client) = slot.as_ref() {
            return Ok(client.clone());
        }
        let client = connect().await?;
        tauri::async_runtime::spawn(forward_events(app.clone(), client.clone()));
        *slot = Some(client.clone());
        Ok(client)
    }
}

/// Pede ao daemon que encerre, sem iniciar um se ele não estiver rodando.
async fn shutdown_daemon(address: &Address) -> Result<Reply, ClientError> {
    Client::connect(address)
        .await?
        .request(Command::Shutdown)
        .await
}

/// Conecta ao daemon, iniciando o daemon se ele não estiver rodando.
async fn connect() -> Result<Client, ClientError> {
    let address = Address::default_for_user()?;
    match Client::connect(&address).await {
        Err(ClientError::NotRunning { .. }) => {}
        other => return other,
    }
    spawn_daemon()?;
    let deadline = tokio::time::Instant::now() + START_TIMEOUT;
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        match Client::connect(&address).await {
            Err(ClientError::NotRunning { .. }) if tokio::time::Instant::now() < deadline => {}
            other => return other,
        }
    }
}

/// Repassa os eventos do daemon à interface até a conexão cair.
async fn forward_events(app: AppHandle, client: Client) {
    let mut events = client.subscribe();
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    let _ = app.emit(EVENT, event);
                }
                // Só com a interface travada; o próximo status corrige.
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            () = client.closed() => break,
        }
    }
    let _ = app.emit(DISCONNECTED, ());
}

/// O executável do daemon: `SOLUNE_DAEMON`, ou o `solune-daemon` na mesma
/// pasta do app (onde o instalador põe, e onde o cargo deixa no dev).
fn daemon_path() -> io::Result<PathBuf> {
    if let Some(path) = std::env::var_os(DAEMON_ENV) {
        return Ok(path.into());
    }
    let exe = std::env::current_exe()?;
    Ok(exe.with_file_name(format!("solune-daemon{}", std::env::consts::EXE_SUFFIX)))
}

/// Inicia o daemon desligado do app: ele segue rodando quando o app sai.
fn spawn_daemon() -> io::Result<()> {
    let path = daemon_path()?;
    tracing::info!("iniciando o daemon: {}", path.display());
    let mut command = std::process::Command::new(&path);
    command.stdin(Stdio::null()).stdout(Stdio::null());
    // No dev, o log do daemon sai no mesmo terminal do app.
    if !cfg!(debug_assertions) {
        command.stderr(Stdio::null());
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Grupo próprio: o Ctrl+C no terminal do app não derruba o daemon.
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let mut child = command
        .spawn()
        .map_err(|e| io::Error::new(e.kind(), format!("{}: {e}", path.display())))?;
    // Recolhe o processo se ele sair antes do app, para não sobrar zumbi.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use solune_ipc::ErrorCode;

    fn code(error: ClientError) -> &'static str {
        CommandError::from(error).code
    }

    #[test]
    fn erros_viram_os_codigos_do_catalogo() {
        assert_eq!(
            code(ClientError::Daemon(ErrorCode::FileNotFound)),
            "file_not_found"
        );
        assert_eq!(
            code(ClientError::Incompatible { daemon: 9 }),
            "incompatible"
        );
        assert_eq!(code(ClientError::Disconnected), "offline");
        assert_eq!(code(ClientError::Unexpected), "internal");
    }

    #[tokio::test]
    async fn sair_pede_ao_daemon_para_encerrar() {
        use solune_ipc::transport::Listener;
        use solune_ipc::{Outcome, ServerMessage};

        let dir = tempfile::tempdir().unwrap();
        let address = Address::Path(dir.path().join("daemon.sock"));
        let listener = Listener::bind(&address).unwrap();
        let daemon = tokio::spawn(async move {
            let (mut reader, mut writer) = listener.accept("x").await.unwrap().into_split();
            let (id, command) = reader.next_request().await.unwrap().unwrap();
            let outcome = Outcome::Ok(Reply::Done);
            writer
                .send(&ServerMessage::Response { id, outcome })
                .await
                .unwrap();
            command
        });

        shutdown_daemon(&address).await.unwrap();
        assert_eq!(daemon.await.unwrap(), Command::Shutdown);
    }

    #[tokio::test]
    async fn sair_sem_daemon_nao_inicia_um() {
        let dir = tempfile::tempdir().unwrap();
        let address = Address::Path(dir.path().join("daemon.sock"));
        assert!(matches!(
            shutdown_daemon(&address).await,
            Err(ClientError::NotRunning { .. })
        ));
    }

    #[test]
    fn erro_vai_para_a_interface_como_objeto_com_code() {
        let json = serde_json::to_string(&CommandError::from(ClientError::Disconnected)).unwrap();
        assert_eq!(json, r#"{"code":"offline"}"#);
    }
}
