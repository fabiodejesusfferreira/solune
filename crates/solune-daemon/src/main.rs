//! Daemon do Solune.
//!
//! Sem argumentos de comando, roda o daemon: toca o wallpaper e atende o app
//! pelo socket local. Com um comando, vira um cliente de linha de comando do
//! daemon que já está rodando, útil para testar sem o app:
//!
//! ```text
//! solune-daemon [--headless] [--hwdec MODO]
//! solune-daemon play <arquivo> | next | pause | resume | stop | status | quit
//! solune-daemon policy | usage | set <preferência> on|off
//! ```

mod config;
mod icons;
mod mpv;
mod player;
#[cfg(any(target_os = "linux", windows))]
mod render;
mod server;
mod show;
mod signals;
mod surface;
mod usage;

use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use solune_ipc::transport::{Address, Client, ClientError, Listener, lock_instance};
use solune_ipc::{Command, PlayerState, Policy, Reply, Status, Usage};
use tokio::sync::{Notify, broadcast, mpsc};
use tracing_subscriber::EnvFilter;

use crate::config::Store;
use crate::player::{Config, Msg, Output, Player};

/// Quanto esperar o player encerrar (superfície e libmpv) ao sair.
const ACTOR_QUIT_TIMEOUT: Duration = Duration::from_secs(10);

/// Quanto esperar as threads do runtime depois que o trabalho acabou.
const RUNTIME_QUIT_TIMEOUT: Duration = Duration::from_secs(2);

const USAGE: &str = "\
uso: solune-daemon [--headless] [--hwdec MODO]
     solune-daemon play <arquivo> | pause | resume | stop | status | quit
     solune-daemon policy | usage | set <preferência> on|off

preferências: pause_on_fullscreen, pause_on_battery, pause_on_lock, release_when_paused, sound, show_desktop_icons";

#[derive(Debug)]
enum Request {
    Command(Command),
    /// Muda uma preferência: lê as atuais, troca uma e manda de volta.
    Set(String, bool),
}

#[derive(Debug)]
enum Mode {
    Serve(Config),
    Client(Request),
}

