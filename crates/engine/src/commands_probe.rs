//! `doctor`（探测）与 `launch`（启动）两条命令。
//!
//! 铁律 6：两者都在引擎里，CLI 与 GUI 共用。GUI 不自己起进程。

use serde_json::Value;

use metis_core::{ErrorKind, HarnessId, MetisError, Mode, PersonaId, Result};

use crate::Engine;
use crate::command::Effects;
use crate::command::{Command, CommandOutput, optional_str, required_bool, required_str};

/// 探测本机装了哪些执行器。
#[derive(Debug)]
pub struct Doctor;

impl Command for Doctor {
    fn id(&self) -> &'static str {
        "harness.doctor"
    }
    fn title(&self) -> &'static str {
        "harness.doctor.title"
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        // 版本探测要起进程，默认开；`--no-version` 可关掉（CI 里快一点）。
        let probe_versions = params.get("versions").and_then(|v| v.as_bool()).unwrap_or(true);
        let options = metis_harness::DetectOptions { probe_versions };
        let user_home =
            dirs::home_dir().ok_or_else(|| MetisError::new(ErrorKind::Io, "找不到用户主目录").with_remediation("设置 HOME / USERPROFILE 后重试"))?;

        let detected = metis_harness::detect_all(|key| std::env::var(key).ok(), &user_home, &options);
        let state = engine.state()?;

        let items: Vec<Value> = detected
            .iter()
            .map(|item| {
                let caps = metis_harness::registry::capabilities(item.harness.as_str());
                serde_json::json!({
                    "id": item.harness.as_str(),
                    "installed": item.installed(),
                    "summary": item.summary(),
                    "binary": item.binary.as_ref().map(|p| p.display().to_string()),
                    "home": item.home.display().to_string(),
                    "homeExists": item.home_exists,
                    "markers": item.markers,
                    "version": item.version,
                    "desktopOnly": item.desktop_only,
                    "injectable": caps.as_ref().map(|c| c.injectable),
                    "armed": state.armed.get(item.harness.as_str()).cloned().unwrap_or_default(),
                })
            })
            .collect();

        let installed = detected.iter().filter(|d| d.installed()).count();
        Ok(CommandOutput::Json(serde_json::json!({
            "installed": installed,
            "total": detected.len(),
            "harnesses": items,
        })))
    }
}

/// 生成启动计划（`--dry-run` 用），或真的启动宿主。
#[derive(Debug)]
pub struct Launch;

impl Command for Launch {
    fn id(&self) -> &'static str {
        "harness.launch"
    }
    fn title(&self) -> &'static str {
        "harness.launch.title"
    }
    fn effects(&self) -> Effects {
        // 默认只生成计划；真的启动由调用方显式要求。
        Effects { writes_disk: false, spawns_process: true, network: false }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(&required_str(params, "harness")?)?;
        let dry_run = params.get("dryRun").and_then(|v| v.as_bool()).unwrap_or(true);
        let exec = required_bool(params, "exec").unwrap_or(false) && !dry_run;
        let extra: Vec<String> =
            params.get("args").and_then(|v| v.as_array()).map(|items| items.iter().filter_map(|v| v.as_str()).map(str::to_owned).collect()).unwrap_or_default();

        let state = engine.state()?;
        let activation_state = state.activation_state()?;
        // 显式启动是最强的许可来源（且只有经进程归属核验的路径才传 `launch: true`）。
        let activation = metis_core::activation::resolve_activation(
            &activation_state,
            &harness,
            &metis_core::ActivationContext { launch: true, mode: optional_str(params, "mode").as_deref().and_then(Mode::parse), ..Default::default() },
        );

        let store = metis_persona::PersonaStore::new(engine.paths().clone());
        let persona = match &activation.resolved.persona {
            Some(id) => store.read(id)?,
            None => None,
        };
        let body = persona.as_ref().map(|p| p.trimmed_body().to_owned());
        let active_path = engine.paths().active_persona_file();

        let plan =
            metis_harness::launch_spec::plan(&harness, activation.effective_mode(), body.as_deref(), Some(active_path.display().to_string().as_str()), &extra);

        let downgrade = metis_harness::launch_spec::replace_downgrade_note(&harness, activation.resolved.mode);
        let summary = serde_json::json!({
            "harness": harness.as_str(),
            "on": activation.on,
            "mode": activation.effective_mode().as_str(),
            "persona": persona.as_ref().map(|p| p.id.to_string()),
            "explain": activation.explain(),
            "command": plan.preview(),
            "program": plan.program,
            // 脱敏：结构化输出里的参数同样可能带完整人设。
            "args": plan.args.iter().map(|a| metis_harness::launch_spec::redact(a)).collect::<Vec<_>>(),
            "note": plan.note,
            "residue": plan.residue,
            "desktopOnly": plan.no_cli,
            "manual": plan.manual,
            "downgrade": downgrade,
            "executed": false,
        });

        if !exec {
            return Ok(CommandOutput::Json(summary));
        }

        // 真启动：程序缺失要给出安装建议，不能 panic。
        if plan.no_cli {
            return Err(MetisError::new(ErrorKind::UnsupportedChannel, format!("{harness} 没有命令行入口"))
                .with_remediation("用 metis arm <h> --channel install 装常驻通道，然后重开那个客户端"));
        }
        let outcome = metis_process::Exec::new(plan.program.clone()).args(plan.args.clone()).run()?;
        let mut value = summary;
        value["executed"] = serde_json::json!(true);
        value["exitCode"] = serde_json::json!(outcome.status);
        value["stdout"] = serde_json::json!(outcome.stdout);
        value["stderr"] = serde_json::json!(outcome.stderr);
        Ok(CommandOutput::Json(value))
    }
}

