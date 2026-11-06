//! `install` 通道：常驻配置 / 插件。
//!
//! 三种写法（`ARCHITECTURE.md` §6）：
//! - 托管块（`AGENTS.md` / `SYSTEM.md` / `SOUL.md`）；
//! - 结构化字段（`settings.json` / `config.toml`）；
//! - 整文件替换（宿主专属、且回滚凭据完整）。

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, Mode, Result};
use metis_fs::footprint::{self, Footprint, Undo};

use crate::blocks::{self, BlockMarker};

/// 一次 install 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOutcome {
    /// 目标文件。
    pub file: PathBuf,
    /// 做了什么。
    pub action: InstallAction,
}

/// install 的动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallAction {
    /// 新建了文件。
    Created,
    /// 替换了已有的托管块。
    ReplacedBlock,
    /// 追加了一个新的托管块。
    AppendedBlock,
    /// 撤销了托管块。
    RemovedBlock,
    /// 文件已一致，什么都没做。
    Unchanged,
    /// 文件原本不存在，撤销时删掉了它。
    RemovedFile,
}

impl InstallAction {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::ReplacedBlock => "replaced-block",
            Self::AppendedBlock => "appended-block",
            Self::RemovedBlock => "removed-block",
            Self::Unchanged => "unchanged",
            Self::RemovedFile => "removed-file",
        }
    }
}

/// 一条 install 目标。
#[derive(Debug, Clone)]
pub struct InstallTarget {
    /// 宿主的可执行 id（进账本）。
    pub harness: String,
    /// 目标文件。
    pub file: PathBuf,
    /// 写入方式。
    pub kind: crate::blocks::BlockMarker,
}

/// 往托管块里写人设。幂等。
pub fn apply_block(home: &Path, harness: &str, channel: &str, file: &Path, body: &str) -> Result<InstallOutcome> {
    let marker = BlockMarker::for_path(file);
    let existing = metis_fs::read_to_string_optional(file)?;
    let (content, action) = match existing.as_deref() {
        None => (blocks::upsert_block("", marker, body), InstallAction::Created),
        Some(current) => {
            let had_block = blocks::has_block(current, marker);
            let next = blocks::upsert_block(current, marker, body);
            if next == current {
                return Ok(InstallOutcome { file: file.to_path_buf(), action: InstallAction::Unchanged });
            }
            (next, if had_block { InstallAction::ReplacedBlock } else { InstallAction::AppendedBlock })
        }
    };

    metis_fs::write_atomic(file, content.as_bytes())?;
    record(home, harness, channel, file, Undo::Block { marker: marker.begin.to_owned() })?;
    Ok(InstallOutcome { file: file.to_path_buf(), action })
}

/// 撤销托管块，逐字节还原块外内容。
pub fn revert_block(home: &Path, harness: &str, channel: &str, file: &Path) -> Result<InstallOutcome> {
    let marker = BlockMarker::for_path(file);
    let Some(current) = metis_fs::read_to_string_optional(file)? else {
        return Ok(InstallOutcome { file: file.to_path_buf(), action: InstallAction::RemovedFile });
    };
    let outcome = blocks::strip_block(&current, marker);
    if !outcome.removed {
        return Ok(InstallOutcome { file: file.to_path_buf(), action: InstallAction::Unchanged });
    }
    if outcome.unterminated {
        return Err(MetisError::new(ErrorKind::RestoreConflict, format!("托管块没有结束标记，已按保守规则切除：{}", file.display()))
            .with_remediation("检查文件末尾，确认被切除的内容确实属于 Metis"));
    }

    if outcome.content.is_empty() {
        // 切完只剩空白：如果原本就是这个文件，删掉它。
        metis_fs::remove_if_exists(file)?;
        forget(home, harness, channel, file)?;
        return Ok(InstallOutcome { file: file.to_path_buf(), action: InstallAction::RemovedFile });
    }
    metis_fs::write_atomic(file, outcome.content.as_bytes())?;
    forget(home, harness, channel, file)?;
    Ok(InstallOutcome { file: file.to_path_buf(), action: InstallAction::RemovedBlock })
}

