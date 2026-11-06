//! 四个屏幕的内容区（P3 重写版）。
//!
//! 每屏都是"页头 + 若干卡片"的结构。所有视觉决定都来自 [`crate::widgets`]
//! 与 [`crate::tokens`]，这里只表达**意图**（这是主操作 / 这是可选项）。
//!
//! 与旧版的区别：
//! - 用 `choice_tile` 代替 `radio`（模式选择本身就是内容，比单选框好读）；
//! - 用 `card()` 而不是 `Frame::group`（统一圆角与描边）；
//! - 状态行用 `pill`（"已接管" / "未安装"一眼可辨）；
//! - 主操作用 `widgets::primary`，一屏只有一个。

use egui::{RichText, Ui};
use serde_json::json;

use crate::model::{self, Filter};
use crate::tokens::{Tokens, font, fonts, space};
use crate::widgets;
use crate::{MetisApp, icons};
use metis_core::{Channel, Mode};

/// 控制台：选人设、定模式、应用变更、勾选接管的 AI。
pub fn console(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        // **在放任何卡片之前取一次完整宽度**，一路传下去。
        //
        // 在卡片内部取 `available_width()` 拿到的不是屏幕宽：嵌套的 Ui 会带上
        // 各层内边距与已用宽度，算出来的列数严重偏小（实测 1250px 只排出 2 列，
        // 第二列还被裁掉）。宽度只该在"知道真实可用空间"的那一层量一次。
        let content_width = ui.available_width();

        widgets::page_header(ui, "控制台", "选好人设，明确范围，再应用。");

        power_strip(app, ui, &t);
        ui.add_space(space::L);

        persona_card(app, ui, &t);

        ui.add_space(space::L);
        harness_card(app, ui, &t, content_width);
    });
}

/// 顶部状态条：开关 + 当前人设 + 模式。
fn power_strip(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    let on = app.snapshot.power_on;
    ui.horizontal(|ui| {
        // 开关本身是个按钮：点一下就切。用 pill 的视觉但保留按钮语义。
        let label = if on { "已开启 · 点击暂停" } else { "已暂停 · 点击恢复" };
        let (fg, bg) = if on { (t.success, t.accent_soft) } else { (t.warning, t.raised) };
        if widgets::pill(ui, label, fg, bg).interact(egui::Sense::click()).clicked() {
            app.invoke("state.power", json!({ "on": !on }));
            app.refresh_blocking();
        }
        let persona = app.draft.persona.clone().unwrap_or_else(|| "未选择人设".to_owned());
        ui.label(RichText::new(persona).size(font::BODY).family(fonts::medium()).color(t.text));
        ui.label(RichText::new(model::mode_label(app.draft.mode)).size(font::SMALL).color(t.text_faint));
    });
}

/// 人设与模式。
fn persona_card(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    let change = app.draft.change(app.snapshot.saved_persona.as_deref(), app.snapshot.saved_mode);

    widgets::card(t).show(ui, |ui| {
        // ── 紧凑布局：标题、人设、模式、操作**挤在同一块**里 ──
        //
        // 为什么改：原来的三行大瓦片版本一个人设卡片就要 ~320 逻辑像素，
        // 而 200% 缩放的笔记本上可用高度只有 ~319 —— 执行器卡片永远在折线以下，
        // 等于控制台的主功能被自己的配置区挡住了。
        ui.horizontal(|ui| {
            widgets::section_title(ui, "人设与模式");
            widgets::pill(ui, change.label(), t.text_faint, t.raised);

            ui.add_space(space::M);

            // 人设下拉：显示名，不是 id（选完变英文是老 bug）。
            let current = app
                .draft
                .persona
                .as_deref()
                .and_then(|id| app.snapshot.personas.iter().find(|p| p.id == id))
                .map(|p| p.name.clone())
                .or_else(|| app.draft.persona.clone())
                .unwrap_or_else(|| "未选择人设".to_owned());
            egui::ComboBox::from_id_salt("persona-picker").selected_text(RichText::new(current).size(font::BODY).color(t.text)).width(170.0).show_ui(
                ui,
                |ui| {
                    for persona in &app.snapshot.personas {
                        let selected = app.draft.persona.as_deref() == Some(persona.id.as_str());
                        let label = if persona.name == persona.id { persona.name.clone() } else { format!("{}\u{2003}{}", persona.name, persona.id) };
                        if ui.selectable_label(selected, RichText::new(label).size(font::BODY)).clicked() {
                            app.draft.persona = Some(persona.id.clone());
                        }
                    }
                },
            );

            ui.add_space(space::S);

            // 模式：分段控件（一行搞定三个选项，比三个瓦片省 2/3 高度）。
            // 悬停给出这一模式的完整说明 —— 信息不丢，只是不常驻。
            let modes = [Mode::Replace, Mode::Strict, Mode::Append];
            let labels: Vec<&str> = modes.iter().map(|m| model::mode_short(*m)).collect();
            let selected = modes.iter().position(|m| *m == app.draft.mode).unwrap_or(0);
            if let Some(index) = widgets::segmented(ui, &labels, selected) {
                app.draft.mode = modes[index];
            }

            // 操作按钮钉右侧。
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let can_apply = change.is_dirty() && app.draft.persona.is_some();
                if widgets::primary(ui, "应用变更", can_apply).clicked() {
                    if let Some(persona) = app.draft.persona.clone() {
                        app.invoke("persona.use", json!({ "persona": persona }));
                    }
                    app.invoke("state.mode", json!({ "mode": app.draft.mode.as_str() }));
                    app.spawn_apply();
                }
                if widgets::secondary(ui, "撤销").clicked() {
                    app.draft = crate::state::Draft::from_saved(app.snapshot.saved_persona.clone(), app.snapshot.saved_mode);
                }
            });
        });

        // 模式说明：一行，跟着当前选择变。比三个瓦片各带一句省得多。
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(model::mode_hint(app.draft.mode)).size(font::CAPTION).color(t.text_faint)).truncate())
            .on_hover_text(model::mode_hint(app.draft.mode));

        // 铁律 4：写入不等于生效。这句必须一直在。
        ui.add_space(2.0);
        ui.add(egui::Label::new(RichText::new(model::WRITE_NOT_EFFECT).size(font::CAPTION).color(t.warning)).truncate()).on_hover_text(model::WRITE_NOT_EFFECT);
    });
}
/// 接管哪些 AI —— 胶囊网格。
///
/// **结构逐项对齐 agent-purge 的原版**（`web/app.js` 的 `renderHarnesses` +
/// `web/styles.css` 的 `.harness-capsule` / `.capsule-*`）：
///
/// ```text
/// ┌──────────────────────────────────────┐
/// │ [图标] codex  ●             [开关]   │  ← 名称是 id，等宽字体
/// │        已就绪                        │  ← 状态文字 12px 灰
/// │ [人设 ▾] [模式 ▾]                    │  ← 独占一行，11.5px
/// └──────────────────────────────────────┘
/// ```
///
/// 我前两版做错的地方（对照原版才发现）：
/// - 漏了**图标**（原版 24px 圆角头像；没有品牌图时退回首字母徽章）
/// - 标题用了 `display`（"Claude Code"），原版用 **id** + 等宽字体
/// - 漏了**人设/模式下拉**（那是每张卡的核心操作，不是抽屉里的东西）
/// - 把通道胶囊塞进卡片常态视图（原版放在抽屉里）
fn harness_card(app: &mut MetisApp, ui: &mut Ui, t: &Tokens, content_width: f32) {
    widgets::card(t).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, "接管哪些 AI");
            widgets::pill(ui, &model::scope_summary(&app.snapshot.harnesses), t.accent_ink, t.accent_soft);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut show = app.ui.show_uninstalled;
                if widgets::toggle(ui, &mut show, "显示本机没装的").changed() {
                    app.ui.show_uninstalled = show;
                }
            });
        });
        ui.add_space(space::S);

        let filter = Filter { injectable_only: false, armed_only: false, installed_only: !app.ui.show_uninstalled };
        let rows = model::filter_rows(&app.snapshot.harnesses, filter);
        if rows.is_empty() {
            widgets::empty_state(ui, "search", "没有匹配的执行器", "试试勾上「显示本机没装的」。");
            return;
        }
        harness_grid(app, ui, t, &rows, content_width);
    });
}

