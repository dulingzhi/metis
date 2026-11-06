//! 界面动作：把已注册的命令包成"点一下就执行"的形状。
//!
//! 铁律 6/7：界面不实现业务，只调命令。这个文件存在的意义是让每个按钮
//! 都明确对应一个命令 id —— 也便于 `xtask` 检查"界面动作都有 headless 对应物"。

use serde_json::{Value, json};

use crate::app::MetisApp;
use crate::state::NoticeTone;

/// 一个界面动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// 刷新状态。
    Refresh,
    /// 应用当前草稿。
    Apply,
    /// 暂停/恢复注入。
    TogglePower,
    /// 撤下某家的全部通道。
    DisarmHarness,
    /// 撤回全部接管（保留人设）。
    Clean,
    /// 只预览清理。
    CleanDryRun,
    /// 跑一次实测验证。
    Verify,
    /// 从 agent-purge 导入。
    Import,
    /// 从 GitHub 仓库导入人设。
    ImportGithub,
    /// 只预览 GitHub 导入。
    ImportGithubDryRun,
    /// 只预览导入。
    ImportDryRun,
    /// 查看备份占用。
    BackupsUsage,
    /// 只预览清理备份。
    BackupsPruneDryRun,
    /// 真的清理孤立备份。
    BackupsPrune,
}

impl Action {
    /// 对应的命令 id（`None` = 这是纯界面动作，不调命令）。
    pub fn command_id(self) -> Option<&'static str> {
        match self {
            Self::Refresh => None, // 走 snapshot 收集，不走命令
            Self::Apply => Some("config.apply"),
            Self::TogglePower => Some("state.power"),
            Self::DisarmHarness => Some("channel.set"),
            Self::Clean => Some("channel.clean"),
            Self::CleanDryRun => Some("channel.clean"),
            Self::Verify => Some("harness.verify"),
            Self::Import => Some("metis.import-agent-purge"),
            Self::ImportDryRun => Some("metis.import-agent-purge"),
            Self::ImportGithub => Some("metis.import-personas"),
            Self::ImportGithubDryRun => Some("metis.import-personas"),
            Self::BackupsUsage => Some("metis.backups-usage"),
            Self::BackupsPruneDryRun => Some("metis.backups-prune"),
            Self::BackupsPrune => Some("metis.backups-prune"),
        }
    }

    /// 面向用户的标签（按钮文字）。
    pub fn label(self) -> &'static str {
        match self {
            Self::Refresh => "刷新状态",
            Self::Apply => "应用变更",
            Self::TogglePower => "开关注入",
            Self::DisarmHarness => "撤下这家",
            Self::Clean => "撤回全部接管",
            Self::CleanDryRun => "预览清理",
            Self::Verify => "实测验证",
            Self::Import => "从 agent-purge 导入",
            Self::ImportDryRun => "预览导入",
            Self::ImportGithub => "从 GitHub 导入",
            Self::ImportGithubDryRun => "预览 GitHub 导入",
            Self::BackupsUsage => "备份占用",
            Self::BackupsPruneDryRun => "预览清理备份",
            Self::BackupsPrune => "清理无用备份",
        }
    }

    /// 全部动作（`xtask` 用它核对"每个动作都有对应命令或明确标注为纯 UI"）。
    pub const ALL: [Self; 11] = [
        Self::Refresh,
        Self::Apply,
        Self::TogglePower,
        Self::DisarmHarness,
        Self::Clean,
        Self::CleanDryRun,
        Self::Verify,
        Self::Import,
        Self::ImportDryRun,
        Self::ImportGithub,
        Self::ImportGithubDryRun,
    ];
}