/// 写一个结构化字段（JSON）。返回原值，供撤销。
///
/// `pointer` 是点分键路径，例如 `personalization.customPrompt`。
///
/// 每次修改都会把**整份原文**存进同一目录的备份文件，撤销时逐字节写回。
/// 只靠"把字段值改回去"还原不够：`serde_json` 会重排键、改缩进，
/// 那样就不是"逐字节还原"了（`ARCHITECTURE.md` 不变式 I1）。
pub fn apply_json_field(home: &Path, harness: &str, channel: &str, file: &Path, pointer: &str, value: &str) -> Result<serde_json::Value> {
    let existing = metis_fs::read_to_string_optional(file)?;
    let original_bytes = existing.as_deref().unwrap_or("").as_bytes().to_vec();
    let mut root: serde_json::Value = match existing.as_deref() {
        None | Some("") => serde_json::json!({}),
        Some(text) => serde_json::from_str(text).map_err(|error| {
            MetisError::new(ErrorKind::StateCorrupt, format!("不是合法的 JSON：{}", file.display()))
                .with_detail(error.to_string())
                .with_remediation("修好这个文件（或先备份再删掉），然后重试")
        })?,
    };
    let previous = get_pointer(&root, pointer);
    set_pointer(&mut root, pointer, serde_json::Value::String(value.to_owned()))?;

    // 写入前把原字节存一份，撤销时用它逐字节还原。
    let backup = write_backup(home, file, &original_bytes, existing.is_some())?;

    let mut text =
        serde_json::to_string_pretty(&root).map_err(|error| MetisError::new(ErrorKind::Internal, "JSON 无法序列化").with_detail(error.to_string()))?;
    text.push('\n');
    metis_fs::write_atomic(file, text.as_bytes())?;

    let undo = match backup {
        Some(backup) => Undo::WholeFile { backup },
        None => Undo::Field { pointer: pointer.to_owned(), previous: previous.clone() },
    };
    record(home, harness, channel, file, undo)?;
    Ok(previous)
}

/// 撤销一个 JSON 字段。优先用备份逐字节还原。
pub fn revert_json_field(home: &Path, harness: &str, channel: &str, file: &Path, pointer: &str, previous: &serde_json::Value) -> Result<()> {
    // 账本里若有备份，直接写回原字节（这才叫"还原"）。
    if let Ok(Some(entry)) = footprint::find_at(home, file)
        && entry.channel == channel
        && entry.harness == harness
        && let Undo::WholeFile { backup } = &entry.undo
    {
        let backup_path = home.join("backups").join(backup);
        if let Some(bytes) = metis_fs::read_optional(&backup_path)? {
            metis_fs::write_atomic(file, &bytes)?;
            forget(home, harness, channel, file)?;
            return Ok(());
        }
    }

    let Some(text) = metis_fs::read_to_string_optional(file)? else { return Ok(()) };
    let mut root: serde_json::Value = serde_json::from_str(&text).map_err(|error| {
        MetisError::new(ErrorKind::StateCorrupt, format!("不是合法的 JSON：{}", file.display()))
            .with_detail(error.to_string())
            .with_remediation("备份该文件后修正它（或删掉让它重新生成），然后重试")
    })?;
    if previous.is_null() {
        remove_pointer(&mut root, pointer);
    } else {
        set_pointer(&mut root, pointer, previous.clone())?;
    }
    let mut out =
        serde_json::to_string_pretty(&root).map_err(|error| MetisError::new(ErrorKind::Internal, "JSON 无法序列化").with_detail(error.to_string()))?;
    out.push('\n');
    metis_fs::write_atomic(file, out.as_bytes())?;
    forget(home, harness, channel, file)?;
    Ok(())
}

