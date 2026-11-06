//! `journal/*.jsonl`：追加式变更日志。
//!
//! 用途：出事之后能回答"**到底是哪一次操作改了这个文件**"。
//!
//! 硬约束：
//! 1. **只追加**，永不修改历史行；
//! 2. **不写人设正文、不写密钥**（`ARCHITECTURE.md` §11 的隐私约束）；
//! 3. 按月分文件，超过保留期自动删最旧的月份 —— 删除动作本身也记一条。

use std::io::Write;
use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, Result};
use serde::{Deserialize, Serialize};

/// 保留月数（超过就删最旧的）。
pub const KEEP_MONTHS: usize = 6;

/// 一条变更日志。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalEntry {
    /// 时间（RFC 3339）。
    pub at: String,
    /// 进程号。
    pub pid: u32,
    /// 谁做的（`cli` / `gui`）。
    pub actor: String,
    /// 操作名（`apply.install` / `clean` / `verify` 这种）。
    pub op: String,
    /// 哪家。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// 哪条通道。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    /// 哪个文件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 做了什么。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// 写前哈希。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "beforeHash")]
    pub before_hash: Option<String>,
    /// 写后哈希。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "afterHash")]
    pub after_hash: Option<String>,
    /// 人设 revision（不是正文）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    /// 成功吗。
    pub ok: bool,
}

impl JournalEntry {
    /// 新建一条（时间与 pid 自动填）。
    pub fn new(actor: &str, op: impl Into<String>, ok: bool) -> Self {
        Self {
            at: time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned()),
            pid: std::process::id(),
            actor: actor.to_owned(),
            op: op.into(),
            harness: None,
            channel: None,
            path: None,
            action: None,
            before_hash: None,
            after_hash: None,
            revision: None,
            ok,
        }
    }

    /// 附上下文。
    #[must_use]
    pub fn with_target(mut self, harness: impl Into<String>, channel: impl Into<String>, path: impl Into<String>) -> Self {
        self.harness = Some(harness.into());
        self.channel = Some(channel.into());
        self.path = Some(path.into());
        self
    }

    /// 附动作名。
    #[must_use]
    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.action = Some(action.into());
        self
    }

    /// 附哈希对。
    #[must_use]
    pub fn with_hashes(mut self, before: Option<String>, after: Option<String>) -> Self {
        self.before_hash = before;
        self.after_hash = after;
        self
    }

    /// 附 revision。
    #[must_use]
    pub fn with_revision(mut self, revision: impl Into<String>) -> Self {
        self.revision = Some(revision.into());
        self
    }
}

/// 按月分文件：`journal/2026-11.jsonl`。
fn month_file(dir: &Path, at: &str) -> PathBuf {
    // `at` 形如 `2026-11-06T…`，取前 7 个字符。
    let month = if at.len() >= 7 { &at[..7] } else { "unknown" };
    dir.join(format!("{month}.jsonl"))
}

/// 追加一条。
///
/// 日志失败**不阻断主流程**：返回 `Ok(false)` 表示"日志没写成"，
/// 调用方可以继续，但应当把这个事实报告出来。
pub fn append(dir: &Path, entry: &JournalEntry) -> Result<bool> {
    if !dir.is_dir() {
        metis_fs::ensure_dir(dir)?;
    }
    let file = month_file(dir, &entry.at);
    let line =
        serde_json::to_string(entry).map_err(|error| metis_core::MetisError::new(ErrorKind::Internal, "日志无法序列化").with_detail(error.to_string()))?;

    // 追加写：`OpenOptions::append`，一行一条。
    let mut handle = match std::fs::OpenOptions::new().create(true).append(true).open(&file) {
        Ok(handle) => handle,
        Err(error) => {
            return Err(metis_core::MetisError::from(error).with_detail(file.display().to_string()));
        }
    };
    if handle.write_all(format!("{line}\n").as_bytes()).is_err() {
        return Ok(false);
    }
    let _ = handle.flush();
    Ok(true)
}

