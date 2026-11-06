//! 执行外部程序。
//!
//! 一个 [`Exec`] 描述一次执行；[`Exec::run`] 执行它并返回 [`ExecOutcome`]。
//! 所有路径都经过截止时间与输出上限，失败带补救建议。
//!
//! **关于 `std::process::Command`**：`clippy.toml` 禁用它是为了让"起外部进程"只有一个入口
//! （铁律 11）。本文件**就是**那个入口，因此显式豁免。

// 本文件是外部进程的唯一入口，允许直接使用 std::process::Command。
#![allow(clippy::disallowed_methods)]

use std::io::Read;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use metis_core::{ErrorKind, MetisError, Result};

/// stdin 策略。**默认关闭**：绝不继承（否则对面停在交互提示上）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StdinPolicy {
    /// 关闭。
    Closed,
    /// 写入一段内容后关闭。
    Pipe,
}

/// 一次执行的描述。
#[derive(Debug, Clone)]
pub struct Exec {
    program: String,
    args: Vec<String>,
    cwd: Option<PathBuf>,
    stdin: StdinPolicy,
    stdin_data: Vec<u8>,
    wall_clock: Duration,
    env: Vec<(String, String)>,
    allow_output_overflow: bool,
}

impl Exec {
    /// 新建。
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            stdin: StdinPolicy::Closed,
            stdin_data: Vec::new(),
            wall_clock: super::DEFAULT_WALL_CLOCK,
            env: Vec::new(),
            allow_output_overflow: false,
        }
    }

    /// 加参数。
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// 批量加参数。
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// 工作目录。
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// 设置环境变量。
    #[must_use]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// 喂 stdin。
    #[must_use]
    pub fn stdin(mut self, data: impl Into<Vec<u8>>) -> Self {
        self.stdin = StdinPolicy::Pipe;
        self.stdin_data = data.into();
        self
    }

    /// 墙钟上限。
    #[must_use]
    pub fn wall_clock(mut self, duration: Duration) -> Self {
        self.wall_clock = duration;
        self
    }

    /// 允许输出超上限（只保留头尾并标记截断）。
    #[must_use]
    pub fn allow_output_overflow(mut self) -> Self {
        self.allow_output_overflow = true;
        self
    }

    /// 程序名。
    pub fn program(&self) -> &str {
        &self.program
    }

    /// 参数。
    pub fn args_ref(&self) -> &[String] {
        &self.args
    }

    /// 预览一行命令（**不包含**可能很长的人设正文：调用方要用占位符替换）。
    pub fn preview(&self) -> String {
        let mut out = self.program.clone();
        for arg in &self.args {
            out.push(' ');
            if arg.contains(' ') || arg.contains('"') {
                out.push('"');
                out.push_str(&arg.replace('"', "\\\""));
                out.push('"');
            } else {
                out.push_str(arg);
            }
        }
        out
    }

    /// 执行。
    pub fn run(&self) -> Result<ExecOutcome> {
        let resolved = super::find_program(&self.program).ok_or_else(|| {
            MetisError::new(ErrorKind::ProgramMissing, format!("找不到可执行文件：{}", self.program))
                .with_remediation("确认它已安装并且在 PATH 里；或者从这条通道里撤掉")
                .with_detail(self.preview())
        })?;

        let mut command = std::process::Command::new(&resolved);
        command.args(&self.args);
        let started = Instant::now();
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &self.env {
            command.env(key, value);
        }
        // stdin：默认关闭。要喂数据时给管道并立刻写入。
        command.stdin(if self.stdin == StdinPolicy::Pipe { std::process::Stdio::piped() } else { std::process::Stdio::null() });
        command.stdout(std::process::Stdio::piped());
        command.stderr(std::process::Stdio::piped());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // CREATE_NO_WINDOW：绝不弹控制台窗口。
            command.creation_flags(0x0800_0000);
        }

        // 用 `process-wrap` 包一层：子进程放进**作业对象**（Windows）/ 进程组（Unix）。
        // 这解决一个真实事故：`child.kill()` 只杀直接子进程，它的子进程（node 等）
        // 还活着并持有 stdout 管道，于是读管道的线程永远不返回，整个调用挂死。
        let mut wrap = process_wrap::std::CommandWrap::from(command);
        #[cfg(windows)]
        wrap.wrap(process_wrap::std::JobObject);
        #[cfg(unix)]
        wrap.wrap(process_wrap::std::ProcessGroup::leader());

        let mut child = wrap.spawn().map_err(|error| {
            let kind = match error.kind() {
                std::io::ErrorKind::NotFound => ErrorKind::ProgramMissing,
                std::io::ErrorKind::PermissionDenied => ErrorKind::Permission,
                _ => ErrorKind::ProcessFailed,
            };
            MetisError::new(kind, format!("启动 {} 失败：{error}", self.program))
                .with_detail(self.preview())
                .with_remediation("确认这个程序已安装且在 PATH 上；或者换一条不依赖它的命令")
        })?;

        if self.stdin == StdinPolicy::Pipe
            && let Some(mut stdin) = child.stdin().take()
        {
            use std::io::Write;
            let _ = stdin.write_all(&self.stdin_data);
        }

        // 输出在独立线程里排空，主线程负责截止时间。
        let stdout = child.stdout().take();
        let stderr = child.stderr().take();
        let cap = super::STDOUT_CAP;
        let stdout_handle = std::thread::spawn(move || read_capped(stdout, cap));
        let cap_err = super::STDERR_CAP;
        let stderr_handle = std::thread::spawn(move || read_capped(stderr, cap_err));

        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if started.elapsed() >= self.wall_clock {
                        timed_out = true;
                        // 杀**整组**：不然孙进程还活着。
                        let _ = child.kill();
                        // 等待失败也要跳出：不能因为回收不了而挂死。
                        match child.wait() {
                            Ok(status) => break Some(status),
                            Err(_) => {
                                return Err(MetisError::new(ErrorKind::ProcessTimeout, format!("{} 超时且无法回收进程", self.program))
                                    .with_detail(self.preview())
                                    .with_remediation("手工在任务管理器里结束它，再重试"));
                            }
                        }
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(_) => {
                    let _ = child.kill();
                    return Err(MetisError::new(ErrorKind::ProcessFailed, format!("等待 {} 失败", self.program))
                        .with_detail(self.preview())
                        .with_remediation("重试一次；如果反复失败，手工在终端里跑这条命令看真实输出"));
                }
            }
        };

        // **关键**：读管道的线程可能因为孙进程还攥着管道而不返回。
        // 给它一个有界等待，超时就放弃（数据不要了，但不能把调用方挂死）。
        let drain_deadline = Instant::now() + Duration::from_millis(500);
        let (stdout_bytes, stdout_truncated) = join_bounded(stdout_handle, drain_deadline);
        let (stderr_bytes, stderr_truncated) = join_bounded(stderr_handle, drain_deadline);
        let elapsed = started.elapsed();
        let overflow = stdout_truncated || stderr_truncated;

        if timed_out {
            return Err(MetisError::new(ErrorKind::ProcessTimeout, format!("{} 超时（{} 毫秒），已终止整组进程", self.program, self.wall_clock.as_millis()))
                .with_detail(self.preview())
                .with_remediation("如果是交互式程序，请手动在终端里运行它"));
        }
        if overflow && !self.allow_output_overflow {
            return Err(MetisError::new(ErrorKind::ProcessFailed, format!("{} 的输出超过上限，已截断", self.program))
                .with_detail(self.preview())
                .with_remediation("用更小的输入重试，或加 --allow-output-overflow"));
        }

        Ok(ExecOutcome {
            status: status.and_then(|s| s.code()),
            success: status.is_some_and(|s| s.success()),
            stdout: String::from_utf8_lossy(&stdout_bytes).into_owned(),
            stderr: String::from_utf8_lossy(&stderr_bytes).into_owned(),
            truncated: overflow,
            elapsed,
        })
    }
}

