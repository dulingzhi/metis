//! 通道实现：把"用户选了什么"翻译成宿主能读到的形式。
//!
//! 四条通道（`docs/ARCHITECTURE.md` §6）：
//! - [`blocks`] 托管块：在既有文件里插入带标记的块（install / project 共用）；
//! - [`install`] 常驻配置 / 插件；
//! - [`shim`] 裸命令接管的包装脚本；
//! - [`project`] 项目规则文件。
//!
//! 每条通道都必须：**幂等**（连做两次，第二次磁盘哈希不变）、**可逆**（撤销后逐字节还原）、
//! **诚实**（失败带补救建议）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod blocks;
pub mod dsh_patch;
pub mod install;
pub mod project;
pub mod shim;
pub mod slash;

pub use blocks::{BlockMarker, StripOutcome};
pub use install::InstallOutcome;
pub use shim::ShimOutcome;
pub use slash::SlashOutcome;

/// 托管块的起止标记（Markdown 用注释，TOML 用井号）。
pub const BLOCK_BEGIN_MD: &str = "<!-- metis:begin -->";
/// Markdown 块的结束标记。
pub const BLOCK_END_MD: &str = "<!-- metis:end -->";
/// TOML 块的起止标记。
pub const BLOCK_BEGIN_TOML: &str = "# >>> metis";
/// TOML 块的结束标记。
pub const BLOCK_END_TOML: &str = "# <<< metis";