/// 把原字节存进 `backups/`，返回备份的相对文件名。
///
/// 文件原本不存在时返回 `None`（撤销的语义是"删掉它"，不需要备份）。
fn write_backup(home: &Path, file: &Path, original: &[u8], existed: bool) -> Result<Option<String>> {
    if !existed {
        return Ok(None);
    }
    let dir = home.join("backups");
    metis_fs::ensure_dir(&dir)?;
    let name = format!("{}.bak", metis_fs::hash_bytes(metis_fs::owner_key(file).as_bytes()));
    metis_fs::write_atomic(&dir.join(&name), original)?;
    Ok(Some(name))
}

/// 模式对应的块正文。
///
/// `strict` 与 `replace` 用同一份正文；`append` 的差别由宿主侧决定（我们只放正文）。
pub fn body_for(mode: Mode, persona_body: &str) -> Option<String> {
    if !mode.injects() {
        return None;
    }
    Some(persona_body.trim().to_owned())
}

fn get_pointer(root: &serde_json::Value, pointer: &str) -> serde_json::Value {
    let mut current = root;
    for key in pointer.split('.').filter(|k| !k.is_empty()) {
        match current.get(key) {
            Some(next) => current = next,
            None => return serde_json::Value::Null,
        }
    }
    current.clone()
}