/// 读某个月的日志（测试与诊断用）。
pub fn read_month(dir: &Path, month: &str) -> Result<Vec<JournalEntry>> {
    let file = dir.join(format!("{month}.jsonl"));
    let Some(text) = metis_fs::read_to_string_optional(&file)? else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // 坏行跳过而不是整份失败：日志是诊断材料，不该因为一行坏了就全读不出来。
        if let Ok(entry) = serde_json::from_str::<JournalEntry>(trimmed) {
            out.push(entry);
        }
    }
    Ok(out)
}

/// 列出现有的月份文件（升序）。
pub fn months(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".jsonl").map(str::to_owned)
        })
        .collect();
    out.sort();
    out
}

/// 按保留期删最旧的月份。返回删掉的月份。
///
/// 删除本身也写一条日志（写进**剩下的**最新月份），所以"谁删了日志"永远可查。
pub fn prune(dir: &Path, keep: usize) -> Result<Vec<String>> {
    let mut all = months(dir);
    if all.len() <= keep {
        return Ok(Vec::new());
    }
    // **先留够 `keep` 个月**，并把删除动作写进"剩下的最新月份"——
    // 如果删完再 append，而当前月恰好是被删的那个，就会立刻新建一个文件，
    // 让"删了 N 个"这个结论和磁盘对不上（实测踩过）。
    let removable: Vec<String> = all.drain(..all.len() - keep).collect();
    let survivor = all.last().cloned();

    let mut actually_removed = Vec::new();
    for month in &removable {
        let file = dir.join(format!("{month}.jsonl"));
        if metis_fs::remove_if_exists(&file).is_ok() {
            actually_removed.push(month.clone());
        }
    }
    if !actually_removed.is_empty()
        && let Some(month) = survivor
    {
        let entry = JournalEntry::new("cli", "journal.prune", true).with_action(format!(
            "删除了 {} 个月份日志：{}",
            actually_removed.len(),
            actually_removed.join(", ")
        ));
        // 写进**剩下的**最新月份（不是"当月"），这样不会凭空多出一个月份文件。
        let _ = append_in_month(dir, &month, &entry);
    }
    Ok(actually_removed)
}

