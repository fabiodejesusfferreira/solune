//! Lógica do Solune que dá para testar sem GPU, compositor ou libmpv.
//!
//! O daemon junta os sinais do sistema (tela cheia, bateria, tela
//! bloqueada) e pergunta aqui o que fazer; quem executa é ele.

pub mod policy;
pub mod theme;
