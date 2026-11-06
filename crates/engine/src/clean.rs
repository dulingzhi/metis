//! `clean` / `uninstall` / `self-uninstall`：按账本撤回写入。
//!
//! 三条硬规则（`docs/ARCHITECTURE.md` §7.3）：
//! 1. **破坏性操作先过安全闸门**，在建锁、写状态**之前**；
//! 2. **只撤自己写过的**（按账本，不按猜测）；
//! 3. 失败**保留恢复材料**，绝不"尽力删除"。

use std::path::Path;

use metis_core::{ErrorKind, HarnessId, MetisError, Result};
use serde::{Deserialize, Serialize};

/// 一步的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanStep {
    /// 做了什么。
    pub what: String,
    /// 成功吗。
    pub ok: bool,
    /// 细节（文件路径、错误原因）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 一次清理的报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanReport {
    /// 是否全部成功。
    pub ok: bool,
    /// 是否只是预览。
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
    /// 每一步。
    pub steps: Vec<CleanStep>,
    /// 撤不掉的（用户要自己处理）。
    pub leftovers: Vec<String>,
}

impl CleanReport {
    /// 新建。
    pub fn new(dry_run: bool) -> Self {
        Self { ok: true, dry_run, steps: Vec::new(), leftovers: Vec::new() }
    }

    fn step(&mut self, what: impl Into<String>, ok: bool, detail: Option<String>) {
        if !ok {
            self.ok = false;
        }
        self.steps.push(CleanStep { what: what.into(), ok, detail });
    }
}

/// 清理的范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CleanScope {
    /// 撤回全部接管，保留人设与 `state.json`。
    Channels,
    /// 撤回接管**并删除数据目录**（不可逆）。
    Everything,
}

/// 撤回全部已登记的写入。
///
/// 顺序有意为之：先过闸门 → 再逐笔撤销 → 最后（仅 `Everything`）删数据目录。
pub fn clean(paths: &metis_paths::Paths, scope: CleanScope, dry_run: bool) -> Result<CleanReport> {
    let home = paths.home();

    // ① 闸门：删除类操作在**任何写操作之前**先拒绝危险的根。
    if scope == CleanScope::Everything {
        metis_paths::destructive_gate(home, dirs::home_dir().as_deref(), Some(&std::env::temp_dir()), std::env::current_dir().ok().as_deref()).map_err(
            |error| {
                MetisError::new(ErrorKind::Refused, format!("拒绝清理：{}", error.message()))
                    .with_remediation(error.remediation().unwrap_or("换一个数据目录后重试").to_owned())
            },
        )?;
    }

    let mut report = CleanReport::new(dry_run);
    let entries = metis_fs::footprint::load(home)?;

    if entries.is_empty() {
        report.step("读账本", true, Some("账本里没有登记任何写入".to_owned()));
    }

    // ② 逐笔撤销。**按账本，不按猜测** —— 只动我们自己写过的路径。
    let mut removed_dir: Vec<std::path::PathBuf> = Vec::new();
    for entry in &entries {
        let label = format!("{} · {} · {}", entry.harness, entry.channel, entry.path.display());
        if dry_run {
            report.step(label, true, Some("（dry-run：将撤回）".to_owned()));
            continue;
        }
        match revert_entry(home, entry) {
            Ok(()) => {
                if let Some(parent) = entry.path.parent() {
                    removed_dir.push(parent.to_path_buf());
                }
                report.step(label, true, None);
            }
            Err(error) => {
                let detail = format!("{}（{}）", error.message(), error.remediation().unwrap_or("请手工处理"));
                report.leftovers.push(format!("{}: {detail}", entry.path.display()));
                report.step(label, false, Some(detail));
            }
        }
    }

    // ③ 清掉我们建的空目录（只删空的）。
    removed_dir.sort();
    removed_dir.dedup();
    if !dry_run {
        for dir in &removed_dir {
            let _ = std::fs::remove_dir(dir);
        }
    }

    // ④ 仅 `Everything`：删数据目录本身。**有残留就不删**。
    if scope == CleanScope::Everything {
        if report.ok && !dry_run {
            match std::fs::remove_dir_all(home) {
                Ok(()) => report.step("删除数据目录", true, Some(home.display().to_string())),
                Err(error) => {
                    report.step("删除数据目录", false, Some(error.to_string()));
                    report.leftovers.push(format!("{}: {error}", home.display()));
                }
            }
        } else if dry_run {
            report.step("删除数据目录", true, Some(format!("（dry-run：确认全部还原后删除 {}）", home.display())));
        } else {
            report.step("删除数据目录", false, Some(format!("有 {} 项没撤掉，保留 {} 供重试", report.leftovers.len(), home.display())));
        }
    }

    Ok(report)
}

