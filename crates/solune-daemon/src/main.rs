//! Daemon do Solune.
//!
//! Sem argumentos de comando, roda o daemon: toca o wallpaper e atende o app
//! pelo socket local. Com um comando, vira um cliente de linha de comando do
//! daemon que já está rodando, útil para testar sem o app:
//!
//! ```text
//! solune-daemon [--headless] [--hwdec MODO]
//! solune-daemon play <arquivo> | pause | resume | stop | status | quit
//! ```

mod mpv;
mod player;
#[cfg(target_os = "linux")]
mod render;
mod server;
mod surface;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use solune_ipc::transport::{Address, Client, ClientError, Listener};
use solune_ipc::{Command, PlayerState, Reply, Status};
use tokio::sync::{Notify, broadcast, mpsc};
use tracing_subscriber::EnvFilter;

use crate::player::{Config, Msg, Output, Player};

const USAGE: &str = "\
uso: solune-daemon [--headless] [--hwdec MODO]
     solune-daemon play <arquivo> | pause | resume | stop | status | quit";

#[derive(Debug)]
enum Mode {
    Serve(Config),
    Client(Command),
}

fn parse_args() -> Result<Mode, String> {
    let mut args = std::env::args().skip(1);
    let mut output = Output::Wallpaper;
    let mut hwdec = String::from("auto-safe");
    let Some(first) = args.next() else {
        return Ok(Mode::Serve(Config { output, hwdec }));
    };
    let command = match first.as_str() {
        "play" => Some(Command::Play {
            path: args.next().ok_or("play precisa de um arquivo")?.into(),
        }),
        "pause" => Some(Command::Pause),
        "resume" => Some(Command::Resume),
        "stop" => Some(Command::Stop),
        "status" => Some(Command::Status),
        "quit" => Some(Command::Shutdown),
        _ => None,
    };
    if let Some(command) = command {
        return match args.next() {
            None => Ok(Mode::Client(command)),
            Some(extra) => Err(format!("argumento inesperado: {extra}\n{USAGE}")),
        };
    }
    let mut next = Some(first);
    while let Some(arg) = next {
        match arg.as_str() {
            "--headless" => output = Output::Headless,
            "--hwdec" => hwdec = args.next().ok_or("--hwdec precisa de um modo")?,
            "-h" | "--help" => return Err(USAGE.into()),
            _ => return Err(format!("argumento inesperado: {arg}\n{USAGE}")),
        }
        next = args.next();
    }
    Ok(Mode::Serve(Config { output, hwdec }))
}

fn main() -> ExitCode {
    let mode = match parse_args() {
        Ok(mode) => mode,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("solune_daemon=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("erro: não foi possível iniciar o runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async {
        match mode {
            Mode::Serve(config) => serve(config).await,
            Mode::Client(command) => client(command).await,
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("erro: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn serve(config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let address = Address::default_for_user()?;
    // Só um daemon por usuário: se outro responde, este sai sem mexer nele.
    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(2), Client::connect(&address)).await
    {
        println!("o daemon já está rodando em {address}");
        return Ok(());
    }
    let listener = Listener::bind(&address)?;
    tracing::info!("ouvindo em {address}");

    let (events, _) = broadcast::channel(64);
    let (inbox_tx, inbox_rx) = mpsc::unbounded_channel();
    let player = Player::new(config, events.clone(), inbox_tx.clone())?;
    let actor = tokio::spawn(player.run(inbox_rx));

    let shutdown = Arc::new(Notify::new());
    let server = tokio::spawn(server::serve(
        listener,
        inbox_tx.clone(),
        events,
        Arc::clone(&shutdown),
    ));

    tokio::select! {
        () = shutdown.notified() => tracing::info!("encerrando a pedido do app"),
        _ = tokio::signal::ctrl_c() => tracing::info!("encerrando por sinal"),
        _ = terminate() => tracing::info!("encerrando por SIGTERM"),
    }
    // O ator sai levando a superfície e o libmpv junto; o socket some com
    // o listener, quando o servidor é abortado.
    server.abort();
    let _ = server.await;
    let _ = inbox_tx.send(Msg::Quit);
    let _ = actor.await;
    Ok(())
}

#[cfg(unix)]
async fn terminate() {
    use tokio::signal::unix::{SignalKind, signal};
    match signal(SignalKind::terminate()) {
        Ok(mut sigterm) => {
            sigterm.recv().await;
        }
        Err(_) => std::future::pending().await,
    }
}

#[cfg(not(unix))]
async fn terminate() {
    std::future::pending().await
}

async fn client(command: Command) -> Result<(), Box<dyn std::error::Error>> {
    let command = match command {
        // O daemon pode estar noutra pasta: o caminho vai absoluto.
        Command::Play { path } => Command::Play {
            path: std::path::absolute(&path).unwrap_or(path),
        },
        other => other,
    };
    let address = Address::default_for_user()?;
    let client = Client::connect(&address).await?;
    match client.request(command).await {
        Ok(Reply::Status(status)) => print_status(&status),
        Ok(Reply::Done) => println!("ok"),
        Err(ClientError::Daemon(code)) => {
            return Err(format!("o daemon recusou: {}", code.code()).into());
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

fn print_status(status: &Status) {
    let state = match status.state {
        PlayerState::Stopped => "parado",
        PlayerState::Playing => "tocando",
        PlayerState::Paused => "pausado",
    };
    println!("estado: {state}");
    if let Some(path) = &status.path {
        println!("arquivo: {}", path.display());
    }
    if let Some(reason) = status.pause_reason {
        println!("motivo da pausa: {}", reason.code());
    }
    if let Some(hwdec) = &status.hwdec {
        println!("hwdec em uso: {hwdec}");
    }
}
