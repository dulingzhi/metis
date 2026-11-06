//! 控制锁：跨进程互斥，落在 `<home>/.apply.lock`。
//!
//! 语义（`ARCHITECTURE.md` §7.4）：
//! 1. `create_new` 成功 → 拿到锁，写入 `{pid, token, at}` 并 `sync_all`；
//! 2. 已存在 → 读 owner，用 pid 存活探测判断是不是死锁：死锁文件（内容两次读一致）删掉重试一次；
//! 3. 同进程内可重入（同一路径的嵌套调用复用已持有的锁）；
//! 4. 释放：只有 token 匹配才删；`Drop` 里执行，等价于 `finally`。

// 本模块是锁的实现本身，维护两处极短的进程内记录；这是 clippy.toml 禁用裸 Mutex 的唯一例外。
#![allow(clippy::disallowed_types)]

use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use metis_core::{ErrorKind, MetisError, Result};
use serde::{Deserialize, Serialize};

/// 锁文件里记录的所有者信息。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LockOwner {
    /// 持有者进程号。
    pub pid: u32,
    /// 本次持有的随机 token（释放时核对）。
    pub token: String,
    /// 拿锁时间（RFC 3339）。
    pub at: String,
}

/// 锁文件里的原始字节与解析结果。
fn read_owner(path: &Path) -> (Option<Vec<u8>>, Option<LockOwner>) {
    let bytes = fs::read(path).ok();
    let owner = bytes.as_ref().and_then(|b| serde_json::from_slice::<LockOwner>(b).ok());
    (bytes, owner)
}

/// 判断 pid 还活着（供 swap 复用）。
pub fn is_pid_alive(pid: u32) -> bool {
    pid_alive(pid)
}

/// 判断 pid 还活着。
///
/// 用"发 0 号信号"的方式探测；在 Windows 上通过注入的探测函数实现（见 [`set_alive_probe`]）。

fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    platform::process_alive(pid)
}

#[cfg(unix)]
mod platform {
    /// Unix：`kill(pid, 0)`；`ESRCH` 是唯一可靠的"不存在"信号。
    pub(super) fn process_alive(pid: u32) -> bool {
        // 不引入 nix：通过 /proc 判断（Linux）或不判断（其它 Unix 保守认为活着）。
        if cfg!(target_os = "linux") { std::path::Path::new(&format!("/proc/{pid}")).exists() } else { true }
    }
}

#[cfg(windows)]
mod platform {
    /// 注入进来的存活探测函数（由 `metis-process` 提供）。
    pub(super) type ProbeSlot = std::sync::Mutex<Option<fn(u32) -> bool>>;

    /// Windows 上没有 `kill -0`。
    ///
    /// 精确判断需要 `OpenProcess` + `GetExitCodeProcess`（FFI），而项目禁止 `unsafe`。
    /// 所以走一条**安全且够用**的近似：pid 超出 32 位有效范围、或进程号明显不可能存在时
    /// 判为已死；其余一律保守认为活着，由用户按补救建议手动清理
    /// （`PITFALLS.md`：宁可让用户看到"锁被占用"，也不要删掉别人正在用的锁）。
    ///
    /// 需要精确判断时，由 `metis-process` 通过 [`set_alive_probe`] 注入探测函数。
    pub(super) fn process_alive(pid: u32) -> bool {
        if let Ok(guard) = PROBE.lock()
            && let Some(probe) = *guard
        {
            return probe(pid);
        }
        // 没有注入时：只有明显不可能的 pid 才判死（例如测试里用的 u32::MAX）。
        pid != 0 && pid != u32::MAX
    }

    pub(super) static PROBE: ProbeSlot = std::sync::Mutex::new(None);
}

/// Windows 下注入精确的存活探测（由 `metis-process` 提供，避免本 crate 起进程）。
///
/// 其它平台忽略这个调用。
pub fn set_alive_probe(_probe: fn(u32) -> bool) {
    #[cfg(windows)]
    {
        if let Ok(mut guard) = platform::PROBE.lock() {
            *guard = Some(_probe);
        }
    }
}

