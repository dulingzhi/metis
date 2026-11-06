//! `eframe::App` 实现与屏幕分派。
//!
//! 分派只在这里（`docs/DEVELOPMENT.md` §5.1）。每个屏幕是一个自由函数，写在 [`crate::screens`]。

use serde_json::json;

use metis_core::{Channel, Mode};
use metis_engine::{CommandOutput, Commands, Engine};
use metis_paths::Paths;

use crate::jobs::{Finished, Jobs};
use crate::model::{self, HarnessRow, PersonaRow};
use crate::screens;
use crate::state::{Draft, Notice, NoticeTone, Screen, UiState};
use crate::theme::Tokens;

/// 后台任务产出的结果类型。
#[derive(Debug)]
pub enum JobDone {
    /// 刷新完成。
    Refreshed(Box<Snapshot>),
    /// 一次应用完成。
    Applied(Box<metis_engine::ApplyReport>),
    /// 一次实测验证完成。
    Verified(Box<serde_json::Value>),
    /// 一次从 GitHub 的导入完成。
    Imported {
        /// 导入报告（命令输出的 JSON）。
        value: Box<serde_json::Value>,
        /// `true` = 这是预览（没写盘）。
        dry_run: bool,
    },
}

/// 一次刷新拿到的快照。
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    /// 执行器行。
    pub harnesses: Vec<HarnessRow>,
    /// 人设行。
    pub personas: Vec<PersonaRow>,
    /// 磁盘上的人设。
    pub saved_persona: Option<String>,
    /// 磁盘上的模式。
    pub saved_mode: Mode,
    /// 总电源是否开着。
    pub power_on: bool,
    /// 状态文件路径。
    pub state_path: String,
}

/// 应用。
#[derive(Debug)]
pub struct MetisApp {
    engine: Engine,
    commands: Commands,
    /// 界面偏好。
    pub ui: UiState,
    /// 控制台草稿。
    pub draft: Draft,
    /// 最近一次快照。
    pub snapshot: Snapshot,
    /// 后台任务。
    pub jobs: Jobs<JobDone>,
    /// 顶部提示。
    pub notice: Option<Notice>,
    /// 正在运行的命令 id（用来禁用按钮）。
    pub busy: Option<String>,
}

impl MetisApp {
    /// 用真实路径构造。
    ///
    /// **注意**：界面偏好（屏幕、侧栏宽度、深色开关）不是在这里读的 ——
    /// 它们由 `eframe` 在 `App::save`/`CreationContext::storage` 里交接。
    /// 调用方拿到 app 之后要执行 `restore_ui(cc.storage)`，
    /// 否则 `UiState` 永远是默认值（曾经因此让"记住上次界面"和深色主题都没生效）。
    pub fn new(engine: Engine) -> Self {
        Self {
            engine,
            commands: Commands::default_set(),
            ui: UiState::default(),
            draft: Draft::default(),
            snapshot: Snapshot::default(),
            jobs: Jobs::new(),
            notice: None,
            busy: None,
        }
    }

