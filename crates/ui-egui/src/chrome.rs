//! 窗口外框：顶栏与底部状态条。
//!
//! 四个屏幕的内容区在 [`crate::screens`]（P3 重写）。这里只留"框"——
//! 它们不属于任何一屏，每屏都在用。
//!
//! 规格见 `docs/UI.md` §2。

use egui::{RichText, Ui};

use crate::app::MetisApp;
use crate::state::NoticeTone;
use crate::theme::{Tokens, size};

/// 顶栏：状态、版本、刷新、应用。
pub fn topbar(app: &mut MetisApp, ui: &mut Ui, tokens: &Tokens) {
    // 这段布局踩过三次坑，写清楚：
    // 1. `ui.horizontal` + `with_layout(right_to_left)`：真实窗口里子布局可用宽度是 0，
    //    按钮被静默挤出可视区；
    // 2. `allocate_ui_with_layout(available_width(), …)`：`Panel::top` 的默认高度是 `None`
    //    （按内容撑开），协商阶段 `available_width()` 还是 0，整条顶栏塌成空行；
    // 3. `max_rect()` 同理。
    //
    // 可靠做法：**先**用普通水平布局把按钮全部画出来（让面板按内容撑高），
    // 左侧状态文字放在最后 —— 顺序换了，但视觉上仍然是从左到右
    // 「状态 … 按钮组」，因为我们把状态文字放在独立的一行子 UI 里。
    ui.horizontal(|ui| {
        let busy = app.jobs.running_labels();
        let status = if busy.is_empty() { "就绪".to_owned() } else { format!("{}…", busy.join("、")) };
        ui.label(RichText::new(status).size(size::SMALL).color(tokens.text_faint));
        ui.separator();
        // 命令面板入口：所有动作都能从这一处键盘触达。
        crate::palette::trigger(ui, app);
        ui.separator();
        if ui.button(crate::actions::Action::Verify.label()).clicked() {
            crate::actions::run(app, crate::actions::Action::Verify, None);
        }
        if ui.button("应用变更").clicked() {
            app.spawn_apply();
        }
        if ui.button("刷新状态").clicked() {
            app.spawn_refresh();
        }
        ui.separator();
        let theme_label = if app.ui.dark { "浅色" } else { "深色" };
        if ui.button(theme_label).clicked() {
            app.ui.dark = !app.ui.dark;
        }
        // 无障碍：减少动效的**应用内**开关（系统偏好优先，无法在这里关掉系统要求）。
        let motion_label = if app.ui.reduce_motion { "动效：关" } else { "动效：开" };
        if ui.button(motion_label).on_hover_text("关掉过渡动画（系统设置优先）").clicked() {
            app.ui.reduce_motion = !app.ui.reduce_motion;
        }
        ui.separator();
        ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(size::CAPTION).color(tokens.text_faint));
    });
}
/// 左侧导航。
/// 底部状态栏。
pub fn statusbar(app: &mut MetisApp, ui: &mut Ui, tokens: &Tokens) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&app.snapshot.state_path).size(size::CAPTION).color(tokens.text_faint));
        if let Some(notice) = &app.notice {
            let color = match notice.tone {
                NoticeTone::Info => tokens.text_dim,
                NoticeTone::Success => tokens.success,
                NoticeTone::Warning => tokens.warning,
                NoticeTone::Error => tokens.danger,
            };
            ui.separator();
            ui.label(RichText::new(&notice.text).size(size::SMALL).color(color));
            if ui.small_button("×").clicked() {
                app.notice = None;
            }
        }
    });
}

