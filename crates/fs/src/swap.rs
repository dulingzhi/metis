//! 文件交换与恢复 journal。
//!
//! 目标：改动别人的文件之后，**要么能原样还原，要么明确告诉用户为什么不能**。
//! 算法逐条对应 `docs/DATA-FORMAT.md` §6，每一步都有测试。
//!
//! 六步（不可简化）：
//! 1. 内容已一致 → 不写 journal、不加锁，直接返回"无事可做"的 undo（幂等）；
//! 2. `create_new` 建 journal；已存在且持有者 pid 已死 → 先恢复它的交换，再重试一次；
//! 3. 读到当前字节，算 `before_hash`，写 journal 并 `sync_all`；
//! 4. 原子写新内容；
//! 5. 恢复时核对 `operation` 与当前哈希（before / after / 其它三态）；
//! 6. 只有成功还原之后才删 journal。

use std::fs;
use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, Result};
use serde::{Deserialize, Serialize};

/// journal 格式版本。只认 2：旧格式无法证明所有权，保留并报告（`recover` 的规则）。
pub const JOURNAL_VERSION: u32 = 2;

/// 一次交换的恢复凭据。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JournalRecord {
    /// 格式版本。
    pub version: u32,
    /// 被交换的文件。
    pub file: PathBuf,
    /// 做这次交换的进程。
    pub pid: u32,
    /// 本次交换的唯一 id。恢复时核对，防止"别的进程已经接管"。
    pub operation: String,
    /// 时间（RFC 3339）。
    pub at: String,
    /// 原文件的字节（base64）；文件原本不存在为 `None`。
    pub backup: Option<String>,
    /// 原文件哈希；原本不存在为 `None`。
    pub before_hash: Option<String>,
    /// 写入后的哈希。
    pub after_hash: String,
    /// 原文件权限位（Unix 有意义）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

/// 一次交换的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapOutcome {
    /// 真的写了。
    Written,
    /// 内容已经一致，什么都没做（幂等路径）。
    AlreadyInSync,
}

/// 可重试、幂等的撤销句柄。
#[derive(Debug)]
pub struct Swap {
    record: JournalRecord,
    journal: PathBuf,
    done: bool,
}

impl Swap {
    /// 这次交换是否什么都没做。
    pub fn is_noop(&self) -> bool {
        self.record.operation.is_empty()
    }

    /// 撤销这次交换。重复调用无害。
    ///
    /// 失败时**保留 journal 与原字节**，并给出可复制的路径。
    pub fn undo(&mut self) -> Result<()> {
        if self.done || self.is_noop() {
            self.done = true;
            return Ok(());
        }
        restore_record(&self.record, &self.journal)?;
        self.done = true;
        Ok(())
    }

    /// 是否已经撤销。
    pub fn is_done(&self) -> bool {
        self.done
    }
}

impl Drop for Swap {
    fn drop(&mut self) {
        // 故意不自动还原：交换的语义是"我来管这段内容，直到有人显式撤销"。
        // 想自动还原的调用方自己持 `Swap` 并显式 undo（配合 metis-process 的 done 语义）。
    }
}

/// 交换 journal 目录。
pub fn journal_dir(dir: &Path) -> PathBuf {
    dir.to_path_buf()
}

fn journal_path(dir: &Path, file: &Path) -> PathBuf {
    let key = super::owner_key(file);
    dir.join(format!("{}.json", super::hash_bytes(key.as_bytes())))
}

/// 读一个 journal。
fn read_journal(path: &Path) -> Option<JournalRecord> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice::<JournalRecord>(&bytes).ok()
}

