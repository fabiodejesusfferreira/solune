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
use solune_ipc::{
    Command, ErrorCode, Event, Item, PauseReason, PlayerState, Playlist, Reply, Status,
};

struct Daemon {
    child: Child,
    address: Address,
    dir: tempfile::TempDir,
}

/// Sobe um daemon sem tela, com o socket e a configuração na pasta dada.
fn spawn(dir: &std::path::Path) -> Child {
    Process::new(env!("CARGO_BIN_EXE_solune-daemon"))
        .args(["--headless", "--hwdec", "no"])
        .env(SOCKET_ENV, dir.join("daemon.sock"))
        .env("SOLUNE_CONFIG_DIR", dir.join("config"))
        .stdout(Stdio::null())
        .spawn()
        .expect("subir o daemon")
}

impl Daemon {
    fn start() -> Self {
        let dir = tempfile::tempdir().unwrap();
        Self {
            child: spawn(dir.path()),
            address: Address::Path(dir.path().join("daemon.sock")),
            dir,
        }
    }

    /// Derruba o daemon sem avisar, como uma queda, e sobe outro com a mesma
    /// configuração.
    fn crash_and_restart(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.child = spawn(self.dir.path());
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

#[tokio::test]
#[ignore = "precisa do libmpv e de SOLUNE_TEST_VIDEO"]
async fn carrossel_troca_com_next_e_respeita_a_pausa() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let dir = tempfile::tempdir().unwrap();
    let [a, b] = ["a.mp4", "b.mp4"].map(|name| {
        let path = dir.path().join(name);
        std::fs::copy(video(), &path).unwrap();
        path
    });
    let item = |path: &PathBuf| Item {
        path: path.clone(),
        dark: None,
    };
    let list = Playlist {
        items: vec![item(&a), item(&b)],
        every_minutes: 30,
        shuffle: false,
    };
    client.request(Command::PlayList { list }).await.unwrap();
    assert_eq!(status(&client).await.path.as_deref(), Some(a.as_path()));

    client.request(Command::Next).await.unwrap();
    let next = status(&client).await;
    assert_eq!(next.state, PlayerState::Playing);
    assert_eq!(next.path.as_deref(), Some(b.as_path()));

    // Trocar pausado não retoma: a pausa do usuário continua valendo.
    client.request(Command::Pause).await.unwrap();
    client.request(Command::Next).await.unwrap();
    let paused = status(&client).await;
    assert_eq!(paused.state, PlayerState::Paused);
    assert_eq!(paused.path.as_deref(), Some(a.as_path()));
}

/// O que o daemon tem salvo em `state.json`.
fn saved_state(daemon: &Daemon) -> String {
    std::fs::read_to_string(daemon.dir.path().join("config/state.json")).unwrap_or_default()
}

/// Espera o daemon tocar o arquivo.
async fn wait_for_path(client: &Client, path: &std::path::Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while status(client).await.path.as_deref() != Some(path) {
        assert!(
            Instant::now() < deadline,
            "o daemon não chegou a {}",
            path.display()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

fn item(path: &std::path::Path) -> Item {
    Item {
        path: path.to_path_buf(),
        dark: None,
    }
}

/// Copia o vídeo de teste para `dir/<name>`.
fn copy_video(dir: &std::path::Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::copy(video(), &path).unwrap();
    path
}

#[tokio::test]
#[ignore = "precisa do libmpv e de SOLUNE_TEST_VIDEO"]
async fn item_quebrado_nao_derruba_o_carrossel() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;
    let mut events = client.subscribe();
    let dir = tempfile::tempdir().unwrap();
    let junk = dir.path().join("lixo.mp4");
    std::fs::write(&junk, b"isto nao e um video").unwrap();
    let good = copy_video(dir.path(), "bom.mp4");

    let list = Playlist {
        items: vec![item(&junk), item(&good)],
        every_minutes: 30,
        shuffle: false,
    };
    client.request(Command::PlayList { list }).await.unwrap();

    // O arquivo quebrado avisa a falha, e o carrossel segue para o bom.
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
    wait_for_path(&client, &good).await;
    assert_eq!(status(&client).await.state, PlayerState::Playing);
}

#[tokio::test]
#[ignore = "precisa do libmpv e de SOLUNE_TEST_VIDEO"]
async fn lista_toda_quebrada_para_mas_continua_salva() {
    let mut daemon = Daemon::start();
    let client = daemon.connect().await;
    let mut events = client.subscribe();
    let dir = tempfile::tempdir().unwrap();
    let [a, b] = ["a.mp4", "b.mp4"].map(|name| {
        let path = dir.path().join(name);
        std::fs::write(&path, b"isto nao e um video").unwrap();
        path
    });
    let list = Playlist {
        items: vec![item(&a), item(&b)],
        every_minutes: 30,
        shuffle: false,
    };
    client.request(Command::PlayList { list }).await.unwrap();
    // Os dois falham, um depois do outro: o daemon para.
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Event::PlaybackFailed { .. } = events.recv().await.unwrap()
                && status(&client).await.state == PlayerState::Stopped
            {
                return;
            }
        }
    })
    .await
    .expect("o daemon não parou");

    // A lista continua salva: quando os arquivos voltam, o próximo início
    // toca de novo.
    let saved = saved_state(&daemon);
    assert!(
        saved.contains("a.mp4") && saved.contains("b.mp4"),
        "{saved}"
    );
    std::fs::copy(video(), &a).unwrap();
    daemon.crash_and_restart();
    let client = daemon.connect().await;
    // O item salvo é o b, que ainda não toca: o carrossel segue para o a.
    wait_for_path(&client, &a).await;
}

#[tokio::test]
#[ignore = "precisa do libmpv e de SOLUNE_TEST_VIDEO"]
async fn volta_ao_wallpaper_salvo_e_pula_o_arquivo_que_sumiu() {
    let mut daemon = Daemon::start();
    let client = daemon.connect().await;
    let dir = tempfile::tempdir().unwrap();
    let a = copy_video(dir.path(), "a.mp4");
    let b = copy_video(dir.path(), "b.mp4");
    let list = Playlist {
        items: vec![item(&a), item(&b)],
        every_minutes: 30,
        shuffle: false,
    };
    client.request(Command::PlayList { list }).await.unwrap();
    assert_eq!(status(&client).await.path.as_deref(), Some(a.as_path()));

    // Uma queda, e o primeiro item some (um disco que não montou).
    drop(client);
    std::fs::remove_file(&a).unwrap();
    daemon.crash_and_restart();
    let client = daemon.connect().await;
    let back = status(&client).await;
    assert_eq!(back.state, PlayerState::Playing);
    assert_eq!(back.path.as_deref(), Some(b.as_path()));

    // A lista inteira continua salva, com o item que sumiu.
    let saved = saved_state(&daemon);
    assert!(
        saved.contains("a.mp4") && saved.contains("b.mp4"),
        "{saved}"
    );
}

#[tokio::test]
#[ignore = "precisa do libmpv"]
async fn segundo_daemon_sai_sem_mexer_no_primeiro() {
    let daemon = Daemon::start();
    let client = daemon.connect().await;

    let mut second = spawn(daemon.dir.path());
    let deadline = Instant::now() + Duration::from_secs(10);
    while second.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline, "o segundo daemon não saiu");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(second.wait().unwrap().success());
    // O primeiro segue de pé, no mesmo socket.
    assert_eq!(status(&client).await.state, PlayerState::Stopped);
}
