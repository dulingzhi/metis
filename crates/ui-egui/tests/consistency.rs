//! CLI 与 GUI 的一致性测试（I9 不变式）。
//!
//! 铁律 2 说"一切皆命令，CLI 与 GUI 同一条路"。这条不变式如果没有测试，
//! 就只会在某天有人"顺手在界面里直接写业务"时悄悄破掉。
//!
//! 放在 `ui-egui` 而不是 `engine`：这里需要同时看到命令注册表**和** `MetisApp`，
//! 而 `engine` 不能依赖 `ui-egui`（分层）。
//!
//! 验证三件事：
//! 1. **注册表是唯一真相**：id 唯一、格式统一、`Effects` 声明存在；
//! 2. **CLI 与 GUI 的每个入口都映射到已注册的 id**（没有游离的专有逻辑）；
//! 3. **同一 id + 同一参数，两条入口的输出逐字段相同**。

use metis_engine::{CommandOutput, Commands, Engine};
use metis_paths::Paths;
use serde_json::json;

fn setup() -> (tempfile::TempDir, Engine, Commands) {
    let dir = tempfile::tempdir().expect("tmp");
    let paths = Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harnesses"));
    std::fs::create_dir_all(paths.home()).expect("mkdir");
    let engine = Engine::new(paths);
    (dir, engine, Commands::default_set())
}

/// CLI 的每个子命令最终会发往的命令 id。
///
/// 这份清单必须与 `apps/metis/src/main.rs` 的 `translate` 保持一致。
const CLI_COMMAND_IDS: &[&str] = &[
    "state.status",
    "state.power",
    "state.mode",
    "persona.list",
    "persona.save",
    "persona.use",
    "persona.remove",
    "channel.arm",
    "channel.set",
    "channel.clean",
    "channel.uninstall",
    "config.apply",
    "harness.list",
    "harness.doctor",
    "harness.launch",
    "harness.ask",
    "harness.verify",
    "harness.verify-report",
    "metis.self-uninstall",
    "metis.import-agent-purge",
    "metis.import-personas",
    "metis.hub-list",
    "metis.hub-show",
    "metis.hub-import",
    "metis.hub-skill-install",
    "metis.hub-skill-remove",
    "metis.update-check",
    "metis.journal",
    "metis.journal-prune",
];
/// GUI 动作层（`ui-egui/src/actions.rs`）会发的 id。
const GUI_ACTION_IDS: &[&str] = &["config.apply", "state.power", "channel.set", "channel.clean", "harness.verify", "metis.import-agent-purge"];

#[test]
fn every_registered_command_is_unique_and_well_formed() {
    let (_dir, _engine, commands) = setup();
    let ids = commands.ids();
    assert!(ids.len() >= 20, "命令数偏少：{}", ids.len());
    let mut seen = std::collections::BTreeSet::new();
    for id in &ids {
        assert!(!id.is_empty(), "有空 id");
        assert!(seen.insert(*id), "id 重复：{id}");
        assert!(id.contains('.'), "id 应该是 `<域>.<动作>` 形式：{id}");
        assert!(
            id.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '-' || c == '_'),
            "id 只能用小写字母/数字/`.`/`-`/`_`：{id}"
        );
    }
}

#[test]
fn every_cli_subcommand_maps_to_a_registered_command() {
    let (_dir, _engine, commands) = setup();
    let ids = commands.ids();
    for id in CLI_COMMAND_IDS {
        assert!(ids.contains(id), "CLI 指向了未注册的命令 {id}");
    }
}

#[test]
fn every_gui_action_maps_to_a_registered_command() {
    let (_dir, _engine, commands) = setup();
    let ids = commands.ids();
    for id in GUI_ACTION_IDS {
        assert!(ids.contains(id), "GUI 动作指向了未注册的命令 {id}");
    }
}

#[test]
fn cli_and_gui_agree_on_every_shared_feature() {
    // 两边都暴露的能力必须指向**同一个** id。任何一边多出个"私房实现"都会被这条抓到。
    for id in GUI_ACTION_IDS {
        assert!(CLI_COMMAND_IDS.contains(id), "GUI 用了 {id}，但 CLI 没有对应子命令（能力不对称）");
    }
}