fn parse_args() -> Result<Mode, String> {
    let mut args = std::env::args().skip(1);
    let mut output = Output::Wallpaper;
    let mut hwdec = String::from("auto-safe");
    let serve = |output, hwdec| {
        Mode::Serve(Config {
            output,
            hwdec,
            store: Store::for_user(),
            signals: output == Output::Wallpaper,
        })
    };
    let Some(first) = args.next() else {
        return Ok(serve(output, hwdec));
    };
    let command = match first.as_str() {
        "play" => Some(Command::Play {
            path: args.next().ok_or("play precisa de um arquivo")?.into(),
        }),
        "next" => Some(Command::Next),
        "pause" => Some(Command::Pause),
        "resume" => Some(Command::Resume),
        "stop" => Some(Command::Stop),
        "status" => Some(Command::Status),
        "policy" => Some(Command::GetPolicy),
        "usage" => Some(Command::Usage),
        "quit" => Some(Command::Shutdown),
        _ => None,
    };
    let request = match (first.as_str(), command) {
        (_, Some(command)) => Some(Request::Command(command)),
        ("set", None) => {
            let name = args.next().ok_or("set precisa de uma preferência")?;
            let on = match args.next().as_deref() {
                Some("on") => true,
                Some("off") => false,
                _ => return Err(format!("set {name} precisa de on ou off")),
            };
            Some(Request::Set(name, on))
        }
        _ => None,
    };
    if let Some(request) = request {
        return match args.next() {
            None => Ok(Mode::Client(request)),
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
    Ok(serve(output, hwdec))
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
    // Largar o runtime espera as threads dele; uma presa dentro do libmpv ou
    // do driver deixaria o processo vivo para sempre.
    runtime.shutdown_timeout(RUNTIME_QUIT_TIMEOUT);
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
    // Só um daemon por usuário. O lock fica com o processo até o fim e vence
    // a corrida de dois daemons que sobem ao mesmo tempo.
    let Some(_instance) = lock_instance(&address)? else {
        println!("o daemon já está rodando em {address}");
        return Ok(());
    };
    // Um daemon de antes do lock não o tem: se ele responde, este sai sem
    // mexer nele.
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
    // Uma superfície travada no compositor ou no driver não pode prender o
    // daemon: passado o prazo, ele sai assim mesmo.
    if tokio::time::timeout(ACTOR_QUIT_TIMEOUT, actor)
        .await
        .is_err()
    {
        tracing::error!("o player não encerrou a tempo; saindo assim mesmo");
    }
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

async fn client(request: Request) -> Result<(), Box<dyn std::error::Error>> {
    let address = Address::default_for_user()?;
    let client = Client::connect(&address).await?;
    let command = match request {
        Request::Command(command) => command,
        Request::Set(name, on) => {
            let Reply::Policy(policy) = client.request(Command::GetPolicy).await? else {
                return Err("resposta inesperada do daemon".into());
            };
            let mut settings = policy.settings;
            let field = match name.as_str() {
                "pause_on_fullscreen" => &mut settings.pause_on_fullscreen,
                "pause_on_battery" => &mut settings.pause_on_battery,
                "pause_on_lock" => &mut settings.pause_on_lock,
                "release_when_paused" => &mut settings.release_when_paused,
                "sound" => &mut settings.sound,
                "show_desktop_icons" => &mut settings.show_desktop_icons,
                _ => return Err(format!("preferência desconhecida: {name}\n{USAGE}").into()),
            };
            *field = on;
            Command::SetSettings { settings }
        }
    };
    let command = match command {
        // O daemon pode estar noutra pasta: o caminho vai absoluto.
        Command::Play { path } => Command::Play {
            path: std::path::absolute(&path).unwrap_or(path),
        },
        other => other,
    };
    match client.request(command).await {
        Ok(Reply::Status(status)) => print_status(&status),
        Ok(Reply::Policy(policy)) => print_policy(&policy),
        Ok(Reply::Usage(usage)) => print_usage(&usage),
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
    println!("tema: {:?}", status.theme);
}

fn print_policy(policy: &Policy) {
    let s = &policy.settings;
    let on = |v: bool| if v { "sim" } else { "não" };
    println!(
        "pausar com app em tela cheia: {}",
        on(s.pause_on_fullscreen)
    );
    println!("pausar na bateria: {}", on(s.pause_on_battery));
    println!("pausar com a tela bloqueada: {}", on(s.pause_on_lock));
    println!(
        "liberar o decodificador após a pausa longa: {}",
        on(s.release_when_paused)
    );
    println!(
        "mostrar ícones da área de trabalho: {}",
        on(s.show_desktop_icons)
    );
    let detected = |v: Option<bool>| match v {
        None => "não detectado",
        Some(true) => "sim",
        Some(false) => "não",
    };
    let c = &policy.conditions;
    println!("agora, app em tela cheia: {}", detected(c.fullscreen));
    println!("agora, na bateria: {}", detected(c.on_battery));
    println!("agora, tela bloqueada: {}", detected(c.screen_locked));
    println!("agora, sistema no tema escuro: {}", detected(c.dark));
}

fn print_usage(usage: &Usage) {
    let permille = |v: Option<u32>| v.map_or("?".into(), |v| format!("{},{}%", v / 10, v % 10));
    let mib = |v: Option<u64>| v.map_or("?".into(), |v| format!("{:.1} MiB", v as f64 / 1048576.0));
    println!("cpu: {} de um núcleo", permille(usage.cpu_permille));
    println!("gpu: {}", permille(usage.gpu_permille));
    println!("memória: {}", mib(usage.memory_bytes));
    println!("memória de vídeo: {}", mib(usage.gpu_memory_bytes));
    println!(
        "quadros por segundo: {}",
        usage.fps.map_or("?".into(), |v| v.to_string())
    );
    println!(
        "decodificador liberado: {}",
        if usage.released { "sim" } else { "não" }
    );
}
