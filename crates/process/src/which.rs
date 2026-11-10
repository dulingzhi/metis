//! 在 PATH 里找可执行文件（含 Windows 的 `PATHEXT`）。
//!
//! 不找"所有可能"，只找**确实能执行的那个**。找不到就是找不到，返回 `None`。

use std::path::{Path, PathBuf};

/// 每个平台的候选扩展名。
pub fn extensions() -> Vec<String> {
    if cfg!(windows) {
        // PATHEXT 决定优先级；给一个稳妥的默认。
        std::env::var("PATHEXT")
            .ok()
            .map(|value| value.split(';').map(|e| e.trim().to_ascii_lowercase()).filter(|e| !e.is_empty()).collect())
            .unwrap_or_else(|| vec![".com".to_owned(), ".exe".to_owned(), ".bat".to_owned(), ".cmd".to_owned()])
    } else {
        Vec::new()
    }
}

/// 在给定的目录列表里找程序。
pub fn resolve_in(program: &str, dirs: &[PathBuf], exts: &[String]) -> Option<PathBuf> {
    // 显式带路径的直接看那个文件。
    if program.contains('/') || program.contains('\\') {
        let candidate = PathBuf::from(program);
        return if candidate.is_file() { Some(candidate) } else { None };
    }
    let has_known_ext =
        Path::new(program).extension().is_some() && exts.iter().any(|e| e == &format!(".{}", program.rsplit('.').next().unwrap_or("").to_ascii_lowercase()));
    for dir in dirs {
        let direct = dir.join(program);
        if has_known_ext && direct.is_file() {
            return Some(direct);
        }
        for ext in exts {
            let candidate = dir.join(format!("{program}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        if !cfg!(windows) && direct.is_file() {
            return Some(direct);
        }
    }
    None
}

/// 在真实 PATH 里找程序。
pub fn find_program(program: &str) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|value| std::env::split_paths(&value).collect()).unwrap_or_default();
    resolve_in(program, &dirs, &extensions())
}

/// 在真实 PATH 里找程序，**跳过指定目录**。
///
/// 用途：`metis launch` 被 Metis 自己的 shim 触发时，PATH 第一项就是 shim 目录。
/// 不跳过的话会再次执行 shim，形成无限递归。
pub fn find_program_excluding(program: &str, excluded: &Path) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|value| std::env::split_paths(&value).filter(|dir| !same_dir(dir, excluded)).collect()).unwrap_or_default();
    resolve_in(program, &dirs, &extensions())
}

/// 两个路径是不是同一个目录（Windows 大小写不敏感；尽量按规范路径比较）。
fn same_dir(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) else { return false };
    #[cfg(windows)]
    {
        a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// 进程存活探测函数（给 `metis-fs` 的控制锁用）。
///
/// 由本 crate 提供，因为"起进程 / 查进程"只能有一个入口（AGENTS.md 铁律 11）。
pub type PidProbe = fn(u32) -> bool;

/// 跨平台的"这个 pid 还活着吗"。
///
/// 注意：拿不到权限时**保守认为活着**——宁可让用户看到"锁被占用"，
/// 也不要在不确定时删掉别人正在用的锁。
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    platform::pid_alive(pid)
}

#[cfg(windows)]
mod platform {
    /// Windows 上没有 `kill -0`；精确判断需要 FFI，而项目禁止 `unsafe`。
    ///
    /// 采用与 `metis-fs` 一致的保守规则：只有明显不可能的 pid 才判死。
    /// 需要精确判断时由调用方传入自己的探测函数。
    pub(super) fn pid_alive(pid: u32) -> bool {
        pid != 0 && pid != u32::MAX
    }
}
#[cfg(unix)]
mod platform {
    /// Linux 用 `/proc/<pid>`；其它 Unix 保守认为活着。
    pub(super) fn pid_alive(pid: u32) -> bool {
        if cfg!(target_os = "linux") { std::path::Path::new(&format!("/proc/{pid}")).exists() } else { true }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_path_is_used_verbatim() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("tool.sh");
        std::fs::write(&file, b"#!/bin/sh\n").expect("seed");
        let found = resolve_in(&file.to_string_lossy(), &[], &[]);
        assert_eq!(found.as_deref(), Some(file.as_path()));
    }

    #[test]
    fn missing_explicit_path_is_none() {
        assert!(resolve_in("/definitely/not/here/tool", &[], &[]).is_none());
    }

    #[test]
    fn finds_program_in_first_matching_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = dir.path().join("a");
        let second = dir.path().join("b");
        std::fs::create_dir_all(&first).expect("mkdir");
        std::fs::create_dir_all(&second).expect("mkdir");
        let exts = if cfg!(windows) { vec![".cmd".to_owned()] } else { Vec::new() };
        let name = if cfg!(windows) { "tool.cmd" } else { "tool" };
        std::fs::write(second.join(name), b"x").expect("seed");

        let found = resolve_in("tool", &[first, second.clone()], &exts);
        assert_eq!(found.as_deref(), Some(second.join(name).as_path()));
    }

    #[test]
    fn returns_none_when_not_found() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(resolve_in("definitely-not-a-real-program-xyz", &[dir.path().to_path_buf()], &[]).is_none());
    }

    #[test]
    fn same_directory_is_recognised() {
        let dir = tempfile::tempdir().expect("tempdir");
        let shim = dir.path().join("shim");
        std::fs::create_dir_all(&shim).expect("mkdir");
        let trailing = PathBuf::from(format!("{}{}", shim.display(), std::path::MAIN_SEPARATOR));
        assert!(same_dir(&shim, &trailing), "末尾分隔符不应让同一个目录变成两个");
        #[cfg(windows)]
        {
            let upper = PathBuf::from(shim.to_string_lossy().to_uppercase());
            assert!(same_dir(&shim, &upper), "Windows 路径大小写不敏感");
        }
        assert!(!same_dir(&shim, &dir.path().join("other")));
    }

    #[test]
    fn pid_zero_is_never_alive() {
        assert!(!pid_alive(0));
    }

    #[test]
    fn current_process_is_alive() {
        // Windows 上本实现保守返回 true；Unix 上 /proc 存在。两种都应为 true。
        assert!(pid_alive(std::process::id()));
    }
}