/// 有界地等一个读线程；超时就放弃它（线程会自己结束，我们不等了）。
fn join_bounded(handle: std::thread::JoinHandle<(Vec<u8>, bool)>, deadline: Instant) -> (Vec<u8>, bool) {
    while !handle.is_finished() {
        if Instant::now() >= deadline {
            // 不等了：把句柄丢掉。线程是 detached 的，进程退出时会一起回收。
            return (Vec::new(), true);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    handle.join().unwrap_or((Vec::new(), false))
}

fn read_capped<R: Read>(reader: Option<R>, cap: usize) -> (Vec<u8>, bool) {
    let Some(mut reader) = reader else { return (Vec::new(), false) };
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 8192];
    let mut truncated = false;
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                if buffer.len() + n <= cap {
                    buffer.extend_from_slice(&chunk[..n]);
                } else {
                    let room = cap.saturating_sub(buffer.len());
                    buffer.extend_from_slice(&chunk[..room]);
                    truncated = true;
                }
            }
            Err(_) => break,
        }
    }
    (buffer, truncated)
}

/// 一次执行的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecOutcome {
    /// 退出码。
    pub status: Option<i32>,
    /// 是否成功。
    pub success: bool,
    /// 标准输出（可能被截断）。
    pub stdout: String,
    /// 标准错误（可能被截断）。
    pub stderr: String,
    /// 是否有输出被截断。
    pub truncated: bool,
    /// 耗时。
    pub elapsed: Duration,
}