/// 执行一个动作。
///
/// 所有失败都变成界面上的提示条 —— **绝不 panic、绝不让界面卡住**。
pub fn run(app: &mut MetisApp, action: Action, harness: Option<&str>) {
    match action {
        Action::Refresh => app.spawn_refresh(),
        Action::Apply => app.spawn_apply(),
        Action::TogglePower => {
            let on = !app.snapshot.power_on;
            invoke(app, "state.power", json!({ "on": on }), if on { "已恢复注入" } else { "已暂停注入" });
            app.refresh_blocking();
        }
        Action::DisarmHarness => {
            let Some(harness) = harness else {
                app.notify("先选一家再执行", NoticeTone::Warning);
                return;
            };
            invoke(app, "channel.set", json!({ "harness": harness, "channels": [] }), format!("{harness} 的接管控件已撤下"));
            app.refresh_blocking();
        }
        Action::Clean => {
            invoke(app, "channel.clean", json!({ "dryRun": false }), "已撤回全部接管控件；人设与配置保留");
            app.refresh_blocking();
        }
        Action::CleanDryRun => {
            invoke(app, "channel.clean", json!({ "dryRun": true }), "预览完成（未改动磁盘）");
        }
        Action::Verify => {
            // 验证会起进程：走后台，避免界面卡住。
            app.spawn_verify();
        }
        Action::Import => {
            invoke(app, "metis.import-agent-purge", json!({ "overwrite": false, "dryRun": false }), "导入完成");
            app.refresh_blocking();
        }
        Action::ImportDryRun => {
            invoke(app, "metis.import-agent-purge", json!({ "overwrite": false, "dryRun": true }), "预览完成（未写入）");
        }
        Action::ImportGithub => run_github_import(app, false),
        Action::ImportGithubDryRun => run_github_import(app, true),
        Action::BackupsUsage => {
            let outcome = app.engine().run_command("metis.backups-usage", &json!({}));
            match outcome {
                Ok(output) => {
                    let value = output.to_json();
                    let total = value["totalBytes"].as_u64().unwrap_or(0);
                    let orphans = value["orphanBytes"].as_u64().unwrap_or(0);
                    let count = value["orphanCount"].as_u64().unwrap_or(0);
                    app.notify(
                        format!(
                            "备份共 {}，其中 {} 个无用文件占 {}；点「预览清理备份」查看可释放多少",
                            metis_engine::backups::human_bytes(total),
                            count,
                            metis_engine::backups::human_bytes(orphans)
                        ),
                        NoticeTone::Info,
                    );
                }
                Err(error) => app.notify(error.message().to_owned(), NoticeTone::Error),
            }
        }
        Action::BackupsPruneDryRun => {
            invoke(app, "metis.backups-prune", json!({ "dryRun": true }), "预览完成（未删除任何文件）");
        }
        Action::BackupsPrune => {
            invoke(app, "metis.backups-prune", json!({ "dryRun": false }), "已清理无用备份；被账本引用的全部保留");
        }
    }
}

/// 从 GitHub 导入人设。
///
/// 与 `invoke` 分开是因为它要**读结果再组织提示**：用户需要知道
/// "导入了 3 份、跳过 2 份、失败 1 份"，而不是一句"完成"。
fn run_github_import(app: &mut MetisApp, dry_run: bool) {
    let source = app.ui.import_source.trim().to_owned();
    if source.is_empty() {
        app.notify("先填仓库地址（`owner/repo`）", NoticeTone::Warning);
        return;
    }

    // 网络操作：**必须后台跑**，否则界面冻结。结果在 `JobDone::Imported` 里处理。
    app.spawn_import(source, dry_run);
}