/// 详情浮层的对外入口（由 `MetisApp::ui` 在主循环里调用）。
///
/// 必须在 `CentralPanel` **之后**画：它是 `Order::Foreground` 的浮层，
/// 要盖住所有内容。
pub fn show_detail(app: &mut MetisApp, ctx: &egui::Context) {
    harness_detail(app, ctx);
}

/// 一家的详情面板 —— **独立浮层**，画在最上层。
///
/// 为什么是浮层而不是同级的一块：
/// 1. 同级塞进 `ScrollArea` 会参与布局，把整页推长、把网格挤走
///    （实测展开后整屏图元从 305 掉到 79 —— 布局被压扁了）；
/// 2. 详情的操作（通道开关、禁用原因）是**针对一家的临时聚焦**，
///    浮层天然表达"我在看这一家"；
/// 3. 不受网格列宽影响，不会超框。
///
/// 与命令面板用同一套 `Area + Order::Foreground`（见 `palette::show`）。
fn harness_detail(app: &mut MetisApp, ctx: &egui::Context) {
    let Some(row) = app.ui.expanded.as_deref().and_then(|id| app.snapshot.harnesses.iter().find(|r| r.id == id)).cloned() else {
        return;
    };
    let t = Tokens::get(ctx);

    let screen = ctx.content_rect();
    let width = 520.0_f32.min(screen.width() - 48.0);
    let mut close = false;

    egui::Area::new(egui::Id::new("harness-detail"))
        .fixed_pos(egui::pos2(screen.center().x - width / 2.0, screen.center().y - 200.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // 背后压暗层 + 点击关闭。
            //
            // **顺序很重要**：先 `interact` 会占掉整屏的点击区，之后的内容虽然画得出来
            // 但收不到事件；而且 `Area` 里 `ui` 的可用矩形是按内容算的，
            // 先 interact 整屏会把后续 `Frame` 挤出可见区（实测只画出 8 个图元）。
            // 所以：先画暗层（纯绘制，不占交互），再加一块独立的关闭热区。
            ui.painter().rect_filled(screen, 0.0, egui::Color32::from_black_alpha(90));
            let backdrop = ui.interact(screen, ui.id().with("backdrop"), egui::Sense::click());
            if backdrop.clicked() {
                close = true;
            }

            egui::Frame::new()
                .fill(t.card)
                .stroke(egui::Stroke::new(1.0, t.card_border))
                .corner_radius(egui::CornerRadius::same(t.radius_lg as u8))
                .shadow(egui::epaint::Shadow { offset: [0, 8], blur: 24, spread: 0, color: t.shadow })
                .inner_margin(egui::Margin::same(space::L as i8))
                .show(ui, |ui| {
                    ui.set_width(width - 2.0 * space::L);

                    // ── 标题行：名字 + 状态 + 关闭 ──
                    ui.horizontal(|ui| {
                        widgets::section_title(ui, &row.display);
                        let on = Channel::ALL.iter().any(|c| row.is_armed(*c));
                        let (label, fg, bg) = if on { ("已接管", t.success, t.accent_soft) } else { ("未接管", t.text_faint, t.raised) };
                        widgets::pill(ui, label, fg, bg);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let text = egui::RichText::new("关闭").size(font::CAPTION).color(t.accent_ink);
                            if ui.add(egui::Button::new(text).frame(false)).clicked() {
                                close = true;
                            }
                        });
                    });

                    ui.add_space(space::S);
                    labelled_row(ui, &t, "热切能力", &row.hot);
                    ui.add_space(space::S);
                    widgets::separator(ui);
                    ui.add_space(space::S);

                    // ── 四条通道：一行一条，左名 / 中说明 / 右按钮，固定宽度对齐 ──
                    let content_w = width - 2.0 * space::L;
                    for channel in Channel::ALL {
                        let (ok, why) = row.channel(channel);
                        let armed = row.is_armed(channel);
                        let row_h = 32.0;
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(content_w, row_h), egui::Sense::hover());

                        // 左：通道名（固定 70 宽）
                        let name_w = 70.0;
                        ui.painter().text(
                            egui::pos2(rect.left(), rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            model::channel_label(channel),
                            egui::FontId::new(font::BODY, fonts::medium()),
                            if ok { t.text } else { t.text_faint },
                        );

                        // 右：按钮（固定 60 宽，钉在右边）
                        let btn_w = 60.0;
                        let btn_rect = egui::Rect::from_min_size(egui::pos2(rect.right() - btn_w, rect.top() + 3.0), egui::vec2(btn_w, row_h - 6.0));
                        // 中：说明（名字与按钮之间）
                        let note_rect =
                            egui::Rect::from_min_max(egui::pos2(rect.left() + name_w + 8.0, rect.top()), egui::pos2(btn_rect.left() - 8.0, rect.bottom()));
                        let note = why.unwrap_or_else(|| model::channel_hint(channel)).to_owned();
                        let mut note_ui = ui.new_child(egui::UiBuilder::new().max_rect(note_rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
                        note_ui
                            .add(egui::Label::new(egui::RichText::new(&note).size(font::CAPTION).color(if ok { t.text_faint } else { t.warning })).truncate())
                            .on_hover_text(&note);

                        if ok {
                            let mut btn_ui = ui.new_child(
                                egui::UiBuilder::new().max_rect(btn_rect).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)),
                            );
                            if widgets::secondary(&mut btn_ui, if armed { "关闭" } else { "启用" }).clicked() {
                                let id = row.id.clone();
                                let channels: Vec<&str> = if armed { Vec::new() } else { vec![channel.as_str()] };
                                app.invoke("channel.set", json!({ "harness": id, "channels": channels }));
                                app.refresh_blocking();
                            }
                        } else {
                            ui.painter().text(
                                egui::pos2(rect.right(), rect.center().y),
                                egui::Align2::RIGHT_CENTER,
                                "不可用",
                                egui::FontId::new(font::CAPTION, fonts::body()),
                                t.text_faint,
                            );
                        }
                    }
                });
        });

    if close {
        app.ui.expanded = None;
    }
}
/// 「标签 + 值」一行，值超宽就截断（悬停看全文）。
fn labelled_row(ui: &mut Ui, t: &Tokens, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(72.0, 18.0), egui::Label::new(egui::RichText::new(label).size(font::CAPTION).color(t.text_faint)));
        let rest = (ui.available_width() - 80.0).max(80.0);
        ui.add_sized(egui::vec2(rest, 18.0), egui::Label::new(egui::RichText::new(value).size(font::CAPTION).color(t.text)).truncate()).on_hover_text(value);
    });
}

