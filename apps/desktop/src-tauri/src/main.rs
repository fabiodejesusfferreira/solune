//! App desktop do Solune.
//!
//! A janela mostra a interface React; fechar a janela só a esconde, e o app
//! continua na bandeja com o vídeo tocando. Quem toca o papel de parede é o
//! daemon, um processo à parte que o app inicia quando precisa. O Sair da
//! bandeja encerra os dois.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod daemon;
mod i18n;
mod media;
mod tray;

use tauri::{Manager, WindowEvent};
use tracing_subscriber::EnvFilter;

/// Argumento do login: abre só na bandeja, sem mostrar a janela.
const HIDDEN_ARG: &str = "--hidden";

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("solune_desktop=info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let result = tauri::Builder::default()
        // Primeiro plugin: uma segunda instância só traz a janela de volta.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        // Ao iniciar com o sistema, o app abre só na bandeja.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec![HIDDEN_ARG]),
        ))
        .manage(daemon::Daemon::default())
        .invoke_handler(tauri::generate_handler![
            daemon::daemon_status,
            daemon::daemon_policy,
            daemon::daemon_set_settings,
            daemon::daemon_usage,
            daemon::daemon_play_list,
            daemon::daemon_next,
            daemon::daemon_pause,
            daemon::daemon_resume,
            daemon::daemon_stop,
            media::media_allow,
            media::media_list,
            media::media_thumb,
            media::desktop_session,
        ])
        .setup(|app| {
            tray::create(app.handle())?;
            if !std::env::args().any(|a| a == HIDDEN_ARG) {
                tray::show_main(app.handle());
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                handle
                    .state::<daemon::Daemon>()
                    .ensure_running(&handle)
                    .await;
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!());
    if let Err(e) = result {
        tracing::error!("o app não conseguiu iniciar: {e}");
        std::process::exit(1);
    }
}