/// 一次性问答（`--ask`）。
#[derive(Debug)]
pub struct Ask;

impl Command for Ask {
    fn id(&self) -> &'static str {
        "harness.ask"
    }
    fn title(&self) -> &'static str {
        "harness.ask.title"
    }
    fn effects(&self) -> Effects {
        Effects { writes_disk: false, spawns_process: true, network: false }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(&required_str(params, "harness")?)?;
        let prompt = required_str(params, "prompt")?;
        let exec = required_bool(params, "exec").unwrap_or(false);

        let state = engine.state()?;
        let activation_state = state.activation_state()?;
        let activation =
            metis_core::activation::resolve_activation(&activation_state, &harness, &metis_core::ActivationContext { launch: true, ..Default::default() });
        let store = metis_persona::PersonaStore::new(engine.paths().clone());
        let persona = match &activation.resolved.persona {
            Some(id) => store.read(id)?,
            None => None,
        };
        let body = persona.as_ref().map(|p| p.trimmed_body().to_owned());
        let path = engine.paths().active_persona_file();

        let plan =
            metis_harness::launch_spec::oneshot(&harness, &prompt, activation.effective_mode(), body.as_deref(), Some(path.display().to_string().as_str()))
                .ok_or_else(|| {
                    MetisError::new(ErrorKind::UnsupportedChannel, format!("{harness} 没有一次性运行模式")).with_remediation(
                        "用 metis launch <h> 起交互式会话，或换一家（pi / claude / codex / grok / omp / opencode / dsh / kilo / codebuddy / hermes / zcode）",
                    )
                })?;

        let mut summary = serde_json::json!({
            "harness": harness.as_str(),
            "command": plan.preview(),
            "program": plan.program,
            // 脱敏：结构化输出里的参数同样可能带完整人设。
            "args": plan.args.iter().map(|a| metis_harness::launch_spec::redact(a)).collect::<Vec<_>>(),
            "note": plan.note,
            "executed": false,
        });
        if !exec {
            return Ok(CommandOutput::Json(summary));
        }
        let outcome = metis_process::Exec::new(plan.program.clone()).args(plan.args.clone()).run()?;
        summary["executed"] = serde_json::json!(true);
        summary["exitCode"] = serde_json::json!(outcome.status);
        summary["stdout"] = serde_json::json!(outcome.stdout);
        Ok(CommandOutput::Json(summary))
    }
}

