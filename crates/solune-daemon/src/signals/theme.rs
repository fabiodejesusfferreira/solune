//! Tema do sistema, pelo portal do freedesktop (`org.freedesktop.appearance`
//! `color-scheme`), que o KDE, o GNOME e os portais do wlroots publicam.
//!
//! O valor é 1 para escuro, 2 para claro e 0 sem preferência (conta como
//! claro). Sem portal, o sinal fica como não detectado.

use futures_util::StreamExt;
use zbus::message::Type;
use zbus::zvariant::{OwnedValue, Value};
use zbus::{Connection, MatchRule, Message, MessageStream};

use super::Latest;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS_IFACE: &str = "org.freedesktop.portal.Settings";
const NAMESPACE: &str = "org.freedesktop.appearance";
const KEY: &str = "color-scheme";

pub async fn watch(notify: impl Fn(Option<bool>)) {
    let Ok(bus) = Connection::session().await else {
        notify(None);
        return;
    };
    let mut latest = Latest::default();
    let first = read(&bus).await;
    tracing::info!("tema do sistema escuro: {first:?}");
    latest.update(first);
    notify(first);

    let rule = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface(SETTINGS_IFACE)
        .and_then(|b| b.member("SettingChanged"))
        .map(|b| b.build());
    let Ok(rule) = rule else { return };
    let Ok(mut stream) = MessageStream::for_match_rule(rule, &bus, None).await else {
        return;
    };
    while let Some(Ok(message)) = stream.next().await {
        if let Some(dark) = change(&message)
            && latest.update(Some(dark))
        {
            tracing::info!("tema do sistema escuro: {dark}");
            notify(Some(dark));
        }
    }
}

/// O valor atual. `ReadOne` é dos portais novos; `Read` devolve o valor
/// dentro de mais uma variante.
async fn read(bus: &Connection) -> Option<bool> {
    for method in ["ReadOne", "Read"] {
        if let Ok(reply) = bus
            .call_method(
                Some(PORTAL),
                PORTAL_PATH,
                Some(SETTINGS_IFACE),
                method,
                &(NAMESPACE, KEY),
            )
            .await
            && let Ok(value) = reply.body().deserialize::<OwnedValue>()
        {
            return is_dark(&value);
        }
    }
    None
}

/// `SettingChanged(namespace, key, value)` do esquema de cores.
fn change(message: &Message) -> Option<bool> {
    let (namespace, key, value): (String, String, OwnedValue) =
        message.body().deserialize().ok()?;
    (namespace == NAMESPACE && key == KEY)
        .then(|| is_dark(&value))
        .flatten()
}

fn is_dark(value: &Value<'_>) -> Option<bool> {
    match value {
        Value::Value(inner) => is_dark(inner),
        Value::U32(scheme) => Some(*scheme == 1),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn um_e_escuro_e_variante_dentro_de_variante_vale() {
        assert_eq!(is_dark(&Value::U32(1)), Some(true));
        assert_eq!(is_dark(&Value::U32(2)), Some(false));
        assert_eq!(is_dark(&Value::U32(0)), Some(false));
        assert_eq!(is_dark(&Value::Value(Box::new(Value::U32(1)))), Some(true));
        assert_eq!(is_dark(&Value::Bool(true)), None);
    }
}
