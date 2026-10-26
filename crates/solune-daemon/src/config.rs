//! O que o daemon guarda em disco: as preferências das pausas e o wallpaper
//! em uso, para voltar a tocar quando o daemon reinicia (no login, por
//! exemplo).
//!
//! Fica em `$XDG_CONFIG_HOME/solune` (ou `~/.config/solune`) no Linux e em
//! `%APPDATA%\Solune` no Windows. `SOLUNE_CONFIG_DIR` troca a pasta.

use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use solune_ipc::{Playlist, Settings};

/// Variável de ambiente que troca a pasta de configuração.
pub const CONFIG_ENV: &str = "SOLUNE_CONFIG_DIR";

const SETTINGS_FILE: &str = "settings.json";
const STATE_FILE: &str = "state.json";

/// O wallpaper em uso. Sem lista, nada toca ao iniciar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Só de versões antigas, que tocavam um arquivo por vez.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    pub list: Option<Playlist>,
    /// O item do carrossel em uso, para seguir dele ao reiniciar.
    pub position: usize,
}

impl State {
    /// A lista em uso, convertendo o formato antigo.
    pub fn playlist(&self) -> Option<Playlist> {
        self.list
            .clone()
            .or_else(|| self.path.clone().map(Playlist::single))
    }
}

#[derive(Debug, Clone)]
pub struct Store {
    dir: Option<PathBuf>,
}

impl Store {
    /// A pasta padrão do usuário. Sem ela, nada é salvo.
    pub fn for_user() -> Self {
        Self { dir: config_dir() }
    }

    #[cfg(test)]
    pub fn at(dir: &Path) -> Self {
        Self {
            dir: Some(dir.to_owned()),
        }
    }

    pub fn settings(&self) -> Settings {
        self.load(SETTINGS_FILE)
    }

    pub fn save_settings(&self, settings: &Settings) {
        self.save(SETTINGS_FILE, settings);
    }

    pub fn state(&self) -> State {
        self.load(STATE_FILE)
    }

    pub fn save_state(&self, state: &State) {
        self.save(STATE_FILE, state);
    }

    /// Lê o arquivo. Se ele não existe ou está estragado, vale o padrão.
    fn load<T: DeserializeOwned + Default>(&self, name: &str) -> T {
        let Some(path) = self.dir.as_ref().map(|d| d.join(name)) else {
            return T::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_else(|e| {
                tracing::warn!("{} ignorado: {e}", path.display());
                T::default()
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => T::default(),
            Err(e) => {
                tracing::warn!("não consegui ler {}: {e}", path.display());
                T::default()
            }
        }
    }

    fn save<T: Serialize>(&self, name: &str, value: &T) {
        let Some(dir) = &self.dir else { return };
        if let Err(e) = write_atomic(dir, name, value) {
            tracing::warn!("não consegui salvar {}: {e}", dir.join(name).display());
        }
    }
}

/// Grava num arquivo temporário e renomeia: uma queda no meio não deixa o
/// arquivo pela metade.
fn write_atomic<T: Serialize>(dir: &Path, name: &str, value: &T) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    std::fs::write(&tmp, text + "\n")?;
    std::fs::rename(&tmp, dir.join(name))
}

fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(CONFIG_ENV) {
        return Some(dir.into());
    }
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Solune"))
    }
    #[cfg(not(windows))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("solune"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn salva_e_le_de_volta() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::at(&dir.path().join("solune"));
        assert_eq!(store.settings(), Settings::default());
        assert_eq!(store.state(), State::default());

        let settings = Settings {
            pause_on_battery: false,
            ..Settings::default()
        };
        store.save_settings(&settings);
        let list = Playlist::single("/v/praia.mp4".into());
        store.save_state(&State {
            list: Some(list.clone()),
            position: 0,
            path: None,
        });
        assert_eq!(store.settings(), settings);
        assert_eq!(store.state().playlist(), Some(list));
    }

    #[test]
    fn estado_antigo_vira_lista() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(STATE_FILE), r#"{"path":"/v/praia.mp4"}"#).unwrap();
        assert_eq!(
            Store::at(dir.path()).state().playlist(),
            Some(Playlist::single("/v/praia.mp4".into()))
        );
    }

    #[test]
    fn arquivo_estragado_vale_o_padrao() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(SETTINGS_FILE), "{ não é json").unwrap();
        assert_eq!(Store::at(dir.path()).settings(), Settings::default());
    }
}
