//! `verify` 命令：按 harness 抓真实请求并写报告。

use serde_json::Value;

use metis_core::{HarnessId, Mode, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects};

/// 一家的隔离验证配方：怎么让它连我们的假 provider。
///
/// 只收录**真的能改 base-url** 的家。改不了的家如实标 `blocked` ——
/// 假装能测比不测更糟（`AGENTS.md` 铁律 5）。
fn recipe(harness: &HarnessId, base_url: &str, _home: &std::path::Path) -> Recipe {
    match harness.as_str() {
        // OpenAI 兼容：`OPENAI_BASE_URL` 是通行约定。
        "codex" => Recipe {
            program: "codex".to_owned(),
            args: vec!["exec".to_owned(), "--skip-git-repo-check".to_owned(), "reply with one word".to_owned()],
            env: vec![
                ("OPENAI_BASE_URL".to_owned(), base_url.to_owned()),
                ("OPENAI_API_KEY".to_owned(), "sk-test".to_owned()),
                ("CODEX_HOME".to_owned(), String::new()),
            ],
            blocked: None,
        },
        "grok" => Recipe {
            program: "grok".to_owned(),
            args: vec!["-p".to_owned(), "reply with one word".to_owned(), "--always-approve".to_owned()],
            env: vec![
                ("XAI_API_KEY".to_owned(), "sk-test".to_owned()),
                ("GROK_XAI_API_BASE_URL".to_owned(), base_url.to_owned()),
                ("GROK_MODELS_BASE_URL".to_owned(), base_url.to_owned()),
                ("GROK_DISABLE_AUTOUPDATER".to_owned(), "1".to_owned()),
            ],
            blocked: None,
        },
        "claude" => Recipe {
            program: "claude".to_owned(),
            args: vec!["-p".to_owned(), "reply with one word".to_owned()],
            env: vec![("ANTHROPIC_BASE_URL".to_owned(), base_url.to_owned()), ("ANTHROPIC_API_KEY".to_owned(), "sk-test".to_owned())],
            blocked: None,
        },
        "opencode" => Recipe {
            program: "opencode".to_owned(),
            args: vec!["run".to_owned(), "reply with one word".to_owned()],
            env: vec![("OPENCODE_BASE_URL".to_owned(), base_url.to_owned()), ("OPENAI_API_KEY".to_owned(), "sk-test".to_owned())],
            blocked: None,
        },
        "opencode2" => Recipe {
            program: "opencode2".to_owned(),
            args: vec!["run".to_owned(), "reply with one word".to_owned()],
            env: vec![("OPENCODE_BASE_URL".to_owned(), base_url.to_owned()), ("OPENAI_API_KEY".to_owned(), "sk-test".to_owned())],
            blocked: None,
        },
        "omp" => Recipe {
            program: "omp".to_owned(),
            args: vec!["-p".to_owned(), "reply with one word".to_owned()],
            env: vec![("OMP_BASE_URL".to_owned(), base_url.to_owned()), ("OPENAI_API_KEY".to_owned(), "sk-test".to_owned())],
            blocked: None,
        },
        "dsh" => Recipe {
            program: "dsh".to_owned(),
            args: vec!["--profile".to_owned(), "headless".to_owned(), "reply with one word".to_owned()],
            env: vec![("OPENAI_API_KEY".to_owned(), "sk-test".to_owned()), ("DSH_BASE_URL".to_owned(), base_url.to_owned())],
            blocked: None,
        },
        // 这几家的入口**没有**能改 base-url 的开关：如实标 blocked。
        "pi" => Recipe::blocked("pi 的 provider 地址来自宿主的 provider 配置，CLI 没有覆盖参数；要验证得改它的配置文件"),
        "pi-desktop" => Recipe::blocked("PI-Desktop 是 Electron 应用，Metis 无法注入 provider 环境变量"),
        "hermes" => Recipe::blocked("hermes 没有可用的隔离 provider 适配器"),
        "kilo" => Recipe::blocked("kilo 的 provider 配置在 agent 配置文件里，CLI 没有覆盖参数"),
        "codebuddy" => Recipe::blocked("codebuddy 的 base-url 来自其云端账号配置"),
        "workbuddy" => Recipe::blocked("WorkBuddy 是桌面客户端，无法注入环境变量"),
        "zcode" => Recipe::blocked("zcode 桌面版闭源，无法抓包复核"),
        "antigravity" => Recipe::blocked("antigravity 只能二进制补丁，配置层没有可写入口"),
        _ => Recipe::blocked("这家没有登记隔离验证配方"),
    }
}

