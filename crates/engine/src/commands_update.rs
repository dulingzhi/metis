//! `update` 与 `journal` 两个命令。

use serde_json::Value;

use metis_core::{ErrorKind, MetisError, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects, required_bool};

/// 检查更新。
#[derive(Debug)]
pub struct CheckUpdate;

impl Command for CheckUpdate {
    fn id(&self) -> &'static str {
        "metis.update-check"
    }
    fn title(&self) -> &'static str {
        "metis.update-check.title"
    }
    fn effects(&self) -> Effects {
        // 只读：联网查一次，不改磁盘。
        Effects { writes_disk: false, spawns_process: false, network: true }
    }
    fn run(&self, _engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let current = params.get("current").and_then(|v| v.as_str()).unwrap_or(env!("CARGO_PKG_VERSION")).to_owned();
        let api = params.get("api").and_then(|v| v.as_str()).map(str::to_owned);
        let options = match api {
            Some(api) => crate::update::UpdateOptions { current, api },
            None => crate::update::UpdateOptions { current, ..Default::default() },
        };
        // 生产用的 fetch：`ureq` 同步 GET，带一个短超时。
        // 核心逻辑接受注入，所以测试永远不联网。
        let result = crate::update::check(&options, |url| {
            let response = ureq::get(url).header("user-agent", "Metis").header("accept", "application/vnd.github+json").call();
            match response {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let body = response.into_body().read_to_string().map_err(|error| error.to_string())?;
                    Ok((status, body))
                }
                Err(ureq::Error::StatusCode(code)) => Ok((code, String::new())),
                Err(error) => Err(error.to_string()),
            }
        });
        Ok(CommandOutput::Json(serde_json::to_value(&result).unwrap_or(Value::Null)))
    }
}

/// 查变更日志。
#[derive(Debug)]
pub struct Journal;

impl Command for Journal {
    fn id(&self) -> &'static str {
        "metis.journal"
    }
    fn title(&self) -> &'static str {
        "metis.journal.title"
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let dir = engine.paths().journal_dir();
        let month = params.get("month").and_then(|v| v.as_str()).map(str::to_owned);
        match month {
            Some(month) => {
                let entries = crate::journal::read_month(&dir, &month)?;
                Ok(CommandOutput::Json(serde_json::json!({ "month": month, "entries": entries })))
            }
            None => {
                let months = crate::journal::months(&dir);
                Ok(CommandOutput::Json(serde_json::json!({ "months": months, "dir": dir.display().to_string() })))
            }
        }
    }
}

/// 按保留期裁剪日志。
#[derive(Debug)]
pub struct PruneJournal;

impl Command for PruneJournal {
    fn id(&self) -> &'static str {
        "metis.journal-prune"
    }
    fn title(&self) -> &'static str {
        "metis.journal-prune.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let keep = params.get("keep").and_then(|v| v.as_u64()).map(|v| v as usize).unwrap_or(crate::journal::KEEP_MONTHS);
        let removed = crate::journal::prune(&engine.paths().journal_dir(), keep)?;
        Ok(CommandOutput::Json(serde_json::json!({ "removed": removed, "keep": keep })))
    }
}

/// 确认升级需要一个 `--yes`（下载并替换是破坏性的）。
pub fn require_confirmation(params: &Value) -> Result<bool> {
    let yes = required_bool(params, "yes").unwrap_or(false);
    if !yes {
        return Err(MetisError::new(ErrorKind::Refused, "升级会替换当前可执行文件，需要明确确认")
            .with_remediation("加 --yes 真的升级；只想看有没有新版本，用 metis update（默认只检查）"));
    }
    Ok(true)
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
    fn update_check_against_a_404_says_it_cannot_check() {
        // 用本地不存在的端口当 API：必然连不上 → 必须报"查不了"而不是 panic。
        let (_dir, engine, commands) = setup();
        let value = commands.run("metis.update-check", &engine, &json!({"api": "http://127.0.0.1:1/latest", "current": "0.1.0"})).expect("check").to_json();
        assert_eq!(value["available"], false);
        assert!(value["error"].as_str().is_some_and(|e| !e.is_empty()), "必须说明为什么查不了：{value}");
    }

    #[test]
    fn update_check_never_writes_disk() {
        let (_dir, engine, commands) = setup();
        let before = std::fs::read_dir(engine.paths().home()).map(|d| d.count()).unwrap_or(0);
        let _ = commands.run("metis.update-check", &engine, &json!({"api": "http://127.0.0.1:1/x", "current": "0.1.0"}));
        let after = std::fs::read_dir(engine.paths().home()).map(|d| d.count()).unwrap_or(0);
        assert_eq!(before, after, "检查更新不该写盘");
    }

    #[test]
    fn journal_lists_months() {
        let (_dir, engine, commands) = setup();
        let entry = crate::journal::JournalEntry::new("cli", "test", true);
        crate::journal::append(&engine.paths().journal_dir(), &entry).expect("append");

        let value = commands.run("metis.journal", &engine, &json!({})).expect("journal").to_json();
        let months = value["months"].as_array().expect("months");
        assert!(!months.is_empty(), "应列出月份：{value}");
    }

    #[test]
    fn journal_reads_a_specific_month() {
        let (_dir, engine, commands) = setup();
        let mut entry = crate::journal::JournalEntry::new("gui", "apply.install", true);
        entry.at = "2026-11-06T00:00:00Z".to_owned();
        crate::journal::append(&engine.paths().journal_dir(), &entry).expect("append");

        let value = commands.run("metis.journal", &engine, &json!({"month": "2026-11"})).expect("journal").to_json();
        let entries = value["entries"].as_array().expect("entries");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["actor"], "gui");
    }

    #[test]
    fn journal_prune_keeps_the_requested_count() {
        let (_dir, engine, commands) = setup();
        let dir = engine.paths().journal_dir();
        for month in ["2026-01", "2026-02", "2026-03"] {
            std::fs::create_dir_all(&dir).expect("mkdir");
            std::fs::write(dir.join(format!("{month}.jsonl")), "").expect("seed");
        }
        let value = commands.run("metis.journal-prune", &engine, &json!({"keep": 1})).expect("prune").to_json();
        assert_eq!(value["removed"].as_array().map(|v| v.len()), Some(2));
        assert_eq!(crate::journal::months(&dir).len(), 1);
    }

    #[test]
    fn apply_writes_a_journal_entry_per_result() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("channel.set", &engine, &json!({"harness": "codex", "channels": ["install"]})).expect("arm");
        commands.run("config.apply", &engine, &json!({})).expect("apply");

        let months = crate::journal::months(&engine.paths().journal_dir());
        assert!(!months.is_empty(), "apply 应写日志");
        let entries = crate::journal::read_month(&engine.paths().journal_dir(), &months[months.len() - 1]).expect("read");
        assert!(entries.iter().any(|e| e.op.starts_with("apply.")), "应有一条 apply 记录：{entries:?}");
    }

    #[test]
    fn upgrade_requires_confirmation() {
        let error = require_confirmation(&json!({})).expect_err("must refuse");
        assert_eq!(error.kind(), ErrorKind::Refused);
        assert!(error.remediation().unwrap_or_default().contains("--yes"));
        assert!(require_confirmation(&json!({"yes": true})).is_ok());
    }
}