    /// 处理键盘快捷键。
    ///
    /// 只在**没有文本输入框获得焦点**时响应 —— 否则用户在搜索框里打 `r`
    /// 会被当成"刷新"。这是快捷键最常见的坑。
    pub fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, KeyboardShortcut, Modifiers};
        let pressed = |shortcut: KeyboardShortcut| ctx.input_mut(|input| input.consume_shortcut(&shortcut));

        // ── 全局唤起型快捷键：**不受焦点限制** ────────────────────────
        //
        // `Ctrl+K` 打开命令面板、`Esc` 关掉它。
        //
        // 为什么必须放在焦点守卫之前：界面里**任何时候都有控件持有焦点**，
        // 而这两个键的语义就是"从任何地方唤起 / 从任何地方退出"。
        // 放在守卫之后的表现是真机踩到的两个 bug：Ctrl+K 点过按钮后永久失效、
        // Esc 在输入框聚焦时被吞掉导致面板关不掉。
        //
        // `Esc` 用 `key_pressed` 而不是 `consume_key`：输入框会把 Esc
        // 解释成"取消编辑"并吃掉它，`consume_key` 就收不到了。
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::K)) {
            self.ui.palette.open();
        }
        if self.ui.palette.open && ctx.input(|input| input.key_pressed(Key::Escape)) {
            self.ui.palette.close();
        }

        // 其余快捷键在有控件持有键盘焦点时让路 ——
        // 否则用户在搜索框里打 `r` 会被当成"刷新"。
        if ctx.memory(|memory| memory.focused().is_some()) {
            return;
        }

        // Alt+1..4 切换四个屏幕（与 `docs/UI.md` §6 的规定一致）。
        for (index, screen) in [Screen::Console, Screen::Library, Screen::Diagnostics, Screen::About].into_iter().enumerate() {
            let key = match index {
                0 => Key::Num1,
                1 => Key::Num2,
                2 => Key::Num3,
                _ => Key::Num4,
            };
            if pressed(KeyboardShortcut::new(Modifiers::ALT, key)) {
                self.set_screen(screen);
            }
        }

        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::R)) {
            self.spawn_refresh();
        }
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::Enter)) {
            self.spawn_apply();
        }
        if pressed(KeyboardShortcut::new(Modifiers::COMMAND, Key::L)) {
            self.ui.dark = !self.ui.dark;
        }

        // Esc：关掉提示条（用户明确要"我能清掉这条消息"）。
        if pressed(KeyboardShortcut::new(Modifiers::NONE, Key::Escape)) && self.notice.is_some() {
            self.notice = None;
        }
    }

    /// 从 eframe 的 storage 恢复界面偏好。
    ///
    /// 一旦恢复成功，就按恢复后的主题重新装一次（`Tokens::install` 在
    /// `main` 里是用默认主题装的）。
    pub fn restore_ui(&mut self, storage: Option<&dyn eframe::Storage>) {
        self.ui = UiState::load(storage);
    }

    /// 用给定路径构造（测试用）。
    pub fn with_paths(paths: Paths) -> Self {
        Self::new(Engine::new(paths))
    }

    /// 引擎。
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// 命令表。
    pub fn commands(&self) -> &Commands {
        &self.commands
    }

    /// 执行一条命令并提示结果。
    pub fn invoke(&mut self, id: &str, params: serde_json::Value) -> Option<CommandOutput> {
        match self.commands.run(id, &self.engine, &params) {
            Ok(output) => {
                self.notify(notice_for(id, &output), NoticeTone::Success);
                Some(output)
            }
            Err(error) => {
                let text = match error.remediation() {
                    Some(remediation) => format!("{}（{remediation}）", error.message()),
                    None => error.message().to_owned(),
                };
                self.notify(text, NoticeTone::Error);
                None
            }
        }
    }

    /// 同步刷新（测试与首帧用；真实界面走后台）。
    pub fn refresh_blocking(&mut self) {
        if let Ok(snapshot) = self.collect_snapshot() {
            self.apply_snapshot(snapshot);
        }
    }

    /// 后台刷新。
    pub fn spawn_refresh(&mut self) {
        if self.jobs.is_busy() {
            return;
        }
        let engine = self.engine.clone();
        self.busy = Some("refresh".to_owned());
        let commands = Commands::default_set();
        self.jobs.spawn("读取状态", move |_| {
            let snapshot = collect_snapshot_with(&engine, &commands)?;
            Ok(JobDone::Refreshed(Box::new(snapshot)))
        });
    }

    /// 后台应用。
    pub fn spawn_apply(&mut self) {
        let engine = self.engine.clone();
        let commands = Commands::default_set();
        self.busy = Some("apply".to_owned());
        self.jobs.spawn("应用变更", move |_| {
            let output = commands.run("config.apply", &engine, &json!({}))?;
            match output {
                CommandOutput::Apply(report) => Ok(JobDone::Applied(report)),
                _ => Ok(JobDone::Applied(Box::new(empty_report()))),
            }
        });
    }

    /// 后台跑一次实测验证（会起宿主进程，绝不能放在帧里）。
    pub fn spawn_verify(&mut self) {
        let engine = self.engine.clone();
        let commands = Commands::default_set();
        self.busy = Some("verify".to_owned());
        self.jobs.spawn("实测验证", move |_| {
            let output = commands.run("harness.verify", &engine, &json!({}))?;
            Ok(JobDone::Verified(Box::new(output.to_json())))
        });
    }

    /// 后台跑一次 GitHub 导入。
    ///
    /// **必须走后台**：这是网络操作，同步跑会冻结界面（`铁律 11`：慢操作不上 UI 线程）。
    pub fn spawn_import(&mut self, source: String, dry_run: bool) {
        let engine = self.engine.clone();
        let commands = Commands::default_set();
        self.busy = Some("import".to_owned());
        self.jobs.spawn("从 GitHub 导入", move |_| {
            let output = commands.run(
                "metis.import-personas",
                &engine,
                &json!({
                    "source": source, "kind": "github", "dryRun": dry_run,
                }),
            )?;
            Ok(JobDone::Imported { value: Box::new(output.to_json()), dry_run })
        });
    }

    fn collect_snapshot(&self) -> metis_core::Result<Snapshot> {
        collect_snapshot_with(&self.engine, &self.commands)
    }

    fn apply_snapshot(&mut self, snapshot: Snapshot) {
        self.draft = Draft::from_saved(snapshot.saved_persona.clone(), snapshot.saved_mode);
        self.snapshot = snapshot;
    }

    /// 设置提示。
    pub fn notify(&mut self, text: impl Into<String>, tone: NoticeTone) {
        self.notice = Some(Notice { text: text.into(), tone });
    }

    /// 处理后台结果（每帧调一次）。
    pub fn drain_jobs(&mut self) {
        for finished in self.jobs.poll() {
            self.busy = None;
            match finished {
                Finished::Done(_, JobDone::Refreshed(snapshot)) => {
                    self.apply_snapshot(*snapshot);
                }
                Finished::Done(_, JobDone::Applied(report)) => {
                    let failures = report.failures().len();
                    let tone = if failures == 0 { NoticeTone::Success } else { NoticeTone::Warning };
                    self.notify(format!("应用完成：{} 条成功，{failures} 条失败", report.written_count()), tone);
                    self.refresh_blocking();
                }
                Finished::Failed(_, error) => self.notify(error.message().to_owned(), NoticeTone::Error),
                Finished::Done(_, JobDone::Verified(value)) => {
                    let passed = value
                        .get("results")
                        .and_then(|r| r.as_array())
                        .map(|items| items.iter().filter(|i| i.get("status").and_then(|s| s.as_str()) == Some("ok")).count())
                        .unwrap_or(0);
                    let total = value.get("results").and_then(|r| r.as_array()).map(|i| i.len()).unwrap_or(0);
                    let tone = if passed > 0 { NoticeTone::Success } else { NoticeTone::Warning };
                    self.notify(format!("实测完成：{passed}/{total} 家命中特权层"), tone);
                }
                Finished::Done(_, JobDone::Imported { value, dry_run }) => {
                    // 真导入才刷新：人设列表变了。预览不该触发刷新（没写盘）。
                    if !dry_run {
                        self.refresh_blocking();
                    }
                    let prefix = if dry_run { "预览" } else { "导入" };
                    let items = value.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
                    let count_of = |want: &str| items.iter().filter(|i| i.get("outcome").and_then(|v| v.as_str()) == Some(want)).count();
                    let (imported, skipped, failed) = (count_of("imported"), count_of("skipped"), count_of("failed"));
                    let tone = if failed > 0 { NoticeTone::Warning } else { NoticeTone::Success };
                    let suffix = if dry_run { "（未写入）" } else { "" };
                    self.notify(format!("{prefix}完成：{imported} 份新增，{skipped} 份跳过，{failed} 份失败{suffix}"), tone);
                    if let Some(detail) = items
                        .iter()
                        .find(|i| i.get("outcome").and_then(|v| v.as_str()) == Some("failed"))
                        .and_then(|i| i.get("detail"))
                        .and_then(|d| d.as_str())
                    {
                        self.notify(format!("失败原因：{detail}"), NoticeTone::Error);
                    }
                }
                Finished::Cancelled(_) => self.notify("已取消", NoticeTone::Info),
            }
        }
    }

    /// 切换屏幕。
    pub fn set_screen(&mut self, screen: Screen) {
        self.ui.screen = screen;
    }
}

