// 测试代码允许直接起进程：要测的就是 CLI 可执行文件本身。
#![allow(clippy::disallowed_methods)]

//! CLI 端到端测试：每个子命令至少一条，断言 stdout、退出码与磁盘结果。
//!
//! 全部经由 `METIS_HOME` 指向临时目录，**不碰真实 `$HOME`**（`docs/TESTING.md` §3）。

use std::path::Path;
use std::process::{Command, Output};

/// 跑一次 CLI。`home` 指向隔离的数据目录。
fn metis(home: &Path, args: &[&str]) -> Output {
    let exe = env!("CARGO_BIN_EXE_metis");
    Command::new(exe)
        .args(args)
        .env("METIS_HOME", home)
        // 清掉可能影响结果的宿主环境变量。
        .env_remove("CODEX_HOME")
        .env_remove("PI_HOME")
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .expect("CLI 必须能启动")
}

fn stdout_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// 把一次 `--json` 调用的输出解析成 JSON。
fn json_of(output: &Output) -> serde_json::Value {
    serde_json::from_str(&stdout_of(output)).expect("必须是合法 JSON")
}

fn stderr_of(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn temp_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

#[test]
fn version_prints_and_exits_zero() {
    let home = temp_home();
    let output = metis(home.path(), &["--version"]);
    assert!(output.status.success());
    assert!(stdout_of(&output).contains("metis 0.1.0"), "{}", stdout_of(&output));
}

#[test]
fn help_prints_guidance_instead_of_opening_the_gui() {
    // 无参数现在打开 GUI（见 `launch_tests`）；帮助改用 `--help`。
    // **不要**在这里跑无参数：那会真的开一个窗口（无显示设备的环境会挂住）。
    let home = temp_home();
    let output = metis(home.path(), &["--help"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(text.contains("Usage"), "缺用法说明：{text}");
    assert!(text.contains("status"), "引导里应给出可用的命令");
}

#[test]
fn status_reports_a_fresh_state() {
    let home = temp_home();
    let output = metis(home.path(), &["status", "--json"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    assert!(text.contains("\"mode\": \"replace\""), "{text}");
    assert!(text.contains("\"activationEpoch\": \"initial\""), "缺省 epoch 必须是 initial：{text}");
}

#[test]
fn harnesses_lists_all_fifteen_with_reasons() {
    let home = temp_home();
    let output = metis(home.path(), &["harnesses"]);
    assert!(output.status.success());
    let text = stdout_of(&output);
    for id in ["codex", "claude", "antigravity", "dsh"] {
        assert!(text.contains(id), "输出里缺少 {id}：{text}");
    }
    assert!(text.contains("不行"), "antigravity 应显示为不可注入");
}

#[test]
fn persona_save_then_list_then_use() {
    let home = temp_home();
    let save = metis(home.path(), &["persona", "concise", "--body", "用简体中文。", "--name", "简洁协作"]);
    assert!(save.status.success(), "{}", stderr_of(&save));
    assert!(home.path().join("personas").join("concise.md").is_file(), "人设文件必须落盘");

    let use_output = metis(home.path(), &["use", "concise"]);
    assert!(use_output.status.success());
    assert!(stdout_of(&use_output).contains("concise"));

    let list = metis(home.path(), &["ls"]);
    assert!(list.status.success());
    let text = stdout_of(&list);
    assert!(text.contains("concise"));
    assert!(text.contains("简洁协作"));
}

#[test]
fn show_prints_the_full_persona() {
    let home = temp_home();
    let save = metis(home.path(), &["persona", "concise", "--body", "# 正文\n\n先结论。", "--name", "简洁协作", "--description", "先结论"]);
    assert!(save.status.success(), "{}", stderr_of(&save));
    assert!(metis(home.path(), &["use", "concise"]).status.success());

    // 人类可读：名称/id/字数 + 正文原样。
    let shown = metis(home.path(), &["show", "concise"]);
    assert!(shown.status.success(), "{}", stderr_of(&shown));
    let text = stdout_of(&shown);
    assert!(text.contains("简洁协作（concise）"), "{text}");
    assert!(text.contains("当前"), "应标出当前人设：{text}");
    assert!(text.contains("先结论"), "{text}");
    assert!(text.contains("# 正文\n\n先结论。"), "正文必须原样输出：{text}");

    // 机器可读：字段齐全，路径指向真实文件。
    let json = json_of(&metis(home.path(), &["show", "concise", "--json"]));
    assert_eq!(json["id"], "concise");
    assert_eq!(json["name"], "简洁协作");
    assert_eq!(json["description"], "先结论");
    assert_eq!(json["body"], "# 正文\n\n先结论。");
    assert_eq!(json["chars"], "# 正文\n\n先结论。".chars().count());
    assert_eq!(json["active"], true);
    assert!(Path::new(json["path"].as_str().unwrap_or_default()).is_file(), "{}", json["path"]);
}

#[test]
fn show_of_a_missing_persona_fails_with_code_2() {
    let home = temp_home();
    let output = metis(home.path(), &["show", "ghost"]);
    assert_eq!(output.status.code(), Some(2), "名字写错属于调用方传参问题");
    let err = stderr_of(&output);
    assert!(err.contains("人设不存在"), "{err}");
    assert!(err.contains("→"), "必须给出补救建议：{err}");
}

#[test]
fn using_a_missing_persona_fails_with_code_2() {
    let home = temp_home();
    let output = metis(home.path(), &["use", "ghost"]);
    // 名字不存在 = 调用方传错了 → 2（运行时失败才是 1）。
    assert_eq!(output.status.code(), Some(2), "语义错误用退出码 2");
    let err = stderr_of(&output);
    assert!(err.contains("没有人设"), "{err}");
    assert!(err.contains("→"), "必须给出补救建议：{err}");
}

#[test]
fn unknown_channel_fails_with_code_2() {
    let home = temp_home();
    let output = metis(home.path(), &["arm", "codex", "--channel", "nope"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr_of(&output).contains("未知通道"));
}

#[test]
fn mode_and_power_round_trip_through_the_cli() {
    let home = temp_home();
    // 先放一份人设，恢复时才有人设可注入。
    assert!(metis(home.path(), &["persona", "p", "--body", "正文"]).status.success());
    assert!(metis(home.path(), &["use", "p"]).status.success());

    assert!(metis(home.path(), &["mode", "append"]).status.success());
    assert!(metis(home.path(), &["pause"]).status.success());

    let paused = stdout_of(&metis(home.path(), &["status", "--json"]));
    assert!(paused.contains("\"mode\": \"off\""), "{paused}");
    assert!(paused.contains("\"prevMode\": \"append\""), "{paused}");

    assert!(metis(home.path(), &["resume"]).status.success());
    let resumed = stdout_of(&metis(home.path(), &["status", "--json"]));
    assert!(resumed.contains("\"mode\": \"append\""), "{resumed}");
}

#[test]
fn pausing_works_without_any_persona() {
    let home = temp_home();
    let output = metis(home.path(), &["pause"]);
    assert!(output.status.success(), "暂停不该要求先有人设：{}", stderr_of(&output));
}

#[test]
fn arm_then_disarm_updates_state() {
    let home = temp_home();
    assert!(metis(home.path(), &["arm", "codex", "--channel", "install"]).status.success());
    let armed = stdout_of(&metis(home.path(), &["status", "--json"]));
    assert!(armed.contains("\"install\""), "{armed}");

    assert!(metis(home.path(), &["disarm", "codex"]).status.success());
    let disarmed = stdout_of(&metis(home.path(), &["status", "--json"]));
    assert!(!disarmed.contains("\"armed\": {\n    \"codex\""), "解除武装后不该还在 armed 里：{disarmed}");
}

#[test]
fn apply_without_persona_fails_clearly() {
    let home = temp_home();
    let output = metis(home.path(), &["apply"]);
    assert_eq!(output.status.code(), Some(1));
    let err = stderr_of(&output);
    assert!(err.contains("没有可用人设"), "{err}");
}

#[test]
fn apply_with_a_persona_succeeds() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "p", "--body", "正文"]).status.success());
    assert!(metis(home.path(), &["use", "p"]).status.success());
    let output = metis(home.path(), &["apply"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("应用完成"), "{text}");
    assert!(text.contains("配置写入不等于当前会话已生效"), "必须保留这条产品语义：{text}");
}

#[test]
fn doctor_lists_every_harness_with_a_home() {
    let home = temp_home();
    let output = metis(home.path(), &["doctor", "--no-version"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    for id in ["codex", "claude", "antigravity"] {
        assert!(text.contains(id), "输出里缺少 {id}：{text}");
    }
}

#[test]
fn doctor_json_reports_all_fifteen() {
    let home = temp_home();
    let output = metis(home.path(), &["doctor", "--no-version", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout_of(&output)).expect("必须是合法 JSON");
    assert_eq!(parsed["total"], 15);
    let items = parsed["harnesses"].as_array().expect("array");
    assert_eq!(items.len(), 15);
    for item in items {
        assert!(item["home"].is_string(), "必须报告主目录");
        assert!(item["installed"].is_boolean());
    }
}

#[test]
fn launch_dry_run_shows_the_command_without_executing() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "p", "--body", "正文"]).status.success());
    assert!(metis(home.path(), &["use", "p"]).status.success());

    let output = metis(home.path(), &["launch", "codex", "--json"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let parsed: serde_json::Value = serde_json::from_str(&stdout_of(&output)).expect("json");
    assert_eq!(parsed["executed"], false, "默认不执行");
    assert!(parsed["command"].as_str().unwrap_or_default().contains("-c"));
}

#[test]
fn launch_never_leaks_the_persona_body() {
    let home = temp_home();
    let secret = "机密人设".repeat(80);
    assert!(metis(home.path(), &["persona", "p", "--body", &secret]).status.success());
    assert!(metis(home.path(), &["use", "p"]).status.success());

    // omp 是内联注入，最容易泄漏。
    let output = metis(home.path(), &["launch", "omp", "--json"]);
    let text = stdout_of(&output);
    assert!(!text.contains(&secret), "命令行预览不能带完整人设");
    assert!(text.contains("字>"), "应替换成字数占位：{text}");
}

#[test]
fn launch_of_a_desktop_only_harness_gives_a_next_step() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "p", "--body", "正文"]).status.success());
    assert!(metis(home.path(), &["use", "p"]).status.success());

    let output = metis(home.path(), &["launch", "pi-desktop", "--json"]);
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_str(&stdout_of(&output)).expect("json");
    assert_eq!(parsed["desktopOnly"], true);
    assert!(parsed["manual"].as_array().is_some_and(|m| !m.is_empty()), "必须给出下一步");
}

