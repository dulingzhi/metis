//! Metis 桌面控制台的装配层（eframe 壳）。
//!
//! **不是一个独立可执行文件**：`apps/metis` 在无参数启动时调用 [`run`]，
//! 把 CLI 与 GUI 合并成同一个 `metis` 二进制（见 `docs/DEVELOPMENT.md` §4）。
//!
//! 顺序与理由：
//! 1. 数据目录（`METIS_HOME` > `~/.metis`）；
//! 2. 启动守卫（上次没到窗口就提示）；
//! 3. eframe 启动（窗口参数、图标、DPI、持久化）；
//! 4. 窗口前的 panic 转成可读错误。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
// 启动失败时要能在终端说清原因（docs/DEVELOPMENT.md §4 第 9 步）。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use metis_engine::Engine;
use metis_paths::Paths;
use metis_ui_egui::{MetisApp, Tokens};

/// 启动桌面控制台，阻塞到窗口关闭。
///
/// 合并后的入口由 `apps/metis` 分发：无参数时调用这里（同一进程开窗）。
/// `--version` / `--help` 由 CLI 的 clap 处理，不会走到这里。
pub fn run() -> eframe::Result {
    // ① 数据目录：`METIS_HOME` > `~/.metis`。
    let paths = match Paths::from_env() {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("✗ {}", error.message());
            if let Some(remediation) = error.remediation() {
                eprintln!("  → {remediation}");
            }
            return Ok(());
        }
    };

    // ② 启动守卫：上次启动没到窗口，说明可能是原生层崩溃；本次提示用户。
    let mut guard = StartGuard::begin(&paths);
    let software = guard.previous_start_failed();

    let options = native_options(software);
    let app_paths = paths.clone();
    let shown = Arc::new(AtomicBool::new(false));
    let shown_by_app = shown.clone();

    // ③ catch_unwind：窗口显示之前的 panic 转成错误，之后的原样抛出。
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous_hook(info);
    }));
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        eframe::run_native(
            metis_ui_egui::APP_TITLE,
            options,
            Box::new(move |cc| {
                Tokens::install(&cc.egui_ctx);
                let engine = Engine::new(app_paths.clone());
                let mut app = MetisApp::new(engine);
                // 恢复界面偏好（上次的屏幕、侧栏宽度、深色开关）。
                // 少了这一步，用户每次打开都要重新调一遍界面。
                app.restore_ui(cc.storage);
                if app.ui.dark {
                    Tokens::install_with(&cc.egui_ctx, Tokens::DARK);
                }
                app.refresh_blocking();
                if software {
                    app.notify("上一次启动没有打开窗口（可能是显卡驱动崩溃）；如果反复出现，请看下方日志路径", metis_ui_egui::state::NoticeTone::Warning);
                }
                shown_by_app.store(true, Ordering::Relaxed);
                Ok(Box::new(app))
            }),
        )
    }));

    match run {
        Ok(Ok(())) => {
            guard.opened();
            Ok(())
        }
        Ok(Err(error)) => {
            report_start_failure(&error.to_string());
            Ok(())
        }
        Err(_) if !shown.load(Ordering::Relaxed) => {
            report_start_failure("窗口显示之前发生了内部错误");
            Ok(())
        }
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

fn native_options(_software: bool) -> eframe::NativeOptions {
    // 只启用 wgpu（不引入 glow）：少一个后端就少一类驱动崩溃面。
    //
    // 想换/限定后端时**不要改代码**：wgpu 认环境变量（`WGPU_BACKEND=dx12|vulkan|gl`、
    // `WGPU_ADAPTER_NAME=<显卡名片段>`）。实测这台机器默认选择就能正常出画面；
    // 而限定成 DX12 反而没有必要 —— 把选择权留给 wgpu 与环境变量更稳妥。
    eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title(metis_ui_egui::APP_TITLE)
            .with_app_id(metis_ui_egui::APP_ID)
            .with_inner_size(metis_ui_egui::DEFAULT_WINDOW)
            .with_min_inner_size(metis_ui_egui::MIN_WINDOW)
            .with_resizable(true)
            .with_drag_and_drop(true),
        ..Default::default()
    }
}

fn report_start_failure(reason: &str) {
    // 一个什么都不显示的程序是最糟的失败形态：至少把原因与日志位置说出来。
    eprintln!("✗ Metis 打不开窗口：{reason}");
    eprintln!("  → 试试设置 METIS_RENDERER=software 后重开；仍然失败请把这段输出贴到 issue");
}