/// 把文件换成本次内容，返回可撤销句柄。
///
/// `home` 只用于错误信息里的路径展示，实际 journal 落在 `journal_root`。
pub fn swap_file(file: &Path, content: &[u8], journal_root: &Path, _home: &Path) -> Result<(Swap, SwapOutcome)> {
    let file = canonical(file)?;
    let next = content;
    let next_hash = super::hash_bytes(next);

    // ① 内容已一致 → 什么都不做。这条让常驻插件可以在每次启动时安全调用而互不打扰。
    if super::hash_file(&file)?.as_deref() == Some(next_hash.as_str()) {
        return Ok((Swap { record: empty_record(&file), journal: PathBuf::new(), done: true }, SwapOutcome::AlreadyInSync));
    }

    super::ensure_dir(journal_root)?;
    let journal = journal_path(journal_root, &file);

    // ② 建 journal（独占）。
    let mut handle = match fs::OpenOptions::new().write(true).create_new(true).open(&journal) {
        Ok(handle) => handle,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let holder = read_journal(&journal);
            let reclaimable = holder.as_ref().is_some_and(|h| h.version == JOURNAL_VERSION && !pid_alive(h.pid));
            if reclaimable {
                if let Some(holder) = holder {
                    restore_record(&holder, &journal).map_err(|error| {
                        MetisError::new(ErrorKind::RestoreConflict, format!("无法回收已退出进程的交换，请先处理 journal：{}", file.display()))
                            .with_detail(error.to_string())
                            .with_remediation(format!("检查并手动恢复 {}", journal.display()))
                    })?;
                }
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&journal)
                    .map_err(|error| MetisError::from(error).with_detail(journal.display().to_string()))?
            } else {
                let pid = holder.as_ref().map(|h| format!("（持有者 PID {}）", h.pid)).unwrap_or_default();
                return Err(MetisError::new(ErrorKind::LockHeld, format!("文件存在未结束的交换{pid}，请先处理 journal：{}", file.display()))
                    .with_detail(journal.display().to_string())
                    .with_remediation(format!("确认没有进程在跑之后删除 {}", journal.display())));
            }
        }
        Err(error) => return Err(MetisError::from(error).with_detail(journal.display().to_string())),
    };

    // ③ 记录原字节。
    let backup = super::read_optional(&file)?;
    let record = JournalRecord {
        version: JOURNAL_VERSION,
        file: file.clone(),
        pid: std::process::id(),
        operation: uuid::Uuid::new_v4().simple().to_string(),
        at: now_rfc3339(),
        backup: backup.as_ref().map(|bytes| base64_encode(bytes)),
        before_hash: backup.as_ref().map(|bytes| super::hash_bytes(bytes)),
        after_hash: next_hash,
        mode: backup.as_ref().and_then(|_| fs::metadata(&file).ok()).map(|meta| metadata_mode(&meta)),
    };
    let write_journal = |handle: &mut fs::File| -> Result<()> {
        use std::io::Write;
        let bytes = serde_json::to_vec(&record).map_err(|error| MetisError::new(ErrorKind::Internal, "journal 无法序列化").with_detail(error.to_string()))?;
        handle.write_all(&bytes).map_err(|error| MetisError::from(error).with_detail(journal.display().to_string()))?;
        handle.sync_all().map_err(|error| MetisError::from(error).with_detail(journal.display().to_string()))?;
        Ok(())
    };
    if let Err(error) = write_journal(&mut handle) {
        let _ = fs::remove_file(&journal);
        return Err(error);
    }
    drop(handle);

    // ④ 原子写新内容。失败时用 journal 回滚（此时磁盘上还是原内容，restore 会识别为 before）。
    if let Err(error) = super::write_atomic(&file, next) {
        let _ = restore_record(&record, &journal);
        return Err(error);
    }

    Ok((Swap { record, journal, done: false }, SwapOutcome::Written))
}

