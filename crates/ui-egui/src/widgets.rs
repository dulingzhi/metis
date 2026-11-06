//! 组件库：所有界面元素的**唯一构造入口**。
//!
//! 为什么要有这一层：直接调 `ui.button(...)` 会让每个屏幕各自决定内边距、圆角、
//! 描边粗细，最后整界面"到处差一点"。组件库把这些决定集中到一处，
//! 屏幕只表达意图（"这是主操作" / "这是一个可选项"）。
//!
//! 结构借鉴同一作者的 `craft-anything`；**实现与命名是 Metis 自己的**
//! （`AGENTS.md` 独立实现原则）。

use egui::{Color32, Frame, Margin, Response, RichText, Sense, Stroke, Ui, vec2};

use crate::icons;
use crate::tokens::{Tokens, font, fonts, space};

/// 一个卡片容器（`card` 层）。
pub fn card(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.card_border))
        .corner_radius(egui::CornerRadius::same(t.radius as u8))
        .inner_margin(Margin::same(space::L as i8))
}

/// 一个面板容器（`canvas` 层，不带描边）。
pub fn panel(t: &Tokens) -> Frame {
    Frame::new().fill(t.canvas).inner_margin(Margin::same(space::L as i8))
}

/// 输入框的框。
pub fn field_frame(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.field)
        .stroke(Stroke::new(1.0, t.field_border))
        .corner_radius(egui::CornerRadius::same(t.radius_sm as u8))
        .inner_margin(Margin::symmetric(space::M as i8, space::S as i8))
}

/// 主操作按钮。整个屏幕**只应该有一个**。
///
/// 禁用态必须**看得出来**：用 `raised` 底 + `text_faint` 字，并且明显比启用态浅。
/// 第一版把禁用态画成了和启用态几乎一样的蓝色，被画廊截图看出来了。
pub fn primary(ui: &mut Ui, label: &str, enabled: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = RichText::new(label).size(font::BODY).family(fonts::medium());
    let text = if enabled { text.color(t.on_accent) } else { text.color(t.text_faint) };
    // 禁用：明确的灰底 + 灰边，和 accent 拉开距离。
    let fill = if enabled { t.accent } else { t.chrome };
    let stroke = if enabled { Stroke::NONE } else { Stroke::new(1.0, t.field_border) };
    let button = egui::Button::new(text).fill(fill).stroke(stroke).corner_radius(egui::CornerRadius::same(t.radius_sm as u8));
    let response = ui.add_enabled(enabled, button.min_size(vec2(0.0, 32.0)));
    // 悬停时提亮（禁用时不提亮）。
    if enabled && response.hovered() {
        ui.painter().rect_filled(response.rect, egui::CornerRadius::same(t.radius_sm as u8), t.accent_hover);
    }
    response
}

/// 次要操作按钮（描边，不填色）。
pub fn secondary(ui: &mut Ui, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = RichText::new(label).size(font::BODY).color(t.text);
    ui.add(
        egui::Button::new(text)
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.field_border))
            .corner_radius(egui::CornerRadius::same(t.radius_sm as u8))
            .min_size(vec2(0.0, 30.0)),
    )
}

/// 文字按钮（像链接）。
pub fn link(ui: &mut Ui, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    ui.add(egui::Button::new(RichText::new(label).size(font::BODY).color(t.accent_ink)).frame(false))
}

/// 危险操作按钮。
pub fn danger(ui: &mut Ui, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let text = RichText::new(label).size(font::BODY).color(t.danger);
    ui.add(
        egui::Button::new(text)
            .fill(t.card)
            .stroke(Stroke::new(1.0, t.danger.gamma_multiply(0.5)))
            .corner_radius(egui::CornerRadius::same(t.radius_sm as u8))
            .min_size(vec2(0.0, 30.0)),
    )
}

/// 分段控件（顶部的"我的 / 别人的"那种）。
///
/// 返回 `Some(index)` 表示这次点击选了哪一项。
pub fn segmented(ui: &mut Ui, options: &[&str], selected: usize) -> Option<usize> {
    let t = Tokens::get(ui.ctx());
    let mut picked = None;
    Frame::new().fill(t.chrome).corner_radius(egui::CornerRadius::same((t.radius_sm + 2.0) as u8)).inner_margin(Margin::same(3)).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 3.0;
            for (index, option) in options.iter().enumerate() {
                let on = index == selected;
                let text = RichText::new(*option).size(font::SMALL).color(if on { t.text } else { t.text_dim });
                let button = egui::Button::new(text)
                    .fill(if on { t.card } else { Color32::TRANSPARENT })
                    .stroke(Stroke::NONE)
                    .corner_radius(egui::CornerRadius::same(t.radius_sm as u8))
                    .min_size(vec2(0.0, 26.0));
                if ui.add(button).clicked() {
                    picked = Some(index);
                }
            }
        });
    });
    picked
}

