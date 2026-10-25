//! Uso de recursos do próprio processo, para a tela Regras e desempenho.
//!
//! No Linux, CPU e memória vêm de `/proc/self`, e a GPU vem do fdinfo do
//! DRM (`/proc/self/fdinfo`), que os drivers i915, xe e amdgpu publicam por
//! processo. CPU e GPU são medidas entre uma leitura e a seguinte, então a
//! primeira leitura não tem esses dois números.

use std::collections::HashMap;
use std::time::Instant;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reading {
    pub cpu_permille: Option<u32>,
    pub gpu_permille: Option<u32>,
    pub memory_bytes: Option<u64>,
    pub gpu_memory_bytes: Option<u64>,
}

#[derive(Debug)]
struct Sample {
    at: Instant,
    cpu_ticks: Option<u64>,
    /// Tempo acumulado de cada motor da GPU, em ns.
    engines: HashMap<String, u64>,
}

#[derive(Debug, Default)]
pub struct Sampler {
    last: Option<Sample>,
}

impl Sampler {
    pub fn read(&mut self) -> Reading {
        let drm = linux::drm();
        let mut reading = Reading {
            memory_bytes: linux::memory_bytes(),
            gpu_memory_bytes: drm.memory_bytes,
            ..Reading::default()
        };
        let now = Sample {
            at: Instant::now(),
            cpu_ticks: linux::cpu_ticks(),
            engines: drm.engines,
        };
        if let Some(last) = &self.last {
            let wall_ns = now.at.duration_since(last.at).as_nanos() as u64;
            if wall_ns > 0 {
                reading.cpu_permille = now
                    .cpu_ticks
                    .zip(last.cpu_ticks)
                    .and_then(|(a, b)| a.checked_sub(b))
                    .and_then(|ticks| {
                        let ns = ticks.checked_mul(1_000_000_000 / linux::ticks_per_second()?)?;
                        Some(permille(ns, wall_ns))
                    });
                reading.gpu_permille = busiest(&last.engines, &now.engines, wall_ns);
            }
        }
        self.last = Some(now);
        reading
    }
}

fn permille(part: u64, whole: u64) -> u32 {
    (part.saturating_mul(1000) / whole.max(1)).min(u32::MAX as u64) as u32
}

/// O motor mais ocupado entre duas leituras, em milésimos do tempo. Os
/// motores (render, vídeo, cópia) trabalham em paralelo, então somar
/// passaria de 100%.
fn busiest(
    before: &HashMap<String, u64>,
    after: &HashMap<String, u64>,
    wall_ns: u64,
) -> Option<u32> {
    if after.is_empty() {
        return None;
    }
    after
        .iter()
        .map(|(engine, &ns)| {
            let delta = ns.saturating_sub(before.get(engine).copied().unwrap_or(ns));
            permille(delta, wall_ns).min(1000)
        })
        .max()
}

#[derive(Debug, Default)]
struct Drm {
    engines: HashMap<String, u64>,
    memory_bytes: Option<u64>,
}

/// Lê o fdinfo de um cliente DRM: motores em ns e memória em bytes. Usa a
/// memória residente (`drm-resident-*`, i915 e xe) e, sem ela, a reservada
/// (`drm-memory-*`, amdgpu).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn parse_fdinfo(text: &str) -> Option<(String, HashMap<String, u64>, Option<u64>)> {
    let mut client = None;
    let mut engines = HashMap::new();
    let (mut resident, mut memory) = (None::<u64>, None::<u64>);
    for line in text.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if key == "drm-client-id" {
            client = Some(value.to_owned());
        } else if let Some(engine) = key.strip_prefix("drm-engine-") {
            if let Some(ns) = value
                .strip_suffix(" ns")
                .and_then(|v| v.trim().parse().ok())
            {
                engines.insert(engine.to_owned(), ns);
            }
        } else if key.starts_with("drm-resident-") {
            resident = Some(resident.unwrap_or(0) + bytes(value)?);
        } else if key.starts_with("drm-memory-") {
            memory = Some(memory.unwrap_or(0) + bytes(value)?);
        }
    }
    Some((client?, engines, resident.or(memory)))
}

/// "123 KiB", "4 MiB" ou "512" (bytes).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn bytes(value: &str) -> Option<u64> {
    let mut parts = value.split_whitespace();
    let n: u64 = parts.next()?.parse().ok()?;
    let unit = match parts.next() {
        None => 1,
        Some("KiB") => 1 << 10,
        Some("MiB") => 1 << 20,
        Some("GiB") => 1 << 30,
        Some(_) => return None,
    };
    n.checked_mul(unit)
}

