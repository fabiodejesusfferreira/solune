//! Memória dos processos do Solune, lida do `/proc` no Linux.
//!
//! O Solune são vários processos: o daemon que toca o wallpaper, o app
//! (janela e bandeja) e os processos da webview, filhos do app. Para o
//! total ser o que o Solune ocupa, cada um entra com a sua PSS: a memória
//! compartilhada entre processos, como as bibliotecas, é dividida entre
//! quem a usa, então somar não conta a mesma página duas vezes. A RSS conta
//! a página inteira em cada processo que a mapeia, e a soma passa do real.
//!
//! A PSS vem de `/proc/<pid>/smaps_rollup`, que só o dono do processo lê.
//! Se algum processo não deixa ler, todos entram pela RSS
//! (`/proc/<pid>/status`), para o total não misturar as duas medidas.
//! Fora do Linux, nada é medido ainda.

use serde::Serialize;

/// Como a memória foi medida.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Metric {
    /// Proportional set size: a parte compartilhada é dividida.
    Pss,
    /// Resident set size: a parte compartilhada conta inteira em cada processo.
    Rss,
}

/// Um processo do sistema, como o `/proc/<pid>/stat` descreve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Process {
    pub pid: u32,
    pub ppid: u32,
    /// O `comm` do kernel: o nome do executável, cortado em 15 bytes.
    pub name: String,
}

/// Lê pid, pai e nome de `/proc/<pid>/stat`.
pub fn parse_stat(text: &str) -> Option<Process> {
    // O nome, entre parênteses, pode ter espaços e parênteses: vai do
    // primeiro '(' ao último ')'. Depois vêm o estado e o pid do pai.
    let (pid, rest) = text.split_once(" (")?;
    let (name, fields) = rest.rsplit_once(')')?;
    let ppid = fields.split_whitespace().nth(1)?.parse().ok()?;
    Some(Process {
        pid: pid.trim().parse().ok()?,
        ppid,
        name: name.to_owned(),
    })
}

/// Valor em bytes de uma linha `Chave:   123 kB` de `smaps_rollup` ou `status`.
pub fn parse_kib(text: &str, key: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?;
        let kib: u64 = value.trim().strip_suffix("kB")?.trim().parse().ok()?;
        kib.checked_mul(1024)
    })
}

/// Os descendentes de `root` (filhos, netos e assim por diante), sem ele.
/// Pula `skip` e tudo abaixo dele.
pub fn descendants(all: &[Process], root: u32, skip: Option<u32>) -> Vec<&Process> {
    let mut found = Vec::new();
    let mut parents = vec![root];
    while let Some(parent) = parents.pop() {
        for process in all {
            // As leituras do /proc não são do mesmo instante: com um pid
            // reaproveitado no meio, a árvore pode fechar um ciclo.
            let seen = process.pid == root || found.iter().any(|f: &&Process| f.pid == process.pid);
            if process.ppid == parent && !seen && Some(process.pid) != skip {
                parents.push(process.pid);
                found.push(process);
            }
        }
    }
    found
}

/// Mede um grupo de processos com a mesma métrica: PSS se todos deixam ler,
/// senão RSS. Devolve a métrica e a memória de cada pid, na ordem pedida;
/// `None` para um processo que saiu no meio da leitura. `None` no todo
/// quando o sistema não informa.
pub fn measure(pids: &[u32]) -> Option<(Metric, Vec<Option<u64>>)> {
    let pss: Vec<Option<u64>> = pids.iter().map(|&pid| sys::pss(pid)).collect();
    let rss: Vec<Option<u64>> = pids.iter().map(|&pid| sys::rss(pid)).collect();
    if rss.iter().all(Option::is_none) {
        return None;
    }
    // Sem PSS mas com RSS: o processo existe e não deixa ler a PSS.
    let unreadable = pss
        .iter()
        .zip(&rss)
        .any(|(p, r)| p.is_none() && r.is_some());
    Some(if unreadable {
        (Metric::Rss, rss)
    } else {
        (Metric::Pss, pss)
    })
}

/// Todos os processos do sistema. Vazio fora do Linux.
pub fn processes() -> Vec<Process> {
    sys::processes()
}

#[cfg(target_os = "linux")]
mod sys {
    use super::{Process, parse_kib, parse_stat};

    pub fn pss(pid: u32) -> Option<u64> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
        parse_kib(&text, "Pss")
    }

    pub fn rss(pid: u32) -> Option<u64> {
        let text = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        parse_kib(&text, "VmRSS")
    }

    pub fn processes() -> Vec<Process> {
        let Ok(dir) = std::fs::read_dir("/proc") else {
            return Vec::new();
        };
        dir.flatten()
            .filter(|entry| entry.file_name().to_str().is_some_and(is_pid))
            .filter_map(|entry| std::fs::read_to_string(entry.path().join("stat")).ok())
            .filter_map(|text| parse_stat(&text))
            .collect()
    }

    fn is_pid(name: &str) -> bool {
        !name.is_empty() && name.bytes().all(|b| b.is_ascii_digit())
    }
}

#[cfg(not(target_os = "linux"))]
mod sys {
    use super::Process;

    pub fn pss(_pid: u32) -> Option<u64> {
        None
    }

    pub fn rss(_pid: u32) -> Option<u64> {
        None
    }

    pub fn processes() -> Vec<Process> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(pid: u32, ppid: u32, name: &str) -> Process {
        Process {
            pid,
            ppid,
            name: name.into(),
        }
    }

    #[test]
    fn le_o_stat_com_nome_estranho() {
        let text = "4242 (Web Content) (x)) S 4100 4242 4100 0 -1 4194560 1 0";
        assert_eq!(
            parse_stat(text),
            Some(process(4242, 4100, "Web Content) (x)"))
        );
        assert_eq!(parse_stat("lixo"), None);
    }

    #[test]
    fn le_pss_e_rss_em_kb() {
        let rollup = "55d0-7ffe ---p 00000000 00:00 0 [rollup]\nRss:  204800 kB\n\
                      Pss:  121344 kB\nPss_Anon:  90000 kB\nShared_Clean: 80000 kB\n";
        assert_eq!(parse_kib(rollup, "Pss"), Some(121_344 * 1024));
        assert_eq!(parse_kib(rollup, "Rss"), Some(204_800 * 1024));
        let status = "Name:\tsolune\nVmRSS:\t  117760 kB\nRssAnon:\t 50000 kB\n";
        assert_eq!(parse_kib(status, "VmRSS"), Some(117_760 * 1024));
        assert_eq!(parse_kib(status, "Pss"), None);
    }

    #[test]
    fn descendentes_pulam_o_daemon() {
        let all = [
            process(1, 0, "systemd"),
            process(100, 1, "solune"),
            process(101, 100, "WebKitWebProces"),
            process(102, 100, "WebKitNetworkPr"),
            process(103, 100, "solune-daemon"),
            process(104, 103, "filho-do-daemon"),
            process(105, 101, "neto"),
            process(200, 1, "outro"),
        ];
        let mut pids: Vec<u32> = descendants(&all, 100, Some(103))
            .iter()
            .map(|p| p.pid)
            .collect();
        pids.sort_unstable();
        assert_eq!(pids, [101, 102, 105]);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn mede_o_proprio_processo() {
        let me = std::process::id();
        let (_, bytes) = measure(&[me]).unwrap();
        assert!(bytes[0].unwrap_or(0) > 0);
        assert!(processes().iter().any(|p| p.pid == me));
    }
}
