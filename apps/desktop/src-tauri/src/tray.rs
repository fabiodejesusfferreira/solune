//! Ícone e menu da bandeja.

use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::daemon::Daemon;
use crate::i18n::tr;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", tr("tray.open"), true, None::<&str>)?;
    let rules = MenuItem::with_id(app, "rules", tr("tray.rules"), true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", tr("tray.quit"), true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &rules, &quit])?;

    // Na bandeja o ícone fica com 16 a 24 px: vai o símbolo, sem a órbita.
    let icon = Image::from_bytes(include_bytes!("../icons/tray.png"))?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip(tr("app.name"))
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "rules" => {
                show_main(app);
                // A interface troca de tela ao ouvir o evento (`App.tsx`).
                let _ = app.emit_to("main", "app://navigate", "rules");
            }
            // Sair encerra tudo: o daemon para o vídeo e devolve o papel de
            // parede do sistema. Fechar a janela, não; o app fica na bandeja.
            "quit" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    app.state::<Daemon>().shutdown().await;
                    app.exit(0);
                });
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Mostra a janela principal e traz para a frente.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
