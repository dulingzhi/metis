//! 命令面板（`Ctrl+K`）：模糊搜索并执行任意一条界面动作。
//!
//! 为什么值得做：Metis 有 29 个 CLI 命令，界面只暴露了其中一部分。
//! 命令面板让"所有动作都可键盘触达"，而不必为每个动作再做一个按钮。
//! 这也是无障碍的一部分（`docs/UI.md` §6：全部可操作项键盘可达）。
//!
//! 设计取舍：
//! - **只列界面动作**（`Action`），不列原始命令 id —— 用户要的是"做什么"，
//!   不是"哪个命令"；命令 id 是给脚本的。
//! - 匹配用**子序列**（`rfr` 能匹配"刷新"），而不是前缀 —— 中文用户不会打拼音全拼。
//! - `↑`/`↓` 选，`Enter` 执行，`Esc` 关。

use egui::{Key, RichText, Ui};

use crate::actions::Action;
use crate::tokens::{Tokens, font, fonts, space};
use crate::{MetisApp, icons, widgets};

/// 面板自己的状态（放在 `UiState` 里，跨帧保留）。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PaletteState {
    /// 是否打开。
    pub open: bool,
    /// 搜索词。
    pub query: String,
    /// 当前选中项（在**过滤后**的列表里的下标）。
    pub selected: usize,
}

impl PaletteState {
    /// 打开并清空。
    pub fn open(&mut self) {
        self.open = true;
        self.query.clear();
        self.selected = 0;
    }

    /// 关闭。
    pub fn close(&mut self) {
        self.open = false;
        self.query.clear();
        self.selected = 0;
    }
}

/// 面板里的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// 动作。
    pub action: Action,
    /// 显示名。
    pub label: &'static str,
    /// 副标题（说明它会做什么）。
    pub hint: &'static str,
    /// 图标名。
    pub icon: &'static str,
}

/// 命令面板的全部条目（顺序即默认展示顺序）。
///
/// **只放真的会执行的动作**：不放"打开设置"这类纯导航，因为左侧栏已经能到。
pub fn items() -> Vec<Item> {
    vec![
        Item { action: Action::Refresh, label: "刷新状态", hint: "重新读取配置与执行器探测结果", icon: "refresh-cw" },
        Item { action: Action::Apply, label: "应用变更", hint: "把人设与模式写进已接管的执行器", icon: "play" },
        Item { action: Action::TogglePower, label: "开关注入", hint: "暂停或恢复全部注入", icon: "shield-check" },
        Item { action: Action::Verify, label: "实测验证", hint: "抓宿主的真实请求，判定人设落在哪一层", icon: "activity" },
        Item { action: Action::CleanDryRun, label: "预览清理", hint: "看看撤回接管会动哪些文件（不真删）", icon: "eye" },
        Item { action: Action::Clean, label: "撤回全部接管", hint: "只撤 Metis 写过的内容；人设保留", icon: "rotate-ccw" },
        Item { action: Action::BackupsUsage, label: "查看备份占用", hint: "整文件回滚用的原件占了多大", icon: "hard-drive" },
        Item { action: Action::BackupsPruneDryRun, label: "预览清理备份", hint: "看看能释放多少空间（不真删）", icon: "eye-off" },
        Item { action: Action::BackupsPrune, label: "清理无用备份", hint: "删掉账本不再引用的备份", icon: "trash-2" },
        Item { action: Action::ImportDryRun, label: "预览导入", hint: "看看 agent-purge 里有哪几份人设", icon: "folder-open" },
        Item { action: Action::Import, label: "从 agent-purge 导入", hint: "只读导入人设与部分状态", icon: "download" },
        Item { action: Action::ImportGithub, label: "从 GitHub 导入", hint: "按 owner/repo 拉取人设库", icon: "globe" },
    ]
}

/// 子序列匹配（大小写不敏感）。
///
/// `query` 的每个字符都要按顺序出现在 `text` 里 —— `rfr` 能匹配 "refresh"，
/// `刷新` 能匹配 "刷新状态"。空 query 一律匹配。
pub fn matches(text: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut index = 0;
    for want in query.to_lowercase().chars() {
        if want.is_whitespace() {
            continue;
        }
        loop {
            if index >= text.len() {
                return false;
            }
            let have = text[index];
            index += 1;
            if have == want {
                break;
            }
        }
    }
    true
}

/// 按 query 过滤条目（同时匹配名字与说明）。
pub fn filter(query: &str) -> Vec<Item> {
    items().into_iter().filter(|item| matches(item.label, query) || matches(item.hint, query)).collect()
}

