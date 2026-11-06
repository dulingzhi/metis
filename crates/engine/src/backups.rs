//! 备份目录的体积管理。
//!
//! `backups/` 存的是"整文件替换前的原件"，每次 apply 都可能新增一份。
//! 长期使用后它会悄悄长大（用户的 home 目录被我们占掉几百 MB 是不体面的）。
//!
//! 三条规则：
//! 1. **先算再删** —— 任何清理都必须先给出"删哪些、能省多少"，用户确认才动；
//! 2. **只删有对应账本条目的备份**，或者明确标注"孤立备份"（账本里已经没有它）；
//! 3. **不按时间无脑删** —— 最近一次 apply 用的备份永远保留（否则回滚会失败）。

use std::path::Path;

use metis_core::Result;
use serde::{Deserialize, Serialize};

/// 一个备份文件的信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupFile {
    /// 文件名。
    pub name: String,
    /// 字节数。
    pub bytes: u64,
    /// 修改时间（RFC 3339，拿不到就是 `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "modifiedAt")]
    pub modified_at: Option<String>,
    /// 账本里还有没有引用它。
    pub referenced: bool,
}

/// 备份目录的整体状况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupUsage {
    /// 目录路径。
    pub dir: String,
    /// 文件总数。
    pub files: usize,
    /// 总字节数。
    #[serde(rename = "totalBytes")]
    pub total_bytes: u64,
    /// 没有任何账本条目引用的备份。
    #[serde(rename = "orphanBytes")]
    pub orphan_bytes: u64,
    /// 孤立备份的个数。
    #[serde(rename = "orphanCount")]
    pub orphan_count: usize,
    /// 每个文件。
    pub items: Vec<BackupFile>,
}

impl BackupUsage {
    /// 人类可读的字节数（`12.3 MB`）。
    pub fn human_total(&self) -> String {
        human_bytes(self.total_bytes)
    }

    /// 人类可读的孤立备份体积。
    pub fn human_orphans(&self) -> String {
        human_bytes(self.orphan_bytes)
    }
}

/// 把字节数变人类可读。
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// 扫描备份目录。
///
/// `referenced` 由调用方给：账本里还有哪些备份名被引用。
pub fn scan(dir: &Path, referenced: &std::collections::BTreeSet<String>) -> Result<BackupUsage> {
    let mut items = Vec::new();
    let mut total_bytes = 0_u64;
    let mut orphan_bytes = 0_u64;
    let mut orphan_count = 0_usize;

    if dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let modified_at =
                entry.metadata().ok().and_then(|m| m.modified().ok()).map(|time| {
                    time::OffsetDateTime::from(time).format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned())
                });
            let referenced = referenced.contains(&name);
            total_bytes = total_bytes.saturating_add(bytes);
            if !referenced {
                orphan_bytes = orphan_bytes.saturating_add(bytes);
                orphan_count += 1;
            }
            items.push(BackupFile { name, bytes, modified_at, referenced });
        }
    }

    // 稳定顺序：大的排前面（用户想看"谁占地方"）。
    items.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));

    Ok(BackupUsage { dir: dir.display().to_string(), files: items.len(), total_bytes, orphan_bytes, orphan_count, items })
}

/// 清理孤立备份。
///
/// **只删 `referenced == false` 的**：账本还在引用的一律保留，
/// 否则回滚会拿着一份不存在的备份去还原，那是最糟的失败形态。
/// `dry_run` 时只报告不删。
pub fn prune(dir: &Path, usage: &BackupUsage, dry_run: bool) -> Result<PruneResult> {
    let mut removed = Vec::new();
    let mut freed = 0_u64;
    for item in &usage.items {
        if item.referenced {
            continue;
        }
        if !dry_run && metis_fs::remove_if_exists(&dir.join(&item.name)).is_err() {
            continue;
        }
        freed = freed.saturating_add(item.bytes);
        removed.push(item.name.clone());
    }
    Ok(PruneResult { dry_run, removed, freed_bytes: freed })
}

/// 清理结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneResult {
    /// 是否只是预览。
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
    /// 删掉（或将要删掉）的文件名。
    pub removed: Vec<String>,
    /// 释放（或将要释放）的字节数。
    #[serde(rename = "freedBytes")]
    pub freed_bytes: u64,
}

impl PruneResult {
    /// 人类可读的释放量。
    pub fn human_freed(&self) -> String {
        human_bytes(self.freed_bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn setup() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("backups")).expect("mkdir");
        dir
    }

    fn seed(dir: &Path, name: &str, bytes: usize) {
        std::fs::write(dir.join("backups").join(name), vec![b'x'; bytes]).expect("seed");
    }

