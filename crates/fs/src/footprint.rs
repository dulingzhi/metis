//! 写入所有权账本 `footprints.json`。
//!
//! 作用：记住"这个位置是我写的、怎么撤"。**只回滚自己写过的字节**（AGENTS.md 铁律 3）。
//!
//! 账本损坏 → 原文件保留、**停止销账**并报错。销账失败绝不能顺手删账本（`PITFALLS.md` B3）。

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, Result};
use serde::{Deserialize, Serialize};

/// 账本格式版本。
pub const LEDGER_VERSION: u32 = 1;

/// 撤销方式。**封闭集合**：遇到不认识的值必须报错并保留账本（`PITFALLS.md` B8）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum Undo {
    /// 托管块：按起止标记切除。
    Block {
        /// 块的唯一标记（用来核对"这确实是我们写的"）。
        marker: String,
    },
    /// 整文件替换：写回 `backups/` 里的原件。
    WholeFile {
        /// 备份文件的相对名（在 `backups/` 下）。
        backup: String,
    },
    /// 结构化字段：把某个键恢复成原值（`null` 表示删除该键）。
    Field {
        /// JSON 指针或 TOML 键路径。
        pointer: String,
        /// 原值（`serde_json::Value::Null` 表示原本不存在）。
        previous: serde_json::Value,
    },
    /// 生成的文件：整份删除即可。
    Generated,
    /// 只登记位置，不做内容回滚（例如 shim 目录被用户自己管）。
    Path,
    /// 目录项：从 PATH 之类的列表里移除。
    PathEnv {
        /// 要移除的条目。
        entry: String,
    },
}

impl Undo {
    /// 序列化值，供日志与报告使用。
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Block { .. } => "block",
            Self::WholeFile { .. } => "whole-file",
            Self::Field { .. } => "field",
            Self::Generated => "generated",
            Self::Path => "path",
            Self::PathEnv { .. } => "path-env",
        }
    }
}

/// 一条写入记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Footprint {
    /// 稳定 id：`sha256(kind \0 owner_key \0 harness \0 channel)[..24]`。
    pub id: String,
    /// 被写的路径（原样保存，便于用户看懂）。
    pub path: PathBuf,
    /// 位置种类（`file` / `dir`）。
    #[serde(default = "default_kind")]
    pub kind: String,
    /// 哪家执行器。
    pub harness: String,
    /// 哪条通道。
    pub channel: String,
    /// 怎么写进去的、怎么撤。
    pub undo: Undo,
    /// 写入后的内容哈希（`None` 表示只登记位置）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// 首次登记时间（RFC 3339）。重复登记时保留原值。
    #[serde(default)]
    pub created_at: String,
}

fn default_kind() -> String {
    "file".to_owned()
}

/// 账本文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct Ledger {
    version: u32,
    entries: Vec<Footprint>,
}

/// 账本路径。
pub fn ledger_file(home: &Path) -> PathBuf {
    home.join("footprints.json")
}

/// 计算账本条目 id。
pub fn entry_id(kind: &str, path: &Path, harness: &str, channel: &str) -> String {
    let joined = format!("{}\0{}\0{}\0{}", kind, super::owner_key(path), harness, channel);
    super::hash_bytes(joined.as_bytes())[..24].to_owned()
}

/// 读账本。文件不存在 → 空账本；损坏 → 错误（原文件不动）。
pub fn load(home: &Path) -> Result<Vec<Footprint>> {
    let file = ledger_file(home);
    let Some(bytes) = super::read_optional(&file)? else { return Ok(Vec::new()) };
    let parsed: Ledger = serde_json::from_slice(&bytes).map_err(|error| {
        MetisError::new(ErrorKind::StateCorrupt, format!("足迹账本损坏，原文件已保留：{}", file.display()))
            .with_detail(error.to_string())
            .with_remediation("备份该文件后删除它，再重新应用一次")
    })?;
    if parsed.version != LEDGER_VERSION {
        return Err(MetisError::new(ErrorKind::StateCorrupt, format!("足迹账本版本不受支持：{}（期望 {LEDGER_VERSION}）", parsed.version))
            .with_detail(file.display().to_string())
            .with_remediation("这个账本是更新的 Metis 写的；升级程序，或备份后删掉它重新登记"));
    }
    Ok(parsed.entries)
}

/// 写账本（原子）。
pub fn save(home: &Path, entries: &[Footprint]) -> Result<()> {
    let ledger = Ledger { version: LEDGER_VERSION, entries: entries.to_vec() };
    let mut bytes =
        serde_json::to_vec_pretty(&ledger).map_err(|error| MetisError::new(ErrorKind::Internal, "账本无法序列化").with_detail(error.to_string()))?;
    bytes.push(b'\n');
    super::write_atomic(&ledger_file(home), &bytes)
}