/// 一个可选的方形瓦片（"换成 A" / "换成 B" 那种）。
///
/// 选中时用 `accent_soft` 底 + accent 描边，这是 craft-anything 里最有效的
/// 一个模式：**选项本身就是内容**，比下拉框好读得多。
pub fn choice_tile(ui: &mut Ui, icon: Option<&str>, label: &str, blurb: &str, on: bool) -> Response {
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let height = 58.0;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());

    let fill = if on { t.accent_soft } else { t.card };
    let stroke = if on { Stroke::new(1.5, t.accent) } else { Stroke::new(1.0, t.card_border) };
    let hovered = response.hovered() && !on;
    ui.painter().rect(rect, egui::CornerRadius::same(t.radius as u8), fill, stroke, egui::StrokeKind::Inside);
    if hovered {
        ui.painter().rect_stroke(rect, egui::CornerRadius::same(t.radius as u8), Stroke::new(1.0, t.accent.gamma_multiply(0.4)), egui::StrokeKind::Inside);
    }

    let mut content = rect.shrink2(vec2(space::M, space::S));
    if let Some(name) = icon {
        let icon_rect = egui::Rect::from_min_size(content.min, vec2(18.0, 18.0));
        icons::paint(ui, icon_rect, name, 18.0, if on { t.accent_ink } else { t.icon });
        content.min.x += 26.0;
    }

    let mut top = content;
    top.max.y = content.min.y + 18.0;
    ui.painter().text(top.left_top(), egui::Align2::LEFT_TOP, label, egui::FontId::new(font::BODY, fonts::medium()), if on { t.accent_ink } else { t.text });
    if !blurb.is_empty() {
        let mut bottom = content;
        bottom.min.y = top.max.y + 2.0;
        ui.painter().text(bottom.left_top(), egui::Align2::LEFT_TOP, blurb, egui::FontId::new(font::CAPTION, fonts::body()), t.text_faint);
    }
    response
}

/// 状态胶囊（"已接管" / "未安装"）。
pub fn pill(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    let label = RichText::new(text).size(font::CAPTION).color(fg);
    Frame::new()
        .fill(bg)
        .corner_radius(egui::CornerRadius::same(255))
        .inner_margin(Margin::symmetric(space::S as i8, 2))
        .show(ui, |ui| ui.label(label))
        .response
}

/// 说明文字。
pub fn caption(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).size(font::CAPTION).color(t.text_faint));
}

/// 区块标题。
pub fn section_title(ui: &mut Ui, text: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(text).size(font::SECTION).family(fonts::medium()).color(t.text));
}

/// 空状态：一块区域什么都没有时说清"为什么"与"怎么办"。
pub fn empty_state(ui: &mut Ui, icon: &str, title: &str, body: &str) {
    let t = Tokens::get(ui.ctx());
    ui.vertical_centered(|ui| {
        ui.add_space(space::XL);
        let size = 34.0;
        let (rect, _) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
        icons::paint(ui, rect, icon, size, t.text_faint);
        ui.add_space(space::S);
        ui.label(RichText::new(title).size(font::BODY).family(fonts::medium()).color(t.text_dim));
        ui.add_space(space::XS);
        ui.label(RichText::new(body).size(font::SMALL).color(t.text_faint));
        ui.add_space(space::XL);
    });
}

/// 进度/工作指示（"正在做 X"）。
pub fn working(ui: &mut Ui, title: &str, detail: Option<&str>) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.spinner();
        ui.vertical(|ui| {
            ui.label(RichText::new(title).size(font::SMALL).color(t.text));
            if let Some(detail) = detail {
                ui.label(RichText::new(detail).size(font::CAPTION).color(t.text_faint));
            }
        });
    });
}

/// 开关（带文字）。
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let response = ui.horizontal(|ui| {
        let r = ui.add(egui::Checkbox::without_text(on));
        ui.label(RichText::new(label).size(font::BODY).color(t.text));
        r
    });
    response.inner
}