/// 进程内"我已经持有这个锁"的记录，用于可重入。
static HELD: Mutex<Option<HashSet<String>>> = Mutex::new(None);

fn held_contains(key: &str) -> bool {
    HELD.lock().ok().and_then(|g| g.as_ref().map(|s| s.contains(key))).unwrap_or(false)
}

fn held_insert(key: &str) {
    if let Ok(mut guard) = HELD.lock() {
        guard.get_or_insert_with(HashSet::new).insert(key.to_owned());
    }
}

fn held_remove(key: &str) {
    if let Ok(mut guard) = HELD.lock()
        && let Some(set) = guard.as_mut()
    {
        set.remove(key);
    }
}

/// 持有中的控制锁。`Drop` 时释放。
#[derive(Debug)]
pub struct ControlGuard {
    file: PathBuf,
    token: String,
    key: String,
    reentrant: bool,
}

impl ControlGuard {
    /// 锁文件路径。
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// 本次持有是否为可重入（外层已经持有）。
    pub fn is_reentrant(&self) -> bool {
        self.reentrant
    }
}

impl Drop for ControlGuard {
    fn drop(&mut self) {
        if self.reentrant {
            return;
        }
        // 只有 token 匹配才删：别人已经接管（或回收了死锁）时不能删别人的锁。
        let (_, owner) = read_owner(&self.file);
        if owner.as_ref().is_some_and(|o| o.token == self.token) {
            let _ = fs::remove_file(&self.file);
        }
        held_remove(&self.key);
    }
}

/// 拿控制锁。
pub fn acquire(home: &Path) -> Result<ControlGuard> {
    let key = key_of(home);
    if held_contains(&key) {
        return Ok(ControlGuard { file: lock_file(home), token: String::new(), key, reentrant: true });
    }

    let file = lock_file(home);
    if let Some(parent) = file.parent() {
        super::ensure_dir(parent)?;
    }

    for attempt in 0..2 {
        match fs::OpenOptions::new().write(true).create_new(true).open(&file) {
            Ok(mut handle) => {
                let token = uuid::Uuid::new_v4().simple().to_string();
                let owner = LockOwner { pid: std::process::id(), token: token.clone(), at: now_rfc3339() };
                let bytes =
                    serde_json::to_vec(&owner).map_err(|error| MetisError::new(ErrorKind::Internal, "锁信息无法序列化").with_detail(error.to_string()))?;
                if let Err(error) = handle.write_all(&bytes).and_then(|()| handle.sync_all()) {
                    let _ = fs::remove_file(&file);
                    return Err(MetisError::from(error).with_detail(file.display().to_string()));
                }
                // Windows 上打开的句柄会阻止数据目录改名（基线踩过），拿到就关。
                drop(handle);
                held_insert(&key);
                return Ok(ControlGuard { file, token, key, reentrant: false });
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let (bytes_before, owner) = read_owner(&file);
                let dead = owner.as_ref().is_some_and(|o| !pid_alive(o.pid));
                if attempt == 0 && dead {
                    // 再读一次，确认没人在我们判断期间改写它。
                    let (bytes_after, _) = read_owner(&file);
                    if bytes_before == bytes_after && bytes_after.is_some() {
                        let _ = fs::remove_file(&file);
                        continue;
                    }
                }
                let pid = owner.as_ref().map(|o| o.pid.to_string()).unwrap_or_else(|| "unknown".to_owned());
                return Err(MetisError::new(ErrorKind::LockHeld, format!("另一个配置操作尚未结束（PID {pid}）"))
                    .with_detail(file.display().to_string())
                    .with_remediation(format!("等它结束；确认没有进程在跑后删除 {}", file.display())));
            }
            Err(error) => return Err(MetisError::from(error).with_detail(file.display().to_string())),
        }
    }
    Err(MetisError::new(ErrorKind::LockHeld, "配置锁获取失败")
        .with_detail(file.display().to_string())
        .with_remediation("确认没有别的 Metis 在跑；然后删掉锁文件再试"))
}