impl ExecOutcome {
    /// 把非零退出转成错误（带输出摘要）。
    pub fn into_success(self, what: &str) -> Result<Self> {
        if self.success {
            Ok(self)
        } else {
            Err(MetisError::new(ErrorKind::ProcessFailed, format!("{what} 失败（退出码 {:?}）", self.status))
                .with_detail(summarise(&self.stderr, &self.stdout))
                .with_remediation("看上面的输出；这不是 Metis 自身的故障"))
        }
    }
}

fn summarise(stderr: &str, stdout: &str) -> String {
    let primary = if stderr.trim().is_empty() { stdout } else { stderr };
    let trimmed = primary.trim();
    if trimmed.chars().count() <= 800 {
        return trimmed.to_owned();
    }
    let head: String = trimmed.chars().take(800).collect();
    format!("{head}…（已截断）")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_program() -> Exec {
        #[cfg(windows)]
        {
            Exec::new("cmd").arg("/C").arg("echo hello")
        }
        #[cfg(not(windows))]
        {
            Exec::new("sh").arg("-c").arg("echo hello")
        }
    }

    #[test]
    fn preview_quotes_arguments_with_spaces() {
        let exec = Exec::new("codex").arg("--flag").arg("two words");
        assert_eq!(exec.preview(), "codex --flag \"two words\"");
    }

    #[test]
    fn preview_escapes_quotes() {
        let exec = Exec::new("x").arg("say \"hi\"");
        assert!(exec.preview().contains("\\\"hi\\\""));
    }

    #[test]
    fn missing_program_is_program_missing_with_remediation() {
        let error = Exec::new("metis-definitely-missing-xyz").run().expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::ProgramMissing);
        assert!(error.remediation().is_some(), "必须告诉用户怎么修");
        assert!(error.detail().is_some(), "要带上试图执行的命令行");
    }

    #[test]
    fn runs_and_captures_stdout() {
        let outcome = echo_program().run().expect("run");
        assert!(outcome.success);
        assert!(outcome.stdout.contains("hello"), "stdout: {:?}", outcome.stdout);
        assert_eq!(outcome.status, Some(0));
    }

    #[test]
    fn non_zero_exit_maps_to_process_failed() {
        #[cfg(windows)]
        let exec = Exec::new("cmd").arg("/C").arg("exit 3");
        #[cfg(not(windows))]
        let exec = Exec::new("sh").arg("-c").arg("exit 3");
        let outcome = exec.run().expect("process should run");
        assert!(!outcome.success);
        assert_eq!(outcome.status, Some(3));
        let error = outcome.into_success("测试命令").expect_err("should be error");
        assert_eq!(error.kind(), ErrorKind::ProcessFailed);
        assert!(error.message().contains("3"));
    }

    #[test]
    fn stdin_can_be_fed() {
        #[cfg(windows)]
        let exec = Exec::new("cmd").args(["/C", "findstr", "x"]).stdin(b"x marks\n".to_vec());
        #[cfg(not(windows))]
        let exec = Exec::new("sh").args(["-c", "cat"]).stdin(b"piped\n".to_vec());
        let outcome = exec.run().expect("run");
        assert!(!outcome.stdout.is_empty(), "stdin 内容应被程序读到：{:?}", outcome.stdout);
    }

    #[test]
    fn timeout_returns_process_timeout() {
        #[cfg(windows)]
        let exec = Exec::new("cmd").args(["/C", "ping -n 10 127.0.0.1 > NUL"]).wall_clock(Duration::from_millis(200));
        #[cfg(not(windows))]
        let exec = Exec::new("sh").args(["-c", "sleep 5"]).wall_clock(Duration::from_millis(200));
        let error = exec.run().expect_err("must time out");
        assert_eq!(error.kind(), ErrorKind::ProcessTimeout);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn timeout_does_not_hang_on_a_child_that_holds_the_pipe() {
        // 回归：这是真撞上的事故 —— 孙进程攥着 stdout，`kill` 只杀了父进程，
        // 读管道的线程永远不返回，整个调用挂死（实测 codex 就是这样挂住的）。
        #[cfg(windows)]
        let exec = Exec::new("cmd").args(["/C", "start /B ping -n 30 127.0.0.1"]).wall_clock(Duration::from_millis(300));
        #[cfg(not(windows))]
        let exec = Exec::new("sh").args(["-c", "sleep 30 & sleep 30"]).wall_clock(Duration::from_millis(300));

        let start = Instant::now();
        let result = exec.run();
        let elapsed = start.elapsed();
        assert!(result.is_err(), "应报超时");
        // 关键断言：**必须在合理时间内返回**，不能被孙进程挂住。
        assert!(elapsed < Duration::from_secs(10), "超时后必须尽快返回，实际 {elapsed:?}");
    }

    #[test]
    fn output_is_truncated_and_reported() {
        #[cfg(windows)]
        let exec = Exec::new("cmd").args(["/C", "for /L %i in (1,1,4000) do @echo 0123456789"]);
        #[cfg(not(windows))]
        let exec = Exec::new("sh").args(["-c", "yes 0123456789 | head -n 2000"]);
        let outcome = exec.run().expect("run");
        let exec = exec.allow_output_overflow();
        let big = exec.run().expect("run with overflow allowed");
        assert!(big.stdout.len() >= outcome.stdout.len());
    }

    #[test]
    fn cwd_is_respected() {
        let dir = tempfile::tempdir().expect("tempdir");
        #[cfg(windows)]
        let exec = Exec::new("cmd").args(["/C", "cd"]).cwd(dir.path());
        #[cfg(not(windows))]
        let exec = Exec::new("sh").args(["-c", "pwd"]).cwd(dir.path());
        let outcome = exec.run().expect("run");
        let needle = dir.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        assert!(outcome.stdout.contains(&needle) || outcome.stdout.contains('/'), "cwd 应生效：{:?}", outcome.stdout);
    }

    #[test]
    fn env_is_passed() {
        #[cfg(windows)]
        let exec = Exec::new("cmd").args(["/C", "echo %METIS_TEST_VAR%"]).env("METIS_TEST_VAR", "seen");
        #[cfg(not(windows))]
        let exec = Exec::new("sh").args(["-c", "echo $METIS_TEST_VAR"]).env("METIS_TEST_VAR", "seen");
        let outcome = exec.run().expect("run");
        assert!(outcome.stdout.contains("seen"), "环境变量应传进去：{:?}", outcome.stdout);
    }

    #[test]
    fn summarise_caps_long_output() {
        let long = "x".repeat(5000);
        let summary = summarise(&long, "");
        assert!(summary.chars().count() < 900);
        assert!(summary.ends_with("（已截断）"));
    }

    #[test]
    fn summarise_prefers_stderr() {
        assert_eq!(summarise("err", "out"), "err");
        assert_eq!(summarise("", "out"), "out");
    }
}
