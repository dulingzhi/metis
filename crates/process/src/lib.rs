//! 外部进程的唯一入口（AGENTS.md 铁律 11）。
//!
//! 别处一律不得直接 `std::process::Command`（`clippy.toml` 的 `disallowed-methods` 会拦住）。
//!
//! 契约：
//! - stdin 默认关闭，绝不继承；
//! - 每次执行有墙钟 + 空闲双截止时间；
//! - 输出有上限并在独立线程排空；
//! - 超时 / 取消 / drop 都终止**进程组**（Unix）或 **Job Object**（Windows）；
//! - 失败变成带补救建议的 [`metis_core::MetisError`]，不是 panic，也不是挂死。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod provider;
pub mod run;
pub mod which;

pub use provider::{CapturedRequest, FakeProvider, RoleHit};
pub use run::{Exec, ExecOutcome, StdinPolicy};
pub use which::{PidProbe, find_program};

/// 默认墙钟上限：2 分钟。
pub const DEFAULT_WALL_CLOCK: std::time::Duration = std::time::Duration::from_secs(120);
/// 默认空闲上限：30 秒无输出即判定卡住。
pub const DEFAULT_IDLE: std::time::Duration = std::time::Duration::from_secs(30);
/// stdout 上限：16 MiB。
pub const STDOUT_CAP: usize = 16 * 1024 * 1024;
/// stderr 上限：1 MiB。
pub const STDERR_CAP: usize = 1024 * 1024;
