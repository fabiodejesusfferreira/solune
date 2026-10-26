//! Tema claro ou escuro, e quando trocar.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Light,
    Dark,
}

/// Horário do dia em minutos desde a meia-noite, no fuso local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TimeOfDay(u16);

impl TimeOfDay {
    pub const MIDNIGHT: Self = Self(0);

    /// `None` fora de 00:00 a 23:59.
    pub fn new(hour: u8, minute: u8) -> Option<Self> {
        (hour < 24 && minute < 60).then(|| Self(u16::from(hour) * 60 + u16::from(minute)))
    }
}

/// De onde vem o tema.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum Schedule {
    /// Segue o tema do sistema. É o padrão.
    #[default]
    System,
    /// Sempre o mesmo tema.
    Fixed { theme: Theme },
    /// Claro a partir de `light_at`, escuro a partir de `dark_at`.
    Times {
        light_at: TimeOfDay,
        dark_at: TimeOfDay,
    },
}

impl Schedule {
    /// O tema agora. `system` é o tema que o sistema informa.
    pub fn theme_at(self, now: TimeOfDay, system: Theme) -> Theme {
        match self {
            Self::System => system,
            Self::Fixed { theme } => theme,
            Self::Times { light_at, dark_at } => {
                // O intervalo claro pode atravessar a meia-noite.
                let light = if light_at <= dark_at {
                    light_at <= now && now < dark_at
                } else {
                    now >= light_at || now < dark_at
                };
                if light { Theme::Light } else { Theme::Dark }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(h: u8, m: u8) -> TimeOfDay {
        TimeOfDay::new(h, m).unwrap()
    }

    #[test]
    fn horario_invalido() {
        assert!(TimeOfDay::new(24, 0).is_none());
        assert!(TimeOfDay::new(12, 60).is_none());
    }

    #[test]
    fn segue_o_sistema_por_padrao() {
        let schedule = Schedule::default();
        assert_eq!(schedule.theme_at(t(12, 0), Theme::Dark), Theme::Dark);
    }

    #[test]
    fn horarios_no_mesmo_dia() {
        let schedule = Schedule::Times {
            light_at: t(7, 0),
            dark_at: t(19, 30),
        };
        assert_eq!(schedule.theme_at(t(6, 59), Theme::Light), Theme::Dark);
        assert_eq!(schedule.theme_at(t(7, 0), Theme::Dark), Theme::Light);
        assert_eq!(schedule.theme_at(t(19, 29), Theme::Dark), Theme::Light);
        assert_eq!(schedule.theme_at(t(19, 30), Theme::Light), Theme::Dark);
    }

    #[test]
    fn claro_atravessando_a_meia_noite() {
        let schedule = Schedule::Times {
            light_at: t(22, 0),
            dark_at: t(2, 0),
        };
        assert_eq!(schedule.theme_at(t(23, 0), Theme::Dark), Theme::Light);
        assert_eq!(schedule.theme_at(t(1, 0), Theme::Dark), Theme::Light);
        assert_eq!(schedule.theme_at(t(12, 0), Theme::Light), Theme::Dark);
    }
}