#[test]
fn ask_refuses_a_harness_without_a_oneshot_mode() {
    let home = temp_home();
    let output = metis(home.path(), &["ask", "pi-desktop", "问题"]);
    assert_eq!(output.status.code(), Some(2), "调用方传错了用 2");
    assert!(stderr_of(&output).contains("一次性"), "{}", stderr_of(&output));
}

#[test]
fn json_output_is_machine_readable() {
    let home = temp_home();
    let output = metis(home.path(), &["status", "--json"]);
    let parsed: serde_json::Value = serde_json::from_str(&stdout_of(&output)).expect("--json 必须是合法 JSON");
    assert_eq!(parsed["default"]["mode"], "replace");
}

#[test]
fn rm_deletes_a_persona() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "temp", "--body", "x"]).status.success());
    assert!(home.path().join("personas").join("temp.md").is_file());
    let output = metis(home.path(), &["rm", "temp"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    assert!(!home.path().join("personas").join("temp.md").exists());
}

#[test]
fn corrupt_state_is_reported_and_left_untouched() {
    let home = temp_home();
    let state = home.path().join("state.json");
    std::fs::write(&state, b"{ broken").expect("seed");
    let before = std::fs::read(&state).expect("read");

    let output = metis(home.path(), &["status"]);
    assert!(!output.status.success());
    assert!(stderr_of(&output).contains("损坏"), "{}", stderr_of(&output));
    assert_eq!(std::fs::read(&state).expect("reread"), before, "损坏的状态文件必须原样保留");
}

#[test]
fn every_command_uses_the_injected_home() {
    let home = temp_home();
    assert!(metis(home.path(), &["status"]).status.success());
    // 读命令也要把数据目录建出来，否则用户找不到自己的配置去哪了。
    assert!(home.path().is_dir(), "数据目录必须被创建在注入的位置");

    // 会写盘的命令，产物必须落在同一个目录里。
    assert!(metis(home.path(), &["persona", "p", "--body", "x"]).status.success());
    assert!(home.path().join("personas").join("p.md").is_file());
}

// ─────────────────────────────────────────────────────────────
// `use` / `sync` / `install` / `slash` 的端到端测试
// ─────────────────────────────────────────────────────────────

#[test]
fn use_without_an_id_prints_the_current_persona() {
    // 与基线一致：不带 id 是"查"，不是"报错"。
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x", "--name", "甲"]).status.success());
    assert!(metis(home.path(), &["use", "a"]).status.success());

    let output = metis(home.path(), &["use"]);
    assert!(output.status.success(), "不带 id 不该失败");
    let text = stdout_of(&output);
    assert!(text.contains("当前人设"), "应打印当前人设：{text}");
    assert!(text.contains('a'), "应显示人设 id：{text}");
    assert!(text.contains("分家例外"), "应说明分家例外：{text}");
}

#[test]
fn use_without_an_id_lists_overrides() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x"]).status.success());
    assert!(metis(home.path(), &["use", "a", "--for", "codex"]).status.success());

    let text = stdout_of(&metis(home.path(), &["use"]));
    assert!(text.contains("codex"), "例外里应出现那一家：{text}");
}