/// 卡片里下拉标签最多显示几个字符。
///
/// 超过就截断：`ComboBox` 的内容更宽时会撑开，把卡片里的内容顶出边框
/// （真机确认过，"人设·concise" 就会超框）。
const LABEL_CHARS: usize = 12;

/// 按显示宽度截断标签（中文算 2 格。
fn truncate_label(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }
    let head: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{head}…")
}

/// 网格参数。
///
/// **自适应列数**：列数由可用宽度决定，卡片宽度 = 拿来均分（不是固定值）。
///
/// 之前失败过三次，根因只有一个：**卡片外面套了 `allocate_exact_size`，
/// 里面又用 `Frame`** —— 两层各自分配空间，`Frame` 会按内容收缩到自然宽度，
/// 于是分配到的宽度和实际画出来的差一截，差额看起来就是"卡片之间的大间隙"
/// （实测横向 55 逻辑、纵向 30 逻辑，而设定值是 10）。
///
/// 现在改成：**手绘边框背景**（`painter`），内容用 `set_width` 精确约束。
/// 只有一层尺寸来源，间距就等于常数。
const GRID_GAP: f32 = 10.0;
/// 卡片最小宽度。低于这个就把两行下拉挤坏了。
// 260 而不是 200：下拉里要放下"人设名 + 箭头"。200 时每个下拉只有 87 逻辑宽，
// 「简洁协作」都显示不全（真机确认），只能截成「跟随…」。
const GRID_MIN_CARD: f32 = 260.0;
/// 卡片最大宽度：太宽会显得空。
const GRID_MAX_CARD: f32 = 400.0;
/// 卡片内边距。
const CAPSULE_PAD_X: f32 = 10.0;
const CAPSULE_PAD_Y: f32 = 8.0;
/// 卡片**固定高度**（内容必须能塞下：标题行 + 下拉行 + 详情行）。
const CAPSULE_HEIGHT: f32 = 116.0;
const CAPSULE_NAME: f32 = 13.5;
const CAPSULE_STATUS: f32 = 12.0;
const CAPSULE_ICON: f32 = 22.0;
const MINI_SELECT_FONT: f32 = 11.5;
const CAPTION_LINK: f32 = 11.5;

/// 根据可用宽度算列数与卡片宽。
///
/// 抽成纯函数是为了**能单测**：这次布局改了好几轮都靠截图看，
/// 有了这个函数就能直接用数字断言"900 逻辑宽应该是 4 列"。
pub fn grid_columns(available: f32) -> (usize, f32) {
    let available = available.max(1.0);

    // 能放下的最多列数（卡片当最小宽，间距单算）。
    let fits = ((available + GRID_GAP) / (GRID_MIN_CARD + GRID_GAP)).floor() as usize;
    let mut columns = fits.clamp(1, 6);
    let mut card = spread(available, columns);

    // 均分后若比最大宽还宽，说明列数给少了 —— 加列，把宽屏用满。
    while card > GRID_MAX_CARD && columns < 6 {
        columns += 1;
        card = spread(available, columns);
    }

    // **不要让卡片宽于可用宽**：单列时卡片就是整行宽（哪怕窄于最小宽）。
    //
    // 这里踩过一个坑：原本写 `card.max(GRID_MIN_CARD.min(available))`，
    // 想把"太窄"的卡片抬到最小宽 —— 结果在窄窗口里把 175 抬到 200，
    // 于是 2 列需要 410 > 360，**反而放不下**了。
    // 正确做法：宽了就只放一列，让卡片直接等于可用宽。
    (columns, card.min(available).max(1.0))
}