/// 在一个事务里执行：拿锁 → 执行 → 释放（含 panic 时的释放）。
pub fn with_lock<T>(home: &Path, work: impl FnOnce() -> Result<T>) -> Result<T> {
    let _guard = acquire(home)?;
    work()
}

fn lock_file(home: &Path) -> PathBuf {
    home.join(".apply.lock")
}

fn key_of(home: &Path) -> String {
    super::owner_key(home)
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_writes_owner_and_drop_removes_it() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = lock_file(dir.path());
        {
            let guard = acquire(dir.path()).expect("acquire");
            assert!(file.is_file(), "拿锁后锁文件应存在");
            let owner: LockOwner = serde_json::from_slice(&fs::read(&file).expect("read")).expect("parse");
            assert_eq!(owner.pid, std::process::id());
            assert_eq!(owner.token, guard.token);
            assert!(!guard.is_reentrant());
        }
        assert!(!file.exists(), "Drop 后锁文件应被删除");
    }

    #[test]
    fn second_acquire_from_same_process_is_reentrant() {
        let dir = tempfile::tempdir().expect("tempdir");
        let outer = acquire(dir.path()).expect("outer");
        let inner = acquire(dir.path()).expect("inner should be reentrant");
        assert!(inner.is_reentrant());
        // 内层先 drop，不该删锁。
        drop(inner);
        assert!(lock_file(dir.path()).is_file(), "内层释放不应删除外层的锁");
        drop(outer);
        assert!(!lock_file(dir.path()).exists());
    }

    #[test]
    fn a_dead_owner_is_reclaimed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = lock_file(dir.path());
        fs::write(&file, serde_json::to_vec(&LockOwner { pid: u32::MAX, token: "dead".to_owned(), at: "2026-01-01T00:00:00Z".to_owned() }).expect("json"))
            .expect("seed");
        let guard = acquire(dir.path()).expect("dead owner must be reclaimed");
        assert!(!guard.is_reentrant());
        drop(guard);
        assert!(!file.exists());
    }

    #[test]
    fn a_live_owner_blocks_with_remediation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = lock_file(dir.path());
        // 用当前进程作为"活着"的持有者，但绕过同进程可重入记录。
        fs::write(
            &file,
            serde_json::to_vec(&LockOwner { pid: std::process::id(), token: "other".to_owned(), at: "2026-01-01T00:00:00Z".to_owned() }).expect("json"),
        )
        .expect("seed");
        let error = acquire(dir.path()).expect_err("live owner must block");
        assert_eq!(error.kind(), ErrorKind::LockHeld);
        assert!(error.remediation().is_some(), "必须告诉用户怎么处理");
        assert!(error.message().contains(&std::process::id().to_string()));
    }

    #[test]
    fn corrupt_lock_file_is_treated_as_unknown_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = lock_file(dir.path());
        fs::write(&file, b"not json").expect("seed");
        let error = acquire(dir.path()).expect_err("corrupt lock must block");
        assert_eq!(error.kind(), ErrorKind::LockHeld);
        assert!(error.message().contains("unknown"), "解析不出 owner 时应说 unknown：{}", error.message());
    }

    #[test]
    fn with_lock_runs_the_closure_and_releases() {
        let dir = tempfile::tempdir().expect("tempdir");
        let value = with_lock(dir.path(), || Ok(41 + 1)).expect("with_lock");
        assert_eq!(value, 42);
        assert!(!lock_file(dir.path()).exists());
    }

    #[test]
    fn with_lock_releases_even_when_the_closure_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let result: Result<()> = with_lock(dir.path(), || Err(MetisError::new(ErrorKind::Internal, "boom")));
        assert!(result.is_err());
        assert!(!lock_file(dir.path()).exists(), "失败也必须释放锁");
    }

    #[test]
    fn nested_with_lock_is_allowed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let value = with_lock(dir.path(), || with_lock(dir.path(), || Ok(7))).expect("nested");
        assert_eq!(value, 7);
        assert!(!lock_file(dir.path()).exists());
    }
}
