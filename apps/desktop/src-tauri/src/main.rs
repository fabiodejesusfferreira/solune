//! App desktop do Solune.
//!
//! A janela mostra a interface React; fechar a janela só a esconde, e o app
//! continua na bandeja com o vídeo tocando. Quem toca o papel de parede é o
//! daemon, um processo à parte que o app inicia quando precisa. O Sair da
//! bandeja encerra os dois.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod daemon;
mod i18n;
mod tray;

use tauri::WindowEvent;
use tracing_subscriber::EnvFilter;

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
        .manage(daemon::Daemon::default())
        .invoke_handler(tauri::generate_handler![
            daemon::daemon_status,
            daemon::daemon_play,
            daemon::daemon_pause,
            daemon::daemon_resume,
            daemon::daemon_stop,
        ])
        .setup(|app| {
            tray::create(app.handle())?;
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