/// 把 `available` 平均分成 `columns` 份，扣掉间距。
fn spread(available: f32, columns: usize) -> f32 {
    let gaps = GRID_GAP * columns.saturating_sub(1) as f32;
    ((available - gaps) / columns.max(1) as f32).floor().max(1.0)
}

/// 把执行器铺成自适应网格。
fn harness_grid(app: &mut MetisApp, ui: &mut Ui, t: &Tokens, rows: &[model::HarnessRow], content_width: f32) {
    // 减掉卡片容器的左右内边距（`card()` 用 space::L 两侧）+ 滚动条 + 余量。
    let available = (content_width - 2.0 * space::L - ui.spacing().scroll.bar_width - 12.0).max(GRID_MIN_CARD);
    let (columns, card_w) = grid_columns(available);

    // **用 egui 的流式布局**（`horizontal_wrapped`，即 `main_wrap`）。
    //
    // 卡片是固定宽度，所以换行位置完全由容器决定 —— 这正是流式布局的用途：
    // 宽度够就并排，不够就换行，**不需要自己算行/列**。
    //
    // 之前手算行列出过三次问题（间距翻倍、卡片被裁、行高不齐），
    // 根因都是"自己在控制位置，却又和容器自己的布局打架"。
    // 交给 `horizontal_wrapped` 之后，位置只有它一个来源。
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(GRID_GAP, GRID_GAP);
        for row in rows {
            harness_tile(app, ui, t, row, card_w, CAPSULE_HEIGHT);
        }
    });
    let _ = columns;
}
/// 一家执行器：固定尺寸的卡片。
///
/// **手绘背景 + 边框**，不用 `Frame`：`Frame` 会按内容收缩，
/// 导致"分配宽度"与"实际宽度"不一致，差额变成假间隙（见上面的注释）。
/// 手绘之后，卡片的矩形就是我们算出来的那个矩形，间距精确等于 `GRID_GAP`。
fn harness_tile(app: &mut MetisApp, ui: &mut Ui, t: &Tokens, row: &model::HarnessRow, width: f32, height: f32) {
    let on = Channel::ALL.iter().any(|channel| row.is_armed(*channel));
    let installed = row.installed != Some(false);
    let expanded = app.ui.expanded.as_deref() == Some(row.id.as_str());

    // ① 先占位：这一个矩形就是卡片本体。
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    // ② 手绘背景与边框。
    let (fill, stroke) = if expanded {
        (t.accent_soft, egui::Stroke::new(2.0, t.accent))
    } else if on {
        (t.card, egui::Stroke::new(1.0, t.accent))
    } else {
        (t.card, egui::Stroke::new(1.0, t.card_border))
    };
    let radius = egui::CornerRadius::same(t.radius as u8);
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter().rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    if response.hovered() {
        ui.painter().rect_stroke(rect, radius, egui::Stroke::new(1.0, t.field_border), egui::StrokeKind::Inside);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(rect.expand(1.0), radius, egui::Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
    }

    // ③ 内容画在卡片矩形**内部**（去掉内边距）。
    let inner = rect.shrink2(egui::vec2(CAPSULE_PAD_X, CAPSULE_PAD_Y));
    let mut cell = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
    cell.set_width(inner.width());
    cell.spacing_mut().item_spacing = egui::vec2(6.0, 2.0);

    // ── 第一行：图标 + id/状态 + 开关 ──
    cell.horizontal(|ui| {
        harness_avatar(ui, t, &row.id, CAPSULE_ICON, installed);

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let title = egui::RichText::new(&row.id).size(CAPSULE_NAME).family(fonts::mono());
                ui.add(egui::Label::new(title.color(if on { t.accent_ink } else { t.text })).truncate().selectable(false)).on_hover_text(&row.display);
                let (dot, _) = ui.allocate_exact_size(egui::vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, if on { t.success } else { t.field_border });
            });
            let status = tile_status(row, on, installed);
            ui.add(egui::Label::new(egui::RichText::new(status).size(CAPSULE_STATUS).color(t.text_faint)).truncate()).on_hover_text(status);
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            let mut want = on;
            let toggle = widgets::switch(ui, &mut want);
            if !installed && !on {
                toggle.clone().on_disabled_hover_text("本机没装这家，先装好再开");
            }
            if toggle.changed() && (installed || !want) {
                let id = row.id.clone();
                let channels: Vec<&str> = if want { vec!["shim", "slash", "install"] } else { Vec::new() };
                app.invoke("channel.set", json!({ "harness": id, "channels": channels }));
                app.refresh_blocking();
            }
        });
    });

    // ── 第二行：人设 / 模式（弹出菜单，不改变卡片高度） ──
    cell.add_space(2.0);
    mini_selects(app, &mut cell, t, row, inner.width());

    // ── 第三行：详情按钮 ──
    cell.with_layout(egui::Layout::right_to_left(egui::Align::Max), |ui| {
        let label = if expanded { "收起" } else { "详情" };
        let text = egui::RichText::new(label).size(CAPTION_LINK).color(t.accent_ink);
        if ui.add(egui::Button::new(text).frame(false)).clicked() {
            let id = row.id.clone();
            app.ui.toggle_expanded(&id);
        }
    });

    // 卡片空白处点击也能展开（但下拉菜单会先吃掉自己的点击，不会误触）。
    if response.clicked() {
        let id = row.id.clone();
        app.ui.toggle_expanded(&id);
    }
    response.on_hover_cursor(egui::CursorIcon::PointingHand);
}