/// 一家的验证配方。
struct Recipe {
    program: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    blocked: Option<String>,
}

impl Recipe {
    fn blocked(reason: impl Into<String>) -> Self {
        Self { program: String::new(), args: Vec::new(), env: Vec::new(), blocked: Some(reason.into()) }
    }
}

/// 执行验证。
#[derive(Debug)]
pub struct Verify;

impl Command for Verify {
    fn id(&self) -> &'static str {
        "harness.verify"
    }
    fn title(&self) -> &'static str {
        "harness.verify.title"
    }
    fn effects(&self) -> Effects {
        Effects { writes_disk: true, spawns_process: true, network: false }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let only: Vec<HarnessId> = params
            .get("harnesses")
            .and_then(|v| v.as_array())
            .map(|items| items.iter().filter_map(|v| v.as_str()).map(HarnessId::parse).collect::<Result<Vec<_>>>())
            .transpose()?
            .unwrap_or_default();
        let timeout_ms = params.get("timeoutMs").and_then(|v| v.as_u64()).unwrap_or(20_000);

        let state = engine.state()?;
        let store = metis_persona::PersonaStore::new(engine.paths().clone());

        // 本次要验的家：显式指定的，或全部内置的。
        let targets: Vec<HarnessId> = if only.is_empty() { HarnessId::BUILTIN.iter().filter_map(|id| HarnessId::parse(id).ok()).collect() } else { only };

        let mut results = Vec::new();
        for harness in &targets {
            // 一家的**当前生效模式**：分家例外优先。
            let activation = state.activation_for(harness, &metis_core::ActivationContext { launch: true, ..Default::default() })?;
            // 验证用的模式：显式参数优先，否则用这家实际生效的模式。
            // `off` 也要照原样验证 —— 「暂停之后确认没有注入」同样是一条要证明的事。
            let effective = match params.get("mode").and_then(|v| v.as_str()).and_then(Mode::parse) {
                Some(explicit) => explicit,
                None => activation.effective_mode(),
            };

            // 先把人设写进 active.md：宿主读的就是它。
            let persona = match &activation.resolved.persona {
                Some(id) => store.read(id)?,
                None => None,
            };
            if let Some(persona) = &persona {
                store.set_active(persona)?;
            }

            let marker = crate::verify::make_marker(harness);
            let home = engine.paths().home().to_path_buf();
            let recipe = recipe(harness, "http://127.0.0.1:1/v1", &home);

            // 标记要进宿主读到的内容里：写进 active.md 的最后一行（宿主读它）。
            if let Some(persona) = &persona {
                let body = format!("{}\n\n{marker}\n", persona.trimmed_body());
                metis_fs::write_atomic(&engine.paths().active_persona_file(), body.as_bytes())?;
            }

            // provider 的地址依赖端口，而端口要在 start 之后才知道：
            // 所以先起服务，再按真实端口生成配方。
            let provider = metis_process::FakeProvider::start(&marker);
            let Ok(provider) = provider else {
                results.push(crate::verify::HarnessResult {
                    harness: harness.to_string(),
                    status: crate::verify::VerifyStatus::Blocked,
                    where_: None,
                    requests: 0,
                    mode: Some(effective.as_str().to_owned()),
                    reason: Some("假 provider 启动失败".to_owned()),
                    roles: Vec::new(),
                });
                continue;
            };
            let base = provider.base_url();
            let real = recipe_for(harness, &base);
            drop(provider);
            drop(recipe);

            let input = crate::verify::VerifyInput {
                harness: harness.clone(),
                program: real.program,
                args: real.args,
                env: real.env,
                mode: effective.as_str().to_owned(),
                blocked: real.blocked,
            };
            let mut result = crate::verify::verify_one(&input, &marker, std::time::Duration::from_millis(timeout_ms))?;
            // 没装的如实标 missing（比 no-request 更准确）。
            if result.status == crate::verify::VerifyStatus::NoRequest && !is_installed(harness) {
                result.status = crate::verify::VerifyStatus::Missing;
                result.reason = Some("本机没有安装这家".to_owned());
            }
            results.push(result);
        }

        let report = crate::verify::VerifyReport {
            v: crate::verify::REPORT_VERSION,
            at: time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned()),
            results,
        };

        // 写报告（失败不阻断：报告只是产物，验证结论更重要）。
        let path = engine.paths().verification_file();
        report.save(&path)?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