#[cfg(target_os = "linux")]
mod linux {
    use std::collections::HashSet;

    use super::{Drm, parse_fdinfo};

    pub fn ticks_per_second() -> Option<u64> {
        // SAFETY: sysconf não tem pré-condições.
        let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        (ticks > 0).then_some(ticks as u64)
    }

    /// utime + stime de `/proc/self/stat`, em ticks.
    pub fn cpu_ticks() -> Option<u64> {
        let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
        // O nome do processo, entre parênteses, pode ter espaços: os campos
        // começam depois do último ')'. utime e stime são o 14º e o 15º.
        let fields: Vec<&str> = stat.rsplit_once(')')?.1.split_whitespace().collect();
        let utime: u64 = fields.get(11)?.parse().ok()?;
        let stime: u64 = fields.get(12)?.parse().ok()?;
        Some(utime + stime)
    }

    pub fn memory_bytes() -> Option<u64> {
        let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
        let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
        // SAFETY: sysconf não tem pré-condições.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        (page > 0).then(|| pages * page as u64)
    }

    /// Soma os clientes DRM do processo, contando cada um uma vez (vários
    /// descritores podem apontar para o mesmo cliente).
    pub fn drm() -> Drm {
        let mut drm = Drm::default();
        let Ok(dir) = std::fs::read_dir("/proc/self/fdinfo") else {
            return drm;
        };
        let mut seen = HashSet::new();
        for entry in dir.flatten() {
            let Ok(text) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let Some((client, engines, memory)) = parse_fdinfo(&text) else {
                continue;
            };
            if !seen.insert(client) {
                continue;
            }
            for (engine, ns) in engines {
                *drm.engines.entry(engine).or_default() += ns;
            }
            if let Some(memory) = memory {
                drm.memory_bytes = Some(drm.memory_bytes.unwrap_or(0) + memory);
            }
        }
        drm
    }
}

#[cfg(not(target_os = "linux"))]
mod linux {
    use super::Drm;

    pub fn ticks_per_second() -> Option<u64> {
        None
    }

    pub fn cpu_ticks() -> Option<u64> {
        None
    }

    pub fn memory_bytes() -> Option<u64> {
        None
    }

    pub fn drm() -> Drm {
        Drm::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_o_fdinfo_da_intel() {
        let text = "pos:\t0\nflags:\t02100002\ndrm-driver:\ti915\ndrm-client-id:\t42\n\
                    drm-engine-render:\t1500000 ns\ndrm-engine-video:\t700000 ns\n\
                    drm-total-system0:\t65536 KiB\ndrm-resident-system0:\t47104 KiB\n";
        let (client, engines, memory) = parse_fdinfo(text).unwrap();
        assert_eq!(client, "42");
        assert_eq!(engines["render"], 1_500_000);
        assert_eq!(engines["video"], 700_000);
        assert_eq!(memory, Some(47104 * 1024));
    }

    #[test]
    fn le_o_fdinfo_da_amd_e_ignora_o_que_nao_e_drm() {
        let text = "drm-client-id: 7\ndrm-memory-vram: 2 MiB\ndrm-memory-gtt: 512 KiB\n";
        let (_, _, memory) = parse_fdinfo(text).unwrap();
        assert_eq!(memory, Some(2 * 1024 * 1024 + 512 * 1024));
        assert!(parse_fdinfo("pos: 0\nflags: 0\n").is_none());
    }

    #[test]
    fn gpu_e_o_motor_mais_ocupado() {
        let before = HashMap::from([("render".into(), 0), ("video".into(), 0)]);
        let after = HashMap::from([("render".into(), 100_000_000), ("video".into(), 50_000_000)]);
        assert_eq!(busiest(&before, &after, 1_000_000_000), Some(100));
        assert_eq!(busiest(&before, &HashMap::new(), 1_000_000_000), None);
    }

    #[test]
    fn segunda_leitura_tem_cpu() {
        let mut sampler = Sampler::default();
        let first = sampler.read();
        assert_eq!(first.cpu_permille, None);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let second = sampler.read();
        if cfg!(target_os = "linux") {
            assert!(second.cpu_permille.is_some());
            assert!(second.memory_bytes.unwrap_or(0) > 0);
        }
    }
}