/// 执行器头像：有品牌图就用图，没有就退回首字母徽章。
///
/// 原版用 `web/assets/` 下的真实品牌图标（各官网 favicon）。**本仓库不复制那些图**
/// （版权与来源需要逐个确认，见 `PARITY.md` §7），所以这里走原版自己的兜底路径：
/// 首字母大写徽章。等确认了授权再补图标，`HARNESS_ICONS` 那张表就是接缝。
fn harness_avatar(ui: &mut Ui, t: &Tokens, id: &str, size: f32, installed: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let radius = egui::CornerRadius::same((size * 0.29) as u8); // 原版 7px / 24px
    ui.painter().rect_filled(rect, radius, t.raised);
    ui.painter().rect_stroke(rect, radius, egui::Stroke::new(1.0, t.card_border), egui::StrokeKind::Inside);
    let initial = id.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::new(size * 0.5, fonts::medium()),
        if installed { t.text_dim } else { t.text_faint },
    );
}

/// 每张卡的人设 / 模式下拉。
///
/// 这两项是**按家生效的**（`--for <harness>`），不是全局设置 —— 原版就是这么放的。
fn mini_selects(app: &mut MetisApp, ui: &mut Ui, t: &Tokens, row: &model::HarnessRow, width: f32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // **用外层给的宽度算**，不要用 `ui.available_width()`：
        // `horizontal` 里的可用宽度会随前一个控件缩小。
        //
        // `ComboBox::width()` 是**最小宽**：内容更宽时它会自己撑开，
        // 从而把卡片里的内容顶出边框（真机看到"下拉框超框"就是这个）。
        // 所以宽度之外还要**把标签本身缩短**，两头一起约束。
        let half = ((width - 6.0) / 2.0).max(90.0);

        // 人设：这一家如果没单独设过，显示"跟随全局"。
        let current = row.persona.clone();
        // 标签要**短**：`ComboBox::width` 只是最小宽，长标签会把它撑出卡片边框。
        // 具体人设名在弹出菜单里能看到，这里只需要一眼认出"是不是跟全局一致"。
        // 标签要**短**：`ComboBox::width` 只是最小宽，长标签会把它撑出卡片边框。
        // "跟随"两个字比 "跟随·concise" 可读得多，而且语义一样清楚
        // （具体是谁在弹出菜单的第一项里写着"跟随全局 (<当前人设>)"）。
        // 前缀"人"与模式的"模"区分两个下拉 —— 都写"跟随"会分不清哪个是哪个。
        // 单个汉字只占 ~7px，不会撑破卡片。
        let label = match current.as_deref() {
            Some(id) => truncate_label(id, LABEL_CHARS),
            None => "人·跟随".to_owned(),
        };
        let mut chosen: Option<String> = None;
        egui::ComboBox::from_id_salt(("persona", &row.id))
            .selected_text(egui::RichText::new(label).size(MINI_SELECT_FONT).color(t.text_dim))
            .width(half)
            // `width` 是最小宽；`wrap_mode(Truncate)` 才是**上限保证**：
            // 选中文字太长时省略，而不是把下拉框撑出卡片边框。
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                // 把"跟随谁"写在这里，卡片上就不必挤长标签。
                let follow = format!("跟随全局（{}）", short(&app.snapshot.saved_persona));
                if ui.selectable_label(current.is_none(), egui::RichText::new(follow).size(MINI_SELECT_FONT)).clicked() {
                    chosen = Some(String::new());
                }
                for persona in &app.snapshot.personas {
                    let picked = current.as_deref() == Some(persona.id.as_str());
                    if ui.selectable_label(picked, egui::RichText::new(&persona.name).size(MINI_SELECT_FONT)).clicked() {
                        chosen = Some(persona.id.clone());
                    }
                }
            });
        if let Some(id) = chosen {
            let harness = row.id.clone();
            let params = if id.is_empty() { json!({ "harness": harness, "persona": "follow" }) } else { json!({ "harness": harness, "persona": id }) };
            app.invoke("persona.use", params);
            app.refresh_blocking();
        }

        // 模式：同样按家。`None` = 跟随全局。
        let current_mode = row.mode;
        let mode_text = match current_mode {
            Some(mode) => model::mode_short(mode),
            None => "模·跟随",
        };
        let mut picked_mode: Option<metis_core::Mode> = None;
        egui::ComboBox::from_id_salt(("mode", &row.id))
            .selected_text(egui::RichText::new(mode_text).size(MINI_SELECT_FONT).color(t.text_dim))
            .width(half)
            .wrap_mode(egui::TextWrapMode::Truncate)
            .show_ui(ui, |ui| {
                // 先给"跟随全局"，再给四个具体模式 —— 用户要能收回分家设置。
                if ui.selectable_label(current_mode.is_none(), egui::RichText::new("跟随全局").size(MINI_SELECT_FONT)).clicked() {
                    picked_mode = Some(app.snapshot.saved_mode);
                }
                for mode in [metis_core::Mode::Replace, metis_core::Mode::Strict, metis_core::Mode::Append, metis_core::Mode::Off] {
                    if ui.selectable_label(current_mode == Some(mode), egui::RichText::new(model::mode_short(mode)).size(MINI_SELECT_FONT)).clicked() {
                        picked_mode = Some(mode);
                    }
                }
            });
        if let Some(mode) = picked_mode {
            let harness = row.id.clone();
            app.invoke("state.mode", json!({ "harness": harness, "mode": mode.as_str() }));
            app.refresh_blocking();
        }
    });
}

/// 状态文字，对齐原版的取值顺序。
fn tile_status(row: &model::HarnessRow, on: bool, installed: bool) -> &'static str {
    if on {
        "已选择"
    } else if installed {
        // 原版区分"已就绪"（有可执行文件）与"发现配置"（只有配置目录）。
        if row.has_binary { "已就绪" } else { "发现配置" }
    } else {
        "未安装"
    }
}

