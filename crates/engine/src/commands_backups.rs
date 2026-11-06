//! `backups` 命令：查看与清理备份目录的体积。

use serde_json::Value;

use metis_core::Result;

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects};

/// 账本里还在引用的备份文件名。
fn referenced_names(engine: &Engine) -> Result<std::collections::BTreeSet<String>> {
    let entries = metis_fs::footprint::load(engine.paths().home())?;
    let mut out = std::collections::BTreeSet::new();
    for entry in entries {
        if let metis_fs::footprint::Undo::WholeFile { backup } = &entry.undo {
            out.insert(backup.clone());
        }
    }
    Ok(out)
}

/// 查看备份目录占用。
#[derive(Debug)]
pub struct BackupsUsage;

impl Command for BackupsUsage {
    fn id(&self) -> &'static str {
        "metis.backups-usage"
    }
    fn title(&self) -> &'static str {
        "metis.backups-usage.title"
    }
    fn run(&self, engine: &Engine, _params: &Value) -> Result<CommandOutput> {
        let referenced = referenced_names(engine)?;
        let usage = crate::backups::scan(&engine.paths().backups_dir(), &referenced)?;
        Ok(CommandOutput::Json(serde_json::to_value(&usage).unwrap_or(Value::Null)))
    }
}

/// 清理孤立备份（账本不再引用的）。
#[derive(Debug)]
pub struct BackupsPrune;

impl Command for BackupsPrune {
    fn id(&self) -> &'static str {
        "metis.backups-prune"
    }
    fn title(&self) -> &'static str {
        "metis.backups-prune.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let dry_run = params.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(true);
        let referenced = referenced_names(engine)?;
        let usage = crate::backups::scan(&engine.paths().backups_dir(), &referenced)?;
        let result = crate::backups::prune(&engine.paths().backups_dir(), &usage, dry_run)?;
        Ok(CommandOutput::Json(serde_json::json!({
            "result": result,
            "usageAfter": crate::backups::scan(&engine.paths().backups_dir(), &referenced)?,
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Commands;
    use metis_paths::Paths;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Engine, Commands) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        std::fs::create_dir_all(paths.backups_dir()).expect("mkdir");
        let engine = Engine::new(paths);
        (dir, engine, Commands::default_set())
    }

    #[test]
    fn usage_of_an_empty_dir_is_zero() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("metis.backups-usage", &engine, &json!({})).expect("usage").to_json();
        assert_eq!(value["files"], 0);
        assert_eq!(value["totalBytes"], 0);
    }

    #[test]
    fn orphan_backups_are_reported() {
        let (_dir, engine, commands) = setup();
        std::fs::write(engine.paths().backups_dir().join("orphan.bak"), vec![b'x'; 500]).expect("seed");
        let value = commands.run("metis.backups-usage", &engine, &json!({})).expect("usage").to_json();
        assert_eq!(value["orphanCount"], 1);
        assert_eq!(value["orphanBytes"], 500);
    }

    #[test]
    fn a_referenced_backup_is_not_orphaned() {
        let (_dir, engine, commands) = setup();
        std::fs::write(engine.paths().backups_dir().join("kept.bak"), b"orig").expect("seed");
        // 造一条引用它的账本条目。
        metis_fs::footprint::record(
            engine.paths().home(),
            metis_fs::footprint::Footprint {
                id: String::new(),
                path: engine.paths().home().join("some-config.toml"),
                kind: "file".to_owned(),
                harness: "codex".to_owned(),
                channel: "install".to_owned(),
                undo: metis_fs::footprint::Undo::WholeFile { backup: "kept.bak".to_owned() },
                sha256: None,
                created_at: String::new(),
            },
        )
        .expect("record");

        let value = commands.run("metis.backups-usage", &engine, &json!({})).expect("usage").to_json();
        assert_eq!(value["orphanCount"], 0, "被账本引用的不算孤儿：{value}");
    }

    #[test]
    fn prune_defaults_to_dry_run() {
        // **默认必须是预览**：销毁性操作不能因为"少写一个 flag"就真删。
        let (_dir, engine, commands) = setup();
        std::fs::write(engine.paths().backups_dir().join("orphan.bak"), b"x").expect("seed");
        let value = commands.run("metis.backups-prune", &engine, &json!({})).expect("prune").to_json();
        assert_eq!(value["result"]["dryRun"], true);
        assert!(engine.paths().backups_dir().join("orphan.bak").is_file(), "默认不能真删");
    }

    #[test]
    fn prune_with_dry_run_false_removes_orphans() {
        let (_dir, engine, commands) = setup();
        std::fs::write(engine.paths().backups_dir().join("orphan.bak"), b"x").expect("seed");
        let value = commands.run("metis.backups-prune", &engine, &json!({"dryRun": false})).expect("prune").to_json();
        assert_eq!(value["result"]["removed"].as_array().map(|a| a.len()), Some(1));
        assert!(!engine.paths().backups_dir().join("orphan.bak").exists());
        assert_eq!(value["usageAfter"]["files"], 0);
    }
}
