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
use solune_ipc::{Command, Event, Playlist, Policy, Reply, Settings, Status, Usage};
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
    /// Já tentou trocar um daemon de versão antiga nesta execução do app.
    replaced: AtomicBool,
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
        _ => Err(ClientError::Unexpected.into()),
    }
}

#[tauri::command]
pub async fn daemon_policy(
    app: AppHandle,
    daemon: State<'_, Daemon>,
) -> Result<Policy, CommandError> {
    match daemon.request(&app, Command::GetPolicy).await? {
        Reply::Policy(policy) => Ok(policy),
        _ => Err(ClientError::Unexpected.into()),
    }
}

#[tauri::command]
pub async fn daemon_set_settings(
    app: AppHandle,
    daemon: State<'_, Daemon>,
    settings: Settings,
) -> Result<(), CommandError> {
    daemon.done(&app, Command::SetSettings { settings }).await
}

#[tauri::command]
pub async fn daemon_usage(
    app: AppHandle,
    daemon: State<'_, Daemon>,
) -> Result<Usage, CommandError> {
    match daemon.request(&app, Command::Usage).await? {
        Reply::Usage(usage) => Ok(usage),
        _ => Err(ClientError::Unexpected.into()),
    }
}

#[tauri::command]
pub async fn daemon_play_list(
    app: AppHandle,
    daemon: State<'_, Daemon>,
    list: Playlist,
) -> Result<(), CommandError> {
    daemon.done(&app, Command::PlayList { list }).await
}