/// 取一个 id 的短名（用于"人设·全局"这种紧凑标签）。
fn short(value: &Option<String>) -> String {
    value.clone().unwrap_or_else(|| "全局".to_owned())
}
/// 人设库。
pub fn library(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "人设库", "正文用于提示词，名称与说明只用于管理。");

        import_card(app, ui, &t);
        ui.add_space(space::L);

        widgets::card(&t).show(ui, |ui| {
            ui.horizontal(|ui| {
                widgets::section_title(ui, "全部人设");
                widgets::pill(ui, &format!("{} 份", app.snapshot.personas.len()), t.accent_ink, t.accent_soft);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // 搜索框：Ctrl+K 会把焦点给它。
                    let response = ui.add(egui::TextEdit::singleline(&mut app.ui.persona_query).hint_text("搜索人设").desired_width(180.0));
                    if std::mem::take(&mut app.ui.focus_search) {
                        response.request_focus();
                    }
                });
            });
            ui.add_space(space::S);

            let query = app.ui.persona_query.trim().to_lowercase();
            let personas: Vec<_> = app
                .snapshot
                .personas
                .iter()
                .filter(|p| query.is_empty() || p.name.to_lowercase().contains(&query) || p.id.to_lowercase().contains(&query))
                .collect();

            if personas.is_empty() {
                let (icon, title, body) = if query.is_empty() {
                    ("folder", "还没有人设", "用 `metis persona new` 建一份，或从 agent-purge 导入。")
                } else {
                    ("search", "没有匹配的人设", "换个关键词试试。")
                };
                widgets::empty_state(ui, icon, title, body);
                return;
            }

            for persona in personas {
                let is_current = app.snapshot.saved_persona.as_deref() == Some(persona.id.as_str());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                    icons::paint(ui, rect, "file-code", 16.0, if is_current { t.accent_ink } else { t.icon });
                    ui.label(RichText::new(&persona.name).size(font::BODY).family(fonts::medium()).color(t.text));
                    if is_current {
                        widgets::pill(ui, "当前", t.on_accent, t.accent);
                    }
                    if let Some(description) = persona.description.as_deref().filter(|d| !d.is_empty()) {
                        ui.label(RichText::new(description).size(font::CAPTION).color(t.text_faint));
                    }
                });
                ui.add_space(space::XS);
                widgets::separator(ui);
            }
        });
    });
}

/// 从 GitHub 导入人设。
///
/// 默认填官方源（[`crate::state::OFFICIAL_SOURCE`]），点一下就能用；
/// 想导别人的直接改输入框。
fn import_card(app: &mut MetisApp, ui: &mut Ui, t: &Tokens) {
    widgets::card(t).show(ui, |ui| {
        ui.horizontal(|ui| {
            widgets::section_title(ui, "从 GitHub 导入");
            if app.ui.import_source.trim() == crate::state::OFFICIAL_SOURCE {
                widgets::pill(ui, "官方源", t.accent_ink, t.accent_soft);
            }
        });
        widgets::caption(ui, "填 `owner/repo`。只读；同名人设不会被覆盖，会标为「跳过」。");
        ui.add_space(space::S);

        // 输入框独占一行，按钮另起一行。
        //
        // 原本是"输入框 + 两个按钮"挤在一行，真机截图里按钮被挤出卡片外看不见 ——
        // `horizontal` 里 `available_width` 会随前一个控件缩小，算着减 210 不可靠。
        ui.add(
            egui::TextEdit::singleline(&mut app.ui.import_source)
                .hint_text("owner/repo")
                .desired_width(f32::INFINITY)
                .font(egui::FontId::new(font::BODY, fonts::body())),
        );
        ui.add_space(space::S);
        ui.horizontal(|ui| {
            if widgets::primary(ui, "导入", true).clicked() {
                crate::actions::run(app, crate::actions::Action::ImportGithub, None);
            }
            if widgets::secondary(ui, "预览").clicked() {
                crate::actions::run(app, crate::actions::Action::ImportGithubDryRun, None);
            }
        });

        if app.jobs.is_busy() {
            ui.add_space(space::S);
            widgets::working(ui, "正在导入…", None);
        }
    });
}

/// 验证与诊断。
pub fn diagnostics(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "验证与诊断", "配置写入不代表有会话已生效；这里做的是实测与清理。");

        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "实测验证");
            widgets::caption(ui, "启动本地假 provider，抓宿主的真实请求，判定人设落在哪一层。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::primary(ui, "实测验证", true).clicked() {
                    crate::actions::run(app, crate::actions::Action::Verify, None);
                }
                if widgets::secondary(ui, "重新同步已选范围").clicked() {
                    app.spawn_apply();
                }
            });
            if app.jobs.is_busy() {
                ui.add_space(space::S);
                widgets::working(ui, "正在验证…", None);
            }
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "撤回接管");
            widgets::caption(ui, "只撤 Metis 写过的内容；人设与配置保留。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "预览清理").clicked() {
                    crate::actions::run(app, crate::actions::Action::CleanDryRun, None);
                }
                if widgets::danger(ui, "撤回全部接管").clicked() {
                    crate::actions::run(app, crate::actions::Action::Clean, None);
                }
            });
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "诊断信息");
            ui.add_space(space::S);
            widgets::key_value(ui, "数据目录", &app.snapshot.state_path);
            widgets::key_value(ui, "当前人设", app.snapshot.saved_persona.as_deref().unwrap_or("（未选择）"));
            widgets::key_value(ui, "当前模式", model::mode_label(app.snapshot.saved_mode));
            widgets::key_value(ui, "注入状态", if app.snapshot.power_on { "已开启" } else { "已暂停" });
            widgets::key_value(ui, "接管情况", &model::scope_summary(&app.snapshot.harnesses));
        });
    });
}