fn set_pointer(root: &mut serde_json::Value, pointer: &str, value: serde_json::Value) -> Result<()> {
    let keys: Vec<&str> = pointer.split('.').filter(|k| !k.is_empty()).collect();
    let Some((last, parents)) = keys.split_last() else {
        return Err(MetisError::new(ErrorKind::InvalidArgument, "字段路径不能为空").with_remediation("用 owner.field 形式，例如 personalization.customPrompt"));
    };
    let mut current = root;
    for key in parents {
        if !current.is_object() {
            *current = serde_json::json!({});
        }
        let map = current.as_object_mut().ok_or_else(|| {
            MetisError::new(ErrorKind::StateCorrupt, "JSON 结构不是对象").with_remediation("这个文件的结构不是 {…}；备份后修正它，或直接换一份配置")
        })?;
        current = map.entry((*key).to_owned()).or_insert_with(|| serde_json::json!({}));
    }
    if !current.is_object() {
        *current = serde_json::json!({});
    }
    let map = current.as_object_mut().ok_or_else(|| {
        MetisError::new(ErrorKind::StateCorrupt, "JSON 结构不是对象").with_remediation("这个文件的结构不是 {…}；备份后修正它，或直接换一份配置")
    })?;
    map.insert((*last).to_owned(), value);
    Ok(())
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

fn record(home: &Path, harness: &str, channel: &str, file: &Path, undo: Undo) -> Result<()> {
    let sha256 = metis_fs::hash_file(file)?;
    footprint::record(
        home,
        Footprint {
            id: String::new(),
            path: file.to_path_buf(),
            kind: "file".to_owned(),
            harness: harness.to_owned(),
            channel: channel.to_owned(),
            undo,
            sha256,
            created_at: String::new(),
        },
    )?;
    Ok(())
}

fn forget(home: &Path, harness: &str, channel: &str, file: &Path) -> Result<()> {
    footprint::cancel(home, footprint::CancelKey::Location { path: file, harness, channel })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        metis_fs::ensure_dir(&home).expect("mkdir");
        (dir, home)
    }

    #[test]
    fn apply_then_revert_restores_bytes_exactly() {
        let (_dir, home) = setup();
        let file = home.join("AGENTS.md");
        std::fs::write(&file, "# my rules\nimportant\n").expect("seed");
        let before = std::fs::read(&file).expect("read");

        let outcome = apply_block(&home, "pi", "install", &file, "人设正文").expect("apply");
        assert_eq!(outcome.action, InstallAction::AppendedBlock);
        assert!(std::fs::read_to_string(&file).expect("read").contains("人设正文"));

        let revert = revert_block(&home, "pi", "install", &file).expect("revert");
        assert_eq!(revert.action, InstallAction::RemovedBlock);
        assert_eq!(std::fs::read(&file).expect("read"), before, "必须逐字节还原");
    }

    #[test]
    fn apply_is_idempotent() {
        let (_dir, home) = setup();
        let file = home.join("AGENTS.md");
        apply_block(&home, "pi", "install", &file, "正文").expect("first");
        let first = std::fs::read(&file).expect("read");
        let second = apply_block(&home, "pi", "install", &file, "正文").expect("second");
        assert_eq!(second.action, InstallAction::Unchanged);
        assert_eq!(std::fs::read(&file).expect("read"), first, "第二次必须不改变字节");
    }

    #[test]
    fn revert_deletes_a_file_we_created() {
        let (_dir, home) = setup();
        let file = home.join("SOUL.md");
        apply_block(&home, "hermes", "install", &file, "正文").expect("apply");
        assert!(file.is_file());
        let outcome = revert_block(&home, "hermes", "install", &file).expect("revert");
        assert_eq!(outcome.action, InstallAction::RemovedFile);
        assert!(!file.exists());
    }

    #[test]
    fn revert_without_a_block_is_a_noop() {
        let (_dir, home) = setup();
        let file = home.join("AGENTS.md");
        std::fs::write(&file, "nothing of ours\n").expect("seed");
        let outcome = revert_block(&home, "pi", "install", &file).expect("revert");
        assert_eq!(outcome.action, InstallAction::Unchanged);
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "nothing of ours\n");
    }

    #[test]
    fn json_field_round_trips() {
        let (_dir, home) = setup();
        let file = home.join("settings.json");
        std::fs::write(&file, "{\n  \"personalization\": {\n    \"customPrompt\": \"原来的\"\n  },\n  \"other\": 1\n}\n").expect("seed");
        let before = std::fs::read(&file).expect("read");

        let previous = apply_json_field(&home, "workbuddy", "install", &file, "personalization.customPrompt", "人设").expect("apply");
        assert_eq!(previous, serde_json::json!("原来的"));
        let after = std::fs::read_to_string(&file).expect("read");
        assert!(after.contains("人设"));
        assert!(after.contains("\"other\": 1"), "其它字段必须保留");

        revert_json_field(&home, "workbuddy", "install", &file, "personalization.customPrompt", &previous).expect("revert");
        assert_eq!(std::fs::read(&file).expect("read"), before, "还原必须逐字节一致");
    }

    #[test]
    fn json_field_on_missing_key_removes_it_on_revert() {
        let (_dir, home) = setup();
        let file = home.join("settings.json");
        std::fs::write(&file, "{}\n").expect("seed");
        let previous = apply_json_field(&home, "workbuddy", "install", &file, "a.b", "x").expect("apply");
        assert!(previous.is_null());
        revert_json_field(&home, "workbuddy", "install", &file, "a.b", &serde_json::Value::Null).expect("revert");
        let text = std::fs::read_to_string(&file).expect("read");
        assert!(!text.contains("\"b\""), "原本不存在的键撤销后应消失：{text}");
    }

    #[test]
    fn invalid_json_is_a_clear_error() {
        let (_dir, home) = setup();
        let file = home.join("settings.json");
        std::fs::write(&file, "{ broken").expect("seed");
        let error = apply_json_field(&home, "workbuddy", "install", &file, "a.b", "x").expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::StateCorrupt);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn apply_records_a_footprint() {
        let (_dir, home) = setup();
        let file = home.join("AGENTS.md");
        apply_block(&home, "pi", "install", &file, "正文").expect("apply");
        let entries = footprint::load(&home).expect("load");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].harness, "pi");
        assert_eq!(entries[0].channel, "install");
        assert_eq!(entries[0].undo.type_name(), "block");
    }

    #[test]
    fn revert_cancels_the_footprint() {
        let (_dir, home) = setup();
        let file = home.join("AGENTS.md");
        apply_block(&home, "pi", "install", &file, "正文").expect("apply");
        revert_block(&home, "pi", "install", &file).expect("revert");
        assert!(footprint::load(&home).expect("load").is_empty(), "撤销后账本应销掉这笔");
    }

    #[test]
    fn body_for_off_is_none() {
        assert_eq!(body_for(Mode::Off, "正文"), None);
        assert_eq!(body_for(Mode::Replace, "  正文  ").as_deref(), Some("正文"));
    }
}
