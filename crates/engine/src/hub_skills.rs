//! 把 Hub skill 落到本地：`~/.metis/skills/<slug>/`。
//!
//! 归属规则（`AGENTS.md` 铁律 3）：只删除 manifest 里列出的文件；
//! 目录里用户自己放的东西一律保留。manifest 是“我写过什么”的唯一凭据。
//!
//! 布局：
//!
//! ```text
//! ~/.metis/skills/<slug>/
//!   SKILL.md              ← skill.content.body
//!   references/…          ← skill.content.files[].path
//!   .metis-hub.json       ← 清单（最后写，作为提交点）
//! ```

use std::path::Path;

use metis_core::{ErrorKind, MetisError, Result};
use metis_paths::Paths;
use serde::{Deserialize, Serialize};

use crate::hub::{HubSkillDetail, safe_skill_path, validate_slug};
use crate::persona_import::ImportOutcome;

/// 入口文件名。
pub const SKILL_ENTRY: &str = "SKILL.md";
/// 清单文件名。
pub const SKILL_MANIFEST: &str = ".metis-hub.json";
/// 清单格式版本。
const MANIFEST_VERSION: u32 = 1;

/// 安装选项。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SkillInstallOptions {
    /// 覆盖已有安装（包括“目录存在但没有清单”的情况）。
    pub overwrite: bool,
    /// 只报告，不写盘。
    pub dry_run: bool,
}

/// 一次技能安装的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallReport {
    /// 技能 slug。
    pub slug: String,
    /// 显示名。
    pub name: String,
    /// 一句话说明（来自 Hub）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 版本。
    pub version: String,
    /// 结局。
    pub outcome: ImportOutcome,
    /// 本机 `SKILL.md` 的绝对路径；只有成功安装（或预览）时有。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// 细节（dry-run、跳过原因、失败原因）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 一次技能移除的结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRemoveReport {
    /// 技能 slug。
    pub slug: String,
    /// 是否真的删除了文件。
    pub removed: bool,
    /// 目录里剩下的、不是 Metis 写的文件。
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub leftovers: Vec<String>,
    /// 说明。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 清单：只记录 Metis 自己写过的相对路径。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillManifest {
    v: u32,
    slug: String,
    name: String,
    version: String,
    hash: String,
    installed_at: i64,
    files: Vec<String>,
}

/// 安装（或更新）一个 skill。
pub fn install_skill(paths: &Paths, detail: &HubSkillDetail, options: &SkillInstallOptions) -> Result<SkillInstallReport> {
    validate_slug(&detail.item.slug)?;
    let dir = paths.skill_dir(&detail.item.slug);
    let entry = dir.join(SKILL_ENTRY);
    let manifest_path = dir.join(SKILL_MANIFEST);
    let old = read_manifest(&manifest_path)?;
    let exists = dir.exists();

    if let Some(manifest) = &old
        && !options.overwrite
        && manifest.version == detail.item.version
        && manifest.hash == detail.item.hash
        && entry.is_file()
    {
        return Ok(report(detail, ImportOutcome::Identical, Some(&entry), None));
    }
    if old.is_none() && exists && !options.overwrite {
        return Ok(report(detail, ImportOutcome::Skipped, None, Some("目录已存在且不是 Metis 安装的；加 --overwrite 覆盖".to_owned())));
    }

    let outcome = if old.is_some() || exists { ImportOutcome::Overwritten } else { ImportOutcome::Imported };
    if options.dry_run {
        return Ok(report(detail, outcome, Some(&entry), Some("（dry-run）".to_owned())));
    }

    // 新文件集：入口 + 附件。附件的相对路径由 `hub::safe_skill_path` 校验。
    let mut files: Vec<(String, Vec<u8>)> = vec![(SKILL_ENTRY.to_owned(), detail.content.body.as_bytes().to_vec())];
    for file in &detail.content.files {
        let Some(path) = safe_skill_path(&file.path) else {
            return Err(MetisError::new(ErrorKind::InvalidArgument, format!("技能附件路径不安全：{}", file.path))
                .with_remediation("这个技能不符合 Metis 的路径规则；换一个技能，或联系 Hub 管理员"));
        };
        files.push((path, file.text.as_bytes().to_vec()));
    }

    // 只删除旧 manifest 列出的、且不在新文件集里的文件。
    if let Some(manifest) = &old {
        for relative in &manifest.files {
            if relative == SKILL_ENTRY || files.iter().any(|(path, _)| path == relative) {
                continue;
            }
            let path = dir.join(relative);
            metis_fs::remove_if_exists(&path)?;
            prune_empty_parents(&dir, &path);
        }
    }

    for (relative, bytes) in &files {
        metis_fs::write_atomic(&dir.join(relative), bytes)?;
    }

    // 清单最后写：中途失败时不会把半成品当成“已安装”。
    let manifest = SkillManifest {
        v: MANIFEST_VERSION,
        slug: detail.item.slug.clone(),
        name: detail.item.name.clone(),
        version: detail.item.version.clone(),
        hash: detail.item.hash.clone(),
        installed_at: now_secs(),
        files: files.iter().map(|(path, _)| path.clone()).collect(),
    };
    write_manifest(&manifest_path, &manifest)?;
    Ok(report(detail, outcome, Some(&entry), None))
}