/// 登记一笔（同一 `(kind, path, harness, channel)` 是覆盖而不是追加）。
///
/// 幂等：重复登记同一笔不会让账本变长，`created_at` 保留首次的值。
pub fn record(home: &Path, mut entry: Footprint) -> Result<Footprint> {
    if entry.harness.is_empty() || entry.channel.is_empty() {
        return Err(MetisError::new(ErrorKind::Internal, "足迹缺少 harness 或 channel").with_remediation("这是程序缺陷，请报告"));
    }
    entry.id = entry_id(&entry.kind, &entry.path, &entry.harness, &entry.channel);
    let mut entries = load(home)?;
    match entries.iter().position(|existing| existing.id == entry.id) {
        Some(index) => {
            let previous = &entries[index];
            if entry.sha256.is_none() {
                entry.sha256 = previous.sha256.clone();
            }
            // 首次登记时间必须保留：它是"这笔写入从什么时候开始"的唯一依据。
            if !previous.created_at.is_empty() {
                entry.created_at = previous.created_at.clone();
            } else if entry.created_at.is_empty() {
                entry.created_at = now_rfc3339();
            }
            entries[index] = entry.clone();
        }
        None => {
            if entry.created_at.is_empty() {
                entry.created_at = now_rfc3339();
            }
            entries.push(entry.clone());
        }
    }
    save(home, &entries)?;
    Ok(entry)
}

/// 销账：按 id 或按 `(path, harness, channel)` 精确删除。
pub fn cancel(home: &Path, id_or_path: CancelKey<'_>) -> Result<bool> {
    let mut entries = load(home)?;
    let before = entries.len();
    entries.retain(|entry| !id_or_path.matches(entry));
    if entries.len() == before {
        return Ok(false);
    }
    save(home, &entries)?;
    Ok(true)
}

/// 销账的匹配条件。
#[derive(Debug, Clone, Copy)]
pub enum CancelKey<'a> {
    /// 按 id 精确删除。
    Id(&'a str),
    /// 按位置 + 归属删除。
    Location {
        /// 路径。
        path: &'a Path,
        /// 归属的 harness。
        harness: &'a str,
        /// 归属的通道。
        channel: &'a str,
    },
}

impl CancelKey<'_> {
    fn matches(&self, entry: &Footprint) -> bool {
        match self {
            Self::Id(id) => entry.id == *id,
            Self::Location { path, harness, channel } => {
                entry.harness == *harness && entry.channel == *channel && super::owner_key(&entry.path) == super::owner_key(path)
            }
        }
    }
}

/// 忘掉某一家（可选限定通道）。
pub fn forget_harness(home: &Path, harness: &str, channel: Option<&str>) -> Result<usize> {
    let mut entries = load(home)?;
    let before = entries.len();
    entries.retain(|entry| entry.harness != harness || channel.is_some_and(|c| entry.channel != c));
    let removed = before - entries.len();
    if removed > 0 {
        save(home, &entries)?;
    }
    Ok(removed)
}

