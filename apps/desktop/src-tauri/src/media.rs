//! Arquivos da biblioteca e o sistema em que o app roda.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;
use tauri::{AppHandle, Manager};

/// Extensões que a biblioteca aceita, as mesmas de `library/store.ts`.
const MEDIA: &[&str] = &[
    "mp4", "webm", "mkv", "mov", "m4v", "png", "jpg", "jpeg", "webp",
];

fn is_media(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA.contains(&e.to_ascii_lowercase().as_str()))
}

/// Os arquivos de mídia entre `paths`, abrindo as pastas (só o primeiro
/// nível), em ordem de nome dentro de cada pasta.
///
/// Os comandos de arquivo são `async`: um `fn` comum roda na thread da
/// janela, e um disco dormindo ou uma montagem de rede travaria o app.
#[tauri::command(async)]
pub fn media_list(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for path in paths {
        if path.is_dir() {
            let Ok(entries) = std::fs::read_dir(&path) else {
                continue;
            };
            let mut files: Vec<_> = entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && is_media(p))
                .collect();
            files.sort();
            found.extend(files);
        } else if is_media(&path) {
            found.push(path);
        }
    }
    found
}

/// Libera os arquivos para o protocolo `asset:` (miniaturas e prévia) e
/// devolve o tamanho de cada um. `None` para o que não é mídia ou não existe:
/// só arquivos da biblioteca ficam visíveis à interface.
#[tauri::command(async)]
pub fn media_allow(app: AppHandle, paths: Vec<PathBuf>) -> Vec<Option<u64>> {
    let scope = app.asset_protocol_scope();
    paths
        .iter()
        .map(|path| {
            if !is_media(path) {
                return None;
            }
            let size = std::fs::metadata(path).ok().filter(|m| m.is_file())?.len();
            scope.allow_file(path).ok()?;
            Some(size)
        })
        .collect()
}

/// A miniatura de um vídeo e o que o arquivo informa.
#[derive(Debug, Serialize)]
pub struct Thumb {
    /// JPEG já liberado para o protocolo `asset:`.
    path: PathBuf,
    width: u32,
    height: u32,
    /// Em segundos.
    duration: f64,
}

/// Tira um quadro do vídeo com o `ffmpeg`. O WebKitGTK não toca `<video>` pelo
/// protocolo `asset:`, então a miniatura vira uma imagem, guardada no cache
/// do app. `None` sem `ffmpeg` ou se o arquivo não abre: a interface mostra o ícone.
#[tauri::command(async)]
pub fn media_thumb(app: AppHandle, path: PathBuf) -> Option<Thumb> {
    if !is_media(&path) {
        return None;
    }
    let modified = std::fs::metadata(&path).ok()?.modified().ok()?;
    let mut hasher = DefaultHasher::new();
    (&path, modified).hash(&mut hasher);
    let dir = app.path().app_cache_dir().ok()?.join("thumbs");
    std::fs::create_dir_all(&dir).ok()?;
    let thumb = dir.join(format!("{:016x}.jpg", hasher.finish()));

    let (width, height, duration) = probe(&path)?;
    if !thumb.is_file() {
        // shortcut: 1 s fixo; um vídeo mais curto fica sem miniatura, trocar por
        // metade da duração se isso aparecer.
        let ok = Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-ss", "1", "-i"])
            .arg(&path)
            .args(["-frames:v", "1", "-vf", "scale=440:-2", "-q:v", "4"])
            .arg(&thumb)
            .stdin(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok || !thumb.is_file() {
            return None;
        }
    }
    app.asset_protocol_scope().allow_file(&thumb).ok()?;
    Some(Thumb {
        path: thumb,
        width,
        height,
        duration,
    })
}

/// Largura, altura e duração pelo `ffprobe`.
fn probe(path: &Path) -> Option<(u32, u32, f64)> {
    let out = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "stream=width,height:format=duration"])
        .args(["-of", "default=nw=1"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    let text = String::from_utf8_lossy(&out.stdout);
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key)?.strip_prefix('='))
    };
    Some((
        field("width")?.parse().ok()?,
        field("height")?.parse().ok()?,
        field("duration")?.parse().ok()?,
    ))
}

/// O sistema, como os primeiros passos mostram.
#[derive(Debug, Serialize)]
pub struct Session {
    /// Nome para mostrar, como "KDE · Wayland".
    name: String,
    /// `ready`, `needs_extension` (GNOME no Wayland) ou `unsupported`.
    support: &'static str,
}

#[tauri::command]
pub fn desktop_session() -> Session {
    let var = |name: &str| std::env::var(name).unwrap_or_default();
    if cfg!(windows) {
        // shortcut: o WorkerW ainda não existe; trocar quando a superfície do Windows entrar.
        return Session {
            name: "Windows".into(),
            support: "unsupported",
        };
    }
    let desktop = var("XDG_CURRENT_DESKTOP");
    let desktop = desktop
        .split(':')
        .next()
        .unwrap_or_default()
        .trim_start_matches("X-");
    let wayland = !var("WAYLAND_DISPLAY").is_empty();
    let x11 = !var("DISPLAY").is_empty();
    let protocol = if wayland { "Wayland" } else { "X11" };
    let name = if desktop.is_empty() {
        protocol.to_owned()
    } else {
        format!("{desktop} · {protocol}")
    };
    // O mesmo critério do daemon (`surface::Kind::detect`); o GNOME não tem layer-shell.
    let support = match (wayland, x11) {
        (true, _) if desktop.eq_ignore_ascii_case("gnome") => "needs_extension",
        (true, _) | (false, true) => "ready",
        (false, false) => "unsupported",
    };
    Session { name, support }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abre_pastas_e_deixa_o_resto_de_fora() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b.png", "a.mp4", "notas.txt"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        std::fs::create_dir(dir.path().join("sub.mp4")).unwrap();
        let loose = PathBuf::from("/fora/praia.webm");
        assert_eq!(
            media_list(vec![
                dir.path().to_owned(),
                loose.clone(),
                "/x/notas.txt".into()
            ]),
            vec![dir.path().join("a.mp4"), dir.path().join("b.png"), loose]
        );
    }

    #[test]
    fn so_extensoes_de_midia() {
        assert!(is_media(Path::new("/v/praia.MP4")));
        assert!(is_media(Path::new("/i/serra.webp")));
        assert!(!is_media(Path::new("/home/ana/.ssh/id_ed25519")));
        assert!(!is_media(Path::new("/sem/extensao")));
    }
}