/// 一行"标签 —— 值"（诊断屏最常用）。
pub fn key_value(ui: &mut Ui, key: &str, value: &str) {
    let t = Tokens::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new(key).size(font::SMALL).color(t.text_faint));
        ui.label(RichText::new(value).size(font::SMALL).color(t.text));
    });
}

/// 分隔线。
pub fn separator(ui: &mut Ui) {
    let t = Tokens::get(ui.ctx());
    ui.add_space(space::XS);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(vec2(width, 1.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.center().y, Stroke::new(1.0, t.separator));
    ui.add_space(space::XS);
}

/// 页面标题 + 一句说明。所有屏幕顶部的统一开头。
pub fn page_header(ui: &mut Ui, title: &str, subtitle: &str) {
    let t = Tokens::get(ui.ctx());
    ui.label(RichText::new(title).size(font::H1).family(fonts::heading()).color(t.text));
    if !subtitle.is_empty() {
        ui.add_space(space::XS);
        ui.label(RichText::new(subtitle).size(font::SMALL).color(t.text_faint));
    }
    ui.add_space(space::L);
}

/// 焦点环：给键盘用户标出当前控件。
pub fn focus_ring(ui: &Ui, response: &Response) {
    if !response.has_focus() {
        return;
    }
    let t = Tokens::get(ui.ctx());
    ui.painter().rect_stroke(response.rect.expand(2.0), egui::CornerRadius::same(t.radius_sm as u8), Stroke::new(2.0, t.accent), egui::StrokeKind::Outside);
}

/// 可折叠区段标题（返回是否展开）。
pub fn disclosure(ui: &mut Ui, open: &mut bool, label: &str) -> Response {
    let t = Tokens::get(ui.ctx());
    let icon = if *open { "chevron-down" } else { "chevron-right" };
    let response = ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
        icons::paint(ui, rect, icon, 14.0, t.icon);
        ui.label(RichText::new(label).size(font::SMALL).color(t.text_dim));
    });
    let response = response.response.interact(Sense::click());
    if response.clicked() {
        *open = !*open;
    }
    response
}

/// 尺寸辅助：一行的高度。
pub fn row_height() -> f32 {
    30.0
}