/// 撤一笔账本记录。
fn revert_entry(home: &Path, entry: &metis_fs::footprint::Footprint) -> Result<()> {
    use metis_fs::footprint::Undo;

    match &entry.undo {
        Undo::Block { marker } => {
            // 托管块：按标记精确切除。
            let harness = HarnessId::parse(&entry.harness)?;
            let _ = harness;
            if let Some(content) = metis_fs::read_to_string_optional(&entry.path)? {
                if !content.contains(marker.as_str()) {
                    // 块已经不在了：说明外部已经处理过，不算失败。
                    metis_fs::footprint::cancel(home, metis_fs::footprint::CancelKey::Id(&entry.id))?;
                    return Ok(());
                }
                let stripped = strip_marker_block(&content, marker);
                if stripped.trim().is_empty() {
                    metis_fs::remove_if_exists(&entry.path)?;
                } else {
                    metis_fs::write_atomic(&entry.path, stripped.as_bytes())?;
                }
            }
            metis_fs::footprint::cancel(home, metis_fs::footprint::CancelKey::Id(&entry.id))?;
            Ok(())
        }
        Undo::Generated | Undo::WholeFile { .. } => {
            // 生成的文件：删掉。整文件替换：若备份还在就还原。
            if let Some(backup) = backup_bytes(home, entry)? {
                metis_fs::write_atomic(&entry.path, &backup)?;
            } else {
                metis_fs::remove_if_exists(&entry.path)?;
            }
            metis_fs::footprint::cancel(home, metis_fs::footprint::CancelKey::Id(&entry.id))?;
            Ok(())
        }
        Undo::Field { pointer, previous } => {
            let Some(text) = metis_fs::read_to_string_optional(&entry.path)? else {
                metis_fs::footprint::cancel(home, metis_fs::footprint::CancelKey::Id(&entry.id))?;
                return Ok(());
            };
            let mut root: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
                MetisError::new(ErrorKind::StateCorrupt, format!("不是合法的 JSON：{}", entry.path.display()))
                    .with_detail(error.to_string())
                    .with_remediation("备份该文件后手工修正")
            })?;
            if previous.is_null() {
                remove_pointer(&mut root, pointer);
            } else {
                set_pointer(&mut root, pointer, previous.clone());
            }
            let mut out =
                serde_json::to_string_pretty(&root).map_err(|error| MetisError::new(ErrorKind::Internal, "JSON 无法序列化").with_detail(error.to_string()))?;
            out.push('\n');
            metis_fs::write_atomic(&entry.path, out.as_bytes())?;
            metis_fs::footprint::cancel(home, metis_fs::footprint::CancelKey::Id(&entry.id))?;
            Ok(())
        }
        Undo::Path | Undo::PathEnv { .. } => {
            // 只登记位置：不动文件内容，只销账。
            metis_fs::footprint::cancel(home, metis_fs::footprint::CancelKey::Id(&entry.id))?;
            Ok(())
        }
    }
}

fn backup_bytes(home: &Path, entry: &metis_fs::footprint::Footprint) -> Result<Option<Vec<u8>>> {
    let metis_fs::footprint::Undo::WholeFile { backup } = &entry.undo else { return Ok(None) };
    metis_fs::read_optional(&home.join("backups").join(backup))
}