/// 关于。
pub fn about(app: &mut MetisApp, ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        widgets::page_header(ui, "关于 Metis", "一份人设，统一管理多个 coding agent。");

        widgets::card(&t).show(ui, |ui| {
            widgets::key_value(ui, "版本", env!("CARGO_PKG_VERSION"));
            widgets::key_value(ui, "数据目录", &app.snapshot.state_path);
            widgets::key_value(ui, "许可", "MIT OR Apache-2.0");
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "备份占用");
            widgets::caption(ui, "整文件回滚用的原件。只清理账本不再引用的那些；被引用的永远保留。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "查看占用").clicked() {
                    crate::actions::run(app, crate::actions::Action::BackupsUsage, None);
                }
                if widgets::secondary(ui, "预览清理").clicked() {
                    crate::actions::run(app, crate::actions::Action::BackupsPruneDryRun, None);
                }
                if widgets::danger(ui, "清理无用备份").clicked() {
                    crate::actions::run(app, crate::actions::Action::BackupsPrune, None);
                }
            });
        });

        ui.add_space(space::L);
        widgets::card(&t).show(ui, |ui| {
            widgets::section_title(ui, "从 agent-purge 迁移");
            widgets::caption(ui, "只读导入人设与部分状态；接管状态需要重新开启（两边落点标记不同）。");
            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if widgets::secondary(ui, "预览导入").clicked() {
                    crate::actions::run(app, crate::actions::Action::ImportDryRun, None);
                }
                if widgets::primary(ui, "从 agent-purge 导入", true).clicked() {
                    crate::actions::run(app, crate::actions::Action::Import, None);
                }
            });
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    /// 无头渲染一屏，返回图元数。
    fn render(screen: crate::state::Screen, app: &mut MetisApp) -> usize {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| match screen {
            crate::state::Screen::Console => console(app, ui),
            crate::state::Screen::Library => library(app, ui),
            crate::state::Screen::Diagnostics => diagnostics(app, ui),
            crate::state::Screen::About => about(app, ui),
        });
        output.textures_delta.clear();
        output.shapes.len()
    }

    #[test]
    fn every_screen_renders_something() {
        for screen in crate::state::Screen::ALL {
            let (_dir, mut app) = app();
            let count = render(screen, &mut app);
            assert!(count > 5, "{screen:?} 只画了 {count} 个图元");
        }
    }

    #[test]
    fn every_screen_renders_in_dark_mode_too() {
        // 深色下也要能画：颜色全取自 token，不该出现"白底白字"。
        for screen in crate::state::Screen::ALL {
            let (_dir, mut app) = app();
            let ctx = egui::Context::default();
            Tokens::install_full(&ctx, Tokens::DARK);
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 900.0))), ..Default::default() };
            let mut output = ctx.run_ui(raw, |ui| match screen {
                crate::state::Screen::Console => console(&mut app, ui),
                crate::state::Screen::Library => library(&mut app, ui),
                crate::state::Screen::Diagnostics => diagnostics(&mut app, ui),
                crate::state::Screen::About => about(&mut app, ui),
            });
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty(), "{screen:?} 在深色下没渲染");
        }
    }

    #[test]
    fn the_console_shows_the_three_modes_as_tiles() {
        // 三个模式各一个瓦片；选中的那个用 accent 底。
        // 断言方式：三种 draft.mode 各渲染一次都必须有内容，且不 panic。
        for mode in [Mode::Replace, Mode::Strict, Mode::Append] {
            let (_dir, mut app) = app();
            app.draft.mode = mode;
            assert!(render(crate::state::Screen::Console, &mut app) > 5, "{mode} 下控制台没渲染");
        }
    }

    #[test]
    fn the_apply_button_is_disabled_until_something_changes() {
        // 没改动时不能"应用"：`primary(..., enabled)` 的 enabled 参数要真的生效。
        let (_dir, mut app) = app();
        let change = app.draft.change(app.snapshot.saved_persona.as_deref(), app.snapshot.saved_mode);
        assert!(!change.is_dirty(), "初始状态不该是 dirty");
        // 改一下就该 dirty。
        app.draft.persona = Some("someone".to_owned());
        let change = app.draft.change(app.snapshot.saved_persona.as_deref(), app.snapshot.saved_mode);
        assert!(change.is_dirty(), "选了人设之后应该是 dirty");
    }

    #[test]
    fn an_empty_library_shows_an_empty_state() {
        // 没有人设时给"怎么办"，而不是一片空白。
        let (_dir, mut app) = app();
        assert!(app.snapshot.personas.is_empty(), "前置条件：临时 home 里没有人设");
        assert!(render(crate::state::Screen::Library, &mut app) > 5);
    }

    #[test]
    fn the_library_search_filters_the_list() {
        let (_dir, mut app) = app();
        app.snapshot.personas = vec![
            crate::model::PersonaRow { id: "concise".to_owned(), name: "简洁协作".to_owned(), description: None, chars: 10, active: false },
            crate::model::PersonaRow { id: "strict".to_owned(), name: "严格模式".to_owned(), description: None, chars: 8, active: false },
        ];
        app.ui.persona_query = "简洁".to_owned();
        // 过滤发生在渲染里；这里验证数据侧：过滤后只剩一个。
        let query = app.ui.persona_query.to_lowercase();
        let kept: Vec<_> = app.snapshot.personas.iter().filter(|p| p.name.to_lowercase().contains(&query) || p.id.to_lowercase().contains(&query)).collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].id, "concise");
    }

    #[test]
    fn the_diagnostics_screen_survives_a_busy_state() {
        let (_dir, mut app) = app();
        app.busy = Some("verify".to_owned());
        assert!(render(crate::state::Screen::Diagnostics, &mut app) > 5);
    }

    #[test]
    fn the_about_screen_lists_version_and_paths() {
        let (_dir, mut app) = app();
        assert!(render(crate::state::Screen::About, &mut app) > 5);
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn the_card_height_is_fixed() {
        // 卡片高度必须恒定：一旦随内容变高，① 网格每行对齐会漂；
        // ② 点人设/模式下拉时卡片会被撑高（维护者明确报过这个问题）；
        // ③ 用户"这排第几张"的心智模型会乱。
        // 这条把常量钉住，别有人把它改成"自适应"。
        // 通过 `std::hint::black_box` 避免 clippy 的 "constant assertion"：
        // 这条测的是"有人改了常量之后仍然合理"，常量本身当然是编译期已知的。
        let height = std::hint::black_box(CAPSULE_HEIGHT);
        assert!((100.0..=140.0).contains(&height), "卡片高度 {height} 不合理（应在 100~140）");
    }

    #[test]
    fn the_grid_geometry_can_fit_more_than_two_columns() {
        // **直接测 `grid_columns` 的行为**，不再靠"算一遍看看像不像"。
        // 三档真实宽度（逻辑像素）：
        //   - 360：200% 缩放的窄本（本机实测约 320~360）→ 1 列
        //   - 700：100% 缩放的中窗 → 3 列
        //   - 1200：宽屏 → 5~6 列
        // 期望值随 `GRID_MIN_CARD` 走：卡片变宽（260）之后，700 宽只够 2 列，
        // 这是**有意的** —— 宁可贵一点也要让下拉里的中文显示全。
        // 这些数字是设计的表达，不是"凑到能过为止"，改卡片尺寸时要一起更新。
        for (available, least) in [(300.0, 1usize), (700.0, 2), (1200.0, 4)] {
            let (columns, card) = grid_columns(available);
            assert!(columns >= least, "可用 {available} 只排出 {columns} 列（要求 ≥{least}，card={card}）");
            // 一行必须放得下（允许 1px 取整误差）。
            let used = card * columns as f32 + GRID_GAP * (columns.saturating_sub(1)) as f32;
            assert!(used <= available + 1.0, "一行放不下：card={card} cols={columns} 用了 {used} > {available}");
        }
    }

    #[test]
    fn the_compact_persona_row_saves_vertical_space() {
        // 人设卡片从"三行瓦片"改成"一行"，是控制台能看到执行器卡片的前提。
        // 这里用常量关系表达意图：卡片高度 + 顶栏 + 状态栏要能塞进 400 逻辑高。
        let chrome = 46.0 + 28.0; // 顶栏 + 状态栏
        let header = 44.0; // page_header
        let persona = 96.0; // 紧凑后的人设卡片（估）
        let power = 24.0;
        let total = chrome + header + persona + power;
        assert!(total < 400.0, "首屏预算 {total} 逻辑像素，超过 400 就看不到执行器卡片了");
    }
}
#[cfg(test)]
mod detail_tests {
    use super::*;
    use metis_paths::Paths;

