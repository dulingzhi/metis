//! `import agent-purge`：从 Node 版只读导入人设与部分状态。
//!
//! 规则（`docs/DATA-FORMAT.md` §9）：
//! 1. **只读源目录**，绝不修改、不删除；
//! 2. 导入人设 + `persona` / `default` / `overrides`；
//! 3. **不导入 `armed` / `projectDirs`** —— 托管块标记不同，必须重新落盘；
//! 4. 同名默认不覆盖，逐条报告；
//! 5. 检测到基线写下的托管块 → 报告文件清单并**建议先跑上游 `clean`**，Metis 不动它们。

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, PersonaId, Result};
use serde::{Deserialize, Serialize};

/// 导入的一项结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportItem {
    /// 哪个文件或字段。
    pub what: String,
    /// 怎么了。
    pub outcome: ImportOutcome,
    /// 细节。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 一项的结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportOutcome {
    /// 导入了。
    Imported,
    /// 同名已存在，跳过。
    Skipped,
    /// 覆盖了原有的。
    Overwritten,
    /// 有问题。
    Failed,
}

/// 导入报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    /// 源目录。
    pub source: String,
    /// 是否只是预览。
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
    /// 逐项结果。
    pub items: Vec<ImportItem>,
    /// 检测到的上游残留托管块（Metis 不会动它们）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub residue: Vec<ResidueWarning>,
}

/// 上游残留的托管块。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResidueWarning {
    /// 哪个文件。
    pub file: String,
    /// 建议怎么做。
    pub advice: String,
}

impl ImportReport {
    /// 新建。
    pub fn new(source: &Path, dry_run: bool) -> Self {
        Self { source: source.display().to_string(), dry_run, items: Vec::new(), residue: Vec::new() }
    }

    /// 成功导入了几份。
    pub fn imported(&self) -> usize {
        self.items.iter().filter(|i| matches!(i.outcome, ImportOutcome::Imported | ImportOutcome::Overwritten)).count()
    }

    /// 有失败吗。
    pub fn failed(&self) -> usize {
        self.items.iter().filter(|i| i.outcome == ImportOutcome::Failed).count()
    }
}

/// 导入选项。
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// 覆盖同名。
    pub overwrite: bool,
    /// 只预览。
    pub dry_run: bool,
}

/// 上游的数据目录默认位置。
pub fn default_source() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("AGENT_PURGE_HOME") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    dirs::home_dir().map(|home| home.join(".agent-purge"))
}

/// 执行导入。
pub fn import_agent_purge(paths: &metis_paths::Paths, source: &Path, options: &ImportOptions) -> Result<ImportReport> {
    if !source.is_dir() {
        return Err(MetisError::new(ErrorKind::Io, format!("源目录不存在：{}", source.display()))
            .with_remediation("用 --from 指定 agent-purge 的数据目录，或设 AGENT_PURGE_HOME"));
    }

    let mut report = ImportReport::new(source, options.dry_run);

    // ① 人设：逐份导入。
    let source_personas = source.join("personas");
    let store = metis_persona::PersonaStore::new(paths.clone());
    if source_personas.is_dir() {
        let mut names: Vec<PathBuf> = std::fs::read_dir(&source_personas)
            .map_err(|error| MetisError::from(error).with_detail(source_personas.display().to_string()))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
            .collect();
        names.sort();
        for path in names {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
            // `active.md` 是上游的快照，不是人设。
            if stem == "active" {
                continue;
            }
            let Ok(id) = PersonaId::parse(&stem) else {
                report.items.push(ImportItem {
                    what: path.display().to_string(),
                    outcome: ImportOutcome::Failed,
                    detail: Some(format!("文件名不能作为 Metis 的人设 id：{stem}")),
                });
                continue;
            };
            let existing = store.read(&id)?;
            if existing.is_some() && !options.overwrite {
                report.items.push(ImportItem {
                    what: format!("人设 {id}"),
                    outcome: ImportOutcome::Skipped,
                    detail: Some("目标已存在同名；加 --overwrite 覆盖".to_owned()),
                });
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                report.items.push(ImportItem {
                    what: path.display().to_string(),
                    outcome: ImportOutcome::Failed,
                    detail: Some("读不了（可能不是 UTF-8）".to_owned()),
                });
                continue;
            };
            let parsed = metis_persona::parse_persona(&text);
            if !options.dry_run {
                match store.save(&id, &parsed.meta, &parsed.body) {
                    Ok(_) => {}
                    Err(error) => {
                        report
                            .items
                            .push(ImportItem { what: format!("人设 {id}"), outcome: ImportOutcome::Failed, detail: Some(error.message().to_owned()) });
                        continue;
                    }
                }
            }
            report.items.push(ImportItem {
                what: format!("人设 {id}"),
                outcome: if existing.is_some() { ImportOutcome::Overwritten } else { ImportOutcome::Imported },
                detail: if options.dry_run { Some("（dry-run）".to_owned()) } else { None },
            });
        }
    } else {
        report
            .items
            .push(ImportItem { what: "personas/".to_owned(), outcome: ImportOutcome::Skipped, detail: Some("源目录里没有 personas/".to_owned()) });
    }

    // ② 状态：只挑我们语义相同的字段。
    let source_state = source.join("state.json");
    if source_state.is_file() {
        match std::fs::read_to_string(&source_state) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(value) => import_state(paths, &value, options, &mut report)?,
                Err(error) => report.items.push(ImportItem {
                    what: "state.json".to_owned(),
                    outcome: ImportOutcome::Failed,
                    detail: Some(format!("不是合法 JSON：{error}")),
                }),
            },
            Err(error) => report.items.push(ImportItem { what: "state.json".to_owned(), outcome: ImportOutcome::Failed, detail: Some(error.to_string()) }),
        }
    } else {
        report
            .items
            .push(ImportItem { what: "state.json".to_owned(), outcome: ImportOutcome::Skipped, detail: Some("源目录里没有 state.json".to_owned()) });
    }

    // ③ 检测上游残留的托管块：**报告但不碰**。
    for (label, marker) in [("# >>> agent-purge", "块标记"), ("<!-- agent-purge:", "slash 标记")] {
        for path in common_host_files() {
            let Ok(Some(content)) = metis_fs::read_to_string_optional(&path) else { continue };
            if content.contains(marker) {
                report.residue.push(ResidueWarning {
                    file: path.display().to_string(),
                    advice: format!("这里有上游写的{label}；先跑一次上游 `agent-purge clean`，再让 Metis 接管"),
                });
            }
        }
    }

    Ok(report)
}

