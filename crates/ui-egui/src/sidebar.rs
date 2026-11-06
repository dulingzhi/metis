//! 左侧导航栏。
//!
//! 结构（借鉴同一作者 `craft-anything` 的**布局思路**，实现是 Metis 自己的）：
//!
//! ```text
//! ┌──────────────┐
//! │ Metis    «   │  ← 品牌 + 折叠按钮
//! │ 提示词控制台  │
//! ├──────────────┤
//! │ ▣ 控制台      │  ← 图标行，选中态是 accent 底 + 白字
//! │ ▤ 人设库      │
//! │ ◇ 验证与诊断  │
//! ├──────────────┤
//! │ ...          │
//! │ 本地·数据不出本机│  ← 底部，贴底
//! └──────────────┘
//! ```
//!
//! 折叠后只剩图标（56px）；窗口窄于 900px 时**自动折叠**并把空间让给内容。

use egui::{RichText, Sense, Ui, Vec2};

use crate::state::Screen;
use crate::tokens::{Tokens, font, fonts, space};
use crate::{MetisApp, icons};

/// 展开宽度。
pub const WIDTH: f32 = 248.0;
/// 折叠宽度（只剩图标）。
pub const FOLDED: f32 = 56.0;
/// 低于这个窗口宽度就自动折叠。
pub const FOLD_BELOW: f32 = 900.0;
/// 一行的高度。
const ROW_H: f32 = 34.0;

/// 每个屏幕对应的图标。
pub fn screen_icon(screen: Screen) -> &'static str {
    match screen {
        Screen::Console => "gauge",
        Screen::Library => "book-open",
        Screen::Diagnostics => "activity",
        Screen::About => "info",
    }
}

/// 这一屏的一句话说明（折叠时不显示，用悬停提示代替）。
pub fn screen_hint(screen: Screen) -> &'static str {
    match screen {
        Screen::Console => "选人设、定模式、应用变更",
        Screen::Library => "新建、编辑、删除人设",
        Screen::Diagnostics => "实测验证、清理、导入",
        Screen::About => "版本、备份、迁移",
    }
}

/// 现在是否折叠：用户手动折了，**或者**窗口太窄。
pub fn folded(app: &MetisApp, ctx: &egui::Context) -> bool {
    app.ui.sidebar_folded || ctx.content_rect().width() < FOLD_BELOW
}

/// 侧栏宽度。
pub fn width(app: &MetisApp, ctx: &egui::Context) -> f32 {
    if folded(app, ctx) { FOLDED } else { WIDTH }
}

/// 画侧栏。调用方负责给它一个 `chrome` 底色的 `Panel::left`。
pub fn show(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    let fold = folded(app, ui.ctx());
    let narrow = ui.ctx().content_rect().width() < FOLD_BELOW;

    // 顶部：品牌与折叠按钮。
    brand(app, ui, &t, fold, narrow);
    ui.add_space(space::M);

    // 导航行。
    for screen in Screen::ALL {
        if row(ui, &t, screen_icon(screen), screen.label(), screen_hint(screen), app.ui.screen == screen, fold).clicked() {
            app.set_screen(screen);
        }
    }

    // 底部：贴底的一句话。折叠时只放一个小点（避免挤成竖排字）。
    let bottom_h = ROW_H + space::S;
    let gap = (ui.available_height() - bottom_h).max(0.0);
    ui.add_space(gap);
    if fold {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new("●").size(font::CAPTION).color(t.success));
        });
    } else {
        ui.horizontal(|ui| {
            ui.label(RichText::new("●").size(font::CAPTION).color(t.success));
            ui.label(RichText::new("本地配置 · 数据不出本机").size(font::CAPTION).color(t.text_faint));
        });
    }
}

/// 品牌头 + 折叠按钮。
fn brand(app: &mut MetisApp, ui: &mut Ui, t: &Tokens, fold: bool, narrow: bool) {
    ui.add_space(space::M);
    if fold {
        // 折叠：只留一个方块标 + 展开按钮（竖排）。
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
            icons::paint(ui, rect, "shield-check", 20.0, t.accent);
            if !narrow {
                ui.add_space(space::XS);
                if icons::button(ui, "chevron-right", 16.0, t.icon).on_hover_text("展开侧栏").clicked() {
                    app.ui.sidebar_folded = false;
                }
            }
        });
        return;
    }

    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
        icons::paint(ui, rect, "shield-check", 20.0, t.accent);
        ui.label(RichText::new(crate::APP_TITLE).size(font::SECTION).family(fonts::medium()).color(t.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !narrow && icons::button(ui, "chevron-left", 16.0, t.icon).on_hover_text("折叠侧栏").clicked() {
                app.ui.sidebar_folded = true;
            }
        });
    });
    ui.label(RichText::new("提示词控制台").size(font::CAPTION).color(t.text_faint));
}

