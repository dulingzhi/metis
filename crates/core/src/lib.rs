//! Metis 的域模型与错误契约。
//!
//! 这一层**没有任何 I/O**：不读文件、不看环境变量、不起进程。所有类型都是纯数据 + 纯函数，
//! 因此可以被上面任何一层复用，也可以在测试里随手构造。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod activation;
pub mod channel;
pub mod error;
pub mod harness;
pub mod i18n;
pub mod mode;
pub mod persona;

pub use activation::{
    Activation, ActivationContext, ActivationSource, ActivationState, BlockReason, Override, Resolved, Source, resolve_activation, resolve_configured,
};
pub use channel::{Channel, ChannelState, Effect};
pub use error::{ErrorKind, MetisError, Result};
pub use harness::HarnessId;
pub use mode::Mode;
pub use persona::PersonaId;