/// 追加到**指定的月份文件**（`prune` 用：确保不新建月份）。
fn append_in_month(dir: &Path, month: &str, entry: &JournalEntry) -> Result<bool> {
    if !dir.is_dir() {
        metis_fs::ensure_dir(dir)?;
    }
    let file = dir.join(format!("{month}.jsonl"));
    let line =
        serde_json::to_string(entry).map_err(|error| metis_core::MetisError::new(ErrorKind::Internal, "日志无法序列化").with_detail(error.to_string()))?;
    let Ok(mut handle) = std::fs::OpenOptions::new().create(true).append(true).open(&file) else { return Ok(false) };
    if handle.write_all(format!("{line}\n").as_bytes()).is_err() {
        return Ok(false);
    }
    let _ = handle.flush();
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tmp");
        std::fs::create_dir_all(dir.path().join("journal")).expect("mkdir");
        dir
    }

    fn entry_at(at: &str, op: &str) -> JournalEntry {
        let mut entry = JournalEntry::new("cli", op, true);
        entry.at = at.to_owned();
        entry
    }

    #[test]
    fn append_creates_the_month_file() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        let entry = entry_at("2026-11-06T01:00:00Z", "apply.install");
        assert!(append(&journal, &entry).expect("append"));
        assert!(journal.join("2026-11.jsonl").is_file());
    }

    #[test]
    fn append_is_append_only() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        append(&journal, &entry_at("2026-11-06T01:00:00Z", "a")).expect("first");
        append(&journal, &entry_at("2026-11-06T02:00:00Z", "b")).expect("second");
        let text = std::fs::read_to_string(journal.join("2026-11.jsonl")).expect("read");
        assert_eq!(text.lines().count(), 2, "两行，第一行没被改");
        assert!(text.lines().next().unwrap_or_default().contains("\"a\""));
    }

    #[test]
    fn entries_round_trip() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        let entry = JournalEntry::new("gui", "apply.install", true)
            .with_target("codex", "install", "C:/x/config.toml")
            .with_action("installed")
            .with_hashes(None, Some("abc".to_owned()))
            .with_revision("9f2c");
        append(&journal, &entry).expect("append");

        // 时间由 `now()` 决定，可能跨月：读实际存在的那个月。
        let month = months(&journal).into_iter().next().expect("month");
        let read = read_month(&journal, &month).expect("read");
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].harness.as_deref(), Some("codex"));
        assert_eq!(read[0].channel.as_deref(), Some("install"));
        assert_eq!(read[0].revision.as_deref(), Some("9f2c"));
    }

    #[test]
    fn corrupt_lines_are_skipped() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        std::fs::write(journal.join("2026-11.jsonl"), "not json\n{\"at\":\"x\",\"pid\":1,\"actor\":\"cli\",\"op\":\"ok\",\"ok\":true}\n").expect("seed");
        let read = read_month(&journal, "2026-11").expect("read");
        assert_eq!(read.len(), 1, "坏行跳过，好行保留");
        assert_eq!(read[0].op, "ok");
    }

    #[test]
    fn missing_month_is_empty_not_an_error() {
        let tmp = dir();
        assert!(read_month(&tmp.path().join("journal"), "1999-01").expect("read").is_empty());
    }

    #[test]
    fn months_are_sorted() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        for month in ["2026-11", "2026-09", "2026-10"] {
            std::fs::write(journal.join(format!("{month}.jsonl")), "").expect("seed");
        }
        assert_eq!(months(&journal), vec!["2026-09", "2026-10", "2026-11"]);
    }

    #[test]
    fn prune_keeps_the_newest_months() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        for month in ["2026-01", "2026-02", "2026-03", "2026-04"] {
            std::fs::write(journal.join(format!("{month}.jsonl")), "").expect("seed");
        }
        let removed = prune(&journal, 2).expect("prune");
        assert_eq!(removed, vec!["2026-01", "2026-02"], "删最旧的");
        assert_eq!(months(&journal), vec!["2026-03", "2026-04"]);
    }

    #[test]
    fn prune_records_itself() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        for month in ["2026-01", "2026-02", "2026-03"] {
            std::fs::write(journal.join(format!("{month}.jsonl")), "").expect("seed");
        }
        prune(&journal, 1).expect("prune");
        let remaining = months(&journal);
        assert_eq!(remaining.len(), 1);
        let entries = read_month(&journal, &remaining[0]).expect("read");
        assert!(entries.iter().any(|e| e.op == "journal.prune"), "删除动作本身要留痕");
    }

    #[test]
    fn prune_within_the_limit_does_nothing() {
        let tmp = dir();
        let journal = tmp.path().join("journal");
        std::fs::write(journal.join("2026-11.jsonl"), "").expect("seed");
        assert!(prune(&journal, KEEP_MONTHS).expect("prune").is_empty());
        assert_eq!(months(&journal).len(), 1);
    }

    #[test]
    fn journal_never_contains_persona_text() {
        // 隐私约束：日志只记路径、哈希、revision，不记正文。
        let entry = JournalEntry::new("cli", "apply", true).with_target("codex", "install", "C:/x").with_revision("abc");
        let json = serde_json::to_string(&entry).expect("json");
        assert!(!json.contains("用简体中文"));
        assert!(json.contains("\"revision\":\"abc\""));
    }

    #[test]
    fn append_creates_a_missing_directory() {
        let tmp = tempfile::tempdir().expect("tmp");
        let journal = tmp.path().join("deep").join("journal");
        assert!(append(&journal, &entry_at("2026-11-06T00:00:00Z", "x")).expect("append"));
        assert!(journal.is_dir());
    }
}