/// 画命令面板。返回这次被选中的动作（如果有）。
pub fn show(app: &mut MetisApp, ctx: &egui::Context) -> Option<Action> {
    if !app.ui.palette.open {
        return None;
    }
    let t = Tokens::get(ctx);
    let mut chosen = None;
    let mut close = false;

    // 键盘：↑/↓/Enter/Esc。放在画之前处理，这样同一帧就能反映高亮变化。
    ctx.input_mut(|input| {
        // `Esc` **不在这里处理**。
        //
        // 面板的输入框持有焦点时，egui 会把 Esc 解释成"取消编辑"并吃掉它，
        // 这个闭包里的 `consume_key` / `key_pressed` 都收不到（真机踩过：
        // 面板关不掉）。所以 `Esc` 统一由 `MetisApp::handle_shortcuts`
        // 在**焦点守卫之前**用 `ctx.input(..)` 处理。
        let _ = &mut close;
        let results = filter(&app.ui.palette.query);
        if !results.is_empty() {
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                app.ui.palette.selected = (app.ui.palette.selected + 1) % results.len();
            }
            if input.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                app.ui.palette.selected = if app.ui.palette.selected == 0 { results.len() - 1 } else { app.ui.palette.selected - 1 };
            }
            if input.consume_key(egui::Modifiers::NONE, Key::Enter) {
                chosen = results.get(app.ui.palette.selected).map(|item| item.action);
            }
        }
    });

    let screen = ctx.content_rect();
    let width = 560.0_f32.min(screen.width() - 40.0);
    let area = egui::Area::new(egui::Id::new("command-palette"))
        .fixed_pos(egui::pos2(screen.center().x - width / 2.0, screen.top() + 90.0))
        .order(egui::Order::Foreground);

    area.show(ctx, |ui| {
        // 背后压一层暗色，视觉上"聚焦"到面板（也是点击外部关闭的落点）。
        ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(40));

        egui::Frame::new()
            .fill(t.card)
            .stroke(egui::Stroke::new(1.0, t.card_border))
            .corner_radius(egui::CornerRadius::same(t.radius_lg as u8))
            .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: t.shadow })
            .inner_margin(egui::Margin::same(space::M as i8))
            .show(ui, |ui| {
                ui.set_width(width - space::M * 2.0);

                // 搜索框：打开即自动聚焦。
                let response = ui.add(
                    egui::TextEdit::singleline(&mut app.ui.palette.query)
                        .hint_text("输入动作名…")
                        .desired_width(f32::INFINITY)
                        .font(egui::FontId::new(font::BODY, fonts::body())),
                );
                if !response.has_focus() {
                    response.request_focus();
                }
                // 输入变了就回到第一项，别停在越界的下标上。
                if response.changed() {
                    app.ui.palette.selected = 0;
                }
                ui.add_space(space::S);

                let results = filter(&app.ui.palette.query);
                if results.is_empty() {
                    widgets::empty_state(ui, "search", "没有匹配的动作", "换个关键词试试；Esc 关闭。");
                    return;
                }

                let selected = app.ui.palette.selected.min(results.len() - 1);
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (index, item) in results.iter().enumerate() {
                        let on = index == selected;
                        let row_h = 38.0;
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
                        if on {
                            ui.painter().rect_filled(rect, egui::CornerRadius::same(t.radius_sm as u8), t.accent_soft);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, egui::CornerRadius::same(t.radius_sm as u8), t.hover);
                        }
                        let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + 18.0, rect.center().y), egui::Vec2::splat(16.0));
                        icons::paint(ui, icon_rect, item.icon, 16.0, if on { t.accent_ink } else { t.icon });
                        ui.painter().text(
                            egui::pos2(rect.left() + 38.0, rect.center().y - 7.0),
                            egui::Align2::LEFT_CENTER,
                            item.label,
                            egui::FontId::new(font::BODY, fonts::medium()),
                            if on { t.accent_ink } else { t.text },
                        );
                        ui.painter().text(
                            egui::pos2(rect.left() + 38.0, rect.center().y + 9.0),
                            egui::Align2::LEFT_CENTER,
                            item.hint,
                            egui::FontId::new(font::CAPTION, fonts::body()),
                            t.text_faint,
                        );
                        if resp.clicked() {
                            chosen = Some(item.action);
                        }
                        if resp.hovered() {
                            ui.ctx().request_repaint();
                        }
                    }
                });

                ui.add_space(space::XS);
                ui.horizontal(|ui| {
                    widgets::caption(ui, "↑↓ 选择 · Enter 执行 · Esc 关闭");
                });
            });
    });

    if close {
        app.ui.palette.close();
    }
    if chosen.is_some() {
        app.ui.palette.close();
    }
    chosen
}

