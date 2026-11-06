//! `project` 命令：登记 / 摘除项目目录。
//!
//! `project` 通道的落点是**用户自己的项目目录**里的一个托管块（比如 `AGENTS.md`）。
//! 通道的写入与撤销早已实现（`metis-channel::project` + `apply`），
//! 但一直没有 CLI 入口 —— 用户没法把目录登记进去。这个模块补的就是那一小段。
//!
//! 两条规则：
//! 1. **必须绝对路径**，且目录要真实存在（写进一个不存在的目录没有意义）；
//! 2. 摘除时**先撤盘再清状态**：只清状态会留下撤不掉的托管块
//!    （`docs/PITFALLS.md` G 节记过同类事故）。

use serde_json::Value;

use metis_core::{ErrorKind, HarnessId, MetisError, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects};

/// 记下一个项目目录。
#[derive(Debug)]
pub struct ProjectAdd;

impl Command for ProjectAdd {
    fn id(&self) -> &'static str {
        "project.add"
    }
    fn title(&self) -> &'static str {
        "project.add.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(crate::command::required_str(params, "harness")?.as_str())?;
        let dir = crate::command::required_str(params, "dir")?;

        // 目录必须存在且是绝对路径 —— 与 `metis-channel::project` 的校验保持一致，
        // 但这层也查一遍：用户输入从这里进来，早报错比晚报错好。
        let path = std::path::Path::new(&dir);
        if !path.is_absolute() {
            return Err(
                MetisError::new(ErrorKind::InvalidArgument, format!("项目目录必须是绝对路径：{dir}")).with_remediation("用完整路径，例如 D:\\work\\demo")
            );
        }
        if !path.is_dir() {
            return Err(MetisError::new(ErrorKind::Io, format!("项目目录不存在：{dir}")).with_remediation("先创建这个目录，或换成现有目录"));
        }

        engine.update_state(|state| {
            let entry = state.project_dirs.entry(harness.to_string()).or_default();
            if !entry.iter().any(|existing| existing == &dir) {
                entry.push(dir.clone());
            }
            Ok(())
        })?;

        Ok(CommandOutput::Message(format!("已登记项目目录：{harness} · {dir}（接着跑 `metis apply` 写入托管块）")))
    }
}

/// 摘下一个项目目录（先撤盘）。
#[derive(Debug)]
pub struct ProjectRemove;

impl Command for ProjectRemove {
    fn id(&self) -> &'static str {
        "project.remove"
    }
    fn title(&self) -> &'static str {
        "project.remove.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(crate::command::required_str(params, "harness")?.as_str())?;
        let dir = crate::command::required_str(params, "dir")?;

        // ① **先撤盘**：直接调通道的 revert，不依赖 apply。
        let spec = metis_harness::registry::find(harness.as_str()).ok_or_else(|| {
            MetisError::new(ErrorKind::UnknownHarness, format!("未注册的执行器：{harness}")).with_remediation("用 `metis harnesses` 看本机支持哪些")
        })?;
        let file_name = spec.project_file.unwrap_or("AGENTS.md");
        let outcome = metis_channel::project::revert(engine.paths().home(), harness.as_str(), std::path::Path::new(&dir), file_name)?;
        // `InstallAction` 是通道内部类型；用它的公开字符串形式判断，
        // 这样不用为了一个提示文案把内部枚举导出。
        let removed = !matches!(outcome.action.as_str(), "already-clean" | "unchanged");

        // ② 再清状态。
        engine.update_state(|state| {
            if let Some(entry) = state.project_dirs.get_mut(harness.as_str()) {
                entry.retain(|existing| existing != &dir);
                if entry.is_empty() {
                    state.project_dirs.remove(harness.as_str());
                }
            }
            Ok(())
        })?;

        Ok(CommandOutput::Message(if removed {
            format!("已摘除项目目录并撤下托管块：{harness} · {dir}")
        } else {
            format!("已摘除项目目录：{harness} · {dir}（那里本来就没有我们的托管块）")
        }))
    }
}

/// 列出已登记的项目目录。
#[derive(Debug)]
pub struct ProjectList;

