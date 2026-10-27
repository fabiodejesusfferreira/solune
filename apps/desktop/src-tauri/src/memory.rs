//! Quanto o Solune inteiro ocupa de memória, por parte.
//!
//! O daemon informa só o próprio uso. Aqui entram também o app (janela e
//! bandeja) e os processos filhos dele, como os da webview
//! (`WebKitWebProcess` e `WebKitNetworkProcess` no Linux). A janela fechada
//! só se esconde, então a webview continua existindo enquanto o app roda.
//! A medida é a de `solune_core::memory`: PSS de todos, ou RSS de todos.

use serde::Serialize;
use solune_core::memory::{self, Metric, Process};

/// Memória de cada parte, em bytes. `None` quando a parte não existe agora
/// ou o processo saiu no meio da leitura.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppMemory {
    pub metric: Metric,
    /// O daemon, que toca o wallpaper.
    pub daemon: Option<u64>,
    /// Este processo: a janela, a bandeja e a ponte com o daemon.
    pub app: Option<u64>,
    /// Os processos da webview, que desenham a interface.
    pub webview: Option<u64>,
    pub webview_processes: u32,
    /// Outros filhos do app, como o `ffmpeg` das miniaturas.
    pub other: Option<u64>,
    pub other_processes: u32,
}

/// Mede o app, os filhos dele e o daemon. `None` fora do Linux, ou se o
/// sistema não informa.
#[tauri::command]
pub fn app_memory(daemon_pid: Option<u32>) -> Option<AppMemory> {
    let me = std::process::id();
    // O daemon iniciado por este app é filho dele: sai da busca e entra
    // como daemon. Iniciado por outro app, já não é filho.
    let daemon_pid = daemon_pid.filter(|&pid| pid != me);
    let all = memory::processes();
    let children = memory::descendants(&all, me, daemon_pid);
    let (webview, other): (Vec<&Process>, Vec<&Process>) =
        children.into_iter().partition(|p| is_webview(&p.name));

    let daemon_pids: Vec<u32> = daemon_pid.into_iter().collect();
    let webview_pids: Vec<u32> = webview.iter().map(|p| p.pid).collect();
    let other_pids: Vec<u32> = other.iter().map(|p| p.pid).collect();
    let groups = [&[me][..], &daemon_pids, &webview_pids, &other_pids];
    let (metric, bytes) = memory::measure(&groups.concat())?;

    // Soma cada grupo na ordem em que entrou em `measure`.
    let mut bytes = bytes.into_iter();
    let mut sum = |pids: &[u32]| {
        bytes
            .by_ref()
            .take(pids.len())
            .flatten()
            .reduce(|a, b| a + b)
    };
    let [app, daemon, webview, other] = groups.map(&mut sum);
    Some(AppMemory {
        metric,
        daemon,
        app,
        webview,
        webview_processes: webview_pids.len() as u32,
        other,
        other_processes: other_pids.len() as u32,
    })
}

/// Os processos da webview pelo nome: `WebKit*` no WebKitGTK e
/// `msedgewebview2` no WebView2, que o app ainda não mede.
fn is_webview(name: &str) -> bool {
    name.starts_with("WebKit") || name.starts_with("msedgewebview2")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconhece_a_webview() {
        // O kernel corta o nome em 15 bytes.
        assert!(is_webview("WebKitWebProces"));
        assert!(is_webview("WebKitNetworkPr"));
        assert!(!is_webview("ffmpeg"));
        assert!(!is_webview("solune-daemon"));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn mede_o_proprio_app_sem_daemon() {
        let memory = app_memory(None).unwrap();
        assert!(memory.app.unwrap_or(0) > 0);
        assert_eq!(memory.daemon, None);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn mede_o_daemon_filho() {
        let mut child = std::process::Command::new("sleep")
            .arg("5")
            .spawn()
            .unwrap();
        let memory = app_memory(Some(child.id())).unwrap();
        child.kill().unwrap();
        let _ = child.wait();
        assert!(memory.daemon.unwrap_or(0) > 0);
    }
}