/// 恢复一条 journal。
fn restore_record(record: &JournalRecord, journal: &Path) -> Result<()> {
    // 重新读 journal：防止所有权已经变了。
    let current = read_journal(journal).ok_or_else(|| {
        MetisError::new(ErrorKind::RestoreConflict, format!("无法读取恢复凭据，文件未改动：{}", journal.display()))
            .with_remediation("检查该文件；不要在没有把握时删除它")
    })?;
    if current.operation != record.operation {
        return Err(MetisError::new(ErrorKind::RestoreConflict, format!("恢复所有权已改变：{}", record.file.display()))
            .with_detail(journal.display().to_string())
            .with_remediation("另一个进程接管了这次交换；先处理它的 journal"));
    }

    let now = super::hash_file(&record.file)?;
    let is_before = now == record.before_hash;
    let is_after = now.as_deref() == Some(record.after_hash.as_str());

    if !is_before && !is_after {
        // 既不等于 before 也不等于 after：用户（或别的程序）改过它。保留一切。
        return Err(MetisError::new(ErrorKind::RestoreConflict, format!("文件在交换后被修改，保留 journal 等待处理：{}", record.file.display()))
            .with_detail(journal.display().to_string())
            .with_remediation("比较文件内容后手动合并；确认无误再删除 journal"));
    }

    if is_before {
        // 已经是原样（可能上一次已经还原过），只需清理 journal。
        let _ = fs::remove_file(journal);
        return Ok(());
    }

    match &record.backup {
        None => super::remove_if_exists(&record.file)?,
        Some(encoded) => {
            let bytes = base64_decode(encoded).ok_or_else(|| {
                MetisError::new(ErrorKind::StateCorrupt, format!("恢复凭据里的备份无法解码：{}", journal.display()))
                    .with_remediation("保留该 journal；从 backups/ 里找原件")
            })?;
            super::write_atomic(&record.file, &bytes)?;
        }
    }
    fs::remove_file(journal).map_err(|error| MetisError::from(error).with_detail(journal.display().to_string()))?;
    Ok(())
}

/// 启动时回收：只处理**已死亡进程**的 v2 journal。
///
/// 旧格式或不完整的 journal 只警告，**绝不覆盖文件**（`PITFALLS.md` B2）。
pub fn recover(journal_root: &Path, warn: &mut dyn FnMut(&str)) -> Result<usize> {
    if !journal_root.is_dir() {
        return Ok(0);
    }
    let mut recovered = 0_usize;
    let entries = fs::read_dir(journal_root).map_err(|error| MetisError::from(error).with_detail(journal_root.display().to_string()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Some(record) = read_journal(&path) else {
            warn(&format!("旧版或不完整 journal，未自动覆盖文件：{}", path.display()));
            continue;
        };
        if record.version != JOURNAL_VERSION || record.operation.is_empty() || !record.file.is_absolute() {
            warn(&format!("旧版或不完整 journal，未自动覆盖文件：{}", path.display()));
            continue;
        }
        if pid_alive(record.pid) {
            continue;
        }
        match restore_record(&record, &path) {
            Ok(()) => recovered += 1,
            Err(error) => warn(&format!("{}: {error}", path.display())),
        }
    }
    Ok(recovered)
}

fn empty_record(file: &Path) -> JournalRecord {
    JournalRecord {
        version: JOURNAL_VERSION,
        file: file.to_path_buf(),
        pid: std::process::id(),
        operation: String::new(),
        at: String::new(),
        backup: None,
        before_hash: None,
        after_hash: String::new(),
        mode: None,
    }
}

/// 同一目录的符号链接指向同一把锁；不跟随不存在的目标。
fn canonical(file: &Path) -> Result<PathBuf> {
    match fs::canonicalize(file) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = file.parent().unwrap_or(Path::new("."));
            match fs::canonicalize(parent) {
                Ok(parent) => Ok(parent.join(file.file_name().unwrap_or_default())),
                Err(_) => Ok(file.to_path_buf()),
            }
        }
        Err(error) => Err(MetisError::from(error).with_detail(file.display().to_string())),
    }
}

fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    super::lock::is_pid_alive(pid)
}