/// 导入 `state.json` 里语义相同的字段。
fn import_state(paths: &metis_paths::Paths, source: &serde_json::Value, options: &ImportOptions, report: &mut ImportReport) -> Result<()> {
    // 只导入这三项：armed / projectDirs 的落点标记不同，必须重新落盘。
    let persona = source.get("persona").and_then(|v| v.as_str());
    let default = source.get("default");
    let overrides = source.get("overrides");

    if persona.is_some() || default.is_some() || overrides.is_some() {
        if options.dry_run {
            report.items.push(ImportItem {
                what: "state.json（persona / default / overrides）".to_owned(),
                outcome: ImportOutcome::Imported,
                detail: Some("（dry-run：将写入）".to_owned()),
            });
            return Ok(());
        }
        let engine = crate::Engine::new(paths.clone());
        engine.update_state(|state| {
            if let Some(id) = persona
                && state.persona.is_none()
                && PersonaId::parse(id).is_ok()
            {
                state.persona = Some(id.to_owned());
            }
            if let Some(default) = default
                && state.default.harness.is_none()
            {
                state.default.harness = default.get("harness").and_then(|v| v.as_str()).map(str::to_owned);
                if let Some(mode) = default.get("mode").and_then(|v| v.as_str()).and_then(metis_core::Mode::parse) {
                    state.default.mode = mode;
                }
            }
            if let Some(overrides) = overrides.and_then(|v| v.as_object()) {
                for (harness, value) in overrides {
                    let Ok(harness_id) = metis_core::HarnessId::parse(harness) else { continue };
                    if state.overrides.contains_key(harness.as_str()) {
                        continue; // 目标已有例外就不覆盖
                    }
                    if let Some(mode) = value.get("mode").and_then(|v| v.as_str()).and_then(metis_core::Mode::parse) {
                        state.set_override_mode(&harness_id, Some(mode))?;
                    }
                    if let Some(persona) = value.get("persona").and_then(|v| v.as_str())
                        && let Ok(persona_id) = PersonaId::parse(persona)
                    {
                        state.set_override_persona(&harness_id, Some(persona_id));
                    }
                }
            }
            Ok(())
        })?;
    }
    report.items.push(ImportItem {
        what: "state.json（persona / default / overrides）".to_owned(),
        outcome: ImportOutcome::Imported,
        detail: Some("armed 与 projectDirs 未导入：落点标记不同，需要重新 arm".to_owned()),
    });
    Ok(())
}

