//! `armor` 的四个命令：`plan` / `preflight` / `run` / `judge`。

use serde_json::Value;

use metis_core::{HarnessId, Mode, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects};

/// 给一家（或全部）生成策略。
#[derive(Debug)]
pub struct ArmorPlan;

impl Command for ArmorPlan {
    fn id(&self) -> &'static str {
        "armor.plan"
    }
    fn title(&self) -> &'static str {
        "armor.plan.title"
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let targets = targets(params)?;
        let store = engine.paths().armor_file();
        let measurements: crate::armor::Store = crate::armor::Store::load(&store)?;

        let plans: Vec<crate::armor::Plan> = targets
            .iter()
            .map(|harness| {
                let measured = measurements.get(harness);
                crate::armor::plan(harness, measured.as_ref())
            })
            .collect();
        Ok(CommandOutput::Json(serde_json::to_value(&plans).unwrap_or(Value::Null)))
    }
}

/// 本机预检。
#[derive(Debug)]
pub struct ArmorPreflight;

impl Command for ArmorPreflight {
    fn id(&self) -> &'static str {
        "armor.preflight"
    }
    fn title(&self) -> &'static str {
        "armor.preflight.title"
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(crate::command::required_str(params, "harness")?.as_str())?;
        let store = crate::armor::Store::load(&engine.paths().armor_file())?;
        let measured = store.get(&harness);
        let plan = crate::armor::plan(&harness, measured.as_ref());

        // 人设就绪？
        let state = engine.state()?;
        let persona_store = metis_persona::PersonaStore::new(engine.paths().clone());
        let persona_ready = match &state.persona {
            Some(id) => match metis_core::PersonaId::parse(id) {
                Ok(parsed) => persona_store.read(&parsed)?.is_some_and(|p| !p.trimmed_body().is_empty()),
                Err(_) => false,
            },
            None => false,
        };
        // 通道可用？（这家至少有一条能注入的通道）
        let channel_usable = metis_harness::registry::find(harness.as_str()).map(|spec| metis_harness::registry::caps_of(spec).injectable).unwrap_or(false);

        let items = crate::armor::preflight(&plan, channel_usable, persona_ready);
        Ok(CommandOutput::Json(serde_json::json!({
            "harness": harness.as_str(),
            "plan": plan,
            "items": items,
            "ok": crate::armor::preflight_ok(&items),
        })))
    }
}

/// 把一次评测结果记进矩阵。
#[derive(Debug)]
pub struct ArmorRecord;

impl Command for ArmorRecord {
    fn id(&self) -> &'static str {
        "armor.record"
    }
    fn title(&self) -> &'static str {
        "armor.record.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(crate::command::required_str(params, "harness")?.as_str())?;
        let layer = params.get("layer").and_then(|v| v.as_str()).and_then(crate::armor::ArmorLayer::parse).ok_or_else(|| {
            metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, "缺少或非法的 layer").with_remediation("layer 只能是 off / sys / user / seed")
        })?;
        let hits = params.get("hits").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        let rounds = params.get("rounds").and_then(|v| v.as_u64()).unwrap_or(1) as u32;

        let result = crate::armor::BenchResult {
            harness: harness.to_string(),
            layer,
            rounds,
            hits: hits.min(rounds),
            where_: params.get("where").and_then(|v| v.as_str()).map(str::to_owned),
        };

        let path = engine.paths().armor_file();
        let mut store = crate::armor::Store::load(&path)?;
        store.record(&result);
        store.save(&path)?;
        Ok(CommandOutput::Json(serde_json::json!({
            "recorded": result,
            "rate": result.rate(),
        })))
    }
}

/// 把已记录的原始结果聚合成矩阵。
#[derive(Debug)]
pub struct ArmorJudge;

impl Command for ArmorJudge {
    fn id(&self) -> &'static str {
        "armor.judge"
    }
    fn title(&self) -> &'static str {
        "armor.judge.title"
    }
    fn run(&self, engine: &Engine, _params: &Value) -> Result<CommandOutput> {
        let store = crate::armor::Store::load(&engine.paths().armor_file())?;
        let harnesses = targets(&Value::Null).unwrap_or_default();
        let matrix = crate::armor::Matrix::build(&store.results, &harnesses);
        Ok(CommandOutput::Json(serde_json::json!({
            "matrix": matrix,
            "table": matrix.render(),
        })))
    }
}

/// 把策略写进 state（用户点"按建议设置"时用）。
#[derive(Debug)]
pub struct ArmorAdopt;

impl Command for ArmorAdopt {
    fn id(&self) -> &'static str {
        "armor.adopt"
    }
    fn title(&self) -> &'static str {
        "armor.adopt.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let harness = HarnessId::parse(crate::command::required_str(params, "harness")?.as_str())?;
        let store = crate::armor::Store::load(&engine.paths().armor_file())?;
        let measured = store.get(&harness);
        let plan = crate::armor::plan(&harness, measured.as_ref());

        let Some(mode) = Mode::parse(&plan.recommended_mode) else {
            return Err(metis_core::MetisError::new(metis_core::ErrorKind::Internal, format!("armor 产出了非法模式：{}", plan.recommended_mode))
                .with_remediation("这是程序缺陷，请把这条输出贴到 issue"));
        };

        engine.update_state(|state| {
            state.set_override_mode(&harness, Some(mode))?;
            Ok(())
        })?;
        Ok(CommandOutput::Message(format!("{} 的模式已按建议设为 {}（{}）", harness, mode, plan.verdict)))
    }
}

