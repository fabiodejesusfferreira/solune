//! Quando o wallpaper toca e quando pausa.

use std::time::Duration;

use solune_ipc::{Conditions, PauseReason};

pub use solune_ipc::Settings;

/// Depois de quanto tempo pausado o daemon libera o decodificador e o
/// contexto de render. Voltar a tocar recria os dois.
pub const RELEASE_AFTER: Duration = Duration::from_secs(5 * 60);

/// O que o daemon sabe do sistema neste momento. Um sinal que não pode ser
/// detectado conta como ausente.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Signals {
    pub screen_locked: bool,
    pub fullscreen_app: bool,
    pub on_battery: bool,
}

impl From<Conditions> for Signals {
    fn from(conditions: Conditions) -> Self {
        Self {
            screen_locked: conditions.screen_locked.unwrap_or(false),
            fullscreen_app: conditions.fullscreen.unwrap_or(false),
            on_battery: conditions.on_battery.unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Play,
    Pause(PauseReason),
}

/// Os motivos automáticos ativos agora, na ordem em que valem: tela
/// bloqueada, tela cheia, bateria. A tela bloqueada vem antes porque
/// ninguém está vendo; a bateria por último porque o vídeo ainda aparece.
fn automatic(signals: Signals, settings: Settings) -> impl Iterator<Item = PauseReason> {
    [
        (signals.screen_locked && settings.pause_on_lock).then_some(PauseReason::Locked),
        (signals.fullscreen_app && settings.pause_on_fullscreen).then_some(PauseReason::Fullscreen),
        (signals.on_battery && settings.pause_on_battery).then_some(PauseReason::Battery),
    ]
    .into_iter()
    .flatten()
}

/// Junta o pedido do usuário, os sinais do sistema e as preferências.
///
/// Pausar pelo app vence tudo e só o usuário desfaz. Retomar pelo app
/// também desfaz as pausas automáticas ativas naquele momento: um motivo
/// desfeito só volta a pausar quando o sinal some e aparece de novo (sair
/// da tela cheia e entrar outra vez, ligar na tomada e tirar).
#[derive(Debug, Clone, Default)]
pub struct Pauses {
    settings: Settings,
    signals: Signals,
    user_paused: bool,
    overridden: Vec<PauseReason>,
}

impl Pauses {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings,
            ..Self::default()
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn decision(&self) -> Decision {
        if self.user_paused {
            return Decision::Pause(PauseReason::User);
        }
        automatic(self.signals, self.settings)
            .find(|reason| !self.overridden.contains(reason))
            .map_or(Decision::Play, Decision::Pause)
    }

    pub fn set_signals(&mut self, signals: Signals) {
        self.signals = signals;
        let active: Vec<_> = automatic(signals, Settings::default()).collect();
        self.overridden.retain(|reason| active.contains(reason));
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    pub fn pause_by_user(&mut self) {
        self.user_paused = true;
    }

    pub fn resume_by_user(&mut self) {
        self.user_paused = false;
        self.overridden = automatic(self.signals, self.settings).collect();
    }

    /// Um wallpaper novo começa sem a pausa do usuário nem motivos desfeitos.
    pub fn reset(&mut self) {
        self.user_paused = false;
        self.overridden.clear();
    }
}

/// Se já passou tempo bastante em pausa para liberar o decodificador.
/// `after` é `RELEASE_AFTER`, menos em testes e medições.
pub fn should_release(paused_for: Duration, after: Duration, settings: Settings) -> bool {
    settings.release_when_paused && paused_for >= after
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals(screen_locked: bool, fullscreen_app: bool, on_battery: bool) -> Signals {
        Signals {
            screen_locked,
            fullscreen_app,
            on_battery,
        }
    }

    #[test]
    fn sem_sinal_toca() {
        assert_eq!(Pauses::default().decision(), Decision::Play);
    }

    #[test]
    fn pausa_do_usuario_vence_as_outras() {
        let mut pauses = Pauses::new(Settings::default());
        pauses.set_signals(signals(true, true, true));
        pauses.pause_by_user();
        assert_eq!(pauses.decision(), Decision::Pause(PauseReason::User));
    }

    #[test]
    fn ordem_dos_motivos_automaticos() {
        let mut pauses = Pauses::new(Settings::default());
        pauses.set_signals(signals(true, true, true));
        assert_eq!(pauses.decision(), Decision::Pause(PauseReason::Locked));
        pauses.set_signals(signals(false, true, true));
        assert_eq!(pauses.decision(), Decision::Pause(PauseReason::Fullscreen));
        pauses.set_signals(signals(false, false, true));
        assert_eq!(pauses.decision(), Decision::Pause(PauseReason::Battery));
    }

    #[test]
    fn preferencias_desligadas_nao_pausam() {
        let mut pauses = Pauses::new(Settings {
            pause_on_fullscreen: false,
            pause_on_battery: false,
            pause_on_lock: false,
            ..Settings::default()
        });
        pauses.set_signals(signals(true, true, true));
        assert_eq!(pauses.decision(), Decision::Play);
    }

    #[test]
    fn retomar_desfaz_a_pausa_automatica_ate_o_sinal_voltar() {
        let mut pauses = Pauses::new(Settings::default());
        pauses.set_signals(signals(false, false, true));
        pauses.resume_by_user();
        assert_eq!(pauses.decision(), Decision::Play);

        // Outro motivo ainda pausa.
        pauses.set_signals(signals(false, true, true));
        assert_eq!(pauses.decision(), Decision::Pause(PauseReason::Fullscreen));
        pauses.set_signals(signals(false, false, true));
        assert_eq!(pauses.decision(), Decision::Play);

        // Ligar na tomada e tirar de novo volta a pausar.
        pauses.set_signals(signals(false, false, false));
        pauses.set_signals(signals(false, false, true));
        assert_eq!(pauses.decision(), Decision::Pause(PauseReason::Battery));
    }

    #[test]
    fn wallpaper_novo_esquece_a_pausa_do_usuario() {
        let mut pauses = Pauses::new(Settings::default());
        pauses.pause_by_user();
        pauses.reset();
        assert_eq!(pauses.decision(), Decision::Play);
    }

    #[test]
    fn condicao_desconhecida_conta_como_ausente() {
        let signals = Signals::from(Conditions {
            fullscreen: None,
            on_battery: Some(true),
            screen_locked: None,
            dark: None,
        });
        assert_eq!(signals, self::signals(false, false, true));
    }

    #[test]
    fn libera_o_decodificador_aos_cinco_minutos_se_ligado() {
        let on = Settings::default();
        assert!(!should_release(Duration::from_secs(299), RELEASE_AFTER, on));
        assert!(should_release(Duration::from_secs(300), RELEASE_AFTER, on));
        let off = Settings {
            release_when_paused: false,
            ..on
        };
        assert!(!should_release(
            Duration::from_secs(3600),
            RELEASE_AFTER,
            off
        ));
    }
}