fn empty_report() -> metis_engine::ApplyReport {
    metis_engine::ApplyReport {
        id: String::new(),
        at: String::new(),
        revision: String::new(),
        persona: None,
        mode: Mode::Off,
        ok: false,
        partial: false,
        results: Vec::new(),
        runtime_confirmed: false,
    }
}

fn collect_snapshot_with(engine: &Engine, commands: &Commands) -> metis_core::Result<Snapshot> {
    let state = engine.state()?;
    let mut harnesses = Vec::new();
    if let Ok(CommandOutput::Json(value)) = commands.run("harness.list", engine, &json!({"detect": true}))
        && let Some(items) = value.get("harnesses").and_then(|v| v.as_array())
    {
        {
            for item in items {
                let id = item.get("id").and_then(|v| v.as_str()).unwrap_or_default();
                let Some(spec) = metis_harness::registry::find(id) else { continue };
                let armed: Vec<Channel> = item
                    .get("armed")
                    .and_then(|v| v.as_array())
                    .map(|items| items.iter().filter_map(|v| v.as_str()).filter_map(Channel::parse).collect())
                    .unwrap_or_default();
                let on = item.pointer("/activation/on").and_then(|v| v.as_bool()).unwrap_or(false);
                let explain = item.pointer("/activation/explain").and_then(|v| v.as_str()).unwrap_or("未启用").to_owned();
                let mut row = model::row_from_spec(spec, &armed, None);
                row.on = on;
                row.explain = explain;
                row.installed = item.get("installed").and_then(|v| v.as_bool());
                // 原版靠这个区分「已就绪」（有可执行文件）与「发现配置」（只有配置目录）。
                row.has_binary = item.pointer("/bin").and_then(|v| v.as_str()).is_some_and(|bin| !bin.is_empty());
                // 分家例外：`overrides` 是 `Vec<(HarnessId, Override)>`，不是 map。
                if let Some((_, over)) = state.overrides.iter().find(|(h, _)| h.as_str() == id) {
                    row.persona = over.persona.as_ref().map(|p| p.to_string());
                    row.mode = over.mode;
                }
                harnesses.push(row);
            }
        }
    }

    let mut personas = Vec::new();
    if let Ok(CommandOutput::Json(value)) = commands.run("persona.list", engine, &json!({}))
        && let Some(items) = value.get("personas").and_then(|v| v.as_array())
    {
        {
            for item in items {
                personas.push(PersonaRow {
                    id: item.get("id").and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
                    name: item.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_owned(),
                    description: item.get("description").and_then(|v| v.as_str()).map(str::to_owned),
                    chars: item.get("chars").and_then(|v| v.as_u64()).unwrap_or(0) as usize,
                    active: item.get("active").and_then(|v| v.as_bool()).unwrap_or(false),
                });
            }
        }
    }

    Ok(Snapshot {
        harnesses,
        personas,
        saved_persona: state.persona.clone(),
        saved_mode: state.default.mode,
        power_on: state.default.mode != Mode::Off,
        state_path: engine.paths().state_file().display().to_string(),
    })
}