/// 显式设置某家的人设例外（`use --for`）。
pub fn persona_exists(engine: &Engine, id: &PersonaId) -> Result<bool> {
    let store = metis_persona::PersonaStore::new(engine.paths().clone());
    Ok(store.read(id)?.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Engine, crate::Commands) {
        let dir = tempfile::tempdir().expect("tempdir");
        // 宿主主目录也重定向：doctor 会读它。
        let paths = Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harnesses"));
        let engine = Engine::new(paths);
        (dir, engine, crate::Commands::default_set())
    }

    #[test]
    fn doctor_reports_every_harness() {
        let (_dir, engine, commands) = setup();
        let output = commands.run("harness.doctor", &engine, &json!({"versions": false})).expect("doctor");
        let value = output.to_json();
        assert_eq!(value["total"], 15);
        let items = value["harnesses"].as_array().expect("array");
        assert_eq!(items.len(), 15);
        // 每一项都要有判定所需的字段。
        for item in items {
            assert!(item["id"].is_string());
            assert!(item["home"].is_string(), "必须报告主目录");
            assert!(item["installed"].is_boolean());
            assert!(!item["summary"].as_str().unwrap_or_default().is_empty());
        }
    }

    #[test]
    fn doctor_marks_antigravity_as_not_injectable() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("harness.doctor", &engine, &json!({"versions": false})).expect("doctor").to_json();
        let antigravity = value["harnesses"].as_array().expect("array").iter().find(|h| h["id"] == "antigravity").expect("antigravity");
        assert_eq!(antigravity["injectable"], false);
    }

    #[test]
    fn launch_defaults_to_dry_run() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        let value = commands.run("harness.launch", &engine, &json!({"harness": "codex"})).expect("launch").to_json();
        assert_eq!(value["executed"], false, "默认必须只生成计划");
        assert!(value["program"].as_str().unwrap_or_default().contains("codex"));
        assert!(value["command"].as_str().unwrap_or_default().contains("-c"), "{}", value["command"]);
    }

    #[test]
    fn launch_reports_the_persona_and_mode() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("state.mode", &engine, &json!({"mode": "append"})).expect("mode");

        let value = commands.run("harness.launch", &engine, &json!({"harness": "grok"})).expect("launch").to_json();
        assert_eq!(value["mode"], "append");
        assert_eq!(value["persona"], "p");
        assert!(value["args"].as_array().is_some_and(|a| a.iter().any(|v| v == "--rules")));
    }

    #[test]
    fn launch_off_mode_produces_no_injection() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("state.power", &engine, &json!({"on": false})).expect("pause");

        let value = commands.run("harness.launch", &engine, &json!({"harness": "claude"})).expect("launch").to_json();
        assert_eq!(value["mode"], "off");
        let args = value["args"].as_array().expect("args").clone();
        assert!(!args.iter().any(|v| v.as_str().unwrap_or_default().contains("--system-prompt")), "暂停时不该带注入参数：{args:?}");
    }

    #[test]
    fn launch_warns_about_replace_downgrade() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        let value = commands.run("harness.launch", &engine, &json!({"harness": "claude"})).expect("launch").to_json();
        assert!(value["downgrade"].as_str().unwrap_or_default().contains("append"), "必须提示 replace 降级");
    }

    #[test]
    fn launch_of_a_desktop_only_harness_refuses_to_execute() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        let value = commands.run("harness.launch", &engine, &json!({"harness": "pi-desktop"})).expect("plan").to_json();
        assert_eq!(value["desktopOnly"], true, "应标记为没有命令行入口");
        assert!(!value["manual"].as_array().unwrap_or(&Vec::new()).is_empty(), "应给出下一步");

        let error = commands.run("harness.launch", &engine, &json!({"harness": "pi-desktop", "dryRun": false, "exec": true})).expect_err("必须拒绝");
        assert_eq!(error.kind(), ErrorKind::UnsupportedChannel);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn launch_preview_never_leaks_the_persona() {
        let (_dir, engine, commands) = setup();
        let long = "机密人设".repeat(100);
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": long})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        let value = commands.run("harness.launch", &engine, &json!({"harness": "omp"})).expect("launch").to_json();
        let command = value["command"].as_str().unwrap_or_default();
        assert!(!command.contains("机密人设"), "预览不能带人设正文：{command}");
    }

    #[test]
    fn ask_covers_known_harnesses_and_refuses_unknown() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        let value = commands.run("harness.ask", &engine, &json!({"harness": "codex", "prompt": "解释这段代码"})).expect("ask").to_json();
        assert!(value["command"].as_str().unwrap_or_default().contains("exec"));

        let error = commands.run("harness.ask", &engine, &json!({"harness": "pi-desktop", "prompt": "x"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::UnsupportedChannel);
        assert!(error.remediation().unwrap_or_default().contains("pi"), "补救建议要列出可用的家");
    }

    #[test]
    fn ask_without_prompt_is_rejected() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("harness.ask", &engine, &json!({"harness": "codex"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    }
}