#[test]
fn use_with_an_id_still_switches() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x"]).status.success());
    assert!(metis(home.path(), &["persona", "b", "--body", "y"]).status.success());
    assert!(metis(home.path(), &["use", "a"]).status.success());
    assert!(metis(home.path(), &["use", "b"]).status.success());

    let json = json_of(&metis(home.path(), &["status", "--json"]));
    assert_eq!(json["persona"], "b");
}

#[test]
fn sync_applies_without_changing_the_persona() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x"]).status.success());
    assert!(metis(home.path(), &["use", "a"]).status.success());
    assert!(metis(home.path(), &["arm", "codex", "--channel", "install"]).status.success());

    let output = metis(home.path(), &["sync"]);
    assert!(output.status.success(), "sync 应成功：{}", stdout_of(&output));
    // 人设没变。
    let json = json_of(&metis(home.path(), &["status", "--json"]));
    assert_eq!(json["persona"], "a");
}

#[test]
fn sync_failed_flag_is_accepted() {
    // 首次跑时没有任何失败项：应成功且什么都不做（不是报错）。
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x"]).status.success());
    assert!(metis(home.path(), &["use", "a"]).status.success());
    let output = metis(home.path(), &["sync", "--failed"]);
    assert!(output.status.success());
}

#[test]
fn install_enables_only_the_install_channel() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x"]).status.success());
    assert!(metis(home.path(), &["use", "a"]).status.success());

    let output = metis(home.path(), &["install", "codex"]);
    assert!(output.status.success(), "{}", stdout_of(&output));
    assert!(stdout_of(&output).contains("codex"));

    let json = json_of(&metis(home.path(), &["status", "--json"]));
    let armed = json["armed"]["codex"].as_array().expect("codex 应被武装");
    assert_eq!(armed.len(), 1, "只应开 install 一条：{armed:?}");
    assert_eq!(armed[0], "install");
}