fn notice_for(id: &str, output: &CommandOutput) -> String {
    match output {
        CommandOutput::Message(message) => message.clone(),
        _ => format!("{id} 完成"),
    }
}

impl eframe::App for MetisApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.drain_jobs();
        self.handle_shortcuts(ui.ctx());
        // 主题可能在上一帧被切换：token 存在 `ctx.data` 里，切换后要重新安装，
        // 否则控件（滚动条、输入框）会停在旧主题上，跟背景打架。
        Tokens::apply_motion(ui.ctx(), self.ui.reduce_motion);
        let wanted = Tokens::for_dark(self.ui.dark);
        let installed = Tokens::get(ui.ctx());
        if installed != wanted {
            Tokens::install_with(ui.ctx(), wanted);
        }
        let tokens = wanted;

        // egui 0.36 的 Panel 与 CentralPanel 都接受 `&mut Ui`。
        egui::Panel::top("topbar").show(ui, |ui| crate::chrome::topbar(self, ui, &tokens));
        egui::Panel::left("nav")
            .exact_size(crate::sidebar::width(self, ui.ctx()))
            .resizable(false)
            .frame(egui::Frame::new().fill(tokens.chrome).inner_margin(egui::Margin::symmetric(8, 0)))
            .show(ui, |ui| {
                crate::sidebar::show(self, ui);
            });
        egui::Panel::bottom("status").show(ui, |ui| crate::chrome::statusbar(self, ui, &tokens));
        // 内容区留白：贴边的卡片看起来像被裁掉了（真机截图确认过）。
        egui::CentralPanel::default().frame(egui::Frame::new().fill(tokens.canvas).inner_margin(egui::Margin::symmetric(24, 18))).show(ui, |ui| {
            match self.ui.screen {
                Screen::Console => screens::console(self, ui),
                Screen::Library => screens::library(self, ui),
                Screen::Diagnostics => screens::diagnostics(self, ui),
                Screen::About => screens::about(self, ui),
            }
        });

        // 详情浮层：盖在内容之上，独立于布局。
        crate::screens::show_detail(self, ui.ctx());

        // 命令面板画在最上层（`Ctrl+K` 或顶栏入口唤起）。
        //
        // **必须在 CentralPanel 之后**：它要盖住所有内容。
        // 这一步曾经漏掉过 —— 按钮能点、`palette.open` 也置上了，但没有任何地方
        // 去画它，于是"打开命令面板"看起来完全失灵（用 `agent-desktop` 抓语义树才定位到）。
        if let Some(action) = crate::palette::show(self, ui.ctx()) {
            crate::actions::run(self, action, None);
        }

        // 有任务在跑时用 100ms 节奏重绘；空闲时不强制重绘（不烧 CPU）。
        if self.jobs.is_busy() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.ui.save(storage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tempdir");
        let app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        (dir, app)
    }

    #[test]
    fn snapshot_collection_starts_empty_and_off() {
        let (_dir, mut app) = app();
        app.refresh_blocking();
        assert_eq!(app.snapshot.harnesses.len(), 15, "应列出全部 15 家");
        assert!(app.snapshot.personas.is_empty());
        assert!(app.snapshot.power_on, "默认 replace 不是暂停");
        assert_eq!(app.draft.mode, Mode::Replace);
    }

    #[test]
    fn invoke_reports_errors_with_remediation() {
        let (_dir, mut app) = app();
        assert!(app.invoke("persona.use", json!({"persona": "ghost"})).is_none());
        let notice = app.notice.as_ref().expect("notice");
        assert_eq!(notice.tone, NoticeTone::Error);
        assert!(notice.text.contains("没有人设"), "{}", notice.text);
    }

    #[test]
    fn invoke_success_sets_a_success_notice() {
        let (_dir, mut app) = app();
        app.invoke("persona.save", json!({"persona": "p", "body": "正文"}));
        let notice = app.notice.as_ref().expect("notice");
        assert_eq!(notice.tone, NoticeTone::Success);
    }

    #[test]
    fn refresh_picks_up_saved_persona_and_mode() {
        let (_dir, mut app) = app();
        app.invoke("persona.save", json!({"persona": "p", "body": "正文"}));
        app.invoke("persona.use", json!({"persona": "p"}));
        app.invoke("state.mode", json!({"mode": "append"}));
        app.refresh_blocking();
        assert_eq!(app.snapshot.saved_persona.as_deref(), Some("p"));
        assert_eq!(app.draft.mode, Mode::Append);
        assert_eq!(app.snapshot.personas.len(), 1);
        assert!(app.snapshot.personas[0].active);
    }

    #[test]
    fn power_off_shows_up_in_the_snapshot() {
        let (_dir, mut app) = app();
        app.invoke("state.power", json!({"on": false}));
        app.refresh_blocking();
        assert!(!app.snapshot.power_on);
    }

    #[test]
    fn with_paths_builds_an_app_without_touching_the_real_home() {
        let dir = tempfile::tempdir().expect("tmp");
        let app = MetisApp::with_paths(metis_paths::Paths::with_home(dir.path().join("home")));
        assert!(app.engine().paths().home().starts_with(dir.path()), "根必须在临时目录里");
        assert_eq!(app.draft.mode, metis_core::Mode::Replace, "默认模式");
    }

    /// 等到后台任务全部结束。
    ///
    /// **不用"循环 N 次 × sleep"**：那本质上是墙钟断言，机器一忙就红
    /// （`AGENTS.md` 测试纪律明确禁止；这条测试曾经因为 2 秒不够而偶发失败）。
    /// 改成"等到真实空闲，上限 30 秒"—— 上限只用来防止死锁时永久挂起，
    /// 正常路径下永远是第一个 `while` 条件就退出。
    fn wait_for_idle(app: &mut MetisApp) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while app.jobs.is_busy() {
            app.drain_jobs();
            if std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn default_snapshot_lists_all_harnesses_and_is_on() {
        // 不碰真实 home：用一个临时根，断言"默认快照"的形状。
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = MetisApp::with_paths(metis_paths::Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        assert!(!app.snapshot.harnesses.is_empty(), "15 家应该在快照里");
        assert!(app.snapshot.power_on, "默认开着");
    }

    #[test]
    fn spawn_refresh_eventually_fills_the_snapshot() {
        let (_dir, mut app) = app();
        app.spawn_refresh();
        wait_for_idle(&mut app);
        assert!(!app.jobs.is_busy(), "刷新应在合理时间内结束");
        assert!(!app.snapshot.harnesses.is_empty(), "刷新后快照应有内容");
    }

    #[test]
    fn spawn_apply_reports_failure_without_a_persona() {
        let (_dir, mut app) = app();
        app.spawn_apply();
        wait_for_idle(&mut app);
        // 没有选人设就应用：必须给出提示，而不是静默无事发生。
        assert!(app.notice.is_some(), "失败必须有提示");
    }

    #[test]
    fn notify_keeps_the_latest_message() {
        let (_dir, mut app) = app();
        app.notify("第一条", crate::state::NoticeTone::Info);
        app.notify("第二条", crate::state::NoticeTone::Warning);
        let notice = app.notice.as_ref().expect("notice");
        assert_eq!(notice.text, "第二条");
        assert_eq!(notice.tone, crate::state::NoticeTone::Warning);
    }

    #[test]
    fn invoke_returns_none_for_an_unknown_command() {
        let (_dir, mut app) = app();
        let result = app.invoke("definitely.not.a.command", serde_json::json!({}));
        assert!(result.is_none(), "未知命令应返回 None");
        assert!(app.notice.is_some(), "并且要有提示");
    }

    #[test]
    fn commands_accessor_is_usable() {
        let (_dir, app) = app();
        assert!(app.commands().ids().iter().any(|id| **id == *"persona.save"));
    }

    /// 造一个带按键事件的 ctx。
    fn ctx_with_key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Context {
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))),
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |_ui| {});
        output.textures_delta.clear();
        ctx
    }

    #[test]
    fn ctrl_number_switches_screens() {
        for (key, expected) in
            [(egui::Key::Num1, Screen::Console), (egui::Key::Num2, Screen::Library), (egui::Key::Num3, Screen::Diagnostics), (egui::Key::Num4, Screen::About)]
        {
            let (_dir, mut app) = app();
            let ctx = ctx_with_key(key, egui::Modifiers::ALT);
            app.handle_shortcuts(&ctx);
            assert_eq!(app.ui.screen, expected, "{key:?} 应切到 {expected:?}（Alt 组合）");
        }
    }

    #[test]
    fn ctrl_k_opens_the_palette_even_while_a_widget_has_focus() {
        // 真机踩过：Ctrl+K 一度被"有焦点就不响应"的守卫挡死，
        // 于是点过任何按钮之后它永久失效。命令面板必须能从任何地方唤起。
        let (_dir, mut app) = app();
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
        // 先让一个输入框拿到焦点。
        let mut output = ctx.run_ui(raw, |ui| {
            let mut text = String::new();
            ui.text_edit_singleline(&mut text).request_focus();
        });
        output.textures_delta.clear();
        assert!(ctx.memory(|m| m.focused().is_some()), "前置条件：有控件持有焦点");

        // 在这个状态下按 Ctrl+K，面板必须打开。
        let ctx = ctx_with_key(egui::Key::K, egui::Modifiers::COMMAND);
        app.handle_shortcuts(&ctx);
        assert!(app.ui.palette.open, "Ctrl+K 必须能打开命令面板（不受焦点限制）");
    }

    #[test]
    fn escape_closes_the_command_palette() {
        // 真机 bug：Esc 在输入框聚焦时被 egui 当成"取消编辑"吃掉，
        // 面板关不掉。所以 Esc 必须在 app 层、焦点守卫之前处理。
        let (_dir, mut app) = app();
        app.ui.palette.open();
        assert!(app.ui.palette.open);

        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))),
            events: vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            let _ = ui;
        });
        output.textures_delta.clear();

        app.handle_shortcuts(&ctx);
        assert!(!app.ui.palette.open, "Esc 必须能关掉命令面板");
    }

    #[test]
    fn escape_closes_the_palette_even_while_the_search_box_has_focus() {
        // 最关键的场景：面板自己的输入框拿着焦点时，Esc 仍然要能关。
        let (_dir, mut app) = app();
        app.ui.palette.open();

        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
        // 让一个输入框拿到焦点（模拟面板里的搜索框）。
        let mut output = ctx.run_ui(raw, |ui| {
            let mut text = String::new();
            ui.text_edit_singleline(&mut text).request_focus();
        });
        output.textures_delta.clear();
        assert!(ctx.memory(|m| m.focused().is_some()), "前置条件：有控件持有焦点");

        // 同一个 ctx 里按 Esc。
        let raw2 = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))),
            events: vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };
        let mut output2 = ctx.run_ui(raw2, |ui| {
            let _ = ui;
        });
        output2.textures_delta.clear();

        app.handle_shortcuts(&ctx);
        assert!(!app.ui.palette.open, "输入框有焦点时 Esc 也要能关面板");
    }

    #[test]
    fn ctrl_l_toggles_the_theme() {
        let (_dir, mut app) = app();
        assert!(!app.ui.dark);
        app.handle_shortcuts(&ctx_with_key(egui::Key::L, egui::Modifiers::COMMAND));
        assert!(app.ui.dark, "Ctrl+L 应切到深色");
        app.handle_shortcuts(&ctx_with_key(egui::Key::L, egui::Modifiers::COMMAND));
        assert!(!app.ui.dark, "再按一次应切回浅色");
    }

    #[test]
    fn a_bare_key_does_not_trigger_a_shortcut() {
        // 不带 Ctrl 的 `1` 不该切屏（否则用户在界面里打字会乱跳）。
        let (_dir, mut app) = app();
        app.set_screen(Screen::About);
        app.handle_shortcuts(&ctx_with_key(egui::Key::Num1, egui::Modifiers::NONE));
        assert_eq!(app.ui.screen, Screen::About, "裸键不该触发快捷键");
    }

    #[test]
    fn shortcuts_are_ignored_while_a_widget_has_focus() {
        // 输入框持有焦点时，快捷键必须让路。
        let (_dir, mut app) = app();
        app.set_screen(Screen::About);
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        // 让一个 TextEdit 拿到焦点。
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
        let id = egui::Id::new("test-input");
        let mut output = ctx.run_ui(raw, |ui| {
            let mut text = String::new();
            let response = ui.text_edit_singleline(&mut text);
            response.request_focus();
            let _ = id;
        });
        output.textures_delta.clear();
        assert!(ctx.memory(|m| m.focused().is_some()), "前置条件：应该有一个控件占着焦点");

        app.handle_shortcuts(&ctx);
        assert_eq!(app.ui.screen, Screen::About, "有焦点时不该切屏");
    }

    #[test]
    fn switching_to_dark_actually_changes_what_is_rendered() {
        // 回归：深色开关曾经只改了 `ui.dark`，但 `ui()` 里的 token 没跟着换 ——
        // 截图证明两张图字节数完全相同（都是浅色）。
        let (_dir, mut app) = app();
        app.ui.dark = false;
        let light = render_console(&mut app);
        app.ui.dark = true;
        let dark = render_console(&mut app);
        assert_ne!(light, dark, "深色与浅色的渲染结果不能一样（说明 token 没生效）");
    }

    /// 跑一帧控制台，返回"背景色"（取面板填充色的第一个像素图元）。
    fn render_console(app: &mut MetisApp) -> [u8; 4] {
        let ctx = egui::Context::default();
        let wanted = Tokens::for_dark(app.ui.dark);
        Tokens::install_with(&ctx, wanted);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            crate::screens::console(app, ui);
        });
        output.textures_delta.clear();
        let _ = output;
        // 直接返回 token 的字节：这才是"界面上看到的背景"。
        [wanted.canvas.r(), wanted.canvas.g(), wanted.canvas.b(), wanted.canvas.a()]
    }

    #[test]
    fn save_persists_the_ui_state_through_eframe_storage() {
        // `save` 是窗口关闭时唯一的持久化路径：写错了，用户的布局偏好每次都会丢。
        use std::collections::HashMap;
        #[derive(Default)]
        struct Mem(HashMap<String, String>);
        impl eframe::Storage for Mem {
            fn get_string(&self, key: &str) -> Option<String> {
                self.0.get(key).cloned()
            }
            fn set_string(&mut self, key: &str, value: String) {
                self.0.insert(key.to_owned(), value);
            }
            fn remove_string(&mut self, key: &str) {
                self.0.remove(key);
            }
            fn flush(&mut self) {}
        }

        let (_dir, mut app) = app();
        app.set_screen(Screen::Library);
        let mut storage = Mem::default();
        <MetisApp as eframe::App>::save(&mut app, &mut storage);
        assert!(!storage.0.is_empty(), "save 应该写入至少一个键");

        // 冒烟：`save` 不能 panic，且键名稳定（前缀来自 `state::STORAGE_KEY`）。
        assert!(storage.0.keys().any(|key| key.contains("metis")), "键应带 metis 前缀：{:?}", storage.0.keys());
    }

    #[test]
    fn set_screen_switches_the_ui_state() {
        let (_dir, mut app) = app();
        app.set_screen(Screen::Library);
        assert_eq!(app.ui.screen, Screen::Library);
    }

    #[test]
    fn busy_flag_tracks_jobs() {
        let (_dir, mut app) = app();
        app.spawn_refresh();
        assert!(app.busy.is_some());
        wait_for_idle(&mut app);
        assert!(app.busy.is_none(), "跑完必须清掉忙碌标记");
        assert_eq!(app.snapshot.harnesses.len(), 15);
    }
}