/// 按标记切除一段块（与 `channel::blocks` 同语义，但这里只按单一标记走）。
fn strip_marker_block(content: &str, marker: &str) -> String {
    let mut out = String::with_capacity(content.len());
    let mut inside = false;
    for line in content.split_inclusive('\n') {
        if inside {
            // 结束标记是开始标记换个方向；两种写法都认。
            let trimmed = line.trim();
            if trimmed == marker.replace(">>>", "<<<")
                || trimmed == marker.replace(">>>", "<<<")
                || trimmed.starts_with("<!-- metis:end")
                || trimmed == "# <<< metis"
            {
                inside = false;
            }
            continue;
        }
        if line.trim() == marker {
            inside = true;
            continue;
        }
        out.push_str(line);
    }
    let trimmed = out.trim_end_matches(['\n', '\r']).to_owned();
    if trimmed.is_empty() { String::new() } else { format!("{trimmed}\n") }
}

fn remove_pointer(root: &mut serde_json::Value, pointer: &str) {
    let keys: Vec<&str> = pointer.split('.').filter(|k| !k.is_empty()).collect();
    let Some((last, parents)) = keys.split_last() else { return };
    let mut current = root;
    for key in parents {
        match current.get_mut(key) {
            Some(next) => current = next,
            None => return,
        }
    }
    if let Some(map) = current.as_object_mut() {
        map.remove(*last);
    }
}