/// 移除一个由 Metis 安装的 skill。
///
/// 没有清单的目录**不删**：无法证明是 Metis 写的。
pub fn remove_skill(paths: &Paths, slug: &str) -> Result<SkillRemoveReport> {
    validate_slug(slug)?;
    let dir = paths.skill_dir(slug);
    let manifest_path = dir.join(SKILL_MANIFEST);
    let Some(manifest) = read_manifest(&manifest_path)? else {
        let detail = if dir.exists() {
            "目录不是 Metis 安装的（没有清单）；没有删除任何文件".to_owned()
        } else {
            "没有安装过这个技能".to_owned()
        };
        return Ok(SkillRemoveReport { slug: slug.to_owned(), removed: false, leftovers: Vec::new(), detail: Some(detail) });
    };

    for relative in &manifest.files {
        let Some(path) = manifest_path_for(relative) else { continue };
        let full = dir.join(path);
        metis_fs::remove_if_exists(&full)?;
        prune_empty_parents(&dir, &full);
    }
    metis_fs::remove_if_exists(&manifest_path)?;

    let mut leftovers = Vec::new();
    if std::fs::remove_dir(&dir).is_err()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        for entry in entries.flatten() {
            leftovers.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(SkillRemoveReport { slug: slug.to_owned(), removed: true, leftovers, detail: None })
}

fn report(detail: &HubSkillDetail, outcome: ImportOutcome, entry: Option<&Path>, note: Option<String>) -> SkillInstallReport {
    let description = (!detail.item.description.trim().is_empty()).then(|| detail.item.description.clone());
    SkillInstallReport {
        slug: detail.item.slug.clone(),
        name: detail.item.name.clone(),
        description,
        version: detail.item.version.clone(),
        outcome,
        entry: entry.map(|path| path.display().to_string()),
        detail: note,
    }
}

fn read_manifest(path: &Path) -> Result<Option<SkillManifest>> {
    if !path.is_file() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path).map_err(|error| MetisError::from(error).with_detail(path.display().to_string()))?;
    let manifest: SkillManifest = serde_json::from_str(&text).map_err(|error| {
        MetisError::new(ErrorKind::Refused, format!("技能清单损坏：{}", path.display()))
            .with_detail(error.to_string())
            .with_remediation("这个目录可能被手工改过；确认后删除它再重试")
    })?;
    if manifest.v != MANIFEST_VERSION {
        return Err(MetisError::new(ErrorKind::Refused, format!("技能清单版本不认识：{}", manifest.v))
            .with_remediation("删除该技能目录后重试；这是更新版本之前的旧清单"));
    }
    Ok(Some(manifest))
}

fn write_manifest(path: &Path, manifest: &SkillManifest) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(manifest).map_err(|error| MetisError::new(ErrorKind::Internal, format!("技能清单序列化失败：{error}")))?;
    metis_fs::write_atomic(path, &bytes)
}

/// 清单里的路径允许入口文件，但仍然拒绝穿越与 manifest 自身。
fn manifest_path_for(relative: &str) -> Option<String> {
    if relative.eq_ignore_ascii_case(SKILL_ENTRY) {
        return Some(SKILL_ENTRY.to_owned());
    }
    safe_skill_path(relative)
}

fn prune_empty_parents(root: &Path, file: &Path) {
    let mut current = file.parent();
    while let Some(dir) = current {
        if dir == root || dir.strip_prefix(root).is_err() {
            break;
        }
        if std::fs::remove_dir(dir).is_err() {
            break;
        }
        current = dir.parent();
    }
}