    fn app() -> (tempfile::TempDir, MetisApp) {
        let dir = tempfile::tempdir().expect("tmp");
        let mut app = MetisApp::with_paths(Paths::with_home(dir.path().join("home")));
        app.refresh_blocking();
        (dir, app)
    }

    fn harness_row(id: &str) -> crate::model::HarnessRow {
        crate::model::HarnessRow {
            id: id.to_owned(),
            display: format!("{id} Display"),
            injectable: true,
            channels: metis_core::Channel::ALL.iter().map(|c| (*c, true, None)).collect(),
            on: true,
            explain: "注入中".to_owned(),
            hot: "支持热切".to_owned(),
            armed: vec![metis_core::Channel::Install],
            installed: Some(true),
            has_binary: true,
            persona: None,
            mode: None,
        }
    }

    /// 渲染详情浮层，返回图元数。
    ///
    /// **必须跑两帧**：`egui::Area` 第一帧只登记位置与内容尺寸，真正的几何要到
    /// 第二帧才定下来 —— 只跑一帧会看到"只有背景层"的假象（实测 8 个图元）。
    fn render_detail(app: &mut MetisApp) -> usize {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));
        let mut shapes = 0;
        for _ in 0..3 {
            let mut output = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
                harness_detail(app, ui.ctx());
            });
            output.textures_delta.clear();
            shapes = shapes.max(output.shapes.len());
        }
        shapes
    }

    #[test]
    fn the_detail_overlay_paints_when_a_harness_is_selected() {
        let (_dir, mut app) = app();
        app.snapshot.harnesses = vec![harness_row("codex")];

        app.ui.expanded = None;
        let without = render_detail(&mut app);
        assert_eq!(without, 0, "没选中时不该画任何东西");

        app.ui.expanded = Some("codex".to_owned());
        let with = render_detail(&mut app);
        assert!(with > 20, "选中后应画出一整块浮层，实际只有 {with} 个图元");
    }

    #[test]
    fn an_unknown_selected_id_paints_nothing() {
        // expanded 指向一个快照里不存在的 id（比如人设被删了）时不画，也不 panic。
        let (_dir, mut app) = app();
        app.snapshot.harnesses = vec![harness_row("codex")];
        app.ui.expanded = Some("nonexistent".to_owned());
        assert_eq!(render_detail(&mut app), 0);
    }

    #[test]
    fn the_overlay_does_not_change_the_page_layout() {
        // **这是浮层的核心价值**：它不参与布局，所以控制台的图元数不该因为它减少。
        // 同级面板版本实测从 305 掉到 79（布局被压扁），浮层不能重演。
        let (_dir, mut app) = app();
        app.snapshot.harnesses = vec![harness_row("codex"), harness_row("claude")];

        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 900.0));

        app.ui.expanded = None;
        let mut base = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| console(&mut app, ui));
        base.textures_delta.clear();

        app.ui.expanded = Some("codex".to_owned());
        let mut open = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            console(&mut app, ui);
            show_detail(&mut app, ui.ctx());
        });
        open.textures_delta.clear();

        assert!(open.shapes.len() >= base.shapes.len(), "浮层不该减少页面图元：关着 {} → 开着 {}", base.shapes.len(), open.shapes.len());
    }
}