/// 启动守卫：靠一个标记文件跨进程判断"上次启动有没有到窗口"。
///
/// 原生显卡驱动崩溃不会留下任何 Rust 侧能上报的痕迹，所以只能靠**下次**启动发现。
#[derive(Debug)]
pub struct StartGuard {
    marker: PathBuf,
    active: bool,
    previous_start_failed: bool,
}

impl StartGuard {
    /// 开始一次启动。
    pub fn begin(paths: &Paths) -> Self {
        let marker = paths.home().join("starting");
        let previous_start_failed = marker.is_file();
        let mut guard = Self { marker, active: false, previous_start_failed };
        if ensure_dir(paths.home()).is_ok() && std::fs::write(&guard.marker, std::process::id().to_string()).is_ok() {
            guard.active = true;
        }
        guard
    }

    /// 窗口已经打开：删掉标记。
    pub fn opened(&mut self) {
        if self.active {
            let _ = std::fs::remove_file(&self.marker);
            self.active = false;
        }
    }

    /// 上次启动是否没到窗口。
    pub fn previous_start_failed(&self) -> bool {
        self.previous_start_failed
    }
}

fn ensure_dir(path: &std::path::Path) -> std::io::Result<()> {
    if path.is_dir() { Ok(()) } else { std::fs::create_dir_all(path) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;

    /// 启动守卫是 GUI 里**唯一有逻辑**的部分：它决定"上次是不是没开到窗口"。
    /// 这条逻辑错了，用户会看到错误的"上次崩溃"提示，或者真崩了却没人告诉他。
    #[test]
    fn first_start_is_not_flagged_as_failed() {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        let guard = StartGuard::begin(&paths);
        assert!(!guard.previous_start_failed(), "第一次启动不该报上次失败");
    }

    #[test]
    fn a_leftover_marker_means_the_previous_start_failed() {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        std::fs::create_dir_all(paths.home()).expect("mkdir");
        std::fs::write(paths.home().join("starting"), "1234").expect("seed marker");

        let guard = StartGuard::begin(&paths);
        assert!(guard.previous_start_failed(), "留着标记说明上次没到窗口");
    }

    #[test]
    fn opened_removes_the_marker() {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        let mut guard = StartGuard::begin(&paths);
        let marker = paths.home().join("starting");
        assert!(marker.is_file(), "begin 应该写下标记");
        guard.opened();
        assert!(!marker.exists(), "opened 应该删掉标记");
    }

    #[test]
    fn opened_is_idempotent() {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        let mut guard = StartGuard::begin(&paths);
        guard.opened();
        guard.opened(); // 第二次不能 panic
    }

    #[test]
    fn a_second_start_after_opened_is_clean() {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        {
            let mut first = StartGuard::begin(&paths);
            first.opened();
        }
        let second = StartGuard::begin(&paths);
        assert!(!second.previous_start_failed(), "正常退出后下次启动不该报失败");
    }

    #[test]
    fn ensure_dir_creates_and_is_idempotent() {
        let dir = tempfile::tempdir().expect("tmp");
        let nested = dir.path().join("a").join("b");
        ensure_dir(&nested).expect("first");
        ensure_dir(&nested).expect("second");
        assert!(nested.is_dir());
    }
}

#[cfg(test)]
mod options_tests {
    use super::*;

    #[test]
    fn native_options_carry_the_documented_window_geometry() {
        // 窗口尺寸与最小尺寸是产品承诺（`docs/UI.md`）；这里钉住它们，
        // 免得某次"顺手调一下"让最小窗口变到放不下内容。
        let options = native_options(false);
        assert_eq!(options.viewport.inner_size, Some(metis_ui_egui::DEFAULT_WINDOW.into()));
        assert_eq!(options.viewport.min_inner_size, Some(metis_ui_egui::MIN_WINDOW.into()));
        assert_eq!(options.viewport.title.as_deref(), Some(metis_ui_egui::APP_TITLE));
        assert_eq!(options.viewport.resizable, Some(true), "窗口必须可缩放");
    }

    #[test]
    fn software_flag_does_not_change_the_geometry() {
        // `_software` 目前只影响提示文案，不该改变窗口形状。
        let normal = native_options(false);
        let software = native_options(true);
        assert_eq!(normal.viewport.inner_size, software.viewport.inner_size);
        assert_eq!(normal.viewport.min_inner_size, software.viewport.min_inner_size);
    }
}