#[test]
fn install_rejects_an_unregistered_harness_with_code_2() {
    // 关键：`HarnessId::parse` 接受任意合法字符串，所以拼错的 `nope` 曾经
    // 一路"成功"（什么都没写）。必须校验"已注册"。
    let home = temp_home();
    let output = metis(home.path(), &["install", "nope"]);
    assert_eq!(output.status.code(), Some(2), "拼错的名字应退出 2");
    let err = stderr_of(&output);
    assert!(err.contains("未注册"), "{err}");
}

#[test]
fn slash_status_lists_harnesses() {
    let home = temp_home();
    let output = metis(home.path(), &["slash", "status"]);
    assert!(output.status.success(), "{}", stdout_of(&output));
    let text = stdout_of(&output);
    assert!(text.contains("pi"), "应列出执行器：{text}");
    assert!(text.contains("可用") || text.contains("不可用"), "应给出是否可用：{text}");
}

#[test]
fn slash_on_off_round_trips() {
    let home = temp_home();
    assert!(metis(home.path(), &["persona", "a", "--body", "x"]).status.success());
    assert!(metis(home.path(), &["use", "a"]).status.success());

    assert!(metis(home.path(), &["slash", "on", "pi"]).status.success());
    let json = json_of(&metis(home.path(), &["status", "--json"]));
    assert!(json["armed"]["pi"].as_array().is_some_and(|a| a.iter().any(|v| v == "slash")), "pi 应武装 slash");

    assert!(metis(home.path(), &["slash", "off", "pi"]).status.success());
    let json = json_of(&metis(home.path(), &["status", "--json"]));
    assert!(json["armed"]["pi"].as_array().is_none_or(|a| !a.iter().any(|v| v == "slash")), "pi 的 slash 应被撤下");
}

