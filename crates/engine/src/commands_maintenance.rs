//! `clean` / `uninstall` / `self-uninstall` / `import` 四个命令。

use serde_json::Value;

use metis_core::{ErrorKind, HarnessId, MetisError, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects, required_bool, required_str};

/// 撤回全部已登记的写入（保留人设与 `state.json`）。
#[derive(Debug)]
pub struct Clean;

impl Command for Clean {
    fn id(&self) -> &'static str {
        "channel.clean"
    }
    fn title(&self) -> &'static str {
        "channel.clean.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let dry_run = params.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(false);
        let report = crate::clean::clean(engine.paths(), crate::clean::CleanScope::Channels, dry_run)?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

/// 撤回该家的全部通道并删掉已写入的内容。
#[derive(Debug)]
pub struct Uninstall;

impl Command for Uninstall {
    fn id(&self) -> &'static str {
        "channel.uninstall"
    }
    fn title(&self) -> &'static str {
        "channel.uninstall.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(&required_str(params, "harness")?)?;
        let _ = required_bool(params, "on"); // 参数形状对齐；卸载总是"关"
        // 复用 `channel.set` 的撤销语义：先摘盘，再清状态。
        let commands = crate::Commands::default_set();
        commands.run("channel.set", engine, &serde_json::json!({ "harness": harness.as_str(), "channels": [] }))?;
        Ok(CommandOutput::Message(format!("{harness} 的接管控件已全部撤下；人设与状态保留")))
    }
}

/// 撤回写入**并删除数据目录**（不可逆）。
#[derive(Debug)]
pub struct SelfUninstall;

impl Command for SelfUninstall {
    fn id(&self) -> &'static str {
        "metis.self-uninstall"
    }
    fn title(&self) -> &'static str {
        "metis.self-uninstall.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let dry_run = params.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(false);
        let confirmed = params.get("yes").and_then(|v| v.as_bool()).unwrap_or(false);
        // 没确认也没预览 → 明确拒绝，不是"默默什么都不做"。
        if !dry_run && !confirmed {
            return Err(MetisError::new(ErrorKind::Refused, "永久删除数据目录需要明确确认").with_remediation("加 --yes 真的删除，或加 --dry-run 只预览"));
        }
        let report = crate::clean::clean(engine.paths(), crate::clean::CleanScope::Everything, dry_run)?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

/// 从 Node 版 agent-purge 只读导入。
#[derive(Debug)]
pub struct ImportAgentPurge;

impl Command for ImportAgentPurge {
    fn id(&self) -> &'static str {
        "metis.import-agent-purge"
    }
    fn title(&self) -> &'static str {
        "metis.import-agent-purge.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let source = match params.get("from").and_then(|v| v.as_str()) {
            Some(explicit) => std::path::PathBuf::from(explicit),
            None => crate::import::default_source()
                .ok_or_else(|| MetisError::new(ErrorKind::Io, "找不到 agent-purge 的数据目录").with_remediation("用 --from 指定它"))?,
        };
        let options = crate::import::ImportOptions {
            overwrite: params.get("overwrite").and_then(|v| v.as_bool()).unwrap_or(false),
            dry_run: params.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(false),
        };
        let report = crate::import::import_agent_purge(engine.paths(), &source, &options)?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
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
        std::fs::create_dir_all(paths.home()).expect("mkdir");
        let engine = Engine::new(paths);
        (dir, engine, Commands::default_set())
    }

    #[test]
    fn clean_reports_a_dry_run() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("channel.clean", &engine, &json!({"dryRun": true})).expect("clean").to_json();
        assert_eq!(value["dryRun"], true);
        assert!(engine.paths().home().exists(), "dry-run 不能删数据目录");
    }

    #[test]
    fn uninstall_requires_a_harness() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("channel.uninstall", &engine, &json!({})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    }

    #[test]
    fn uninstall_clears_the_armed_list() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("channel.set", &engine, &json!({"harness": "codex", "channels": ["install"]})).expect("arm");

        commands.run("channel.uninstall", &engine, &json!({"harness": "codex"})).expect("uninstall");
        let state = engine.state().expect("state");
        assert!(state.armed_for(&HarnessId::parse("codex").expect("id")).is_empty(), "通道应被撤下");
        assert_eq!(state.persona.as_deref(), Some("p"), "人设必须保留");
    }

    #[test]
    fn self_uninstall_refuses_without_confirmation() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("metis.self-uninstall", &engine, &json!({})).expect_err("must refuse");
        assert_eq!(error.kind(), ErrorKind::Refused);
        assert!(error.remediation().unwrap_or_default().contains("--yes"));
        assert!(engine.paths().home().exists(), "拒绝时不能动数据");
    }

    #[test]
    fn self_uninstall_dry_run_shows_what_would_happen() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("metis.self-uninstall", &engine, &json!({"dryRun": true})).expect("dry-run").to_json();
        assert_eq!(value["dryRun"], true);
        assert!(engine.paths().home().exists(), "dry-run 不能删");
    }

    #[test]
    fn import_agent_purge_reads_the_source() {
        let (dir, engine, commands) = setup();
        let source = dir.path().join("upstream");
        std::fs::create_dir_all(source.join("personas")).expect("mkdir");
        std::fs::write(source.join("personas").join("concise.md"), "---\nid: concise\n---\n\n正文\n").expect("write");

        let value = commands.run("metis.import-agent-purge", &engine, &json!({"from": source.display().to_string()})).expect("import").to_json();
        assert!(value["source"].as_str().is_some());
        assert_eq!(value["dryRun"], false);
        let items = value["items"].as_array().expect("items");
        assert!(items.iter().any(|i| i["outcome"] == "imported"), "应导入至少一项：{items:?}");
    }

    #[test]
    fn import_missing_source_is_a_clear_error() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("metis.import-agent-purge", &engine, &json!({"from": "/definitely/not/here"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some());
    }
}
