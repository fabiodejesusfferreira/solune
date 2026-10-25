//! Spike do player do Solune.
//!
//! Etapa 1: o libmpv toca um vídeo em loop, sem áudio e com decodificação
//! por hardware quando disponível, na janela do próprio mpv. As próximas
//! etapas trocam essa janela pela superfície de wallpaper (WorkerW no
//! Windows, layer-shell no KDE e wlroots).

mod mpv;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use mpv::{Event, Mpv};

const USAGE: &str = "uso: solune-player-spike <video> [--seconds N] [--vo SAIDA]";

struct Args {
    video: String,
    seconds: Option<u64>,
    /// Saída de vídeo do mpv; `null` serve para teste de fumaça sem tela.
    vo: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut video = None;
    let mut seconds = None;
    let mut vo = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--seconds" => {
                let value = args.next().ok_or("--seconds precisa de um número")?;
                seconds = Some(
                    value
                        .parse()
                        .map_err(|_| "--seconds precisa de um número")?,
                );
            }
            "--vo" => vo = Some(args.next().ok_or("--vo precisa de um nome")?),
            "-h" | "--help" => return Err(USAGE.into()),
            _ if video.is_none() => video = Some(arg),
            _ => return Err(format!("argumento inesperado: {arg}\n{USAGE}")),
        }
    }
    let video = video.ok_or(USAGE)?;
    Ok(Args { video, seconds, vo })
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let (major, minor) = mpv::client_api_version();
    println!("libmpv: API de cliente {major}.{minor}");

    let mut options = vec![
        ("terminal", "yes"),
        ("msg-level", "all=warn"),
        ("hwdec", "auto-safe"),
        ("loop-file", "inf"),
        ("aid", "no"),
    ];
    if let Some(vo) = &args.vo {
        options.push(("vo", vo));
    }
    let player = Mpv::new(&options)?;
    player.command(&["loadfile", &args.video])?;

    let deadline = args
        .seconds
        .map(|s| Instant::now() + Duration::from_secs(s));
    loop {
        if deadline.is_some_and(|d| Instant::now() >= d) {
            println!("tempo esgotado, encerrando");
            return Ok(());
        }
        match player.wait_event(0.5) {
            Some(Event::FileLoaded) => println!("arquivo carregado: {}", args.video),
            Some(Event::PlaybackRestart) => report(&player),
            Some(Event::EndFile { error: Some(code) }) => {
                return Err(mpv::Error {
                    code,
                    context: format!("não foi possível tocar {}", args.video),
                }
                .into());
            }
            Some(Event::Shutdown) => return Ok(()),
            Some(Event::EndFile { error: None } | Event::Other) | None => {}
        }
    }
}

/// Imprime o que importa para o spike: decodificador, saída e formato.
fn report(player: &Mpv) {
    let show = |label: &str, prop: &str| {
        let value = player
            .property(prop)
            .unwrap_or_else(|| "indisponível".into());
        println!("  {label}: {value}");
    };
    println!("reprodução iniciada");
    show("hwdec em uso", "hwdec-current");
    show("saída de vídeo", "current-vo");
    show("codec", "video-codec");
    show("largura", "width");
    show("altura", "height");
    show("fps do arquivo", "container-fps");
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(2);
        }
    };
    match run(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("erro: {err}");
            ExitCode::FAILURE
        }
    }
}