#[tauri::command]
pub async fn daemon_next(app: AppHandle, daemon: State<'_, Daemon>) -> Result<(), CommandError> {
    daemon.done(&app, Command::Next).await
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
    /// por exemplo), `client` abre outra e o pedido se repete uma vez.
    pub(crate) async fn request(
        &self,
        app: &AppHandle,
        command: Command,
    ) -> Result<Reply, ClientError> {
        let client = self.client(app).await?;
        match client.request(command.clone()).await {
            Err(ClientError::Disconnected) => self.client(app).await?.request(command).await,
            other => other,
        }
    }

    /// Conecta (e inicia o daemon, se preciso) sem esperar a interface: ao
    /// abrir o app no login, o wallpaper volta antes da janela aparecer.
    pub async fn ensure_running(&self, app: &AppHandle) {
        if let Err(e) = self.client(app).await {
            tracing::warn!("o daemon não iniciou: {e}");
            return;
        }
        // A bandeja começa com o estado atual; depois, segue os eventos.
        if let Ok(Reply::Status(status)) = self.request(app, Command::Status).await {
            crate::tray::show_status(app, &status);
        }
        if let Ok(Reply::Policy(policy)) = self.request(app, Command::GetPolicy).await {
            crate::tray::show_policy(app, &policy);
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
        // Só uma conexão que caiu é trocada. Vários pedidos que falham juntos
        // reaproveitam a que o primeiro abriu, em vez de abrir uma cada.
        if let Some(client) = slot.as_ref().filter(|client| !client.is_closed()) {
            return Ok(client.clone());
        }
        let client = connect(&self.replaced).await?;
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

/// Conecta ao daemon, iniciando o daemon se ele não estiver rodando. Um
/// daemon de versão antiga, que sobrou de antes de uma atualização, é
/// encerrado uma vez por execução do app para dar lugar ao desta versão.
async fn connect(replaced: &AtomicBool) -> Result<Client, ClientError> {
    let address = Address::default_for_user()?;
    match Client::connect(&address).await {
        Err(ClientError::NotRunning { .. }) => {}
        Err(ClientError::Incompatible { daemon }) => {
            if replaced.swap(true, Ordering::SeqCst) || !stop_old(&address).await {
                return Err(ClientError::Incompatible { daemon });
            }
        }
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

/// Pede que o daemon que ocupa o socket saia, achando o processo pelo
/// próprio socket: vale para qualquer versão do protocolo, que o daemon
/// antigo recusaria no `hello`. `true` quando o socket ficou livre.
#[cfg(unix)]
async fn stop_old(address: &Address) -> bool {
    let Some(pid) = socket_owner(address).await else {
        return false;
    };
    tracing::info!("encerrando o daemon de versão antiga (pid {pid})");
    // SAFETY: kill só manda um sinal ao pid, que não é este processo nem o init.
    if unsafe { libc::kill(pid, libc::SIGTERM) } != 0 {
        return false;
    }
    let deadline = tokio::time::Instant::now() + SHUTDOWN_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(100)).await;
        if matches!(
            Client::connect(address).await,
            Err(ClientError::NotRunning { .. })
        ) {
            return true;
        }
    }
    false
}

#[cfg(not(unix))]
async fn stop_old(_address: &Address) -> bool {
    false
}

/// O processo que escuta no socket. Nunca este próprio processo nem o init.
#[cfg(unix)]
async fn socket_owner(address: &Address) -> Option<libc::pid_t> {
    let Address::Path(path) = address;
    let stream = tokio::net::UnixStream::connect(path).await.ok()?;
    let pid = stream.peer_cred().ok()?.pid()?;
    (pid > 1 && pid.unsigned_abs() != std::process::id()).then_some(pid)
}

/// Repassa os eventos do daemon à interface até a conexão cair.
async fn forward_events(app: AppHandle, client: Client) {
    let mut events = client.subscribe();
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => {
                    match &event {
                        Event::StatusChanged { status } => crate::tray::show_status(&app, status),
                        Event::PolicyChanged { policy } => crate::tray::show_policy(&app, policy),
                        Event::PlaybackFailed { .. } => {}
                    }
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

/// Para onde vai o log do daemon. No Linux ele herda o stderr do app: no dev,
/// o mesmo terminal; instalado, o destino que a sessão deu ao app. Sem isso,
/// um daemon que morre ao iniciar deixa só o aviso de "desligado".
fn daemon_log() -> Stdio {
    #[cfg(unix)]
    {
        Stdio::inherit()
    }
    // shortcut: o log do Windows cresce sem limite; trocar por rotação quando
    // o daemon rodar lá.
    #[cfg(not(unix))]
    {
        log_file().map_or_else(Stdio::null, Stdio::from)
    }
}

#[cfg(not(unix))]
fn log_file() -> Option<std::fs::File> {
    let dir = PathBuf::from(std::env::var_os("APPDATA")?).join("Solune");
    std::fs::create_dir_all(&dir).ok()?;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("daemon.log"))
        .ok()
}

/// Inicia o daemon desligado do app: ele segue rodando quando o app sai.
fn spawn_daemon() -> io::Result<()> {
    let path = daemon_path()?;
    tracing::info!("iniciando o daemon: {}", path.display());
    let mut command = std::process::Command::new(&path);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(daemon_log());
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

    #[cfg(unix)]
    #[tokio::test]
    async fn nunca_mata_o_proprio_processo() {
        use solune_ipc::transport::Listener;

        let dir = tempfile::tempdir().unwrap();
        let address = Address::Path(dir.path().join("daemon.sock"));
        let _listener = Listener::bind(&address).unwrap();
        // O socket é deste mesmo processo, então não há daemon antigo a encerrar.
        assert_eq!(socket_owner(&address).await, None);
        assert!(!stop_old(&address).await);
    }

    /// Faz o papel de um daemon de versão antiga: escuta no socket e não
    /// responde a ninguém. Só age quando `encerra_o_daemon_antigo...` roda
    /// este mesmo binário de teste como processo filho; sozinho, não faz nada.
    #[cfg(unix)]
    #[test]
    fn ocupa_o_socket_sem_responder() {
        if let Some(path) = std::env::var_os("SOLUNE_TEST_OCUPA_SOCKET") {
            let _listener = std::os::unix::net::UnixListener::bind(path).unwrap();
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn encerra_o_daemon_antigo_que_ocupa_o_socket() {
        use std::os::unix::process::ExitStatusExt;

        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("daemon.sock");
        let mut old = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "daemon::tests::ocupa_o_socket_sem_responder",
                "--exact",
                "--nocapture",
            ])
            .env("SOLUNE_TEST_OCUPA_SOCKET", &socket)
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..100 {
            if socket.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        assert!(socket.exists(), "o processo filho não abriu o socket");

        let address = Address::Path(socket);
        assert_eq!(socket_owner(&address).await, Some(old.id() as libc::pid_t));
        assert!(stop_old(&address).await);
        // Terminou por SIGTERM, sem passar pelo fim normal do teste.
        assert_eq!(old.wait().unwrap().signal(), Some(libc::SIGTERM));
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
