//! `project` 通道：在项目根写托管块（`AGENTS.md` / `CLAUDE.md`）。
//!
//! 目录以**绝对路径**登记：后续同步只处理已登记的目录，不跟着当前工作目录漂移
//! （`docs/PARITY.md` 的 `projectDirs` 语义）。

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, Result};
use metis_fs::footprint;

use crate::blocks::{self, BlockMarker};
use crate::install::{InstallAction, InstallOutcome};

/// 给用户看的注意事项（项目通道与全局通道同时开会有两份人设）。
pub const PROJECT_NOTE: &str = "宿主会共同读取同名规则文件；同时开启全局与项目通道会有两份人设";

/// 项目目录的规范化：必须是绝对路径。
pub fn normalise_dir(dir: &Path) -> Result<PathBuf> {
    if !dir.is_absolute() {
        return Err(MetisError::new(ErrorKind::InvalidArgument, format!("项目目录必须是绝对路径：{}", dir.display()))
            .with_remediation("用完整路径，例如 D:\\work\\demo"));
    }
    Ok(metis_fs::owner_key(dir).into())
}

/// 项目规则文件路径。
pub fn target_file(dir: &Path, file_name: &str) -> PathBuf {
    dir.join(file_name)
}

/// 写项目托管块。幂等。
pub fn apply(home: &Path, harness: &str, dir: &Path, file_name: &str, body: &str) -> Result<InstallOutcome> {
    let dir = normalise_dir(dir)?;
    let file = target_file(&dir, file_name);
    if !dir.is_dir() {
        return Err(MetisError::new(ErrorKind::Io, format!("项目目录不存在：{}", dir.display())).with_remediation("先创建这个目录，或换成现有目录"));
    }
    let marker = BlockMarker::for_path(&file);
    let existing = metis_fs::read_to_string_optional(&file)?;
    let (content, action) = match existing.as_deref() {
        None => (blocks::upsert_block("", marker, body), InstallAction::Created),
        Some(current) => {
            let had = blocks::has_block(current, marker);
            let next = blocks::upsert_block(current, marker, body);
            if next == current {
                return Ok(InstallOutcome { file, action: InstallAction::Unchanged });
            }
            (next, if had { InstallAction::ReplacedBlock } else { InstallAction::AppendedBlock })
        }
    };
    metis_fs::write_atomic(&file, content.as_bytes())?;
    footprint::record(
        home,
        footprint::Footprint {
            id: String::new(),
            path: file.clone(),
            kind: "file".to_owned(),
            harness: harness.to_owned(),
            channel: "project".to_owned(),
            undo: footprint::Undo::Block { marker: marker.begin.to_owned() },
            sha256: metis_fs::hash_file(&file)?,
            created_at: String::new(),
        },
    )?;
    Ok(InstallOutcome { file, action })
}

/// 撤掉项目托管块，逐字节还原。
pub fn revert(home: &Path, harness: &str, dir: &Path, file_name: &str) -> Result<InstallOutcome> {
    let dir = normalise_dir(dir)?;
    let file = target_file(&dir, file_name);
    let Some(current) = metis_fs::read_to_string_optional(&file)? else {
        return Ok(InstallOutcome { file, action: InstallAction::RemovedFile });
    };
    let marker = BlockMarker::for_path(&file);
    let outcome = blocks::strip_block(&current, marker);
    if !outcome.removed {
        return Ok(InstallOutcome { file, action: InstallAction::Unchanged });
    }
    if outcome.content.is_empty() {
        metis_fs::remove_if_exists(&file)?;
        footprint::cancel(home, footprint::CancelKey::Location { path: &file, harness, channel: "project" })?;
        return Ok(InstallOutcome { file, action: InstallAction::RemovedFile });
    }
    metis_fs::write_atomic(&file, outcome.content.as_bytes())?;
    footprint::cancel(home, footprint::CancelKey::Location { path: &file, harness, channel: "project" })?;
    Ok(InstallOutcome { file, action: InstallAction::RemovedBlock })
}

/// 这个目录里有没有我们的块。
pub fn is_managed(dir: &Path, file_name: &str) -> bool {
    let file = target_file(dir, file_name);
    let marker = BlockMarker::for_path(&file);
    metis_fs::read_to_string_optional(&file).ok().flatten().is_some_and(|content| blocks::has_block(&content, marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_directories() {
        let error = normalise_dir(Path::new("relative/path")).expect_err("must reject");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn accepts_absolute_directories() {
        let dir = if cfg!(windows) { Path::new("D:\\work\\demo") } else { Path::new("/work/demo") };
        assert!(normalise_dir(dir).is_ok());
    }

    #[test]
    fn apply_then_revert_restores_bytes() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir");
        let file = project.join("AGENTS.md");
        std::fs::write(&file, "# project rules\n").expect("seed");
        let before = std::fs::read(&file).expect("read");

        let outcome = apply(&home, "pi", &project, "AGENTS.md", "人设").expect("apply");
        assert_eq!(outcome.action, InstallAction::AppendedBlock);
        assert!(std::fs::read_to_string(&file).expect("read").contains("人设"));

        let reverted = revert(&home, "pi", &project, "AGENTS.md").expect("revert");
        assert_eq!(reverted.action, InstallAction::RemovedBlock);
        assert_eq!(std::fs::read(&file).expect("read"), before, "必须逐字节还原");
    }

    #[test]
    fn apply_is_idempotent() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir");
        apply(&home, "pi", &project, "AGENTS.md", "人设").expect("first");
        let first = std::fs::read(project.join("AGENTS.md")).expect("read");
        let second = apply(&home, "pi", &project, "AGENTS.md", "人设").expect("second");
        assert_eq!(second.action, InstallAction::Unchanged);
        assert_eq!(std::fs::read(project.join("AGENTS.md")).expect("read"), first);
    }

    #[test]
    fn missing_project_directory_is_a_clear_error() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let missing = root.path().join("nope");
        let error = apply(&home, "pi", &missing, "AGENTS.md", "人设").expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn managed_detection_reflects_state() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir");
        assert!(!is_managed(&project, "AGENTS.md"));
        apply(&home, "pi", &project, "AGENTS.md", "人设").expect("apply");
        assert!(is_managed(&project, "AGENTS.md"));
        revert(&home, "pi", &project, "AGENTS.md").expect("revert");
        assert!(!is_managed(&project, "AGENTS.md"));
    }

    #[test]
    fn project_uses_a_distinct_channel_in_the_ledger() {
        let root = tempfile::tempdir().expect("tempdir");
        let home = root.path().join("home");
        let project = root.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir");
        apply(&home, "pi", &project, "AGENTS.md", "人设").expect("apply");
        let entries = footprint::load(&home).expect("load");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].channel, "project");
    }
}
