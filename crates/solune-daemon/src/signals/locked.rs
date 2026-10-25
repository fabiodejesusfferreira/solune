//! Tela bloqueada, pelo D-Bus.
//!
//! Duas fontes, e basta uma dizer que está bloqueada:
//!
//! - o protetor de tela da sessão (`ActiveChanged` e `GetActive`), que o
//!   Cinnamon, o KDE, o GNOME e o MATE publicam, cada um com o seu nome;
//! - o `LockedHint` da sessão no logind, que alguns bloqueadores marcam.
//!
//! Se nenhuma das duas responde (um Sway com swaylock, por exemplo), o
//! sinal fica como não detectado.

use std::collections::HashMap;

use futures_util::StreamExt;
use zbus::message::Type;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus::{Connection, MatchRule, Message, MessageStream};

use super::Latest;

/// Protetores de tela conhecidos: serviço, caminho e interface.
const SCREENSAVERS: &[(&str, &str, &str)] = &[
    (
        "org.cinnamon.ScreenSaver",
        "/org/cinnamon/ScreenSaver",
        "org.cinnamon.ScreenSaver",
    ),
    (
        "org.freedesktop.ScreenSaver",
        "/org/freedesktop/ScreenSaver",
        "org.freedesktop.ScreenSaver",
    ),
    (
        "org.gnome.ScreenSaver",
        "/org/gnome/ScreenSaver",
        "org.gnome.ScreenSaver",
    ),
    (
        "org.mate.ScreenSaver",
        "/org/mate/ScreenSaver",
        "org.mate.ScreenSaver",
    ),
];

const LOGIND: &str = "org.freedesktop.login1";
const SESSION_IFACE: &str = "org.freedesktop.login1.Session";
const LOCKED_HINT: &str = "LockedHint";

/// O que cada fonte disse por último.
#[derive(Debug, Default)]
struct Sources(HashMap<String, bool>);

impl Sources {
    fn set(&mut self, source: &str, active: bool) {
        self.0.insert(source.to_owned(), active);
    }

    fn locked(&self) -> Option<bool> {
        (!self.0.is_empty()).then(|| self.0.values().any(|&v| v))
    }
}

pub async fn watch(notify: impl Fn(Option<bool>)) {
    let mut sources = Sources::default();
    let session = Connection::session().await.ok();
    let system = Connection::system().await.ok();

    let mut screensaver = None;
    if let Some(bus) = &session {
        for (service, path, iface) in SCREENSAVERS {
            if let Ok(reply) = bus
                .call_method(Some(*service), *path, Some(*iface), "GetActive", &())
                .await
                && let Ok(active) = reply.body().deserialize::<bool>()
            {
                sources.set(iface, active);
            }
        }
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .member("ActiveChanged")
            .map(|b| b.build());
        if let Ok(rule) = rule {
            screensaver = MessageStream::for_match_rule(rule, bus, None).await.ok();
        }
    }

    let mut logind = None;
    if let Some(bus) = &system
        && let Some(path) = session_path(bus).await
    {
        if let Some(hint) = locked_hint(bus, &path).await {
            sources.set(LOCKED_HINT, hint);
        }
        let rule = MatchRule::builder()
            .msg_type(Type::Signal)
            .sender(LOGIND)
            .and_then(|b| b.path(path.as_str()))
            .and_then(|b| b.interface("org.freedesktop.DBus.Properties"))
            .and_then(|b| b.member("PropertiesChanged"))
            .map(|b| b.build());
        if let Ok(rule) = rule {
            logind = MessageStream::for_match_rule(rule, bus, None)
                .await
                .ok()
                .map(|stream| (stream, path));
        }
    }

    let mut latest = Latest::default();
    loop {
        let value = sources.locked();
        if latest.update(value) {
            tracing::info!("tela bloqueada: {value:?}");
            notify(value);
        }
        if screensaver.is_none() && logind.is_none() {
            return;
        }
        tokio::select! {
            Some(Ok(message)) = next(screensaver.as_mut()) => {
                if let Some((iface, active)) = screensaver_change(&message) {
                    sources.set(&iface, active);
                }
            }
            Some(Ok(message)) = next(logind.as_mut().map(|(s, _)| s)) => {
                if let Some(hint) = locked_hint_change(&message) {
                    sources.set(LOCKED_HINT, hint);
                }
            }
            else => return,
        }
    }
}

