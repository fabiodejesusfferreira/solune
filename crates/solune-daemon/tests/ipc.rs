//! Teste de ponta a ponta do daemon sem tela (`--headless`, `vo=null`):
//! sobe o binário, conversa pelo socket e confere o estado.
//!
//! Precisa do libmpv e de um vídeo curto em `SOLUNE_TEST_VIDEO`, por isso
//! fica fora do `cargo test` comum. No CI:
//! `cargo test -p solune-daemon -- --ignored`.

use std::path::PathBuf;
use std::process::{Child, Command as Process, Stdio};
use std::time::{Duration, Instant};

use solune_ipc::transport::{Address, Client, ClientError, SOCKET_ENV};
use solune_ipc::{Command, ErrorCode, Event, PauseReason, PlayerState, Reply, Status};

struct Daemon {
    child: Child,
    address: Address,
    _dir: tempfile::TempDir,
}

impl Daemon {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("daemon.sock");
        let child = Process::new(env!("CARGO_BIN_EXE_solune-daemon"))
            .args(["--headless", "--hwdec", "no"])
            .env(SOCKET_ENV, &socket)
            .env("SOLUNE_CONFIG_DIR", dir.path().join("config"))
            .stdout(Stdio::null())
            .spawn()
            .expect("subir o daemon");
        Self {
            child,
            address: Address::Path(socket),
            _dir: dir,
        }
    }

    async fn connect(&self) -> Client {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match Client::connect(&self.address).await {
                Ok(client) => return client,
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => panic!("o daemon não respondeu: {e}"),
            }
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn video() -> PathBuf {
    std::env::var_os("SOLUNE_TEST_VIDEO")
        .expect("defina SOLUNE_TEST_VIDEO com um vídeo curto")
        .into()
}

async fn status(client: &Client) -> Status {
    match client.request(Command::Status).await.unwrap() {
        Reply::Status(status) => status,
        other => panic!("esperava status, veio {other:?}"),
    }
}

#[tokio::test]
#[ignore = "precisa do libmpv e de SOLUNE_TEST_VIDEO"]
async fn toca_pausa_para_e_encerra() {
    let mut daemon = Daemon::start();
    let client = daemon.connect().await;
    let mut events = client.subscribe();

    assert_eq!(status(&client).await.state, PlayerState::Stopped);

    let video = video();
    client
        .request(Command::Play {
            path: video.clone(),
        })
        .await
        .unwrap();
    let playing = status(&client).await;
    assert_eq!(playing.state, PlayerState::Playing);
    assert_eq!(playing.path.as_deref(), Some(video.as_path()));

    // O primeiro quadro traz o hwdec em uso (aqui, "no").
    let hwdec = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Event::StatusChanged { status } = events.recv().await.unwrap()
                && status.hwdec.is_some()
            {
                return status.hwdec;
            }
        }
    })
    .await
    .expect("o daemon não avisou o hwdec");
    assert_eq!(hwdec.as_deref(), Some("no"));

    client.request(Command::Pause).await.unwrap();
    let paused = status(&client).await;
    assert_eq!(paused.state, PlayerState::Paused);
    assert_eq!(paused.pause_reason, Some(PauseReason::User));

    client.request(Command::Resume).await.unwrap();
    assert_eq!(status(&client).await.state, PlayerState::Playing);

    client.request(Command::Stop).await.unwrap();
    assert_eq!(status(&client).await, Status::default());

    let missing = client
        .request(Command::Play {
            path: "/nao/existe.mp4".into(),
        })
        .await
        .unwrap_err();
    assert!(matches!(
        missing,
        ClientError::Daemon(ErrorCode::FileNotFound)
    ));

    client.request(Command::Shutdown).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while daemon.child.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "o daemon não encerrou");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(daemon.child.wait().unwrap().success());
}

#[tokio::test]
#[ignore = "precisa do libmpv"]
async fn arquivo_que_nao_e_video_falha_com_codigo() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let mut events = client.subscribe();

    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("lixo.mp4");
    std::fs::write(&junk, b"isto nao e um video").unwrap();
    client.request(Command::Play { path: junk }).await.unwrap();

    let error = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Event::PlaybackFailed { error } = events.recv().await.unwrap() {
                return error;
            }
        }
    })
    .await
    .expect("o daemon não avisou a falha");
    assert_eq!(error, ErrorCode::UnsupportedFormat);
    assert_eq!(status(&client).await.state, PlayerState::Stopped);
}
