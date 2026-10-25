//! Quando o wallpaper toca e quando pausa.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use solune_ipc::PauseReason;

/// Depois de quanto tempo pausado o daemon libera o decodificador e o
/// contexto de render. Voltar a tocar recria os dois.
pub const RELEASE_AFTER: Duration = Duration::from_secs(5 * 60);

/// Preferências do usuário sobre pausas automáticas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub pause_on_fullscreen: bool,
    pub pause_on_battery: bool,
}

impl Default for Settings {
    /// As duas pausas vêm ligadas: o carro-chefe é o baixo consumo.
    fn default() -> Self {
        Self {
            pause_on_fullscreen: true,
            pause_on_battery: true,
        }
    }
}

/// O que o daemon sabe do sistema e do usuário neste momento.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Signals {
    pub user_paused: bool,
    pub screen_locked: bool,
    pub fullscreen_app: bool,
    pub on_battery: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Play,
    Pause(PauseReason),
}

/// Decide se o wallpaper toca. Com mais de um motivo, vale o primeiro desta
/// ordem: usuário, tela bloqueada, tela cheia, bateria. O usuário vem
/// primeiro porque é a única pausa que só ele desfaz; a tela bloqueada vem
/// antes das outras porque não depende de preferência.
pub fn decide(signals: Signals, settings: Settings) -> Decision {
    if signals.user_paused {
        Decision::Pause(PauseReason::User)
    } else if signals.screen_locked {
        Decision::Pause(PauseReason::Locked)
    } else if signals.fullscreen_app && settings.pause_on_fullscreen {
        Decision::Pause(PauseReason::Fullscreen)
    } else if signals.on_battery && settings.pause_on_battery {
        Decision::Pause(PauseReason::Battery)
    } else {
        Decision::Play
    }
}

/// Se já passou tempo bastante em pausa para liberar o decodificador.
pub fn should_release(paused_for: Duration) -> bool {
    paused_for >= RELEASE_AFTER
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sem_sinal_toca() {
        assert_eq!(
            decide(Signals::default(), Settings::default()),
            Decision::Play
        );
    }

    #[test]
    fn pausa_do_usuario_vence_as_outras() {
        let all = Signals {
            user_paused: true,
            screen_locked: true,
            fullscreen_app: true,
            on_battery: true,
        };
        assert_eq!(
            decide(all, Settings::default()),
            Decision::Pause(PauseReason::User)
        );
    }

    #[test]
    fn ordem_dos_motivos_automaticos() {
        let settings = Settings::default();
        let locked = Signals {
            screen_locked: true,
            fullscreen_app: true,
            on_battery: true,
            ..Signals::default()
        };
        assert_eq!(
            decide(locked, settings),
            Decision::Pause(PauseReason::Locked)
        );
        let fullscreen = Signals {
            fullscreen_app: true,
            on_battery: true,
            ..Signals::default()
        };
        assert_eq!(
            decide(fullscreen, settings),
            Decision::Pause(PauseReason::Fullscreen)
        );
    }

    #[test]
    fn preferencias_desligadas_nao_pausam() {
        let settings = Settings {
            pause_on_fullscreen: false,
            pause_on_battery: false,
        };
        let signals = Signals {
            fullscreen_app: true,
            on_battery: true,
            ..Signals::default()
        };
        assert_eq!(decide(signals, settings), Decision::Play);
        // A tela bloqueada não tem preferência: sempre pausa.
        let locked = Signals {
            screen_locked: true,
            ..signals
        };
        assert_eq!(
            decide(locked, settings),
            Decision::Pause(PauseReason::Locked)
        );
    }

    #[test]
    fn libera_o_decodificador_aos_cinco_minutos() {
        assert!(!should_release(Duration::from_secs(299)));
        assert!(should_release(Duration::from_secs(300)));
    }
}