/// 一行导航。
///
/// 选中态是 `accent_soft` 底 + `accent_ink` 字；悬停是 `hover` 底。
/// 折叠时只画图标，用悬停提示代替文字。
fn row(ui: &mut Ui, t: &Tokens, icon: &str, label: &str, hint: &str, selected: bool, fold: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());

    let fill = if selected {
        t.accent_soft
    } else if response.hovered() {
        t.hover
    } else {
        egui::Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, egui::CornerRadius::same(t.radius_sm as u8), fill);
    if response.has_focus() {
        ui.painter().rect_stroke(rect.shrink(1.0), egui::CornerRadius::same(t.radius_sm as u8), egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    }

    // **无障碍语义**：`allocate_exact_size` 画的控件不会自动产出语义节点
    // （`ui.painter().text` 也不会）。不显式声明的话，侧栏在无障碍树里
    // **整个不可见** —— 用 agent-desktop 查过：只有第一个条目能读到，
    // 而且是被当成 `statictext` 而不是可点击的按钮。
    //
    // 手绘控件必须自己往 accesskit 里登记角色、名字与选中态。
    let role = egui::accesskit::Role::Button;
    response.ctx.accesskit_node_builder(response.id, |node| {
        node.set_role(role);
        node.set_label(label);
        node.set_description(hint);
        if selected {
            // "当前在哪一屏"对读屏用户是必要信息。
            node.set_selected(true);
        }
        if response.enabled() {
            node.add_action(egui::accesskit::Action::Click);
        }
    });

    let tint = if selected { t.accent_ink } else { t.icon };
    let icon_size = 17.0;
    if fold {
        // 折叠：图标居中。
        icons::paint(ui, rect, icon, icon_size, tint);
        return response.on_hover_text(format!("{label}\n{hint}"));
    }

    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0 + icon_size / 2.0, rect.center().y), Vec2::splat(icon_size));
    icons::paint(ui, icon_rect, icon, icon_size, tint);
    ui.painter().text(
        egui::pos2(icon_rect.right() + space::M, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(font::BODY, if selected { fonts::medium() } else { fonts::body() }),
        if selected { t.accent_ink } else { t.text },
    );
    response.on_hover_text(hint)
}

/// 侧栏底部的一个小操作行（P4 会用到；先留在这里保持结构完整）。
pub fn footer_row(ui: &mut Ui, icon: &str, label: &str) -> egui::Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_H), Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(t.radius_sm as u8), t.hover);
    }
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0 + 8.5, rect.center().y), Vec2::splat(17.0));
    icons::paint(ui, icon_rect, icon, 17.0, t.icon);
    ui.painter().text(
        egui::pos2(icon_rect.right() + space::M, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::new(font::BODY, fonts::body()),
        t.text,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::Tokens;
    use metis_paths::Paths;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    fn ctx_with_width(width: f32) -> egui::Context {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |_ui| {});
        output.textures_delta.clear();
        ctx
    }

    #[test]
    fn every_screen_has_an_icon_and_a_hint() {
        for screen in Screen::ALL {
            let icon = screen_icon(screen);
            assert!(icons::exists(icon), "{screen:?} 的图标 {icon} 不存在");
            assert!(!screen_hint(screen).is_empty(), "{screen:?} 缺少说明");
        }
    }

    #[test]
    fn a_narrow_window_folds_the_sidebar() {
        let (_dir, app) = app();
        let narrow = ctx_with_width(700.0);
        let wide = ctx_with_width(1400.0);
        assert!(folded(&app, &narrow), "窄窗口必须自动折叠");
        assert!(!folded(&app, &wide), "宽窗口不该折叠");
        assert_eq!(width(&app, &narrow), FOLDED);
        assert_eq!(width(&app, &wide), WIDTH);
    }

    #[test]
    fn the_manual_fold_wins_over_the_window_width() {
        let (_dir, mut app) = app();
        let wide = ctx_with_width(1400.0);
        app.ui.sidebar_folded = true;
        assert!(folded(&app, &wide), "用户手动折了就该折");
        assert_eq!(width(&app, &wide), FOLDED);
    }

    #[test]
    fn the_sidebar_renders_in_both_states() {
        for fold in [false, true] {
            let (_dir, mut app) = app();
            app.ui.sidebar_folded = fold;
            let ctx = ctx_with_width(1400.0);
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(260.0, 800.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| {
                egui::Panel::left("nav").exact_size(width(&app, ui.ctx())).show(ui, |ui| show(&mut app, ui));
            });
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "fold={fold} 时侧栏没渲染");
        }
    }

    #[test]
    fn the_sidebar_highlights_the_active_screen() {
        // 四个屏幕各渲染一次：选中态分支都要走到，且不能 panic。
        for screen in Screen::ALL {
            let (_dir, mut app) = app();
            app.set_screen(screen);
            let ctx = ctx_with_width(1400.0);
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(260.0, 800.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| {
                egui::Panel::left("nav").exact_size(WIDTH).show(ui, |ui| show(&mut app, ui));
            });
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "{screen:?} 的侧栏没渲染");
        }
    }

    #[test]
    fn footer_row_renders() {
        let ctx = ctx_with_width(400.0);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(240.0, 100.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            footer_row(ui, "settings", "设置");
        });
        output.textures_delta.clear();
        assert!(!output.shapes.is_empty());
    }
}
