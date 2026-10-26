//! A lista em uso: qual item toca agora e quando trocar.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use solune_ipc::{Item, Playlist, TimeOfDay};

#[derive(Debug)]
pub struct Show {
    pub list: Playlist,
    /// Ordem dos itens; embaralhada a cada volta com `shuffle`.
    order: Vec<usize>,
    /// Onde estamos em `order`.
    pub position: usize,
    since: Instant,
}

impl Show {
    /// `list` não pode ser vazia. `position` dá a volta se passar do fim.
    pub fn new(list: Playlist, position: usize) -> Self {
        let mut show = Self {
            order: (0..list.items.len()).collect(),
            position: position % list.items.len().max(1),
            list,
            since: Instant::now(),
        };
        if show.list.shuffle {
            shuffle(&mut show.order);
        }
        show
    }

    pub fn item(&self) -> &Item {
        &self.list.items[self.order[self.position]]
    }

    /// Quantos itens a lista tem.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn advance(&mut self) {
        self.position = (self.position + 1) % self.order.len();
        if self.position == 0 && self.list.shuffle {
            shuffle(&mut self.order);
        }
        self.since = Instant::now();
    }

    /// Passou o intervalo do carrossel.
    pub fn due(&self) -> bool {
        self.order.len() > 1
            && self.list.every_minutes > 0
            && self.since.elapsed() >= Duration::from_secs(u64::from(self.list.every_minutes) * 60)
    }
}

/// Fisher-Yates com um xorshift semeado pelo relógio: não precisa ser
/// imprevisível, só variar.
fn shuffle(order: &mut [usize]) {
    let mut seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64)
        | 1;
    for i in (1..order.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        order.swap(i, (seed % (i as u64 + 1)) as usize);
    }
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    /// POSIX; o crate libc não a declara para este alvo.
    fn tzset();
}

/// A hora local, para a agenda de temas.
pub fn local_time() -> TimeOfDay {
    #[cfg(target_os = "linux")]
    // SAFETY: tzset, time e localtime_r só escrevem nos valores que passamos.
    unsafe {
        // O glibc só lê o fuso na primeira chamada de localtime_r; o daemon
        // roda por semanas e o computador pode mudar de fuso nesse tempo.
        tzset();
        let now = libc::time(std::ptr::null_mut());
        let mut tm: libc::tm = std::mem::zeroed();
        if !libc::localtime_r(&now, &mut tm).is_null()
            && let Some(time) = TimeOfDay::new(tm.tm_hour as u8, tm.tm_min as u8)
        {
            return time;
        }
    }
    // shortcut: fora do Linux vale a hora UTC; trocar pela hora local quando o Windows tiver superfície.
    let minutes = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 60 % 1440);
    TimeOfDay::new((minutes / 60) as u8, (minutes % 60) as u8).unwrap_or(TimeOfDay::MIDNIGHT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(n: usize, shuffle: bool) -> Playlist {
        Playlist {
            items: (0..n)
                .map(|i| Item {
                    path: format!("/{i}.mp4").into(),
                    dark: None,
                })
                .collect(),
            every_minutes: 5,
            shuffle,
        }
    }

    #[test]
    fn avanca_e_da_a_volta() {
        let mut show = Show::new(list(3, false), 4);
        assert_eq!(show.item().path.to_str(), Some("/1.mp4"));
        show.advance();
        show.advance();
        assert_eq!(show.item().path.to_str(), Some("/0.mp4"));
        assert!(!show.due());
    }

    #[test]
    fn embaralhar_passa_por_todos() {
        let mut show = Show::new(list(5, true), 0);
        let mut seen: Vec<_> = (0..5)
            .map(|_| {
                let path = show.item().path.clone();
                show.advance();
                path
            })
            .collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 5);
    }

    #[test]
    fn um_item_so_nunca_troca() {
        let mut one = list(1, false);
        one.every_minutes = 0;
        let show = Show::new(one, 0);
        assert!(!show.due());
    }
}