    #[test]
    fn an_empty_directory_scans_to_zero() {
        let dir = setup();
        let usage = scan(&dir.path().join("backups"), &BTreeSet::new()).expect("scan");
        assert_eq!(usage.files, 0);
        assert_eq!(usage.total_bytes, 0);
        assert_eq!(usage.orphan_count, 0);
    }

    #[test]
    fn a_missing_directory_is_not_an_error() {
        let dir = setup();
        let usage = scan(&dir.path().join("nope"), &BTreeSet::new()).expect("scan");
        assert_eq!(usage.files, 0);
    }

    #[test]
    fn referenced_backups_are_counted_separately() {
        let dir = setup();
        seed(dir.path(), "used.bak", 100);
        seed(dir.path(), "orphan.bak", 300);

        let mut referenced = BTreeSet::new();
        referenced.insert("used.bak".to_owned());
        let usage = scan(&dir.path().join("backups"), &referenced).expect("scan");

        assert_eq!(usage.files, 2);
        assert_eq!(usage.total_bytes, 400);
        assert_eq!(usage.orphan_bytes, 300, "只有孤儿算进可清理体积");
        assert_eq!(usage.orphan_count, 1);
    }

    #[test]
    fn items_are_sorted_largest_first() {
        let dir = setup();
        seed(dir.path(), "small.bak", 10);
        seed(dir.path(), "big.bak", 1000);
        seed(dir.path(), "mid.bak", 100);
        let usage = scan(&dir.path().join("backups"), &BTreeSet::new()).expect("scan");
        let names: Vec<&str> = usage.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(names, vec!["big.bak", "mid.bak", "small.bak"]);
    }

    #[test]
    fn prune_removes_only_orphans() {
        let dir = setup();
        seed(dir.path(), "used.bak", 100);
        seed(dir.path(), "orphan.bak", 300);
        let backups = dir.path().join("backups");

        let mut referenced = BTreeSet::new();
        referenced.insert("used.bak".to_owned());
        let usage = scan(&backups, &referenced).expect("scan");

        let result = prune(&backups, &usage, false).expect("prune");
        assert_eq!(result.removed, vec!["orphan.bak"]);
        assert_eq!(result.freed_bytes, 300);
        assert!(backups.join("used.bak").is_file(), "被引用的备份必须保留");
        assert!(!backups.join("orphan.bak").exists());
    }

    #[test]
    fn dry_run_deletes_nothing() {
        let dir = setup();
        seed(dir.path(), "orphan.bak", 300);
        let backups = dir.path().join("backups");
        let usage = scan(&backups, &BTreeSet::new()).expect("scan");

        let result = prune(&backups, &usage, true).expect("prune");
        assert!(result.dry_run);
        assert_eq!(result.removed, vec!["orphan.bak"], "预览也要如实报告将要删什么");
        assert_eq!(result.freed_bytes, 300);
        assert!(backups.join("orphan.bak").is_file(), "预览不能真删");
    }

    #[test]
    fn nothing_referenced_means_everything_is_prunable() {
        let dir = setup();
        seed(dir.path(), "a.bak", 10);
        seed(dir.path(), "b.bak", 20);
        let backups = dir.path().join("backups");
        let usage = scan(&backups, &BTreeSet::new()).expect("scan");
        let result = prune(&backups, &usage, false).expect("prune");
        assert_eq!(result.removed.len(), 2);
        assert_eq!(result.freed_bytes, 30);
    }

    #[test]
    fn everything_referenced_means_nothing_is_prunable() {
        // 最危险的情况：账本全都在引用。这时候"清理"必须是空操作。
        let dir = setup();
        seed(dir.path(), "a.bak", 10);
        let backups = dir.path().join("backups");
        let mut referenced = BTreeSet::new();
        referenced.insert("a.bak".to_owned());
        let usage = scan(&backups, &referenced).expect("scan");
        let result = prune(&backups, &usage, false).expect("prune");
        assert!(result.removed.is_empty());
        assert_eq!(result.freed_bytes, 0);
        assert!(backups.join("a.bak").is_file());
    }

    #[test]
    fn human_bytes_reads_naturally() {
        assert_eq!(human_bytes(0), "0 B");
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(1024), "1.0 KB");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(1024 * 1024), "1.0 MB");
        assert_eq!(human_bytes(1024 * 1024 * 1024), "1.0 GB");
    }

    #[test]
    fn usage_serialises_with_camel_case() {
        let dir = setup();
        seed(dir.path(), "a.bak", 10);
        let usage = scan(&dir.path().join("backups"), &BTreeSet::new()).expect("scan");
        let json = serde_json::to_value(&usage).expect("json");
        assert!(json.get("totalBytes").is_some());
        assert!(json.get("orphanBytes").is_some());
        assert!(json.get("orphanCount").is_some());
        assert_eq!(json["items"][0]["referenced"], false);
    }
}