#[test]
fn the_same_id_and_params_produce_the_same_output_from_either_entry_point() {
    // 这就是 I9：CLI 走 `commands.run`，GUI 走 `MetisApp::invoke` ——
    // 后者内部也是 `commands.run`，所以结果必须逐字段相同。
    let (dir, engine, commands) = setup();
    commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");

    let direct: CommandOutput = commands.run("persona.list", &engine, &json!({})).expect("direct");

    let mut app = metis_ui_egui::MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
    let via_gui = app.invoke("persona.list", json!({})).expect("gui");

    assert_eq!(direct.to_json(), via_gui.to_json(), "两条入口的输出了不一致");
}

#[test]
fn an_unknown_id_fails_the_same_way_from_both_entry_points() {
    let (dir, _engine, commands) = setup();
    let paths = Paths::with_home(dir.path().join("home"));
    let direct = commands.run("nope.nope", &Engine::new(paths.clone()), &json!({})).expect_err("direct");

    let mut app = metis_ui_egui::MetisApp::with_paths(paths);
    assert!(app.invoke("nope.nope", json!({})).is_none(), "未知命令应返回 None");
    let notice = app.notice.as_ref().expect("提示");
    assert!(notice.text.contains(direct.message()) || notice.text.contains("未知命令"), "GUI 的提示应保留引擎的错误：{}", notice.text);
}

#[test]
fn read_only_commands_do_not_mutate_state() {
    // 声明是只读的命令，跑一遍之后状态文件不该变化。
    let (_dir, engine, commands) = setup();
    commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
    let before = engine.state().expect("state");

    for id in ["state.status", "persona.list", "harness.list", "harness.verify-report", "metis.journal"] {
        if commands.ids().contains(&id) {
            let _ = commands.run(id, &engine, &json!({}));
        }
    }
    let after = engine.state().expect("state");
    assert_eq!(before.persona, after.persona, "只读命令不该改动状态");
    assert_eq!(before.default.mode, after.default.mode);
}

#[test]
fn every_command_title_key_is_translated() {
    // 引擎给每个命令声明了一个 `<id>.title` 键；它们必须都在**两张**表里，
    // 否则切换语言后 CLI 与界面都会打出 `state.status.title` 这种原始键。
    use metis_core::i18n::{Lang, keys};
    let (_dir, _engine, commands) = setup();
    let zh = keys(Lang::ZhHans);
    let en = keys(Lang::En);

    let mut missing = Vec::new();
    for id in commands.ids() {
        let Some(command) = commands.get(id) else { continue };
        let title = command.title();
        // 标题要么是 i18n 键，要么是直接的中文文案（少数内置命令）。
        if !title.contains('.') {
            continue;
        }
        if !zh.contains(&title.to_owned()) || !en.contains(&title.to_owned()) {
            missing.push(format!("{id} → {title}"));
        }
    }
    assert!(missing.is_empty(), "这些命令的标题键缺翻译：{missing:?}");
}

#[test]
fn every_registered_command_has_a_title_key() {
    // 标题是 i18n 的键（`docs/UI.md` §7）。没有标题键的命令在界面上无法正确显示。
    let (_dir, _engine, commands) = setup();
    for id in commands.ids() {
        let Some(command) = commands.get(id) else {
            panic!("{id} 在注册表里但取不到");
        };
        let title = command.title();
        assert!(!title.is_empty(), "{id} 缺标题");
        assert!(title.contains('.') || !title.is_ascii(), "{id} 的标题应该是一个 i18n 键或中文文案：{title}");
    }
}

#[test]
fn effects_declarations_are_consistent_with_names() {
    // 名字里带 `revert` / `clean` / `uninstall` / `import` / `apply` 的命令必须是写盘类。
    let (_dir, _engine, commands) = setup();
    for id in commands.ids() {
        let Some(command) = commands.get(id) else { continue };
        let effects = command.effects();
        let looks_like_a_write = ["apply", "revert", "clean", "uninstall", "import", "save", "set", "prune"].iter().any(|verb| id.ends_with(verb));
        if looks_like_a_write {
            assert!(effects.writes_disk, "{id} 名字像写操作，但 Effects 说它不写盘");
        }
    }
}

/// 让 `Command` 的若干 getter 出现在这个集成测试的覆盖面里。
mod getters {
    /// 编译期断言：`Commands` 提供 `get`，且返回的 trait object 有 `title` / `effects`。
    ///
    /// 这不是运行时测试，而是"接口形状"的守卫：CLI/GUI 都靠这些 getter，
    /// 有人把它们改成私有或换签名时，这里会先编译失败。
    #[allow(dead_code)]
    fn shape(commands: &metis_engine::Commands) {
        let _: Option<&dyn metis_engine::Command> = commands.get("state.status");
    }
}
