//! Ícone e menu da bandeja: o controle rápido sem abrir a janela.

use solune_ipc::{Command, PlayerState, Policy, Reply, Schedule, Status, Theme};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::daemon::Daemon;
use crate::i18n::tr;

/// Os itens que mudam com o estado do daemon.
#[allow(missing_debug_implementations)] // os itens de menu do tauri não têm Debug
struct Items {
    state: MenuItem<tauri::Wry>,
    toggle: MenuItem<tauri::Wry>,
    next: MenuItem<tauri::Wry>,
    follow: CheckMenuItem<tauri::Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, text: String, enabled: bool| {
        MenuItem::with_id(app, id, text, enabled, None::<&str>)
    };
    let items = Items {
        state: item("state", tr("player.state.stopped"), false)?,
        toggle: item("toggle", tr("player.pause"), false)?,
        next: item("next", tr("tray.next"), false)?,
        follow: CheckMenuItem::with_id(app, "follow", tr("tray.follow"), true, true, None::<&str>)?,
    };
    let light = item("light", tr("tray.light"), true)?;
    let dark = item("dark", tr("tray.dark"), true)?;
    let open = item("open", tr("tray.open"), true)?;
    let rules = item("rules", tr("tray.rules"), true)?;
    let quit = item("quit", tr("tray.quit"), true)?;
    let menu = Menu::with_items(
        app,
        &[
            &items.state,
            &items.toggle,
            &items.next,
            &PredefinedMenuItem::separator(app)?,
            &items.follow,
            &light,
            &dark,
            &PredefinedMenuItem::separator(app)?,
            &open,
            &rules,
            &quit,
        ],
    )?;
    app.manage(items);

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
            "toggle" => send(app, Action::Toggle),
            "next" => send(app, Action::Command(Command::Next)),
            "follow" => send(app, Action::Schedule(Schedule::System)),
            "light" => send(
                app,
                Action::Schedule(Schedule::Fixed {
                    theme: Theme::Light,
                }),
            ),
            "dark" => send(
                app,
                Action::Schedule(Schedule::Fixed { theme: Theme::Dark }),
            ),
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

#[derive(Debug)]
enum Action {
    /// Pausa se está tocando, retoma se está pausado.
    Toggle,
    Command(Command),
    /// Troca só a agenda de temas, mantendo as outras preferências.
    Schedule(Schedule),
}

fn send(app: &AppHandle, action: Action) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let daemon = app.state::<Daemon>();
        let command = match action {
            Action::Command(command) => command,
            Action::Toggle => match daemon.request(&app, Command::Status).await {
                Ok(Reply::Status(status)) if status.state == PlayerState::Playing => Command::Pause,
                Ok(_) => Command::Resume,
                Err(e) => return tracing::warn!("bandeja: {e}"),
            },
            Action::Schedule(schedule) => match daemon.request(&app, Command::GetPolicy).await {
                Ok(Reply::Policy(policy)) => Command::SetSettings {
                    settings: solune_ipc::Settings {
                        schedule,
                        ..policy.settings
                    },
                },
                Ok(_) => return,
                Err(e) => return tracing::warn!("bandeja: {e}"),
            },
        };
        if let Err(e) = daemon.request(&app, command).await {
            tracing::warn!("bandeja: {e}");
        }
    });
}

/// Mostra o estado do player no menu.
pub fn show_status(app: &AppHandle, status: &Status) {
    let Some(items) = app.try_state::<Items>() else {
        return;
    };
    let (state, toggle) = match status.state {
        PlayerState::Playing => (tr("player.state.playing"), tr("player.pause")),
        PlayerState::Paused => (tr("player.state.paused"), tr("player.resume")),
        PlayerState::Stopped => (tr("player.state.stopped"), tr("player.pause")),
    };
    let on = status.state != PlayerState::Stopped;
    let _ = items.state.set_text(state);
    let _ = items.toggle.set_text(toggle);
    let _ = items.toggle.set_enabled(on);
    let _ = items.next.set_enabled(on);
}

/// Marca "Seguir o tema do sistema" conforme a agenda.
pub fn show_policy(app: &AppHandle, policy: &Policy) {
    if let Some(items) = app.try_state::<Items>() {
        let _ = items
            .follow
            .set_checked(policy.settings.schedule == Schedule::System);
    }
}

/// Mostra a janela principal e traz para a frente.
pub fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
