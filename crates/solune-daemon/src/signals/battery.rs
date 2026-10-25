//! Bateria, pelo sysfs (`/sys/class/power_supply`), a cada 10 segundos.
//!
//! Ler o sysfs não depende de UPower nem de D-Bus, e dez segundos de atraso
//! para pausar ao tirar da tomada não fazem diferença.

use std::fs;
use std::path::Path;
use std::time::Duration;

use super::Latest;

const POWER_SUPPLY: &str = "/sys/class/power_supply";
const INTERVAL: Duration = Duration::from_secs(10);

pub async fn watch(notify: impl Fn(Option<bool>)) {
    let mut latest = Latest::default();
    let mut tick = tokio::time::interval(INTERVAL);
    loop {
        tick.tick().await;
        let value = on_battery(Path::new(POWER_SUPPLY));
        if latest.update(value) {
            tracing::info!("bateria: {}", describe(value));
            notify(value);
        }
    }
}

fn describe(value: Option<bool>) -> &'static str {
    match value {
        None => "sem bateria",
        Some(true) => "descarregando",
        Some(false) => "na tomada",
    }
}

/// Se o computador está rodando na bateria. `None` quando não há bateria
/// do sistema (um desktop, ou só baterias de periféricos, como um mouse).
pub(super) fn on_battery(dir: &Path) -> Option<bool> {
    let mut battery = false;
    let mut discharging = false;
    let mut plugged = false;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let read = |name: &str| {
            fs::read_to_string(path.join(name))
                .ok()
                .map(|s| s.trim().to_owned())
        };
        match read("type").as_deref() {
            Some("Battery") => {
                if read("scope").as_deref() == Some("Device") {
                    continue;
                }
                battery = true;
                discharging |= read("status").as_deref() == Some("Discharging");
            }
            Some("Mains" | "USB" | "USB_C" | "USB_PD" | "USB_PD_DRP") => {
                plugged |= read("online").as_deref() == Some("1");
            }
            _ => {}
        }
    }
    battery.then_some(discharging && !plugged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply(dir: &Path, name: &str, files: &[(&str, &str)]) {
        let path = dir.join(name);
        fs::create_dir_all(&path).unwrap();
        for (file, value) in files {
            fs::write(path.join(file), format!("{value}\n")).unwrap();
        }
    }

    #[test]
    fn notebook_na_bateria_e_na_tomada() {
        let dir = tempfile::tempdir().unwrap();
        supply(dir.path(), "AC", &[("type", "Mains"), ("online", "0")]);
        supply(
            dir.path(),
            "BAT0",
            &[("type", "Battery"), ("status", "Discharging")],
        );
        assert_eq!(on_battery(dir.path()), Some(true));

        supply(dir.path(), "AC", &[("type", "Mains"), ("online", "1")]);
        supply(
            dir.path(),
            "BAT0",
            &[("type", "Battery"), ("status", "Charging")],
        );
        assert_eq!(on_battery(dir.path()), Some(false));
    }

    #[test]
    fn desktop_e_bateria_de_mouse_nao_contam() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(on_battery(dir.path()), None);
        supply(
            dir.path(),
            "hidpp_battery_0",
            &[
                ("type", "Battery"),
                ("scope", "Device"),
                ("status", "Discharging"),
            ],
        );
        assert_eq!(on_battery(dir.path()), None);
        assert_eq!(on_battery(&dir.path().join("nao-existe")), None);
    }
}