fn now_secs() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{HubSkillDetail, SkillContent, SkillFile};

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        (dir, paths)
    }

    fn detail(version: &str, files: Vec<(&str, &str)>) -> HubSkillDetail {
        HubSkillDetail {
            item: crate::hub::HubSummary {
                kind: "skill".to_owned(),
                slug: "wb-apiz".to_owned(),
                name: "Apiz".to_owned(),
                description: "网关 CLI".to_owned(),
                version: version.to_owned(),
                hash: format!("sha256:{version}"),
                verified: true,
                ..Default::default()
            },
            content: SkillContent {
                entry: "SKILL.md".to_owned(),
                body: format!("# Apiz {version}\n"),
                files: files.into_iter().map(|(path, text)| SkillFile { path: path.to_owned(), text: text.to_owned() }).collect(),
                notes: None,
            },
            fetched_at: 0,
            source: crate::hub::DataSource::Network,
            warning: None,
        }
    }

    #[test]
    fn install_writes_entry_files_and_manifest() {
        let (_dir, paths) = paths();
        let report = install_skill(&paths, &detail("1.0.0", vec![("references/x.md", "附件")]), &SkillInstallOptions::default()).expect("install");
        assert_eq!(report.outcome, ImportOutcome::Imported);
        let dir = paths.skill_dir("wb-apiz");
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).expect("entry"), "# Apiz 1.0.0\n");
        assert_eq!(std::fs::read_to_string(dir.join("references/x.md")).expect("file"), "附件");
        assert!(dir.join(SKILL_MANIFEST).is_file());
        let manifest = read_manifest(&dir.join(SKILL_MANIFEST)).expect("manifest").expect("present");
        assert_eq!(manifest.files, vec!["SKILL.md".to_owned(), "references/x.md".to_owned()]);
    }

    #[test]
    fn install_is_idempotent() {
        let (_dir, paths) = paths();
        let options = SkillInstallOptions::default();
        let first = install_skill(&paths, &detail("1.0.0", vec![]), &options).expect("first");
        let second = install_skill(&paths, &detail("1.0.0", vec![]), &options).expect("second");
        assert_eq!(first.outcome, ImportOutcome::Imported);
        assert_eq!(second.outcome, ImportOutcome::Identical);
    }

    #[test]
    fn a_newer_version_removes_stale_files() {
        let (_dir, paths) = paths();
        install_skill(&paths, &detail("1.0.0", vec![("references/old.md", "旧")]), &SkillInstallOptions::default()).expect("first");
        let report = install_skill(&paths, &detail("1.1.0", vec![("references/new.md", "新")]), &SkillInstallOptions::default()).expect("second");
        assert_eq!(report.outcome, ImportOutcome::Overwritten);
        let dir = paths.skill_dir("wb-apiz");
        assert!(!dir.join("references/old.md").exists(), "旧版本的文件必须清掉");
        assert!(dir.join("references/new.md").is_file());
    }

    #[test]
    fn a_directory_without_manifest_is_not_overwritten() {
        let (_dir, paths) = paths();
        let dir = paths.skill_dir("wb-apiz");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("SKILL.md"), "用户的文件").expect("seed");
        let report = install_skill(&paths, &detail("1.0.0", vec![]), &SkillInstallOptions::default()).expect("install");
        assert_eq!(report.outcome, ImportOutcome::Skipped);
        assert_eq!(std::fs::read_to_string(dir.join("SKILL.md")).expect("read"), "用户的文件");
    }

    #[test]
    fn dry_run_touches_nothing() {
        let (_dir, paths) = paths();
        let report =
            install_skill(&paths, &detail("1.0.0", vec![("references/x.md", "x")]), &SkillInstallOptions { overwrite: false, dry_run: true }).expect("dry");
        assert_eq!(report.outcome, ImportOutcome::Imported);
        assert!(report.entry.is_some());
        assert!(!paths.skill_dir("wb-apiz").exists(), "dry-run 不能建目录");
    }

    #[test]
    fn unsafe_attachment_is_rejected() {
        let (_dir, paths) = paths();
        let error = install_skill(&paths, &detail("1.0.0", vec![("../evil.md", "x")]), &SkillInstallOptions::default()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(!paths.skill_dir("wb-apiz").join("../evil.md").exists());
    }

    #[test]
    fn remove_deletes_only_managed_files_and_keeps_user_files() {
        let (_dir, paths) = paths();
        install_skill(&paths, &detail("1.0.0", vec![("references/x.md", "x")]), &SkillInstallOptions::default()).expect("install");
        let dir = paths.skill_dir("wb-apiz");
        std::fs::write(dir.join("user-note.md"), "用户的").expect("seed");
        let report = remove_skill(&paths, "wb-apiz").expect("remove");
        assert!(report.removed);
        assert!(!dir.join("SKILL.md").exists());
        assert!(!dir.join("references/x.md").exists());
        assert!(dir.join("user-note.md").is_file(), "用户文件必须保留");
        assert!(report.leftovers.iter().any(|name| name == "user-note.md"), "{:?}", report.leftovers);
    }

    #[test]
    fn remove_without_manifest_refuses() {
        let (_dir, paths) = paths();
        let dir = paths.skill_dir("wb-apiz");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join("SKILL.md"), "用户的文件").expect("seed");
        let report = remove_skill(&paths, "wb-apiz").expect("remove");
        assert!(!report.removed);
        assert!(dir.join("SKILL.md").is_file());
    }

    #[test]
    fn a_corrupt_manifest_is_refused_not_guessed() {
        let (_dir, paths) = paths();
        let dir = paths.skill_dir("wb-apiz");
        std::fs::create_dir_all(&dir).expect("mkdir");
        std::fs::write(dir.join(SKILL_MANIFEST), "not json").expect("seed");
        let error = install_skill(&paths, &detail("1.0.0", vec![]), &SkillInstallOptions::default()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Refused);
        assert!(error.remediation().is_some());
    }
}
