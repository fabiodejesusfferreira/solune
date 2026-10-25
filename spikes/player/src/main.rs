//! Spike do player do Solune.
//!
//! O libmpv toca um vídeo em loop, sem áudio e com decodificação por
//! hardware quando disponível. Onde o vídeo aparece depende da superfície:
//!
//! - `janela`: a janela do próprio mpv (etapa 1, referência).
//! - `layer-shell`: camada de fundo do Wayland, no KDE 6 e no wlroots.
//! - `x11`: janela do tipo desktop, em sessões X11 como o Cinnamon.

#[cfg(target_os = "linux")]
mod events;
#[cfg(target_os = "linux")]
mod gl;
#[cfg(target_os = "linux")]
mod layer_shell;
mod mpv;
#[cfg(target_os = "linux")]
mod render;
#[cfg(target_os = "linux")]
mod x11;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use mpv::{Event, Mpv};

const USAGE: &str = "uso: solune-player-spike <video> [--surface janela|layer-shell|x11] [--seconds N] [--hwdec MODO] [--vo SAIDA]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Surface {
    Window,
    #[cfg(target_os = "linux")]
    LayerShell,
    #[cfg(target_os = "linux")]
    X11,
}

struct Args {
    video: String,
    surface: Surface,
    seconds: Option<u64>,
    /// Modo de decodificação por hardware do mpv; o padrão é `auto-safe`.
    hwdec: String,
    /// Saída de vídeo do mpv no modo janela; `null` serve para teste de
    /// fumaça sem tela.
    vo: Option<String>,
}

/// Se a reprodução deve continuar depois de um evento do libmpv.
#[derive(Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

fn parse_args() -> Result<Args, String> {
    let mut video = None;
    let mut surface = Surface::Window;
    let mut seconds = None;
    let mut hwdec = String::from("auto-safe");
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
            "--surface" => {
                surface = match args.next().as_deref() {
                    Some("janela") => Surface::Window,
                    #[cfg(target_os = "linux")]
                    Some("layer-shell") => Surface::LayerShell,
                    #[cfg(target_os = "linux")]
                    Some("x11") => Surface::X11,
                    _ => return Err(format!("superfície desconhecida\n{USAGE}")),
                };
            }
            "--hwdec" => hwdec = args.next().ok_or("--hwdec precisa de um modo")?,
            "--vo" => vo = Some(args.next().ok_or("--vo precisa de um nome")?),
            "-h" | "--help" => return Err(USAGE.into()),
            _ if video.is_none() => video = Some(arg),
            _ => return Err(format!("argumento inesperado: {arg}\n{USAGE}")),
        }
    }
    let video = video.ok_or(USAGE)?;
    if vo.is_some() && surface != Surface::Window {
        return Err("--vo só vale com --surface janela".into());
    }
    Ok(Args {
        video,
        surface,
        seconds,
        hwdec,
        vo,
    })
}

fn run(args: Args) -> Result<(), Box<dyn std::error::Error>> {
    let (major, minor) = mpv::client_api_version();
    println!("libmpv: API de cliente {major}.{minor}");

    let mut options = vec![
        ("terminal", "yes"),
        ("msg-level", "all=warn"),
        ("hwdec", &args.hwdec),
        ("loop-file", "inf"),
        ("aid", "no"),
    ];
    match args.surface {
        Surface::Window => {
            if let Some(vo) = &args.vo {
                options.push(("vo", vo));
            }
        }
        // As superfícies desenham os quadros pela render API.
        #[cfg(target_os = "linux")]
        Surface::LayerShell | Surface::X11 => {
            options.push(("vo", "libmpv"));
        }
    }
    let player = Mpv::new(&options)?;
    let deadline = args
        .seconds
        .map(|s| Instant::now() + Duration::from_secs(s));

    match args.surface {
        Surface::Window => {
            player.command(&["loadfile", &args.video])?;
            let mut reported = false;
            while !past(deadline) {
                if poll(&player, &args.video, 0.5, &mut reported)? == Flow::Stop {
                    return Ok(());
                }
            }
            Ok(())
        }
        #[cfg(target_os = "linux")]
        Surface::LayerShell => layer_shell::run(player, &args.video, &args.hwdec, deadline),
        #[cfg(target_os = "linux")]
        Surface::X11 => x11::run(player, &args.video, &args.hwdec, deadline),
    }
}

/// Diz se o prazo de `--seconds` passou, e avisa quando passa.
fn past(deadline: Option<Instant>) -> bool {
    let over = deadline.is_some_and(|d| Instant::now() >= d);
    if over {
        println!("tempo esgotado, encerrando");
    }
    over
}

/// Trata os eventos pendentes do libmpv, esperando até `timeout` segundos
/// pelo primeiro. `reported` evita repetir o relatório a cada volta do loop.
fn poll(player: &Mpv, video: &str, timeout: f64, reported: &mut bool) -> Result<Flow, mpv::Error> {
    let mut wait = timeout;
    while let Some(event) = player.wait_event(wait) {
        wait = 0.0;
        match event {
            Event::FileLoaded => println!("arquivo carregado: {video}"),
            Event::PlaybackRestart if !*reported => {
                *reported = true;
                report(player);
            }
            Event::EndFile { error: Some(code) } => {
                return Err(mpv::Error {
                    code,
                    context: format!("não foi possível tocar {video}"),
                });
            }
            Event::Shutdown => return Ok(Flow::Stop),
            Event::PlaybackRestart | Event::EndFile { error: None } | Event::Other => {}
        }
    }
    Ok(Flow::Continue)
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