/// 查找某个位置是否已有我们登记的记录。
pub fn find_at(home: &Path, path: &Path) -> Result<Option<Footprint>> {
    let key = super::owner_key(path);
    Ok(load(home)?.into_iter().find(|entry| super::owner_key(&entry.path) == key))
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, harness: &str, channel: &str) -> Footprint {
        Footprint {
            id: String::new(),
            path: PathBuf::from(path),
            kind: "file".to_owned(),
            harness: harness.to_owned(),
            channel: channel.to_owned(),
            undo: Undo::Generated,
            sha256: None,
            created_at: String::new(),
        }
    }

    #[test]
    fn missing_ledger_is_empty_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(load(dir.path()).expect("load"), Vec::new());
    }

    #[test]
    fn record_then_load_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let saved = record(dir.path(), entry("D:\\a\\b.txt", "codex", "install")).expect("record");
        assert_eq!(saved.id.len(), 24);
        let entries = load(dir.path()).expect("load");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].harness, "codex");
        assert!(!entries[0].created_at.is_empty());
    }

    #[test]
    fn recording_the_same_owner_twice_does_not_grow_the_ledger() {
        let dir = tempfile::tempdir().expect("tempdir");
        let first = record(dir.path(), entry("D:\\a\\b.txt", "codex", "install")).expect("first");
        let second = record(dir.path(), entry("D:\\a\\b.txt", "codex", "install")).expect("second");
        assert_eq!(first.id, second.id);
        assert_eq!(load(dir.path()).expect("load").len(), 1);
        assert_eq!(second.created_at, first.created_at, "重复登记应保留首次时间");
    }

    #[test]
    fn different_channel_or_harness_is_a_different_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        record(dir.path(), entry("D:\\a\\b.txt", "codex", "install")).expect("1");
        record(dir.path(), entry("D:\\a\\b.txt", "codex", "slash")).expect("2");
        record(dir.path(), entry("D:\\a\\b.txt", "claude", "install")).expect("3");
        assert_eq!(load(dir.path()).expect("load").len(), 3, "共享同一路径的不同归属不能互相覆盖");
    }

    #[test]
    fn record_requires_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut bad = entry("D:\\a\\b.txt", "", "install");
        bad.harness = String::new();
        let result = record(dir.path(), bad);
        assert!(result.is_err(), "没有 harness 的足迹必须被拒绝");
    }

    /// 平台无关的绝对路径：测试里**不要用 Windows 字面量**，
    /// 反斜杠在 Unix 上只是普通字符（CI 的 Linux 测试踩过）。
    fn abs(parts: &[&str]) -> PathBuf {
        let mut out = if cfg!(windows) { PathBuf::from("D:\\") } else { PathBuf::from("/") };
        for part in parts {
            out.push(part);
        }
        assert!(out.is_absolute(), "abs() 必须产出绝对路径：{out:?}");
        out
    }

    #[test]
    fn cancel_by_location_only_touches_that_owner() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = abs(&["a", "b.txt"]);
        record(dir.path(), entry(&target.display().to_string(), "codex", "install")).expect("1");
        record(dir.path(), entry(&target.display().to_string(), "codex", "slash")).expect("2");
        let removed = cancel(dir.path(), CancelKey::Location { path: &target, harness: "codex", channel: "install" }).expect("cancel");
        assert!(removed);
        let entries = load(dir.path()).expect("load");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].channel, "slash");
    }

    #[test]
    fn cancel_by_id_removes_exactly_one() {
        let dir = tempfile::tempdir().expect("tempdir");
        let saved = record(dir.path(), entry("D:\\a\\b.txt", "codex", "install")).expect("record");
        assert!(cancel(dir.path(), CancelKey::Id(&saved.id)).expect("cancel"));
        assert!(!cancel(dir.path(), CancelKey::Id(&saved.id)).expect("second cancel"), "再次取消应返回 false");
        assert!(load(dir.path()).expect("load").is_empty());
    }

    #[test]
    fn forget_harness_can_be_scoped_to_a_channel() {
        let dir = tempfile::tempdir().expect("tempdir");
        record(dir.path(), entry("D:\\a\\b.txt", "codex", "install")).expect("1");
        record(dir.path(), entry("D:\\a\\c.txt", "codex", "slash")).expect("2");
        record(dir.path(), entry("D:\\a\\d.txt", "claude", "install")).expect("3");
        let removed = forget_harness(dir.path(), "codex", Some("install")).expect("forget");
        assert_eq!(removed, 1);
        assert_eq!(load(dir.path()).expect("load").len(), 2);
        let removed_all = forget_harness(dir.path(), "codex", None).expect("forget all");
        assert_eq!(removed_all, 1);
        assert_eq!(load(dir.path()).expect("load").len(), 1);
    }

    #[test]
    fn corrupt_ledger_preserves_the_original_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = ledger_file(dir.path());
        std::fs::write(&file, b"{ not json").expect("seed");
        let before = std::fs::read(&file).expect("read");
        let error = load(dir.path()).expect_err("corrupt ledger must fail");
        assert_eq!(error.kind(), ErrorKind::StateCorrupt);
        assert!(error.remediation().is_some());
        assert_eq!(std::fs::read(&file).expect("reread"), before, "损坏时原字节必须保持不变");
    }

    #[test]
    fn unsupported_version_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(ledger_file(dir.path()), br#"{"version": 99, "entries": []}"#).expect("seed");
        let error = load(dir.path()).expect_err("must reject");
        assert_eq!(error.kind(), ErrorKind::StateCorrupt);
        assert!(error.message().contains("99"));
    }

    #[test]
    fn find_at_matches_by_canonical_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = abs(&["a", "b.txt"]);
        record(dir.path(), entry(&target.display().to_string(), "codex", "install")).expect("record");
        assert!(find_at(dir.path(), &target).expect("find").is_some());
        assert!(find_at(dir.path(), &abs(&["a", "other.txt"])).expect("find").is_none());
    }

    #[test]
    fn undo_type_names_are_stable() {
        assert_eq!(Undo::Generated.type_name(), "generated");
        assert_eq!(Undo::Path.type_name(), "path");
        assert_eq!(Undo::Block { marker: "m".to_owned() }.type_name(), "block");
        assert_eq!(Undo::WholeFile { backup: "b".to_owned() }.type_name(), "whole-file");
        assert_eq!(Undo::PathEnv { entry: "e".to_owned() }.type_name(), "path-env");
    }

    #[test]
    fn unknown_undo_type_is_a_hard_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        // 模拟上游那个真实事故：写入了一种清理逻辑不认识的撤销方式。
        std::fs::write(
            ledger_file(dir.path()),
            br#"{"version":1,"entries":[{"id":"x","path":"D:\\a","harness":"h","channel":"c","undo":{"type":"file-stash"}}]}"#,
        )
        .expect("seed");
        let error = load(dir.path()).expect_err("unknown undo type must not be silently dropped");
        assert_eq!(error.kind(), ErrorKind::StateCorrupt);
    }
}