#[cfg(unix)]
fn metadata_mode(meta: &fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn metadata_mode(_meta: &fs::Metadata) -> u32 {
    0
}

const BASE64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// 极简 base64 编码（只为 journal 的备份字段；避免为了这一个需求引入依赖）。
pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(BASE64_ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(BASE64_ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { BASE64_ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { BASE64_ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// 极简 base64 解码。非法输入返回 `None`。
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let index = |c: u8| BASE64_ALPHABET.iter().position(|x| *x == c).map(|p| p as u32);
    let cleaned: Vec<u8> = text.bytes().filter(|c| !c.is_ascii_whitespace()).collect();
    if !cleaned.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(cleaned.len() / 4 * 3);
    for chunk in cleaned.chunks(4) {
        let mut n = 0_u32;
        let mut pad = 0;
        for (i, byte) in chunk.iter().enumerate() {
            if *byte == b'=' {
                pad += 1;
                n <<= 6;
            } else {
                if pad > 0 {
                    return None;
                }
                let _ = i;
                n = (n << 6) | index(*byte)?;
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().to_path_buf();
        let journals = home.join("journals");
        (dir, home, journals)
    }

    #[test]
    fn swap_writes_and_undo_restores_bytes_exactly() {
        let (dir, home, journals) = setup();
        let file = home.join("config.toml");
        fs::write(&file, b"original\r\nkeep me").expect("seed");
        let before = fs::read(&file).expect("read");

        let (mut swap, outcome) = swap_file(&file, b"new content", &journals, &home).expect("swap");
        assert_eq!(outcome, SwapOutcome::Written);
        assert_eq!(fs::read(&file).expect("read"), b"new content".to_vec());

        swap.undo().expect("undo");
        assert_eq!(fs::read(&file).expect("read"), before, "必须逐字节还原（含 CRLF）");
        assert!(swap.is_done());
        drop(dir);
    }

    #[test]
    fn swap_of_new_file_deletes_it_on_undo() {
        let (_dir, home, journals) = setup();
        let file = home.join("created.txt");
        let (mut swap, _) = swap_file(&file, b"hello", &journals, &home).expect("swap");
        assert!(file.is_file());
        swap.undo().expect("undo");
        assert!(!file.exists(), "原本不存在的文件，撤销后应消失");
    }

    #[test]
    fn identical_content_is_a_noop_without_a_journal() {
        let (_dir, home, journals) = setup();
        let file = home.join("same.txt");
        fs::write(&file, b"same").expect("seed");
        let (swap, outcome) = swap_file(&file, b"same", &journals, &home).expect("swap");
        assert_eq!(outcome, SwapOutcome::AlreadyInSync);
        assert!(swap.is_noop());
        let journals_left = fs::read_dir(&journals).map(|d| d.count()).unwrap_or(0);
        assert_eq!(journals_left, 0, "内容一致时不该写 journal");
    }

    #[test]
    fn undo_is_idempotent() {
        let (_dir, home, journals) = setup();
        let file = home.join("a.txt");
        fs::write(&file, b"one").expect("seed");
        let (mut swap, _) = swap_file(&file, b"two", &journals, &home).expect("swap");
        swap.undo().expect("first undo");
        swap.undo().expect("second undo");
        assert_eq!(fs::read(&file).expect("read"), b"one".to_vec());
    }

    #[test]
    fn external_modification_blocks_restore_and_keeps_the_journal() {
        let (_dir, home, journals) = setup();
        let file = home.join("b.txt");
        fs::write(&file, b"one").expect("seed");
        let (mut swap, _) = swap_file(&file, b"two", &journals, &home).expect("swap");
        // 用户在中间改了它。
        fs::write(&file, b"user edit").expect("edit");

        let error = swap.undo().expect_err("must refuse to overwrite user edit");
        assert_eq!(error.kind(), ErrorKind::RestoreConflict);
        assert!(error.remediation().is_some());
        assert_eq!(fs::read(&file).expect("read"), b"user edit".to_vec(), "用户的修改必须保留");
        let journals_left = fs::read_dir(&journals).map(|d| d.count()).unwrap_or(0);
        assert_eq!(journals_left, 1, "冲突时 journal 必须保留");
    }

    #[test]
    fn concurrent_swap_on_the_same_file_is_refused() {
        let (_dir, home, journals) = setup();
        let file = home.join("c.txt");
        fs::write(&file, b"one").expect("seed");
        let (first, _) = swap_file(&file, b"two", &journals, &home).expect("first swap");
        let error = swap_file(&file, b"three", &journals, &home).expect_err("second swap must be refused");
        assert_eq!(error.kind(), ErrorKind::LockHeld);
        drop(first);
    }

    #[test]
    fn dead_owner_journal_is_reclaimed_before_the_swap() {
        let (_dir, home, journals) = setup();
        super::super::ensure_dir(&journals).expect("mkdir");
        let file = home.join("d.txt");
        fs::write(&file, b"dead-state").expect("seed");
        let journal = journal_path(&journals, &file);
        let record = JournalRecord {
            version: JOURNAL_VERSION,
            file: file.clone(),
            pid: u32::MAX,
            operation: "dead-op".to_owned(),
            at: "2026-01-01T00:00:00Z".to_owned(),
            backup: Some(base64_encode(b"original")),
            before_hash: Some(super::super::hash_bytes(b"original")),
            after_hash: super::super::hash_bytes(b"dead-state"),
            mode: None,
        };
        fs::write(&journal, serde_json::to_vec(&record).expect("json")).expect("seed journal");

        let (mut swap, _) = swap_file(&file, b"fresh", &journals, &home).expect("reclaim then swap");
        swap.undo().expect("undo");
        assert_eq!(fs::read(&file).expect("read"), b"dead-state".to_vec(), "撤销应回到回收后的状态");
    }

    #[test]
    fn recover_restores_only_dead_owners() {
        let (_dir, home, journals) = setup();
        super::super::ensure_dir(&journals).expect("mkdir");
        let file = home.join("e.txt");
        fs::write(&file, b"after").expect("seed");
        let journal = journal_path(&journals, &file);
        let record = JournalRecord {
            version: JOURNAL_VERSION,
            file: file.clone(),
            pid: u32::MAX,
            operation: "op".to_owned(),
            at: "2026-01-01T00:00:00Z".to_owned(),
            backup: Some(base64_encode(b"before")),
            before_hash: Some(super::super::hash_bytes(b"before")),
            after_hash: super::super::hash_bytes(b"after"),
            mode: None,
        };
        fs::write(&journal, serde_json::to_vec(&record).expect("json")).expect("seed");

        let mut warnings = Vec::new();
        let recovered = recover(&journals, &mut |message| warnings.push(message.to_owned())).expect("recover");
        assert_eq!(recovered, 1);
        assert_eq!(fs::read(&file).expect("read"), b"before".to_vec());
        assert!(warnings.is_empty(), "不该有警告：{warnings:?}");
    }

    #[test]
    fn recover_warns_about_legacy_journals_without_touching_files() {
        let (_dir, home, journals) = setup();
        super::super::ensure_dir(&journals).expect("mkdir");
        let file = home.join("f.txt");
        fs::write(&file, b"untouched").expect("seed");
        let journal = journals.join("legacy.json");
        fs::write(&journal, br#"{"version":1,"file":"x"}"#).expect("seed");

        let mut warnings = Vec::new();
        let recovered = recover(&journals, &mut |message| warnings.push(message.to_owned())).expect("recover");
        assert_eq!(recovered, 0);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("未自动覆盖"));
        assert_eq!(fs::read(&file).expect("read"), b"untouched".to_vec());
    }

    #[test]
    fn recover_ignores_non_json_files() {
        let (_dir, _home, journals) = setup();
        super::super::ensure_dir(&journals).expect("mkdir");
        fs::write(journals.join("notes.txt"), b"hi").expect("seed");
        let mut warnings = Vec::new();
        assert_eq!(recover(&journals, &mut |m| warnings.push(m.to_owned())).expect("recover"), 0);
        assert!(warnings.is_empty());
    }

    #[test]
    fn base64_round_trips_arbitrary_bytes() {
        for sample in [&b""[..], b"a", b"ab", b"abc", b"abcd", &[0u8, 255, 128, 7][..]] {
            let encoded = base64_encode(sample);
            assert_eq!(base64_decode(&encoded).as_deref(), Some(sample), "往返失败：{sample:?}");
        }
    }

    #[test]
    fn base64_decode_rejects_garbage() {
        assert!(base64_decode("!!!").is_none());
        assert!(base64_decode("abc").is_none(), "长度不是 4 的倍数");
    }
}
