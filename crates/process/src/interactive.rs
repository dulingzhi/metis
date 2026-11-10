//! 交互式启动：把终端交给宿主。
//!
//! 这是 AGENTS.md 铁律 11 的**唯一例外**：`metis launch --exec` 是用户主动发起的
//! 交互式会话（`claude`、`codex` 这类 TUI），必须继承 stdin/stdout/stderr；
//! 关闭 stdin 或截获输出会让会话直接不可用。自动化外部命令仍然必须走 [`crate::Exec`]
//! （关闭 stdin、双截止时间、输出上限）。
//!
//! 本文件与 `run.rs` 同属外部进程的唯一入口，因此显式豁免 clippy 的 `Command::new`。

#![allow(clippy::disallowed_methods)]

use std::path::Path;
use std::process::ExitStatus;

use metis_core::{ErrorKind, MetisError, Result};

/// 以**继承终端**的方式启动宿主，阻塞到它退出，返回退出码。
///
/// - Unix：`exec` 替换当前进程，成功时永远不返回（退出码由宿主直接交给 shell）。
/// - Windows：spawn + wait；stdin/stdout/stderr 默认继承。
pub fn run_interactive(program: &Path, args: &[String]) -> Result<i32> {
    let mut command = std::process::Command::new(program);
    command.args(args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // 成功时进程被宿主替换，不会返回；返回只可能是启动失败。
        let error = command.exec();
        return Err(process_error(program, &error));
    }

    #[cfg(not(unix))]
    {
        let status = command.status().map_err(|error| process_error(program, &error))?;
        Ok(exit_code(status))
    }
}

#[cfg(not(unix))]
fn exit_code(status: ExitStatus) -> i32 {
    status.code().unwrap_or(1)
}

fn process_error(program: &Path, error: &std::io::Error) -> MetisError {
    MetisError::new(ErrorKind::ProcessFailed, format!("启动失败：{}（{error}）", program.display()))
        .with_remediation("确认程序已安装且可执行；或者从这条通道里撤掉")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_program_is_a_process_failure_not_a_panic() {
        let error = process_error(Path::new("definitely-missing-metis-harness.exe"), &std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(error.kind(), ErrorKind::ProcessFailed);
        assert!(error.remediation().is_some(), "错误必须带补救建议");
    }
}
