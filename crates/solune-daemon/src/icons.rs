//! Ícones da área de trabalho: escondidos enquanto o wallpaper toca, se o
//! usuário pediu (`show_desktop_icons` desligado).
//!
//! shortcut: só o Nemo do Cinnamon (`org.nemo.desktop`), pela mesma chave que
//! o Linux Mint usa; KDE e GNOME ficam como estão. Um daemon que cai à força
//! deixa os ícones escondidos até o próximo `stop`; trocar por restaurar ao
//! iniciar quando outros ambientes entrarem.

use std::process::{Command, Stdio};

const SCHEMA: &str = "org.nemo.desktop";
const KEY: &str = "show-desktop-icons";

#[derive(Debug, Default)]
pub struct Icons {
    /// O valor que a chave tinha antes de esconder; `Some` = escondido por nós.
    saved: Option<String>,
}

impl Icons {
    /// Esconde ou devolve os ícones. Chamar com o mesmo valor é inofensivo.
    pub fn set_hidden(&mut self, hidden: bool) {
        if hidden == self.saved.is_some() {
            return;
        }
        if hidden {
            // Sem o esquema (outro ambiente) ou com os ícones já escondidos
            // pelo usuário, não há o que fazer nem o que devolver depois.
            if gsettings(&["get", SCHEMA, KEY]).as_deref() == Some("true")
                && gsettings(&["set", SCHEMA, KEY, "false"]).is_some()
            {
                self.saved = Some("true".into());
            }
        } else if let Some(value) = self.saved.take() {
            gsettings(&["set", SCHEMA, KEY, &value]);
        }
    }
}

impl Drop for Icons {
    fn drop(&mut self) {
        self.set_hidden(false);
    }
}

/// A saída do `gsettings`, ou `None` se ele não existe ou falhou.
fn gsettings(args: &[&str]) -> Option<String> {
    let out = Command::new("gsettings")
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}
