//! 人设库：`<home>/personas/<id>.md`。
//!
//! 格式见 `docs/DATA-FORMAT.md` §3。两条硬规则：
//! 1. **只有正文进入提示词**，名称与描述只用于管理；
//! 2. 保存时**保留表单未涉及的元数据**（包括我们不认识的键）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod meta;
pub mod parse;
pub mod store;

pub use meta::Meta;
pub use parse::{parse_persona, render_persona};
pub use store::{Persona, PersonaStore, StoreError};

/// 内置模板：中性人设。新建人设时用它起步。
pub const TEMPLATE_NEUTRAL: &str = include_str!("../assets/neutral.md");