async fn next(stream: Option<&mut MessageStream>) -> Option<zbus::Result<Message>> {
    match stream {
        Some(stream) => stream.next().await,
        None => std::future::pending().await,
    }
}

/// O caminho da sessão deste processo no logind.
async fn session_path(bus: &Connection) -> Option<OwnedObjectPath> {
    let reply = bus
        .call_method(
            Some(LOGIND),
            "/org/freedesktop/login1",
            Some("org.freedesktop.login1.Manager"),
            "GetSessionByPID",
            &(std::process::id()),
        )
        .await
        .ok()?;
    reply.body().deserialize().ok()
}

async fn locked_hint(bus: &Connection, path: &OwnedObjectPath) -> Option<bool> {
    let reply = bus
        .call_method(
            Some(LOGIND),
            path.as_str(),
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &(SESSION_IFACE, LOCKED_HINT),
        )
        .await
        .ok()?;
    let value: OwnedValue = reply.body().deserialize().ok()?;
    bool::try_from(value).ok()
}

/// `ActiveChanged(b)` de um protetor de tela: a interface e o valor.
fn screensaver_change(message: &Message) -> Option<(String, bool)> {
    let header = message.header();
    let iface = header.interface()?.to_string();
    if !iface.ends_with("ScreenSaver") {
        return None;
    }
    let active = message.body().deserialize::<bool>().ok()?;
    Some((iface, active))
}

/// `PropertiesChanged` da sessão: o `LockedHint` novo, se veio.
fn locked_hint_change(message: &Message) -> Option<bool> {
    type Changed = (String, HashMap<String, OwnedValue>, Vec<String>);
    let (iface, changed, _): Changed = message.body().deserialize().ok()?;
    if iface != SESSION_IFACE {
        return None;
    }
    let value = changed.get(LOCKED_HINT)?.try_clone().ok()?;
    bool::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Precisa de um barramento de sessão: rode com `dbus-run-session --`.
    #[tokio::test]
    #[ignore = "precisa de um barramento D-Bus de sessão"]
    async fn protetor_de_tela_avisa_pelo_sinal() {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_none() {
            eprintln!("sem DBUS_SESSION_BUS_ADDRESS; rode com dbus-run-session");
            return;
        }
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let watcher = tokio::spawn(watch(move |v| {
            let _ = tx.send(v);
        }));
        // Sem protetor de tela respondendo, começa como não detectado.
        assert_eq!(rx.recv().await, Some(None));

        let bus = Connection::session().await.unwrap();
        let emit = |active: bool| {
            let bus = bus.clone();
            async move {
                bus.emit_signal(
                    None::<&str>,
                    "/org/cinnamon/ScreenSaver",
                    "org.cinnamon.ScreenSaver",
                    "ActiveChanged",
                    &(active),
                )
                .await
                .unwrap();
            }
        };
        let wait = std::time::Duration::from_secs(5);
        // O watcher assina o sinal logo depois do primeiro aviso; repete
        // até ele ouvir.
        let mut heard = None;
        for _ in 0..50 {
            emit(true).await;
            if let Ok(v) =
                tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await
            {
                heard = v;
                break;
            }
        }
        assert_eq!(heard, Some(Some(true)));
        emit(false).await;
        assert_eq!(
            tokio::time::timeout(wait, rx.recv()).await.unwrap(),
            Some(Some(false))
        );
        watcher.abort();
    }

    #[test]
    fn basta_uma_fonte_dizer_bloqueada() {
        let mut sources = Sources::default();
        assert_eq!(sources.locked(), None);
        sources.set("org.cinnamon.ScreenSaver", false);
        sources.set(LOCKED_HINT, false);
        assert_eq!(sources.locked(), Some(false));
        sources.set(LOCKED_HINT, true);
        assert_eq!(sources.locked(), Some(true));
    }
}
