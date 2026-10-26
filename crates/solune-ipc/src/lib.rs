//! Protocolo entre o app do Solune e o daemon.
//!
//! O app só conhece o daemon por este crate: as mensagens ([`protocol`]),
//! o framing de uma mensagem JSON por linha ([`codec`]) e o socket local
//! ([`transport`], com a feature `transport`).

pub mod protocol;
pub mod theme;

#[cfg(feature = "transport")]
pub mod codec;
#[cfg(feature = "transport")]
pub mod transport;

pub use protocol::*;
pub use theme::{Schedule, Theme, TimeOfDay};