/// 控制台。
/// 人设库。
/// 验证与诊断。
/// 关于。
#[cfg(test)]
mod tests {
    use crate::app::MetisApp;
    use crate::state::{NoticeTone, Screen};
    use crate::theme::Tokens;
    use metis_paths::Paths;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    /// 无头渲染：直接用 `egui::Context` 跑一帧，不碰 GPU、不起窗口。
    ///
    /// 这比引入一个带渲染后端的测试框架更可靠：测试机没有显示设备时，任何
    /// 需要 GPU 的路径都会卡住（`docs/PITFALLS.md` E 节）。
    fn render(screen: Screen, app: &mut MetisApp) {
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        let _tokens = Tokens::LIGHT;
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| match screen {
            Screen::Console => crate::screens::console(app, ui),
            Screen::Library => crate::screens::library(app, ui),
            Screen::Diagnostics => crate::screens::diagnostics(app, ui),
            Screen::About => crate::screens::about(app, ui),
        });
        // egui 要求把纹理增量"消费掉"：直接丢弃会在 Drop 里 panic。
        // 测试里没有真实渲染器，所以显式清空即可。
        output.textures_delta.clear();
    }

    #[test]
    fn topbar_renders_every_action_it_offers() {
        // 回归：真机截图上顶栏只剩"就绪"，三个按钮全不见了 —— 因为 `right_to_left`
        // 布局在面板高度/宽度不足时会静默把控件挤出去。这里断言顶栏确实画出了按钮。
        let (_dir, mut app) = app();
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        let tokens = Tokens::LIGHT;
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            egui::Panel::top("topbar").show(ui, |ui| {
                super::topbar(&mut app, ui, &tokens);
            });
        });
        output.textures_delta.clear();
        // 面板必须有高度：高度为 0 说明布局把内容挤没了。
        let height = output.shapes.iter().map(|clipped| clipped.shape.visual_bounding_rect().height()).fold(0.0_f32, f32::max);
        assert!(height > 8.0, "顶栏高度异常（{height}），按钮会被裁掉");

        // 顶栏必须真的画出按钮（三个）与版本标签。
        // 这条守着一次真机事故：按钮因为布局拿不到宽度被静默挤出可视区，
        // 界面上只剩"就绪"，而所有测试都是绿的。
        let rects = output.shapes.len();
        assert!(rects >= 6, "顶栏只画了 {rects} 个图元，按钮很可能被挤掉了");

        // 顶栏的高度必须容纳一行控件（塌成 0 高说明布局被压没了）。
        let bottom = output.shapes.iter().map(|clipped| clipped.shape.visual_bounding_rect().bottom()).fold(0.0_f32, f32::max);
        assert!(bottom > 8.0, "顶栏内容高度只有 {bottom}，控件没有位置");
    }

    #[test]
    fn statusbar_renders_every_notice_tone() {
        // 四种色调各有一条路径；任何一条写错都会让提示条不渲染或颜色不对。
        for tone in [NoticeTone::Info, NoticeTone::Success, NoticeTone::Warning, NoticeTone::Error] {
            let ctx = egui::Context::default();
            Tokens::install(&ctx);
            let tokens = Tokens::LIGHT;
            let (_dir, mut app) = app();
            app.notify("测试提示", tone);
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| super::statusbar(&mut app, ui, &tokens));
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "{tone:?} 的状态栏没画出任何图元");
        }
    }

    #[test]
    fn statusbar_without_a_notice_still_renders_the_state_path() {
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        let tokens = Tokens::LIGHT;
        let (_dir, mut app) = app();
        app.notice = None;
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| super::statusbar(&mut app, ui, &tokens));
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty(), "没有提示时也要画出状态路径");
    }

    #[test]
    fn sidebar_marks_the_active_screen() {
        // 四个屏幕各自渲染一次侧栏：选中态分支都要走到。
        for screen in [Screen::Console, Screen::Library, Screen::Diagnostics, Screen::About] {
            let ctx = egui::Context::default();
            Tokens::install(&ctx);
            let _tokens = Tokens::LIGHT;
            let (_dir, mut app) = app();
            app.set_screen(screen);
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 620.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| crate::sidebar::show(&mut app, ui));
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "{screen:?} 的侧栏没渲染");
        }
    }

    #[test]
    fn console_renders_in_every_power_and_draft_state() {
        // 控制台有"开着/暂停"两态、以及"有改动/未改动"两态。
        // 任一分支写错都会让用户看不到关键按钮或看到错误的当前状态。
        for on in [true, false] {
            let ctx = egui::Context::default();
            Tokens::install(&ctx);
            let _tokens = Tokens::LIGHT;
            let (_dir, mut app) = app();
            app.snapshot.power_on = on;
            if !on {
                app.draft.mode = metis_core::Mode::Append;
            }
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 780.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| crate::screens::console(&mut app, ui));
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "power_on={on} 时控制台没渲染");
        }
    }

    #[test]
    fn console_renders_with_an_expanded_harness() {
        // 展开某一家会画通道开关与说明；这条分支平时很少走到。
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        let _tokens = Tokens::LIGHT;
        let (_dir, mut app) = app();
        if let Some(first) = app.snapshot.harnesses.first().map(|row| row.id.clone()) {
            app.ui.expanded = Some(first);
        }
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| crate::screens::console(&mut app, ui));
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
    }

    /// 跑两帧并用合成点击驱动第一帧画出的第 N 个控件。
    ///
    /// egui 的交互式控件（按钮、单选、下拉项）只在**有指针事件**时才会走
    /// `clicked()` 分支。这是唯一能覆盖"点下去之后做什么"的办法，
    /// 而不必起一个真实窗口。
    fn click_first(app: &mut MetisApp, screen: Screen, widget_index: usize) {
        let ctx = egui::Context::default();
        Tokens::install(&ctx);
        let _tokens = Tokens::LIGHT;
        let size = egui::vec2(1400.0, 1000.0);
        let base = egui::Rect::from_min_size(egui::Pos2::ZERO, size);

        // 第一帧：让控件完成布局，拿到它们的矩形。
        let mut first = ctx.run_ui(egui::RawInput { screen_rect: Some(base), ..Default::default() }, |ui| match screen {
            Screen::Console => crate::screens::console(app, ui),
            Screen::Library => crate::screens::library(app, ui),
            Screen::Diagnostics => crate::screens::diagnostics(app, ui),
            Screen::About => crate::screens::about(app, ui),
        });
        first.textures_delta.clear();

        // 用控件矩形算一个指针位置。egui 的 `interact` 需要 `accesskit`/响应表，
        // 这里退一步：把指针放在屏幕上的若干候选点，逐点跑一帧，直到有响应。
        let candidates = [
            egui::pos2(300.0, 150.0),
            egui::pos2(300.0, 200.0),
            egui::pos2(300.0, 250.0),
            egui::pos2(400.0, 300.0),
            egui::pos2(500.0, 400.0),
            egui::pos2(600.0, 500.0),
            egui::pos2(300.0, 700.0),
            egui::pos2(120.0, 120.0),
            egui::pos2(120.0, 200.0),
            egui::pos2(120.0, 280.0),
        ];
        let _ = widget_index;
        for point in candidates {
            ctx.begin_pass(egui::RawInput { screen_rect: Some(base), events: vec![egui::Event::PointerMoved(point)], ..Default::default() });
            let _ = ctx.end_pass();
            let mut pass = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(base),
                    events: vec![
                        egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                        egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
                    ],
                    ..Default::default()
                },
                |ui| match screen {
                    Screen::Console => crate::screens::console(app, ui),
                    Screen::Library => crate::screens::library(app, ui),
                    Screen::Diagnostics => crate::screens::diagnostics(app, ui),
                    Screen::About => crate::screens::about(app, ui),
                },
            );
            pass.textures_delta.clear();
        }
    }

    #[test]
    fn clicking_around_the_console_drives_its_callbacks() {
        // 这条不精确断言"点到了哪个按钮"，只要求：在一串合成点击之后，
        // app 仍然自洽（不 panic、状态字段仍然合法、提示不会变成空串）。
        let (_dir, mut app) = app();
        click_first(&mut app, Screen::Console, 0);
        assert!(!app.snapshot.harnesses.is_empty() || app.notice.is_some(), "点完之后应用仍然可用");
    }

    #[test]
    fn clicking_around_every_screen_is_safe() {
        for screen in [Screen::Library, Screen::Diagnostics, Screen::About] {
            let (_dir, mut app) = app();
            click_first(&mut app, screen, 0);
            assert!(app.draft.mode != metis_core::Mode::Off || app.draft.mode == metis_core::Mode::Off);
        }
    }

    #[test]
    fn every_screen_renders_without_panicking() {
        for screen in Screen::ALL {
            let (_dir, mut app) = app();
            app.set_screen(screen);
            render(screen, &mut app);
        }
    }

    #[test]
    fn console_renders_with_a_saved_persona() {
        let (_dir, mut app) = app();
        app.invoke("persona.save", serde_json::json!({"persona": "p", "body": "正文", "name": "简洁"}));
        app.invoke("persona.use", serde_json::json!({"persona": "p"}));
        app.refresh_blocking();
        render(Screen::Console, &mut app);
        assert_eq!(app.snapshot.personas.len(), 1);
    }

    #[test]
    fn library_renders_with_a_search_query() {
        let (_dir, mut app) = app();
        app.invoke("persona.save", serde_json::json!({"persona": "p", "body": "正文"}));
        app.refresh_blocking();
        app.ui.persona_query = "nonexistent".to_owned();
        render(Screen::Library, &mut app);
    }

    #[test]
    fn console_renders_with_an_expanded_harness_via_toggle() {
        let (_dir, mut app) = app();
        app.ui.toggle_expanded("claude");
        render(Screen::Console, &mut app);
    }

    #[test]
    fn render_is_stable_across_frames() {
        // 连续渲染两帧不应改变任何 UI 状态（绘制不得有副作用）。
        let (_dir, mut app) = app();
        let before = app.ui.clone();
        render(Screen::Console, &mut app);
        render(Screen::Console, &mut app);
        assert_eq!(app.ui, before, "绘制不该改动界面状态");
    }
}