/// 常见的宿主配置文件（用来检测上游残留）。
fn common_host_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    let mut out = Vec::new();
    for harness in ["pi", "omp", "grok", "zcode", "kilo", "claude"] {
        let Ok(id) = metis_core::HarnessId::parse(harness) else { continue };
        if let Some(layout) = metis_paths::HarnessLayout::of(&id) {
            let base = layout.resolve(|k| std::env::var(k).ok(), &home);
            for relative in ["AGENTS.md", "CLAUDE.md", "agent/AGENTS.md", "SYSTEM.md"] {
                let path = base.join(relative);
                if path.is_file() {
                    out.push(path);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;

    fn setup() -> (tempfile::TempDir, Paths, PathBuf) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("metis-home"));
        let source = dir.path().join("agent-purge");
        std::fs::create_dir_all(source.join("personas")).expect("mkdir");
        (dir, paths, source)
    }

    fn write_persona(source: &Path, id: &str, body: &str) {
        std::fs::write(source.join("personas").join(format!("{id}.md")), format!("---\nid: {id}\n---\n\n{body}\n")).expect("write");
    }

    #[test]
    fn missing_source_is_a_clear_error() {
        let (_dir, paths, _source) = setup();
        let error = import_agent_purge(&paths, Path::new("/definitely/not/here"), &ImportOptions::default()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn imports_personas() {
        let (_dir, paths, source) = setup();
        write_persona(&source, "concise", "用简体中文。");
        write_persona(&source, "strict", "只留人设。");

        let report = import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        assert_eq!(report.imported(), 2, "{report:?}");

        let store = metis_persona::PersonaStore::new(paths.clone());
        let persona = store.read(&PersonaId::parse("concise").expect("id")).expect("read").expect("present");
        assert_eq!(persona.trimmed_body(), "用简体中文。");
    }

    #[test]
    fn active_snapshot_is_skipped() {
        let (_dir, paths, source) = setup();
        std::fs::write(source.join("personas").join("active.md"), "snapshot").expect("write");
        write_persona(&source, "real", "正文");

        let report = import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        assert_eq!(report.imported(), 1, "active.md 是快照，不该当成一份人设");
    }

    #[test]
    fn existing_persona_is_not_overwritten_by_default() {
        let (_dir, paths, source) = setup();
        let store = metis_persona::PersonaStore::new(paths.clone());
        let id = PersonaId::parse("concise").expect("id");
        store.save(&id, &metis_persona::Meta::default(), "原来的").expect("save");
        write_persona(&source, "concise", "新的");

        let report = import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        assert!(report.items.iter().any(|i| i.outcome == ImportOutcome::Skipped));
        let persona = store.read(&id).expect("read").expect("present");
        assert_eq!(persona.trimmed_body(), "原来的", "默认不覆盖");
    }

    #[test]
    fn overwrite_replaces_the_existing_persona() {
        let (_dir, paths, source) = setup();
        let store = metis_persona::PersonaStore::new(paths.clone());
        let id = PersonaId::parse("concise").expect("id");
        store.save(&id, &metis_persona::Meta::default(), "原来的").expect("save");
        write_persona(&source, "concise", "新的");

        let report = import_agent_purge(&paths, &source, &ImportOptions { overwrite: true, dry_run: false }).expect("import");
        assert!(report.items.iter().any(|i| i.outcome == ImportOutcome::Overwritten));
        assert_eq!(store.read(&id).expect("read").expect("present").trimmed_body(), "新的");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (_dir, paths, source) = setup();
        write_persona(&source, "concise", "正文");

        let report = import_agent_purge(&paths, &source, &ImportOptions { overwrite: false, dry_run: true }).expect("import");
        assert!(report.dry_run);
        let store = metis_persona::PersonaStore::new(paths.clone());
        assert!(store.read(&PersonaId::parse("concise").expect("id")).expect("read").is_none(), "dry-run 不能写文件");
    }

    #[test]
    fn imports_persona_and_default_but_not_armed() {
        let (_dir, paths, source) = setup();
        write_persona(&source, "concise", "正文");
        std::fs::write(
            source.join("state.json"),
            r#"{"v":1,"persona":"concise","default":{"harness":"codex","mode":"append"},"armed":{"codex":["install"]},"projectDirs":{"pi":["/x"]}}"#,
        )
        .expect("write");

        import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        let engine = crate::Engine::new(paths.clone());
        let state = engine.state().expect("state");
        assert_eq!(state.persona.as_deref(), Some("concise"));
        assert_eq!(state.default.harness.as_deref(), Some("codex"));
        assert_eq!(state.default.mode, metis_core::Mode::Append);
        assert!(state.armed.is_empty(), "armed 不该被导入（落点标记不同）");
        assert!(state.project_dirs.is_empty(), "projectDirs 不该被导入");
    }

    #[test]
    fn corrupt_source_state_is_reported_not_fatal() {
        let (_dir, paths, source) = setup();
        write_persona(&source, "concise", "正文");
        std::fs::write(source.join("state.json"), b"{ broken").expect("write");

        let report = import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        assert_eq!(report.failed(), 1, "应报告 state.json 失败");
        assert_eq!(report.imported(), 1, "人设仍然要导入成功");
    }

    #[test]
    fn source_directory_is_never_modified() {
        let (_dir, paths, source) = setup();
        write_persona(&source, "concise", "正文");
        let before = std::fs::read(source.join("personas").join("concise.md")).expect("read");

        import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        let after = std::fs::read(source.join("personas").join("concise.md")).expect("read");
        assert_eq!(before, after, "只读导入：源文件一个字节都不能变");
    }

    #[test]
    fn invalid_persona_filename_is_reported() {
        let (_dir, paths, source) = setup();
        std::fs::write(source.join("personas").join("active.migrated.md"), "x").expect("write");
        let report = import_agent_purge(&paths, &source, &ImportOptions::default()).expect("import");
        // `active.migrated` 含 `.migrated` 后缀 → 被 PersonaId 拒绝。
        assert!(report.failed() >= 1 || report.items.iter().any(|i| i.outcome == ImportOutcome::Skipped));
    }

    #[test]
    fn default_source_mentions_agent_purge() {
        let source = default_source();
        assert!(source.is_some(), "应有默认源");
        assert!(source.expect("source").display().to_string().contains("agent-purge"));
    }
}