fn set_pointer(root: &mut serde_json::Value, pointer: &str, value: serde_json::Value) {
    let keys: Vec<&str> = pointer.split('.').filter(|k| !k.is_empty()).collect();
    let Some((last, parents)) = keys.split_last() else { return };
    let mut current = root;
    for key in parents {
        if !current.is_object() {
            *current = serde_json::json!({});
        }
        let Some(map) = current.as_object_mut() else { return };
        current = map.entry((*key).to_owned()).or_insert_with(|| serde_json::json!({}));
    }
    if let Some(map) = current.as_object_mut() {
        map.insert((*last).to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_fs::footprint::{self, Footprint, Undo};
    use metis_paths::Paths;

    fn setup() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        std::fs::create_dir_all(paths.home()).expect("mkdir");
        (dir, paths)
    }

    fn record(paths: &Paths, path: &Path, undo: Undo) {
        footprint::record(
            paths.home(),
            Footprint {
                id: String::new(),
                path: path.to_path_buf(),
                kind: "file".to_owned(),
                harness: "pi".to_owned(),
                channel: "install".to_owned(),
                undo,
                sha256: None,
                created_at: String::new(),
            },
        )
        .expect("record");
    }

    #[test]
    fn clean_with_an_empty_ledger_is_ok() {
        let (_dir, paths) = setup();
        let report = clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(report.ok);
        assert!(report.steps.iter().any(|s| s.detail.as_deref().is_some_and(|d| d.contains("没有登记"))));
    }

    #[test]
    fn dry_run_touches_nothing() {
        let (_dir, paths) = setup();
        let file = paths.home().join("target.txt");
        std::fs::write(&file, b"managed").expect("seed");
        record(&paths, &file, Undo::Generated);

        let report = clean(&paths, CleanScope::Channels, true).expect("clean");
        assert!(report.dry_run);
        assert!(file.exists(), "dry-run 不能真删");
        assert_eq!(footprint::load(paths.home()).expect("load").len(), 1, "dry-run 不能销账");
    }

    #[test]
    fn generated_files_are_removed_and_the_ledger_cleared() {
        let (_dir, paths) = setup();
        let file = paths.home().join("gen.txt");
        std::fs::write(&file, b"x").expect("seed");
        record(&paths, &file, Undo::Generated);

        let report = clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(report.ok);
        assert!(!file.exists(), "生成的文件应被删除");
        assert!(footprint::load(paths.home()).expect("load").is_empty(), "账本应清空");
    }

    #[test]
    fn block_entries_are_stripped_not_whole_file_deleted() {
        let (_dir, paths) = setup();
        let file = paths.home().join("AGENTS.md");
        std::fs::write(&file, "用户内容\n# >>> metis\n人设\n# <<< metis\n").expect("seed");
        record(&paths, &file, Undo::Block { marker: "# >>> metis".to_owned() });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        let left = std::fs::read_to_string(&file).expect("read");
        assert!(left.contains("用户内容"), "块外内容必须保留：{left}");
        assert!(!left.contains("人设"), "块内容必须切掉：{left}");
    }

    #[test]
    fn block_already_gone_is_not_an_error() {
        let (_dir, paths) = setup();
        let file = paths.home().join("AGENTS.md");
        std::fs::write(&file, "只有用户内容\n").expect("seed");
        record(&paths, &file, Undo::Block { marker: "# >>> metis".to_owned() });

        let report = clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(report.ok, "块已经不在应视为成功");
        assert!(footprint::load(paths.home()).expect("load").is_empty());
    }

    #[test]
    fn whole_file_entries_restore_the_backup() {
        let (_dir, paths) = setup();
        let file = paths.home().join("settings.json");
        std::fs::write(&file, b"managed").expect("seed");
        // 造一个备份。
        let backups = paths.home().join("backups");
        std::fs::create_dir_all(&backups).expect("mkdir");
        std::fs::write(backups.join("orig.bak"), b"original").expect("seed backup");
        record(&paths, &file, Undo::WholeFile { backup: "orig.bak".to_owned() });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        assert_eq!(std::fs::read(&file).expect("read"), b"original".to_vec(), "应还原备份而不是删除");
    }

    #[test]
    fn everything_scope_refuses_a_dangerous_home() {
        // 闸门必须生效：数据目录指向用户主目录时拒绝。
        let Some(home) = dirs::home_dir() else { return };
        let paths = Paths::with_home(&home);
        let error = clean(&paths, CleanScope::Everything, false).expect_err("must refuse");
        assert_eq!(error.kind(), ErrorKind::Refused);
        assert!(error.message().contains("拒绝"), "{}", error.message());
    }

    #[test]
    fn everything_scope_deletes_the_data_dir_when_clean() {
        let (dir, paths) = setup();
        let file = paths.home().join("gen.txt");
        std::fs::write(&file, b"x").expect("seed");
        record(&paths, &file, Undo::Generated);

        let report = clean(&paths, CleanScope::Everything, false).expect("clean");
        assert!(report.ok);
        assert!(!paths.home().exists(), "数据目录应被删除：{}", paths.home().display());
        drop(dir);
    }

    #[test]
    fn everything_scope_keeps_the_dir_when_something_failed() {
        let (_dir, paths) = setup();
        // 造一笔注定失败记录：字段撤销指向一个损坏的 JSON。
        let file = paths.home().join("broken.json");
        std::fs::write(&file, b"{ broken").expect("seed");
        record(&paths, &file, Undo::Field { pointer: "a.b".to_owned(), previous: serde_json::Value::Null });

        let report = clean(&paths, CleanScope::Everything, false).expect("clean");
        assert!(!report.ok, "有失败时应报告");
        assert!(paths.home().exists(), "有残留时必须保留数据目录供重试");
        assert!(report.leftovers.iter().any(|l| l.contains("broken.json")));
    }

    #[test]
    fn field_undo_removes_the_key_when_there_was_no_previous() {
        let (_dir, paths) = setup();
        let file = paths.home().join("cfg.json");
        std::fs::write(&file, br#"{"keep": 1, "drop": 2}"#).expect("seed");
        record(&paths, &file, Undo::Field { pointer: "drop".to_owned(), previous: serde_json::Value::Null });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        assert_eq!(value["keep"], 1, "别的字段必须保留");
        assert!(value.get("drop").is_none(), "原来不存在的键应被删掉：{value}");
    }

    #[test]
    fn field_undo_restores_the_previous_value() {
        let (_dir, paths) = setup();
        let file = paths.home().join("cfg.json");
        std::fs::write(&file, br#"{"model": "metis-managed"}"#).expect("seed");
        record(&paths, &file, Undo::Field { pointer: "model".to_owned(), previous: serde_json::json!("original") });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        assert_eq!(value["model"], "original", "应还原成原值：{value}");
    }

    #[test]
    fn field_undo_handles_nested_pointers() {
        let (_dir, paths) = setup();
        let file = paths.home().join("cfg.json");
        std::fs::write(&file, br#"{"a": {"b": {"c": "managed"}}}"#).expect("seed");
        record(&paths, &file, Undo::Field { pointer: "a.b.c".to_owned(), previous: serde_json::json!("restored") });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        let value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&file).expect("read")).expect("json");
        assert_eq!(value["a"]["b"]["c"], "restored");
    }

    #[test]
    fn field_undo_on_a_missing_file_is_not_an_error() {
        let (_dir, paths) = setup();
        let file = paths.home().join("gone.json");
        record(&paths, &file, Undo::Field { pointer: "x".to_owned(), previous: serde_json::Value::Null });
        let report = clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(report.ok, "文件已经不在了应视为成功：{report:?}");
    }

    #[test]
    fn path_entries_are_only_deregistered() {
        let (_dir, paths) = setup();
        // `Path` 只登记位置：不动文件内容。
        let file = paths.home().join("marker.txt");
        std::fs::write(&file, "用户自己的内容".as_bytes()).expect("seed");
        record(&paths, &file, Undo::Path);

        let report = clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(report.ok);
        assert!(file.exists(), "Path 类型不能删文件");
        assert_eq!(std::fs::read(&file).expect("read"), "用户自己的内容".as_bytes());
        assert!(footprint::load(paths.home()).expect("load").is_empty(), "只销账");
    }

    #[test]
    fn path_env_entries_are_only_deregistered() {
        let (_dir, paths) = setup();
        let file = paths.home().join("bin");
        std::fs::create_dir_all(&file).expect("mkdir");
        record(&paths, &file, Undo::PathEnv { entry: "C:\\bin".to_owned() });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(file.exists(), "PathEnv 不能删目录");
        assert!(footprint::load(paths.home()).expect("load").is_empty());
    }

    #[test]
    fn block_that_leaves_only_whitespace_removes_the_file() {
        let (_dir, paths) = setup();
        let file = paths.home().join("AGENTS.md");
        // 文件里除了托管块什么都没有：切完之后应该把文件删掉，而不是留一个空文件。
        std::fs::write(&file, "# >>> metis\n人设\n# <<< metis\n").expect("seed");
        record(&paths, &file, Undo::Block { marker: "# >>> metis".to_owned() });

        clean(&paths, CleanScope::Channels, false).expect("clean");
        assert!(!file.exists(), "只剩空内容时应删掉文件");
    }

    #[test]
    fn dry_run_reports_all_scopes_without_touching_disk() {
        let (_dir, paths) = setup();
        let file = paths.home().join("x.json");
        std::fs::write(&file, b"{}").expect("seed");
        record(&paths, &file, Undo::Field { pointer: "a".to_owned(), previous: serde_json::Value::Null });

        let report = clean(&paths, CleanScope::Everything, true).expect("clean");
        assert!(report.dry_run);
        assert!(file.exists());
        assert!(paths.home().exists(), "everything 的 dry-run 也不能删数据目录");
    }

    #[test]
    fn report_serialises_with_camel_case() {
        let (_dir, paths) = setup();
        let report = clean(&paths, CleanScope::Channels, true).expect("clean");
        let json = serde_json::to_value(&report).expect("json");
        assert!(json.get("dryRun").is_some());
        assert!(json.get("steps").is_some());
        assert!(json.get("leftovers").is_some());
    }
}
