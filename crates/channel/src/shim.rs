//! `shim` 通道：在 PATH 最前放同名命令，裸敲即经 Metis 启动。
//!
//! 包装脚本**不内嵌人设**（会过期，且人设是私密的）：只转发到 `metis launch <harness>`，
//! 人设在启动时读取（`ARCHITECTURE.md` §6）。

use std::path::{Path, PathBuf};

use metis_core::Result;
use metis_fs::footprint::{self, Footprint, Undo};

/// 一次 shim 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShimOutcome {
    /// 脚本路径。
    pub file: PathBuf,
    /// 是否新写的（`false` 表示内容已一致）。
    pub written: bool,
}

/// 生成包装脚本的内容。
///
/// 脚本只做一件事：把参数原样转交给 `metis launch <harness>`。
pub fn script_for(harness: &str, metis_exe: &str) -> String {
    if cfg!(windows) {
        // `--exec` 必须带：`metis launch` 默认只打印命令（dry-run）。
        // `%*` 原样透传全部参数；`exit /b` 保留退出码。
        format!(
            "@echo off\r\nrem Metis 命令接管：裸敲 {harness} 等于经 Metis 启动，人设启动时读取。\r\n\"{metis_exe}\" launch {harness} --exec %*\r\nexit /b %ERRORLEVEL%\r\n"
        )
    } else {
        // `exec` 让 metis 进程被宿主替换，退出码与信号语义都保持原样。
        format!("#!/bin/sh\n# Metis 命令接管：裸敲 {harness} 等于经 Metis 启动，人设启动时读取。\nexec \"{metis_exe}\" launch {harness} --exec \"$@\"\n")
    }
}

/// 脚本文件名（Windows 带 `.cmd`）。
pub fn script_name(harness: &str) -> String {
    if cfg!(windows) { format!("{harness}.cmd") } else { harness.to_owned() }
}

/// 安装 shim。
pub fn install(home: &Path, shim_dir: &Path, harness: &str, metis_exe: &str) -> Result<ShimOutcome> {
    metis_fs::ensure_dir(shim_dir)?;
    let file = shim_dir.join(script_name(harness));
    let content = script_for(harness, metis_exe);
    let existing = metis_fs::read_to_string_optional(&file)?;
    if existing.as_deref() == Some(content.as_str()) {
        return Ok(ShimOutcome { file, written: false });
    }
    metis_fs::write_atomic(&file, content.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&file) {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = std::fs::set_permissions(&file, perms);
        }
    }
    footprint::record(
        home,
        Footprint {
            id: String::new(),
            path: file.clone(),
            kind: "file".to_owned(),
            harness: harness.to_owned(),
            channel: "shim".to_owned(),
            undo: Undo::Generated,
            sha256: metis_fs::hash_file(&file)?,
            created_at: String::new(),
        },
    )?;
    Ok(ShimOutcome { file, written: true })
}

/// 卸载 shim。
pub fn revert(home: &Path, shim_dir: &Path, harness: &str) -> Result<bool> {
    let file = shim_dir.join(script_name(harness));
    if !file.exists() {
        return Ok(false);
    }
    metis_fs::remove_if_exists(&file)?;
    footprint::cancel(home, footprint::CancelKey::Location { path: &file, harness, channel: "shim" })?;
    Ok(true)
}

/// shim 目录是否在 PATH 前面。
pub fn on_path(shim_dir: &Path) -> bool {
    std::env::var_os("PATH").map(|value| std::env::split_paths(&value).any(|entry| entry == shim_dir)).unwrap_or(false)
}

/// 给用户的提醒（没在 PATH 上时）。
pub fn path_hint(shim_dir: &Path) -> String {
    if cfg!(windows) {
        format!("把 {} 加到 PATH 最前面，裸敲命令才会经过 Metis", shim_dir.display())
    } else {
        format!("在 shell 的 rc 文件里加：export PATH=\"{0}:$PATH\"", shim_dir.display())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_forwards_to_metis_and_passes_args() {
        let script = script_for("codex", "C:\\bin\\metis.exe");
        assert!(script.contains("metis.exe"), "必须指向 metis 可执行文件");
        assert!(script.contains("launch codex"));
        // 不带 `--exec` 的话 `launch` 只打印计划，接管等于没生效。
        assert!(script.contains("--exec"), "必须真的启动宿主，而不是只打印命令");
        if cfg!(windows) {
            assert!(script.contains("%*"), "Windows 脚本必须透传参数");
            assert!(script.contains("ERRORLEVEL"), "必须保留退出码");
        } else {
            assert!(script.contains("\"$@\""), "Unix 脚本必须透传参数");
        }
    }

    #[test]
    fn script_never_embeds_the_persona_body() {
        let script = script_for("codex", "/usr/local/bin/metis");
        assert!(!script.contains("用简体中文"), "包装脚本不能内嵌人设正文");
    }

    #[test]
    fn script_name_has_cmd_on_windows() {
        let name = script_name("codex");
        if cfg!(windows) {
            assert_eq!(name, "codex.cmd");
        } else {
            assert_eq!(name, "codex");
        }
    }

    #[test]
    fn install_then_revert_cleans_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let shim_dir = dir.path().join("shim");
        let outcome = install(&home, &shim_dir, "codex", "/bin/metis").expect("install");
        assert!(outcome.written);
        assert!(outcome.file.is_file());
        assert_eq!(footprint::load(&home).expect("load").len(), 1);

        assert!(revert(&home, &shim_dir, "codex").expect("revert"));
        assert!(!outcome.file.exists());
        assert!(footprint::load(&home).expect("load").is_empty(), "撤销后账本应销掉");
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let shim_dir = dir.path().join("shim");
        let first = install(&home, &shim_dir, "codex", "/bin/metis").expect("first");
        assert!(first.written);
        let second = install(&home, &shim_dir, "codex", "/bin/metis").expect("second");
        assert!(!second.written, "内容一致时不该重写");
    }

    #[test]
    fn revert_of_missing_shim_is_false() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!revert(dir.path(), &dir.path().join("shim"), "codex").expect("revert"));
    }

    #[test]
    fn path_hint_mentions_the_directory() {
        let hint = path_hint(Path::new("/home/u/.metis/shim"));
        assert!(hint.contains(".metis"));
        assert!(!hint.is_empty());
    }

    #[test]
    fn on_path_reports_a_definite_boolean() {
        // 不能断言一定是 true/false（取决于机器），但必须能安全求值。
        let _ = on_path(Path::new("/definitely/not/on/path"));
    }
}