impl Command for ProjectList {
    fn id(&self) -> &'static str {
        "project.list"
    }
    fn title(&self) -> &'static str {
        "project.list.title"
    }
    fn run(&self, engine: &Engine, _params: &Value) -> Result<CommandOutput> {
        let state = engine.state()?;
        let items: Vec<Value> = state
            .project_dirs
            .iter()
            .flat_map(|(harness, dirs)| {
                dirs.iter().map(move |dir| {
                    serde_json::json!({
                        "harness": harness,
                        "dir": dir,
                        // 顺手报告"那个目录还在不在"——目录被删掉是常见情况。
                        "exists": std::path::Path::new(dir).is_dir(),
                    })
                })
            })
            .collect();
        Ok(CommandOutput::Json(serde_json::json!({ "projects": items })))
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
        let paths = Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harnesses"));
        std::fs::create_dir_all(paths.home()).expect("mkdir");
        let project = dir.path().join("my-project");
        std::fs::create_dir_all(&project).expect("mkdir");
        let engine = Engine::new(paths);
        (dir, engine, Commands::default_set())
    }

    #[test]
    fn add_records_the_directory() {
        let (dir, engine, commands) = setup();
        let project = dir.path().join("my-project");
        commands.run("project.add", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("add");

        let state = engine.state().expect("state");
        assert_eq!(state.project_dirs.get("pi").map(|v| v.len()), Some(1));
    }

    #[test]
    fn add_is_idempotent() {
        let (dir, engine, commands) = setup();
        let project = dir.path().join("my-project");
        for _ in 0..3 {
            commands.run("project.add", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("add");
        }
        let state = engine.state().expect("state");
        assert_eq!(state.project_dirs.get("pi").map(|v| v.len()), Some(1), "重复登记不该产生重复条目");
    }

    #[test]
    fn a_relative_path_is_rejected() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("project.add", &engine, &json!({"harness": "pi", "dir": "relative/path"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.remediation().unwrap_or_default().contains("完整路径"), "补救建议要告诉用户怎么写");
    }

    #[test]
    fn a_missing_directory_is_rejected() {
        let (dir, engine, commands) = setup();
        let missing = dir.path().join("nope");
        let error = commands.run("project.add", &engine, &json!({"harness": "pi", "dir": missing.display().to_string()})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn remove_clears_the_state_even_when_nothing_was_written() {
        let (dir, engine, commands) = setup();
        let project = dir.path().join("my-project");
        commands.run("project.add", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("add");
        commands.run("project.remove", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("remove");

        let state = engine.state().expect("state");
        assert!(state.project_dirs.get("pi").is_none_or(|v| v.is_empty()), "摘除后不该留空条目");
    }

    #[test]
    fn remove_takes_the_block_off_disk() {
        let (dir, engine, commands) = setup();
        let project = dir.path().join("my-project");
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("project.add", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("add");
        commands.run("channel.set", &engine, &json!({"harness": "pi", "channels": ["project"]})).expect("arm");
        commands.run("config.apply", &engine, &json!({})).expect("apply");

        // 确认托管块真的写在项目目录里了（用能力表的文件名）。
        let file = project.join("AGENTS.md");
        assert!(file.is_file(), "apply 应该在项目目录写下托管块");
        let after_apply = std::fs::read_to_string(&file).expect("read");
        assert!(after_apply.contains("metis"), "块里应该有我们的标记：{after_apply}");

        commands.run("project.remove", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("remove");
        let after_remove = std::fs::read_to_string(&file).unwrap_or_default();
        assert!(!after_remove.contains("metis"), "摘除后托管块必须消失：{after_remove}");
    }

    #[test]
    fn list_reports_registered_directories() {
        let (dir, engine, commands) = setup();
        let project = dir.path().join("my-project");
        commands.run("project.add", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("add");

        let value = commands.run("project.list", &engine, &json!({})).expect("list").to_json();
        let projects = value["projects"].as_array().expect("array");
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0]["harness"], "pi");
        assert_eq!(projects[0]["exists"], true);
    }

    #[test]
    fn list_flags_a_directory_that_was_deleted() {
        let (dir, engine, commands) = setup();
        let project = dir.path().join("my-project");
        commands.run("project.add", &engine, &json!({"harness": "pi", "dir": project.display().to_string()})).expect("add");
        std::fs::remove_dir_all(&project).expect("rm");

        let value = commands.run("project.list", &engine, &json!({})).expect("list").to_json();
        assert_eq!(value["projects"][0]["exists"], false, "目录被删了要如实报告");
    }
}