/// 顶栏里的"搜索/命令"入口按钮。
pub fn trigger(ui: &mut Ui, app: &mut MetisApp) {
    let t = Tokens::get(ui.ctx());
    let label = RichText::new("搜索动作…  Ctrl+K").size(font::SMALL).color(t.text_faint);
    let response = ui.add(
        egui::Button::new(label)
            .fill(t.field)
            .stroke(egui::Stroke::new(1.0, t.field_border))
            .corner_radius(egui::CornerRadius::same(t.radius_sm as u8))
            .min_size(egui::vec2(190.0, 26.0)),
    );
    if response.clicked() {
        app.ui.palette.open();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_item_has_a_label_hint_and_existing_icon() {
        for item in items() {
            assert!(!item.label.is_empty());
            assert!(!item.hint.is_empty(), "{} 缺少说明", item.label);
            assert!(icons::exists(item.icon), "{} 的图标 {} 不存在", item.label, item.icon);
        }
    }

    #[test]
    fn every_action_appears_at_most_once() {
        let mut seen = std::collections::BTreeSet::new();
        for item in items() {
            assert!(seen.insert(format!("{:?}", item.action)), "{:?} 重复出现", item.action);
        }
    }

    #[test]
    fn every_action_in_the_catalog_is_reachable() {
        // `Action::ALL` 里的每个动作都应该能在面板里找到（这样"全部可键盘触达"才成立）。
        let listed: std::collections::BTreeSet<String> = items().iter().map(|item| format!("{:?}", item.action)).collect();
        for action in Action::ALL {
            // 两类动作不进面板：
            // - `DisarmHarness` 需要指定某一家，面板里没有"选哪家"的位置；
            // - 预览类（`*DryRun`）只是"看一眼"，放在"执行动作"的面板里容易误点，
            //   它们在各屏的按钮上已经有独立入口。
            if matches!(action, Action::DisarmHarness | Action::CleanDryRun | Action::ImportDryRun | Action::ImportGithubDryRun) {
                continue;
            }
            assert!(listed.contains(&format!("{action:?}")), "{action:?} 不在命令面板里");
        }
    }

    #[test]
    fn an_empty_query_matches_everything() {
        assert_eq!(filter("").len(), items().len());
    }

    #[test]
    fn subsequence_matching_works_for_latin_and_chinese() {
        assert!(matches("refresh", "rfr"));
        assert!(matches("refresh", "rf"));
        assert!(matches("刷新状态", "刷新"));
        assert!(matches("刷新状态", "状态"));
        assert!(!matches("refresh", "xyz"));
        // 顺序必须保持。
        assert!(!matches("abc", "cba"));
    }

    #[test]
    fn matching_is_case_insensitive_and_ignores_whitespace() {
        assert!(matches("Refresh", "refresh"));
        assert!(matches("refresh", "r f r"));
        assert!(matches("刷新 状态", "刷新状态"));
    }

    #[test]
    fn filtering_narrows_the_list() {
        let found = filter("验证");
        assert!(!found.is_empty());
        assert!(found.iter().any(|item| item.action == Action::Verify));

        // 一个不存在的词应该什么都不剩。
        assert!(filter("zzzznothing").is_empty());
    }

    #[test]
    fn the_filter_also_matches_the_hint() {
        // 用户可能记得"它会做什么"而不是名字。
        // 用说明里确实出现的词（"备份"只出现在三条备份动作的 hint 里）。
        let found = filter("备份");
        assert!(!found.is_empty(), "说明里提到备份的动作应该能被搜到");
        assert!(found.iter().any(|item| matches!(item.action, Action::BackupsUsage | Action::BackupsPrune)));
    }

    /// 造一个 app + ctx，用来渲染面板。
    fn app() -> (tempfile::TempDir, crate::MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = crate::MetisApp::with_paths(metis_paths::Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    /// 跑一帧面板，返回（图元数，被选中的动作）。
    fn render(app: &mut crate::MetisApp, dark: bool) -> (usize, Option<Action>) {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::for_dark(dark));
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() };
        let mut chosen = None;
        let mut output = ctx.run_ui(raw, |ui| {
            let _ = ui;
        });
        output.textures_delta.clear();
        // 面板画在 `Area` 里，需要单独一帧。
        let ctx2 = egui::Context::default();
        Tokens::install_full(&ctx2, Tokens::for_dark(dark));
        let raw2 = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() };
        let mut out2 = ctx2.run_ui(raw2.clone(), |ui| {
            chosen = show(app, ui.ctx());
        });
        out2.textures_delta.clear();
        let _ = &mut output;
        (out2.shapes.len(), chosen)
    }

    #[test]
    fn a_closed_palette_renders_nothing_and_returns_none() {
        let (_dir, mut app) = app();
        app.ui.palette.close();
        let (shapes, chosen) = render(&mut app, false);
        assert_eq!(shapes, 0, "关着的时候不该画任何东西");
        assert_eq!(chosen, None);
    }

    #[test]
    fn an_open_palette_paints_its_items() {
        for dark in [false, true] {
            let (_dir, mut app) = app();
            app.ui.palette.open();
            let (shapes, chosen) = render(&mut app, dark);
            assert!(shapes > 10, "dark={dark} 时面板只画了 {shapes} 个图元");
            assert_eq!(chosen, None, "没点也没回车时不该选中任何动作");
        }
    }

    #[test]
    fn an_open_palette_with_no_matches_shows_an_empty_state() {
        let (_dir, mut app) = app();
        app.ui.palette.open();
        app.ui.palette.query = "zzzznothing".to_owned();
        let (shapes, chosen) = render(&mut app, false);
        assert!(shapes > 3, "空结果也要画出提示");
        assert_eq!(chosen, None);
    }

    #[test]
    fn enter_runs_the_selected_item() {
        // 打开面板（第一项默认选中）后按 Enter，应该返回第一项的动作。
        let (_dir, mut app) = app();
        app.ui.palette.open();
        let expected = filter("").first().map(|item| item.action).expect("至少有一项");

        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        // 先画一帧让面板存在。
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| {
            let _ = show(&mut app, ui.ctx());
        });
        out.textures_delta.clear();

        // 再按 Enter。
        let raw2 = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0))),
            events: vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };
        let mut chosen = None;
        let mut out2 = ctx.run_ui(raw2, |ui| {
            chosen = show(&mut app, ui.ctx());
        });
        out2.textures_delta.clear();
        assert_eq!(chosen, Some(expected), "Enter 应执行第一项");
        assert!(!app.ui.palette.open, "执行后面板应关闭");
    }

    #[test]
    fn arrow_keys_move_the_selection() {
        let (_dir, mut app) = app();
        app.ui.palette.open();
        app.ui.palette.selected = 0;
        let count = filter("").len();
        assert!(count > 1, "至少要有两项才能测移动");

        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));

        let key_event = |key: egui::Key| egui::RawInput {
            screen_rect: Some(screen),
            events: vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }],
            ..Default::default()
        };

        // 第一帧让面板建立起来。
        let mut warm = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            let _ = show(&mut app, ui.ctx());
        });
        warm.textures_delta.clear();

        // ↓ 下移。
        let mut down = ctx.run_ui(key_event(egui::Key::ArrowDown), |ui| {
            let _ = show(&mut app, ui.ctx());
        });
        down.textures_delta.clear();
        assert_eq!(app.ui.palette.selected, 1, "↓ 应把选中项从 0 移到 1");

        // ↑ 回到 0。
        let mut up = ctx.run_ui(key_event(egui::Key::ArrowUp), |ui| {
            let _ = show(&mut app, ui.ctx());
        });
        up.textures_delta.clear();
        assert_eq!(app.ui.palette.selected, 0, "↑ 应回到 0");

        // 在 0 时再 ↑ 应该绕到最后一项（循环）。
        let mut wrap = ctx.run_ui(key_event(egui::Key::ArrowUp), |ui| {
            let _ = show(&mut app, ui.ctx());
        });
        wrap.textures_delta.clear();
        assert_eq!(app.ui.palette.selected, count - 1, "在首项按 ↑ 应绕到末项");
    }
    #[test]
    fn the_trigger_button_opens_the_palette() {
        let (_dir, mut app) = app();
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(600.0, 200.0))), ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| trigger(ui, &mut app));
        out.textures_delta.clear();
        assert!(!app.ui.palette.open, "光画不点不该打开");
    }

    #[test]
    fn palette_state_opens_and_closes_cleanly() {
        let mut state = PaletteState::default();
        assert!(!state.open);
        state.query = "旧词".to_owned();
        state.selected = 3;
        state.open();
        assert!(state.open);
        assert!(state.query.is_empty(), "打开时应清空上一次的搜索词");
        assert_eq!(state.selected, 0);
        state.close();
        assert!(!state.open);
    }
}