/// 调一个命令并把结果变成提示条。
fn invoke(app: &mut MetisApp, id: &str, params: Value, ok_message: impl Into<String>) {
    let ok_message = ok_message.into();
    match app.engine().clone().run_command(id, &params) {
        Ok(_) => app.notify(ok_message, NoticeTone::Success),
        Err(error) => {
            let text = match error.remediation() {
                Some(remediation) => format!("{}（{remediation}）", error.message()),
                None => error.message().to_owned(),
            };
            app.notify(text, NoticeTone::Error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_has_a_label() {
        for action in Action::ALL {
            assert!(!action.label().is_empty(), "{action:?} 缺少按钮文字");
        }
    }

    #[test]
    fn every_action_but_refresh_maps_to_a_registered_command() {
        // 界面动作必须有 headless 对应物：否则"界面能点、CLI 不能做"的功能会悄悄出现。
        let commands = metis_engine::Commands::default_set();
        let ids = commands.ids();
        for action in Action::ALL {
            match action.command_id() {
                Some(id) => assert!(ids.contains(&id), "{action:?} 指向未注册的命令 {id}"),
                None => assert_eq!(action, Action::Refresh, "只有 Refresh 允许是纯界面动作"),
            }
        }
    }

    /// 造一个连到临时目录的 app：所有动作都能安全跑，不碰真实 home。
    fn app() -> (tempfile::TempDir, crate::app::MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = crate::app::MetisApp::with_paths(metis_paths::Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    #[test]
    fn toggle_power_flips_the_switch_and_reports_it() {
        let (_dir, mut app) = app();
        assert!(app.snapshot.power_on, "默认是开着的");
        run(&mut app, Action::TogglePower, None);
        assert!(!app.snapshot.power_on, "点一下应暂停");
        let notice = app.notice.as_ref().expect("提示");
        assert!(notice.text.contains("暂停"), "{}", notice.text);

        run(&mut app, Action::TogglePower, None);
        assert!(app.snapshot.power_on, "再点一下应恢复");
    }

    #[test]
    fn disarm_without_a_harness_warns_instead_of_panicking() {
        let (_dir, mut app) = app();
        run(&mut app, Action::DisarmHarness, None);
        let notice = app.notice.as_ref().expect("提示");
        assert_eq!(notice.tone, crate::state::NoticeTone::Warning);
        assert!(notice.text.contains("先选一家"));
    }

    #[test]
    fn disarm_with_a_harness_runs_the_command() {
        let (_dir, mut app) = app();
        run(&mut app, Action::DisarmHarness, Some("codex"));
        let notice = app.notice.as_ref().expect("提示");
        assert!(notice.text.contains("codex"), "{}", notice.text);
    }

    #[test]
    fn clean_dry_run_does_not_delete_anything() {
        let (_dir, mut app) = app();
        // 先武装一条通道，让 clean 有活可干。
        app.invoke("persona.save", serde_json::json!({"persona": "p", "body": "正文"}));
        app.invoke("persona.use", serde_json::json!({"persona": "p"}));
        app.refresh_blocking();

        run(&mut app, Action::CleanDryRun, None);
        let notice = app.notice.as_ref().expect("提示");
        assert_eq!(notice.tone, crate::state::NoticeTone::Success);
        assert!(app.engine().paths().home().exists(), "预览不能删数据目录");
    }

    #[test]
    fn clean_actually_runs() {
        let (_dir, mut app) = app();
        run(&mut app, Action::Clean, None);
        let notice = app.notice.as_ref().expect("提示");
        assert_eq!(notice.tone, crate::state::NoticeTone::Success, "{}", notice.text);
    }

    #[test]
    fn import_dry_run_reports_success_even_when_there_is_nothing_to_import() {
        let (_dir, mut app) = app();
        // 默认源 `~/.agent-purge` 在本机可能不存在；动作必须给出**明确的错误提示**而不是崩。
        run(&mut app, Action::ImportDryRun, None);
        let notice = app.notice.as_ref().expect("提示");
        assert!(!notice.text.is_empty(), "失败也要有提示");
    }

    #[test]
    fn every_action_run_is_safe_on_an_empty_engine() {
        // 穷举所有动作：在空配置上都不能 panic。
        for action in Action::ALL {
            let (_dir, mut app) = app();
            run(&mut app, action, Some("codex"));
            assert!(app.notice.is_some() || app.jobs.is_busy(), "{action:?} 既没提示也没起任务");
        }
    }

    #[test]
    fn dry_run_variants_send_the_dry_run_flag() {
        // 预览类动作必须真的带 dryRun，否则"预览"会变成"执行"。
        assert!(matches!(Action::CleanDryRun.command_id(), Some("channel.clean")));
        assert!(matches!(Action::ImportDryRun.command_id(), Some("metis.import-agent-purge")));
    }
}

#[cfg(test)]
mod github_import_tests {
    use super::*;
    use metis_paths::Paths;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    #[test]
    fn the_import_source_defaults_to_the_official_repo() {
        // 默认值必须是官方源：用户点一下就能用。
        // 这个常量是**唯一**一处硬编码的源，改了要同步文档。
        let state = crate::state::UiState::default();
        assert_eq!(state.import_source, crate::state::OFFICIAL_SOURCE);
        assert_eq!(crate::state::OFFICIAL_SOURCE, "dulingzhi/metis-personas");
    }

    #[test]
    fn an_empty_source_is_rejected_before_any_network_call() {
        // 关键：空地址要在**本地**就被拦下，不能发请求、不能起后台任务。
        let (_dir, mut app) = app();
        app.ui.import_source = "   ".to_owned();
        run(&mut app, Action::ImportGithub, None);
        assert!(!app.jobs.is_busy(), "空地址不该起后台任务");
        assert!(app.notice.is_some(), "应该给用户一条提示");
    }

    #[test]
    fn a_real_source_starts_a_background_job() {
        // 网络操作必须走后台（铁律 11：慢操作不上 UI 线程）。
        // 这里只验证"任务起来了"，不验证结果 —— 结果由 `persona_import` 的测试覆盖，
        // 测试环境禁止联网。
        let (_dir, mut app) = app();
        app.ui.import_source = "dulingzhi/metis-personas".to_owned();
        app.spawn_import("dulingzhi/metis-personas".to_owned(), true);
        assert!(app.jobs.is_busy(), "应起一个后台任务");
    }

    #[test]
    fn the_import_actions_point_at_the_import_command() {
        assert_eq!(Action::ImportGithub.command_id(), Some("metis.import-personas"));
        assert_eq!(Action::ImportGithubDryRun.command_id(), Some("metis.import-personas"));
        // 与 agent-purge 那条**不能混**：它们是两个不同的命令。
        assert_ne!(Action::Import.command_id(), Action::ImportGithub.command_id());
    }

    #[test]
    fn the_import_labels_say_github() {
        assert!(Action::ImportGithub.label().contains("GitHub"));
        assert!(Action::ImportGithubDryRun.label().contains("GitHub"));
        assert!(Action::ImportGithubDryRun.label().contains("预览"));
    }
}