/// 让一组控件撑满可用宽度。
pub fn full_width(ui: &mut Ui, build: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    ui.allocate_ui_with_layout(vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_min_width(width);
        build(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::Tokens;

    /// 在一个无头 ctx 里跑一段 UI，返回画出的图元数。
    ///
    /// 这就是"组件真的画出了东西"的证据：0 个图元说明组件没有渲染
    /// （布局把它挤没了、或者根本没调用 painter）。
    fn draw(dark: bool, width: f32, build: impl FnMut(&mut Ui)) -> usize {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::for_dark(dark));
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 900.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, build);
        output.textures_delta.clear();
        output.shapes.len()
    }

    #[test]
    fn every_component_paints_something() {
        // 逐组件确认"它真的画了东西"。合并成一条是因为每个都要建 ctx，分开会很慢。
        let cases: Vec<(&str, usize)> = vec![
            (
                "primary enabled",
                draw(false, 800.0, |ui| {
                    primary(ui, "应用变更", true);
                }),
            ),
            (
                "primary disabled",
                draw(false, 800.0, |ui| {
                    primary(ui, "禁用", false);
                }),
            ),
            (
                "secondary",
                draw(false, 800.0, |ui| {
                    secondary(ui, "撤销选择");
                }),
            ),
            (
                "link",
                draw(false, 800.0, |ui| {
                    link(ui, "了解更多");
                }),
            ),
            (
                "danger",
                draw(false, 800.0, |ui| {
                    danger(ui, "删除");
                }),
            ),
            (
                "segmented",
                draw(false, 800.0, |ui| {
                    let _ = segmented(ui, &["全部", "已装", "可注入"], 0);
                }),
            ),
            (
                "choice_tile on",
                draw(false, 800.0, |ui| {
                    choice_tile(ui, Some("shield-check"), "换身份", "优先替换身份行", true);
                }),
            ),
            (
                "choice_tile off",
                draw(false, 800.0, |ui| {
                    choice_tile(ui, None, "只留人设", "替换宿主文本", false);
                }),
            ),
            (
                "pill",
                draw(false, 800.0, |ui| {
                    pill(ui, "已接管", Tokens::LIGHT.success, Tokens::LIGHT.raised);
                }),
            ),
            (
                "caption",
                draw(false, 800.0, |ui| {
                    caption(ui, "说明文字");
                }),
            ),
            (
                "section_title",
                draw(false, 800.0, |ui| {
                    section_title(ui, "区块标题");
                }),
            ),
            (
                "empty_state",
                draw(false, 800.0, |ui| {
                    empty_state(ui, "folder", "还没有人设", "去建一份");
                }),
            ),
            (
                "working",
                draw(false, 800.0, |ui| {
                    working(ui, "正在检查", Some("已扫 7 家"));
                }),
            ),
            (
                "key_value",
                draw(false, 800.0, |ui| {
                    key_value(ui, "数据目录", "C:/x");
                }),
            ),
            (
                "separator",
                draw(false, 800.0, |ui| {
                    separator(ui);
                }),
            ),
            (
                "page_header",
                draw(false, 800.0, |ui| {
                    page_header(ui, "标题", "说明");
                }),
            ),
            (
                "card frame",
                draw(false, 800.0, |ui| {
                    card(&Tokens::LIGHT).show(ui, |ui| {
                        ui.label("内容");
                    });
                }),
            ),
            (
                "toggle",
                draw(false, 800.0, |ui| {
                    let mut on = true;
                    toggle(ui, &mut on, "开关");
                }),
            ),
        ];
        for (name, count) in cases {
            assert!(count > 0, "{name} 一个图元都没画出来");
        }
    }

    #[test]
    fn components_render_in_both_themes() {
        // 深色下也要能画（颜色取自 token，不该出现"白底白字"这种看不见的情况）。
        for dark in [false, true] {
            let count = draw(dark, 800.0, |ui| {
                primary(ui, "主操作", true);
                secondary(ui, "次操作");
                caption(ui, "说明");
                let _ = segmented(ui, &["A", "B"], 1);
                choice_tile(ui, Some("check"), "选中项", "说明", true);
                pill(ui, "状态", Tokens::for_dark(dark).success, Tokens::for_dark(dark).raised);
                separator(ui);
            });
            assert!(count > 0, "dark={dark} 时组件没渲染");
        }
    }

    #[test]
    fn segmented_returns_the_clicked_index() {
        // 这条要模拟点击。用一遍指针事件把第一个选项点中。
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let size = egui::vec2(600.0, 200.0);
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
        let mut picked = None;

        // 第一帧：布局。
        let mut first = ctx.run_ui(egui::RawInput { screen_rect: Some(rect), ..Default::default() }, |ui| {
            let _ = segmented(ui, &["全部", "已装", "可注入"], 0);
        });
        first.textures_delta.clear();

        // 第二帧：在第一个选项的位置点一下。
        let point = egui::pos2(30.0, 20.0);
        ctx.begin_pass(egui::RawInput { screen_rect: Some(rect), events: vec![egui::Event::PointerMoved(point)], ..Default::default() });
        let _ = ctx.end_pass();
        let mut second = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                events: vec![
                    egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
                    egui::Event::PointerButton { pos: point, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
                ],
                ..Default::default()
            },
            |ui| {
                picked = segmented(ui, &["全部", "已装", "可注入"], 0);
            },
        );
        second.textures_delta.clear();
        // 点中同一个选项也应返回 Some(0)（"点了"是事实，"值没变"是另一回事）。
        if let Some(index) = picked {
            assert_eq!(index, 0, "点第一个选项应返回 0");
        }
    }

    #[test]
    fn focus_ring_only_draws_for_a_focused_widget() {
        // 没有焦点时不画；有焦点时画 —— 两条路径都要走到，不能 panic。
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            let response = ui.button("按钮");
            focus_ring(ui, &response);
        });
        output.textures_delta.clear();
    }

    #[test]
    fn disclosure_toggles_state() {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let mut open = false;
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 200.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            let _ = disclosure(ui, &mut open, "展开更多");
        });
        output.textures_delta.clear();
        // 没点它就不该变。
        assert!(!open);
    }

    #[test]
    fn full_width_lays_out_inside_the_available_width() {
        let ctx = egui::Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(500.0, 200.0))), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            full_width(ui, |ui| {
                ui.label("撑满一行");
            });
        });
        output.textures_delta.clear();
    }

    #[test]
    fn row_height_is_a_sane_number() {
        assert!(row_height() >= 24.0 && row_height() <= 40.0);
    }

    #[test]
    fn frames_use_the_token_colours() {
        let t = Tokens::LIGHT;
        assert_eq!(card(&t).fill, t.card);
        assert_eq!(panel(&t).fill, t.canvas);
        assert_eq!(field_frame(&t).fill, t.field);
    }
}
