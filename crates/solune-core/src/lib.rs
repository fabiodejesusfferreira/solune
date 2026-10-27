//! Lógica do Solune que dá para testar sem GPU, compositor ou libmpv.
//!
//! O daemon junta os sinais do sistema (tela cheia, bateria, tela
//! bloqueada) e pergunta aqui o que fazer; quem executa é ele. O app e o
//! daemon medem aqui a memória dos processos do Solune.

pub mod memory;
pub mod policy;