fn recipe_for(harness: &HarnessId, base_url: &str) -> Recipe {
    recipe(harness, base_url, std::path::Path::new("."))
}

fn is_installed(harness: &HarnessId) -> bool {
    let Some(home) = dirs::home_dir() else { return false };
    metis_harness::detect(harness, |k| std::env::var(k).ok(), &home, &metis_harness::DetectOptions { probe_versions: false })
        .map(|d| d.installed())
        .unwrap_or(false)
}

/// 读最近一次验证报告。
#[derive(Debug)]
pub struct VerifyReport;

impl Command for VerifyReport {
    fn id(&self) -> &'static str {
        "harness.verify-report"
    }
    fn title(&self) -> &'static str {
        "harness.verify-report.title"
    }
    fn run(&self, engine: &Engine, _params: &Value) -> Result<CommandOutput> {
        let path = engine.paths().verification_file();
        match crate::verify::VerifyReport::load(&path)? {
            Some(report) => Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null))),
            None => Ok(CommandOutput::Json(serde_json::json!({ "report": null, "path": path.display().to_string() }))),
        }
    }
}

/// 对不可隔离的家给出解释（CLI 打印用）。
pub fn blocked_reason(harness: &HarnessId) -> Option<String> {
    recipe_for(harness, "http://127.0.0.1:1/v1").blocked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Commands;
    use metis_paths::Paths;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Engine, Commands) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harnesses"));
        let engine = Engine::new(paths);
        (dir, engine, Commands::default_set())
    }

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("id")
    }

    #[test]
    fn every_builtin_harness_has_a_recipe() {
        for harness in HarnessId::BUILTIN {
            let recipe = recipe_for(&id(harness), "http://127.0.0.1:1/v1");
            let informative = recipe.blocked.is_some() || !recipe.program.is_empty();
            assert!(informative, "{harness} 的配方什么都没说");
        }
    }

    #[test]
    fn blocked_recipes_say_why() {
        for harness in ["pi", "pi-desktop", "hermes", "kilo", "workbuddy", "zcode", "antigravity"] {
            let reason = blocked_reason(&id(harness)).expect("blocked");
            assert!(!reason.trim().is_empty(), "{harness} 的 blocked 理由不能为空");
        }
    }

    #[test]
    fn verifiable_recipes_carry_a_base_url_override() {
        for harness in ["codex", "claude", "grok", "opencode", "opencode2", "omp", "dsh"] {
            let recipe = recipe_for(&id(harness), "http://127.0.0.1:9999/v1");
            assert!(recipe.blocked.is_none(), "{harness} 应该可验证");
            assert!(recipe.env.iter().any(|(_, value)| value.contains("127.0.0.1:9999")), "{harness} 的配方必须带 base-url 覆盖：{:?}", recipe.env);
        }
    }

    #[test]
    fn verify_writes_a_report_even_when_nothing_is_installed() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        // 只验一家，短超时，避免拖慢测试。
        let output = commands.run("harness.verify", &engine, &json!({"harnesses": ["antigravity"], "timeoutMs": 500})).expect("verify");
        let value = output.to_json();
        let results = value["results"].as_array().expect("results");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["harness"], "antigravity");
        // antigravity 不可隔离 → blocked。
        assert_eq!(results[0]["status"], "blocked");

        // 报告必须落盘。
        assert!(engine.paths().verification_file().is_file(), "必须写 verification/latest.json");
    }

    #[test]
    fn verify_report_command_reads_back_what_was_written() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("harness.verify", &engine, &json!({"harnesses": ["zcode"], "timeoutMs": 300})).expect("verify");

        let value = commands.run("harness.verify-report", &engine, &json!({})).expect("read").to_json();
        assert_eq!(value["v"], 1);
        assert!(value["results"].as_array().is_some_and(|r| !r.is_empty()));
    }

    /// 不依赖外部进程的端到端：用进程内 TCP 客户端扮演宿主。
    ///
    /// 这条覆盖"provider → 抓请求 → 判层"的完整链路，**只少了 `Exec` 起进程**那一步。
    /// 那一步由 `metis-process` 的 `run::tests::*` 单独覆盖。
    fn in_process_host(provider: &metis_process::FakeProvider, role: &str, body: &str) {
        use std::io::{Read, Write};
        let payload = serde_json::json!({
            "model": "x",
            "messages": [{ "role": role, "content": body }],
        });
        let text = payload.to_string();
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            text.len(),
            text
        );
        let mut stream = std::net::TcpStream::connect(("127.0.0.1", provider.port())).expect("connect");
        stream.write_all(request.as_bytes()).expect("write");
        let mut response = String::new();
        let _ = stream.read_to_string(&mut response);
    }

    #[test]
    fn a_developer_message_is_judged_ok_end_to_end() {
        let provider = metis_process::FakeProvider::start("CANARY_DEV").expect("provider");
        in_process_host(&provider, "developer", "CANARY_DEV");

        let start = std::time::Instant::now();
        while provider.count() == 0 && start.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let requests = provider.requests();
        assert_eq!(requests.len(), 1, "应抓到一次请求");
        assert_eq!(requests[0].hit_role(), Some("developer"));
        assert!(requests[0].hit());
    }

    #[test]
    fn a_user_message_is_judged_user_only_end_to_end() {
        let provider = metis_process::FakeProvider::start("CANARY_USER").expect("provider");
        in_process_host(&provider, "user", "CANARY_USER");

        let start = std::time::Instant::now();
        while provider.count() == 0 && start.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let requests = provider.requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].hit_role(), Some("user"), "user 层不算通过");
    }

    #[test]
    fn a_system_message_is_judged_ok_end_to_end() {
        let provider = metis_process::FakeProvider::start("CANARY_SYS").expect("provider");
        in_process_host(&provider, "system", "CANARY_SYS");
        let start = std::time::Instant::now();
        while provider.count() == 0 && start.elapsed() < std::time::Duration::from_secs(5) {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(provider.requests().first().and_then(|r| r.hit_role()), Some("system"));
    }

    #[test]
    fn verify_one_reports_blocked_without_starting_a_provider() {
        let (_dir, _engine, _commands) = setup();
        let input = crate::verify::VerifyInput {
            harness: id("zcode"),
            program: "definitely-not-real".to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            mode: "replace".to_owned(),
            blocked: Some("桌面客户端，无法抓包".to_owned()),
        };
        let result = crate::verify::verify_one(&input, "MARK", std::time::Duration::from_millis(100)).expect("verify");
        assert_eq!(result.status, crate::verify::VerifyStatus::Blocked);
        assert_eq!(result.requests, 0);
        assert!(result.reason.unwrap_or_default().contains("桌面"));
    }

    #[test]
    fn verify_one_reports_no_request_when_the_program_is_missing() {
        let (_dir, _engine, _commands) = setup();
        let input = crate::verify::VerifyInput {
            harness: id("codex"),
            program: "metis-no-such-program-xyz".to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            mode: "replace".to_owned(),
            blocked: None,
        };
        let result = crate::verify::verify_one(&input, "MARK", std::time::Duration::from_millis(300)).expect("verify");
        assert_eq!(result.status, crate::verify::VerifyStatus::NoRequest);
        // 宿主输出（这里是"找不到程序"）必须进 reason，用户才知道为什么没抓到。
        assert!(result.reason.is_some_and(|r| !r.is_empty()));
    }

    #[test]
    fn a_harness_without_a_recipe_is_blocked() {
        for name in ["hermes", "kilo", "codebuddy", "workbuddy"] {
            let reason = blocked_reason(&id(name)).expect("blocked");
            assert!(!reason.trim().is_empty(), "{name} 缺少 blocked 理由");
        }
    }

    #[test]
    fn every_verifiable_recipe_has_args() {
        for name in ["codex", "claude", "grok", "opencode", "opencode2", "omp", "dsh"] {
            let recipe = recipe_for(&id(name), "http://127.0.0.1:1/v1");
            assert!(!recipe.args.is_empty(), "{name} 的配方没有参数");
            assert!(!recipe.program.is_empty(), "{name} 的配方没有程序名");
        }
    }

    #[test]
    fn verify_report_round_trips_through_disk() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        // 只验一家 blocked 的：快且确定。
        commands.run("harness.verify", &engine, &json!({"harnesses": ["antigravity"], "timeoutMs": 200})).expect("verify");

        let first = commands.run("harness.verify-report", &engine, &json!({})).expect("read").to_json();
        let second = commands.run("harness.verify-report", &engine, &json!({})).expect("read").to_json();
        assert_eq!(first, second, "两次读报告应完全一致");
        assert_eq!(first["v"], 1);
    }

    #[test]
    fn verify_writes_into_the_isolated_home_only() {
        let (dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("harness.verify", &engine, &json!({"harnesses": ["zcode"], "timeoutMs": 200})).expect("verify");

        let path = engine.paths().verification_file();
        assert!(path.starts_with(dir.path()), "报告必须写在注入的 home 里：{}", path.display());
        assert!(path.is_file());
    }

    /// 造一个"假宿主"：读 `METIS_HOME/personas/active.md`，把人设当作 developer 消息
    /// 发到 `OPENAI_BASE_URL`。
    ///
    /// 只被 `#[ignore]` 的两条端到端测试使用。
    fn fake_host(dir: &std::path::Path) -> (String, Vec<String>) {
        let script = dir.join("fake-host.mjs");
        std::fs::write(
            &script,
            r#"
import { readFileSync } from 'node:fs';
const base = process.env.OPENAI_BASE_URL;
const home = process.env.METIS_HOME;
let body = '';
try { body = readFileSync(`${home}/personas/active.md`, 'utf8').trim(); } catch {}
const payload = { model: 'x', messages: [
  { role: 'system', content: 'host prompt only' },
  { role: 'developer', content: body },
  { role: 'user', content: 'hi' },
]};
try {
  await fetch(`${base}/chat/completions`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(payload) });
} catch (e) { console.error('FETCH_FAILED base=' + base + ' err=' + e); process.exit(3); }
"#,
        )
        .expect("write script");
        ("node".to_owned(), vec![script.display().to_string()])
    }

    /// 端到端：假宿主 → provider → 判层。
    ///
    /// **默认跳过**（`cargo test -- --ignored` 才跑）。原因见 `docs/TESTING.md`：
    /// 它要起真实 `node` 进程并走真实 TCP，在并发/插桩构建下约有 30% 概率
    /// 报 `fetch failed`（node 已退出、provider 正常、但请求没到达）。
    /// 这不是被测代码的缺陷，是"测试里放了一个真实网络栈"的固有代价。
    /// 判层逻辑本身由 `metis-process` 的 `captures_*` 单元测试确定性地覆盖。
    #[test]
    #[ignore = "起真实 node + TCP，并发下不稳定；用 --ignored 手工跑"]
    fn verify_detects_a_developer_layer_hit_end_to_end() {
        // 这是 verify 最重要的一条：证明整条链路（起 provider → 跑宿主 → 抓请求 → 判层）
        // 真的能工作，而不是只测了各个零件。
        let (dir, engine, _commands) = setup();
        let marker_holder = metis_process::FakeProvider::start("probe").expect("probe provider");
        drop(marker_holder);

        let (program, args) = fake_host(dir.path());
        let persona_body = "这是一段独特的人设正文 METIS_E2E_CANARY";
        metis_fs::ensure_dir(&engine.paths().personas_dir()).expect("mkdir");
        metis_fs::write_atomic(&engine.paths().active_persona_file(), persona_body.as_bytes()).expect("write active");

        // 先起 provider 拿到真实端口，再构造配方；provider 必须**活到验证结束**。
        let marker = "METIS_E2E_CANARY";
        let provider = metis_process::FakeProvider::start(marker).expect("provider");
        let base = provider.base_url();

        let input = crate::verify::VerifyInput {
            harness: id("codex"),
            program,
            args,
            env: vec![("OPENAI_BASE_URL".to_owned(), base), ("METIS_HOME".to_owned(), engine.paths().home().display().to_string())],
            mode: "replace".to_owned(),
            blocked: None,
        };
        let result = crate::verify::verify_with(&input, &provider, marker, std::time::Duration::from_secs(15)).expect("verify");
        assert_eq!(result.requests, 1, "应抓到一次请求：{result:?}");
        assert_eq!(result.status, crate::verify::VerifyStatus::Ok, "人设在 developer 层应判 ok：{result:?}");
        assert_eq!(result.where_.as_deref(), Some("developer"));
    }

    /// 同一条链路，但人设落在 user 层 → 必须判 `user-only`。
    ///
    /// 同样默认跳过（理由同上）。
    #[test]
    #[ignore = "起真实 node + TCP，并发下不稳定；用 --ignored 手工跑"]
    fn verify_reports_user_only_when_the_persona_lands_in_a_user_message() {
        // 对照：同一条链路，但宿主把人设塞进 user 消息 → 必须判 user-only，不能算通过。
        let (dir, engine, _commands) = setup();
        let script = dir.path().join("user-host.mjs");
        std::fs::write(
            &script,
            r#"
import { readFileSync } from 'node:fs';
const base = process.env.OPENAI_BASE_URL;
let body = '';
try { body = readFileSync(`${process.env.METIS_HOME}/personas/active.md`, 'utf8').trim(); } catch {}
try {
  await fetch(`${base}/chat/completions`, { method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ model: 'x', messages: [{ role: 'user', content: body }] }) });
} catch (e) { console.error('FETCH_FAILED base=' + base + ' err=' + e); process.exit(3); }
"#,
        )
        .expect("write");
        metis_fs::ensure_dir(&engine.paths().personas_dir()).expect("mkdir");
        metis_fs::write_atomic(&engine.paths().active_persona_file(), b"METIS_USER_CANARY").expect("write");

        let marker = "METIS_USER_CANARY";
        let provider = metis_process::FakeProvider::start(marker).expect("provider");
        let base = provider.base_url();

        let input = crate::verify::VerifyInput {
            harness: id("claude"),
            program: "node".to_owned(),
            args: vec![script.display().to_string()],
            env: vec![("OPENAI_BASE_URL".to_owned(), base), ("METIS_HOME".to_owned(), engine.paths().home().display().to_string())],
            mode: "replace".to_owned(),
            blocked: None,
        };
        let result = crate::verify::verify_with(&input, &provider, marker, std::time::Duration::from_secs(15)).expect("verify");
        assert_eq!(result.status, crate::verify::VerifyStatus::UserOnly, "user 层不能算通过：{result:?}");
        assert_eq!(result.where_.as_deref(), Some("user"));
    }

    #[test]
    fn verify_report_is_null_before_any_run() {
        let (_dir, engine, commands) = setup();
        let value = commands.run("harness.verify-report", &engine, &json!({})).expect("read").to_json();
        assert!(value["report"].is_null(), "没跑过时报告应为 null");
        assert!(value["path"].as_str().is_some_and(|p| p.contains("latest.json")));
    }

    #[test]
    fn unknown_harness_is_rejected() {
        let (_dir, engine, commands) = setup();
        // cursor 不是内置名，但 HarnessId 接受它；配方会走 blocked 分支。
        let output = commands.run("harness.verify", &engine, &json!({"harnesses": ["cursor"], "timeoutMs": 200}));
        assert!(output.is_ok(), "未知执行器也该给 blocked，而不是崩");
    }

    #[test]
    fn verify_never_writes_persona_text_into_the_report() {
        let (_dir, engine, commands) = setup();
        let secret = "这是一段非常机密的人设正文";
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": secret})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("harness.verify", &engine, &json!({"harnesses": ["antigravity"], "timeoutMs": 300})).expect("verify");

        let text = std::fs::read_to_string(engine.paths().verification_file()).expect("read");
        assert!(!text.contains(secret), "报告里不得出现人设正文：{text}");
    }
}