/// 解析目标：显式给的，或者全部内置的。
fn targets(params: &Value) -> Result<Vec<HarnessId>> {
    let explicit = params.get("harnesses").and_then(|v| v.as_array());
    match explicit {
        Some(items) if !items.is_empty() => items.iter().filter_map(|v| v.as_str()).map(HarnessId::parse).collect::<Result<Vec<_>>>(),
        _ => Ok(HarnessId::BUILTIN.iter().filter_map(|id| HarnessId::parse(id).ok()).collect()),
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
    fn plan_without_data_says_measure_first() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("armor.plan", &engine, &json!({"harnesses": ["codex"]})).expect("plan").to_json();
        let plans = value.as_array().expect("array");
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0]["recommended_layer"], "off");
        assert!(plans[0]["missing"].as_str().is_some());
    }

    #[test]
    fn record_then_plan_recommends_the_measured_layer() {
        let (_dir, engine, commands) = setup();
        commands
            .run(
                "armor.record",
                &engine,
                &json!({
                    "harness": "codex", "layer": "sys", "hits": 2, "rounds": 2, "where": "developer",
                }),
            )
            .expect("record");

        let value = commands.run("armor.plan", &engine, &json!({"harnesses": ["codex"]})).expect("plan").to_json();
        assert_eq!(value[0]["recommended_layer"], "sys");
        assert_eq!(value[0]["recommended_mode"], "replace");
    }

    #[test]
    fn record_clamps_hits_to_rounds() {
        // 用户手抖写了 hits > rounds：不能算出 >100%。
        let (_dir, engine, commands) = setup();
        let value = commands
            .run(
                "armor.record",
                &engine,
                &json!({
                    "harness": "codex", "layer": "sys", "hits": 99, "rounds": 2,
                }),
            )
            .expect("record")
            .to_json();
        assert_eq!(value["rate"], 100);
    }

    #[test]
    fn record_rejects_an_unknown_layer() {
        let (_dir, engine, commands) = setup();
        let error = commands
            .run(
                "armor.record",
                &engine,
                &json!({
                    "harness": "codex", "layer": "nope", "hits": 1, "rounds": 1,
                }),
            )
            .expect_err("must fail");
        assert_eq!(error.kind(), metis_core::ErrorKind::InvalidArgument);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn judge_renders_a_table_from_recorded_results() {
        let (_dir, engine, commands) = setup();
        commands.run("armor.record", &engine, &json!({"harness": "codex", "layer": "sys", "hits": 2, "rounds": 2})).expect("record");
        let value = commands.run("armor.judge", &engine, &json!({})).expect("judge").to_json();
        let table = value["table"].as_str().expect("table");
        assert!(table.contains("codex"));
        assert!(table.contains("harness"));
    }

    #[test]
    fn preflight_reports_missing_persona() {
        let (_dir, engine, commands) = setup();
        commands.run("armor.record", &engine, &json!({"harness": "codex", "layer": "sys", "hits": 2, "rounds": 2})).expect("record");
        let value = commands.run("armor.preflight", &engine, &json!({"harness": "codex"})).expect("preflight").to_json();
        assert_eq!(value["ok"], false, "没有人设时预检不该通过");
        let items = value["items"].as_array().expect("items");
        assert!(items.iter().any(|i| i["ok"] == false && i["detail"].as_str().is_some_and(|d| d.contains("metis use"))));
    }

    #[test]
    fn preflight_passes_when_persona_and_channel_are_ready() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("armor.record", &engine, &json!({"harness": "codex", "layer": "sys", "hits": 2, "rounds": 2})).expect("record");

        let value = commands.run("armor.preflight", &engine, &json!({"harness": "codex"})).expect("preflight").to_json();
        assert_eq!(value["ok"], true, "{value}");
    }

    #[test]
    fn adopt_writes_the_recommended_mode_into_state() {
        let (_dir, engine, commands) = setup();
        commands.run("armor.record", &engine, &json!({"harness": "codex", "layer": "sys", "hits": 2, "rounds": 2})).expect("record");
        commands.run("armor.adopt", &engine, &json!({"harness": "codex"})).expect("adopt");

        let state = engine.state().expect("state");
        let mode = state.overrides.get("codex").and_then(|o| o.mode);
        assert_eq!(mode, Some(Mode::Replace), "应把 replace 写进分家例外");
    }

    #[test]
    fn adopt_of_an_unmeasured_harness_sets_off() {
        // 没测过 → 建议 off → adopt 之后也必须是 off（不能默默当成"能注入"）。
        let (_dir, engine, commands) = setup();
        commands.run("armor.adopt", &engine, &json!({"harness": "claude"})).expect("adopt");
        let state = engine.state().expect("state");
        assert_eq!(state.overrides.get("claude").and_then(|o| o.mode), Some(Mode::Off));
    }

    #[test]
    fn plan_for_all_harnesses_by_default() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("armor.plan", &engine, &json!({})).expect("plan").to_json();
        assert_eq!(value.as_array().map(|a| a.len()), Some(HarnessId::BUILTIN.len()));
    }
}