#[test]
fn slash_rejects_an_unknown_action_with_code_2() {
    let home = temp_home();
    let output = metis(home.path(), &["slash", "bogus"]);
    assert_eq!(output.status.code(), Some(2));
    let err = stderr_of(&output);
    assert!(err.contains("status") && err.contains("on") && err.contains("off"), "补救建议应列出可用动作：{err}");
}

#[test]
fn hub_help_lists_every_subcommand() {
    let home = temp_home();
    let output = metis(home.path(), &["hub", "--help"]);
    assert!(output.status.success(), "{}", stderr_of(&output));
    let text = stdout_of(&output);
    for sub in ["list", "show", "import", "skill"] {
        assert!(text.contains(sub), "hub 帮助里缺少 {sub}：{text}");
    }
}

#[test]
fn hub_subcommands_reject_bad_input_before_network() {
    let home = temp_home();

    // 坏 URL：本地就拒绝，不发请求。
    let list = metis(home.path(), &["hub", "list", "--url", "http://example.com"]);
    assert_eq!(list.status.code(), Some(2), "{}", stderr_of(&list));

    // 坏 provider / limit：参数或引擎校验拒绝。
    let provider = metis(home.path(), &["hub", "list", "--provider", "nope", "--url", "https://n.kkpt.tech:8443"]);
    assert_eq!(provider.status.code(), Some(2), "{}", stderr_of(&provider));
    let limit = metis(home.path(), &["hub", "list", "--limit", "abc"]);
    assert_eq!(limit.status.code(), Some(2), "{}", stderr_of(&limit));

    // 坏 slug：`show_expert` 在发请求之前校验。
    let show = metis(home.path(), &["hub", "show", "../x", "--url", "https://n.kkpt.tech:8443"]);
    assert_eq!(show.status.code(), Some(2), "{}", stderr_of(&show));

    // 目前只支持 expert。
    let import = metis(home.path(), &["hub", "import", "skill/foo", "--url", "https://n.kkpt.tech:8443"]);
    assert_eq!(import.status.code(), Some(2), "{}", stderr_of(&import));
    assert!(stderr_of(&import).contains("expert"), "{}", stderr_of(&import));

    // 空的 skills 列表在本地拒绝。
    let skills = metis(home.path(), &["hub", "import", "expert/foo", "--skills", ",,", "--url", "https://n.kkpt.tech:8443"]);
    assert_eq!(skills.status.code(), Some(2), "{}", stderr_of(&skills));

    // 移除时坏 slug 不碰磁盘。
    let remove = metis(home.path(), &["hub", "skill", "remove", "../x"]);
    assert_eq!(remove.status.code(), Some(2), "{}", stderr_of(&remove));

    // 安装时坏 URL 不碰磁盘。
    let install = metis(home.path(), &["hub", "skill", "install", "wb-apiz", "--url", "http://example.com"]);
    assert_eq!(install.status.code(), Some(2), "{}", stderr_of(&install));
}
