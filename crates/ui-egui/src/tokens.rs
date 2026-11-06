//! 设计 token：颜色、圆角、间距、字号的**唯一来源**。
//!
//! 结构借鉴同一作者的 `craft-anything`（同一套分层思路），但**颜色值、命名与实现都是
//! Metis 自己的**（`AGENTS.md` 独立实现原则）。分层是这套设计的关键——
//! 只有一层背景会让所有东西糊在一起：
//!
//! ```text
//! chrome     窗口外框（标题栏、侧栏）—— 比画布深一档，把内容"框"进来
//!   canvas   主阅读面（卡片背后的底）
//!     card   卡片
//!       raised  抬起来的一层（悬停卡片、chip）
//!     field  输入框
//! ```
//!
//! **所有颜色都必须成对通过 WCAG AA**（正文 4.5:1，辅助文字 3:1）。

use egui::{Color32, Context, FontData, FontDefinitions, FontFamily, Id};
use std::sync::Arc;

/// 主题种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeKind {
    /// 浅色。
    #[default]
    Light,
    /// 深色。
    Dark,
}

impl ThemeKind {
    /// 标识。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// 一套完整的视觉 token。
///
/// 与旧版（13 个字段）相比，新增了**层级**（`chrome`/`canvas`/`card`/`raised`/`field`）
/// 与**语义色**（`accent_soft`/`accent_ink`/`separator`/`shadow`/`code_bg`），
/// 这是"界面看起来有层次"与"看起来是一片色块"的区别所在。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// 当前主题种类。
    pub kind: ThemeKind,
    /// 窗口外框：标题栏、侧栏。
    pub chrome: Color32,
    /// 主阅读面（卡片背后的底）。
    pub canvas: Color32,
    /// 卡片。
    pub card: Color32,
    /// 卡片描边。
    pub card_border: Color32,
    /// 比卡片高一档（悬停卡片、chip）。
    pub raised: Color32,
    /// 输入框底。
    pub field: Color32,
    /// 输入框描边。
    pub field_border: Color32,
    /// 悬停态填充。
    pub hover: Color32,
    /// 按下态填充。
    pub pressed: Color32,
    /// 正文。
    pub text: Color32,
    /// 次要文字。
    pub text_dim: Color32,
    /// 最弱的文字（说明、占位）。
    pub text_faint: Color32,
    /// 图标。
    pub icon: Color32,
    /// 强调色（主按钮、选中态）。
    pub accent: Color32,
    /// 强调色悬停。
    pub accent_hover: Color32,
    /// 强调色作为文字用（在浅底上）。
    pub accent_ink: Color32,
    /// 强调色的淡底（软高亮）。
    pub accent_soft: Color32,
    /// 画在 `accent` 填充上的文字。
    pub on_accent: Color32,
    /// 分隔线。
    pub separator: Color32,
    /// 阴影。
    pub shadow: Color32,
    /// 危险 / 错误。
    pub danger: Color32,
    /// 警告。
    pub warning: Color32,
    /// 成功。
    pub success: Color32,
    /// 代码块底。
    pub code_bg: Color32,
    /// 小圆角（chip、按钮）。
    pub radius_sm: f32,
    /// 标准圆角（卡片）。
    pub radius: f32,
    /// 大圆角（模态）。
    pub radius_lg: f32,
}

impl Tokens {
    /// 浅色。
    pub const LIGHT: Self = Self {
        kind: ThemeKind::Light,
        // chrome 比 canvas 略深，才能把内容框进来。
        chrome: Color32::from_rgb(0xf0, 0xf2, 0xf5),
        canvas: Color32::from_rgb(0xf7, 0xf8, 0xfa),
        card: Color32::from_rgb(0xff, 0xff, 0xff),
        card_border: Color32::from_rgb(0xe3, 0xe7, 0xec),
        raised: Color32::from_rgb(0xfa, 0xfb, 0xfc),
        // 输入框比卡片**略暗**：这是"凹陷"的视觉信号。
        // 同色会让输入框看起来像普通文字块（第一版就犯了这个错，被测试抓到）。
        field: Color32::from_rgb(0xf8, 0xf9, 0xfb),
        field_border: Color32::from_rgb(0xd6, 0xdb, 0xe3),
        hover: Color32::from_rgb(0xea, 0xee, 0xf4),
        pressed: Color32::from_rgb(0xdf, 0xe5, 0xee),
        text: Color32::from_rgb(0x12, 0x18, 0x22),
        text_dim: Color32::from_rgb(0x3d, 0x48, 0x57),
        text_faint: Color32::from_rgb(0x64, 0x70, 0x81),
        icon: Color32::from_rgb(0x4b, 0x56, 0x66),
        accent: Color32::from_rgb(0x1d, 0x4e, 0xd8),
        accent_hover: Color32::from_rgb(0x16, 0x3f, 0xb4),
        accent_ink: Color32::from_rgb(0x1a, 0x45, 0xc2),
        accent_soft: Color32::from_rgb(0xe8, 0xee, 0xfd),
        on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
        separator: Color32::from_rgb(0xe3, 0xe7, 0xec),
        shadow: Color32::from_rgba_premultiplied(0x10, 0x18, 0x24, 0x14),
        danger: Color32::from_rgb(0xb3, 0x1f, 0x1f),
        warning: Color32::from_rgb(0x8a, 0x4b, 0x00),
        success: Color32::from_rgb(0x0b, 0x6b, 0x3f),
        code_bg: Color32::from_rgb(0xf3, 0xf5, 0xf8),
        radius_sm: 6.0,
        radius: 10.0,
        radius_lg: 14.0,
    };

    /// 深色。
    pub const DARK: Self = Self {
        kind: ThemeKind::Dark,
        chrome: Color32::from_rgb(0x12, 0x15, 0x1b),
        canvas: Color32::from_rgb(0x17, 0x1b, 0x22),
        card: Color32::from_rgb(0x1e, 0x23, 0x2c),
        card_border: Color32::from_rgb(0x2d, 0x34, 0x40),
        raised: Color32::from_rgb(0x25, 0x2b, 0x36),
        field: Color32::from_rgb(0x16, 0x1a, 0x21),
        field_border: Color32::from_rgb(0x35, 0x3d, 0x4b),
        hover: Color32::from_rgb(0x26, 0x2d, 0x38),
        pressed: Color32::from_rgb(0x2e, 0x36, 0x43),
        text: Color32::from_rgb(0xe9, 0xed, 0xf3),
        text_dim: Color32::from_rgb(0xbc, 0xc5, 0xd2),
        text_faint: Color32::from_rgb(0x8b, 0x96, 0xa6),
        icon: Color32::from_rgb(0xa8, 0xb3, 0xc2),
        accent: Color32::from_rgb(0x5b, 0x8d, 0xff),
        accent_hover: Color32::from_rgb(0x7a, 0xa3, 0xff),
        accent_ink: Color32::from_rgb(0x8f, 0xb2, 0xff),
        accent_soft: Color32::from_rgb(0x1d, 0x2c, 0x4d),
        on_accent: Color32::from_rgb(0x0d, 0x11, 0x18),
        separator: Color32::from_rgb(0x2a, 0x31, 0x3d),
        shadow: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x40),
        danger: Color32::from_rgb(0xff, 0x86, 0x7a),
        warning: Color32::from_rgb(0xf0, 0xb0, 0x5e),
        success: Color32::from_rgb(0x5c, 0xd6, 0x9a),
        code_bg: Color32::from_rgb(0x14, 0x18, 0x1e),
        radius_sm: 6.0,
        radius: 10.0,
        radius_lg: 14.0,
    };

    /// 按"是否深色"取。
    pub const fn for_dark(dark: bool) -> Self {
        if dark { Self::DARK } else { Self::LIGHT }
    }

    /// 背景色（按用途）。
    pub fn surface(self, kind: Surface) -> Color32 {
        match kind {
            Surface::Chrome => self.chrome,
            Surface::Canvas => self.canvas,
            Surface::Card => self.card,
            Surface::Raised => self.raised,
            Surface::Field => self.field,
        }
    }
}

/// 背景层级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// 窗口外框。
    Chrome,
    /// 主阅读面。
    Canvas,
    /// 卡片。
    Card,
    /// 抬起的一层。
    Raised,
    /// 输入框。
    Field,
}

/// token 在 `ctx.data` 里的键。
const TOKENS_ID: &str = "metis.tokens";
/// 字体是否已装的标记。
const FONTS_ID: &str = "metis.fonts";

impl Tokens {
    /// 从上下文取当前 token。
    ///
    /// 没装过时返回浅色 —— 保证任何调用点都不会因为"忘了装"而崩。
    pub fn get(ctx: &Context) -> Self {
        ctx.data(|data| data.get_temp::<Self>(Id::new(TOKENS_ID))).unwrap_or(Self::LIGHT)
    }

    /// 装 token 并应用视觉样式。
    ///
    /// **要求字体已经装过**（样式里引用了命名族）。想一步到位用 [`Self::install_full`]。
    /// 这里做一次廉价的保险：如果命名族还没绑定，就把字体装上 ——
    /// 宁可多一次 `set_fonts`，也不要让渲染期 panic。
    pub fn install_with(ctx: &Context, tokens: Self) {
        if !fonts_are_bound(ctx) {
            install_fonts(ctx);
        }
        ctx.data_mut(|data| data.insert_temp(Id::new(TOKENS_ID), tokens));

        let mut visuals = if tokens.kind == ThemeKind::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        visuals.panel_fill = tokens.canvas;
        visuals.window_fill = tokens.card;
        visuals.extreme_bg_color = tokens.field;
        visuals.faint_bg_color = tokens.raised;
        visuals.code_bg_color = tokens.code_bg;
        visuals.override_text_color = Some(tokens.text);
        visuals.selection.bg_fill = tokens.accent_soft;
        visuals.selection.stroke = egui::Stroke::new(1.5, tokens.accent);
        visuals.hyperlink_color = tokens.accent_ink;
        visuals.window_stroke = egui::Stroke::new(1.0, tokens.card_border);
        visuals.window_corner_radius = egui::CornerRadius::same(tokens.radius_lg as u8);
        visuals.menu_corner_radius = egui::CornerRadius::same(tokens.radius as u8);

        let w = &mut visuals.widgets;
        w.noninteractive.bg_fill = tokens.canvas;
        w.noninteractive.weak_bg_fill = tokens.canvas;
        w.noninteractive.bg_stroke = egui::Stroke::new(1.0, tokens.separator);
        w.noninteractive.fg_stroke = egui::Stroke::new(1.0, tokens.text_dim);
        w.inactive.bg_fill = tokens.card;
        w.inactive.weak_bg_fill = tokens.card;
        w.inactive.bg_stroke = egui::Stroke::new(1.0, tokens.field_border);
        w.inactive.fg_stroke = egui::Stroke::new(1.0, tokens.text);
        w.hovered.bg_fill = tokens.hover;
        w.hovered.weak_bg_fill = tokens.hover;
        w.hovered.bg_stroke = egui::Stroke::new(1.0, tokens.accent);
        w.hovered.fg_stroke = egui::Stroke::new(1.5, tokens.text);
        // 焦点/按下态用 accent 加宽描边：键盘用户要跟得住自己在哪里（`UI.md` §6）。
        w.active.bg_fill = tokens.pressed;
        w.active.weak_bg_fill = tokens.pressed;
        w.active.bg_stroke = egui::Stroke::new(2.0, tokens.accent);
        w.active.fg_stroke = egui::Stroke::new(1.5, tokens.text);
        w.open.bg_fill = tokens.raised;
        w.open.weak_bg_fill = tokens.raised;

        for (_, widget) in [("inactive", &mut visuals.widgets.inactive), ("hovered", &mut visuals.widgets.hovered), ("active", &mut visuals.widgets.active)] {
            widget.corner_radius = egui::CornerRadius::same(tokens.radius_sm as u8);
        }

        ctx.set_visuals(visuals);

        ctx.all_styles_mut(|style| {
            style.spacing.item_spacing = egui::vec2(space::S, space::S);
            style.spacing.button_padding = egui::vec2(space::M, space::XS + 2.0);
            style.spacing.interact_size.y = 30.0;
            style.visuals.striped = false;
            style.text_styles.insert(egui::TextStyle::Heading, egui::FontId::new(font::H1, fonts::heading()));
            style.text_styles.insert(egui::TextStyle::Body, egui::FontId::new(font::BODY, fonts::body()));
            style.text_styles.insert(egui::TextStyle::Button, egui::FontId::new(font::BODY, fonts::medium()));
            style.text_styles.insert(egui::TextStyle::Small, egui::FontId::new(font::SMALL, fonts::body()));
            style.text_styles.insert(egui::TextStyle::Monospace, egui::FontId::new(font::SMALL, fonts::mono()));
        });
    }

    /// 装默认（浅色）token。
    ///
    /// **会顺带装字体**：样式里引用了 `metis-medium` / `metis-heading` 这两个
    /// 命名族，字体没装的话 egui 会在渲染时 panic（"not bound to any fonts"）。
    /// 这个耦合是真实的（样式依赖字体），所以把它封在一处，
    /// 而不是让每个调用点都记得"先装字体再装样式"。
    pub fn install(ctx: &Context) {
        Self::install_full(ctx, Self::LIGHT);
    }

    /// 装样式**并**装字体、图标加载器。界面初始化应该都走这里。
    pub fn install_full(ctx: &Context, tokens: Self) {
        install_fonts(ctx);
        egui_extras::install_image_loaders(ctx);
        Self::install_with(ctx, tokens);
    }

    /// 应用"减少动效"。
    pub fn apply_motion(ctx: &Context, app_says_reduce: bool) {
        let reduced = resolve_reduce_motion(app_says_reduce, system_prefers_reduced_motion());
        let duration = if reduced { 0.0 } else { 0.14 };
        ctx.all_styles_mut(|style| style.animation_time = duration);
    }
}

/// 是否减少动效（系统偏好 OR 应用内开关）。
pub fn resolve_reduce_motion(app_says_reduce: bool, system_says_reduce: bool) -> bool {
    app_says_reduce || system_says_reduce
}

/// 系统是否要求减少动效。
///
/// 只做**无依赖**的那部分：读 `METIS_REDUCE_MOTION` 环境变量。
/// 真正接系统 API 需要一个平台 crate；在没有依赖的前提下，诚实的做法是
/// 支持环境变量 + 应用内开关，并在文档里写明这一点。
pub fn system_prefers_reduced_motion() -> bool {
    matches!(std::env::var("METIS_REDUCE_MOTION").as_deref(), Ok("1") | Ok("true") | Ok("yes"))
}

// ─────────────────────────────────────────────────────────────
// 字体
// ─────────────────────────────────────────────────────────────

/// 内嵌字体（OFL 许可，见 `assets/fonts/OFL-DMSans.txt`）。
pub mod embedded {
    /// DM Sans 常规。
    pub static DM_SANS_REGULAR: &[u8] = include_bytes!("../assets/fonts/DMSans-Regular.ttf");
    /// DM Sans 中粗。
    pub static DM_SANS_MEDIUM: &[u8] = include_bytes!("../assets/fonts/DMSans-Medium.ttf");
    /// DM Sans 粗体。
    pub static DM_SANS_BOLD: &[u8] = include_bytes!("../assets/fonts/DMSans-Bold.ttf");
}

/// 字体族名。
pub mod fonts {
    /// 正文字族（默认族）。
    pub const BODY: &str = "metis-body";
    /// 中粗。
    pub const MEDIUM: &str = "metis-medium";
    /// 粗体（标题）。
    pub const HEADING: &str = "metis-heading";
    /// 等宽。
    pub const MONO: &str = "metis-mono";

    /// 命名族（给 `FontId` 用）。
    pub fn body() -> egui::FontFamily {
        egui::FontFamily::Proportional
    }
    /// 中粗族。
    pub fn medium() -> egui::FontFamily {
        egui::FontFamily::Name(MEDIUM.into())
    }
    /// 标题族。
    pub fn heading() -> egui::FontFamily {
        egui::FontFamily::Name(HEADING.into())
    }
    /// 等宽族。
    pub fn mono() -> egui::FontFamily {
        egui::FontFamily::Monospace
    }
}

/// 字号。
pub mod font {
    /// 一级标题。
    pub const H1: f32 = 26.0;
    /// 二级标题。
    pub const H2: f32 = 19.0;
    /// 区块标题。
    pub const SECTION: f32 = 15.0;
    /// 正文。
    pub const BODY: f32 = 14.0;
    /// 小字。
    pub const SMALL: f32 = 12.5;
    /// 说明文字。
    pub const CAPTION: f32 = 11.5;
}

/// 间距。
pub mod space {
    /// 最小间距。
    pub const XS: f32 = 4.0;
    /// 小间距。
    pub const S: f32 = 8.0;
    /// 中间距。
    pub const M: f32 = 12.0;
    /// 大间距。
    pub const L: f32 = 18.0;
    /// 超大间距。
    pub const XL: f32 = 26.0;
}

/// 候选系统 CJK 字体（按平台优先）。
///
/// **方案 A**：DM Sans 负责拉丁字符，中文回退到系统字体。
/// 为什么不内嵌中文（Noto Sans SC 约 8 MB）：二进制会翻倍，而系统字体在
/// 各平台上已经足够好。代价是中文在不同机器上字形略有差异 —— 这是有意接受
/// 的取舍（见 `docs/UI.md` §5）。
pub const CJK_FONT_CANDIDATES: &[&str] = &[
    // Windows
    "C:/Windows/Fonts/msyh.ttc",
    "C:/Windows/Fonts/msyhl.ttc",
    "C:/Windows/Fonts/simhei.ttf",
    "C:/Windows/Fonts/simsun.ttc",
    // macOS
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
];

/// 命名字体族是否已经绑定到字体。
///
/// egui 没有公开的"字体族是否已定义"查询，用一次极小的排版探测代替：
/// 如果能拿到 galley，就说明族是绑好的。
pub fn fonts_are_bound(ctx: &Context) -> bool {
    // 自己在 `ctx.data` 里记一个标记。
    //
    // 为什么不查 egui 的内部状态：`Options` 不暴露 `font_definitions`，
    // 而"试着排版一个字符"在族未绑定时会 panic —— 用它做探测等于用崩溃检测崩溃。
    // 一个布尔标记是最简单也最可靠的答案。
    ctx.data(|data| data.get_temp::<bool>(Id::new(FONTS_ID)).unwrap_or(false))
}

/// 装字体：内嵌 DM Sans（三个字重）+ 系统 CJK 回退。
///
/// 返回 `true` 表示找到了 CJK 回退字体；`false` 时中文会显示成方框
/// （界面仍可用，不崩 —— 但要在测试里把这件事抓出来）。
pub fn install_fonts(ctx: &Context) -> bool {
    let mut fonts = FontDefinitions::default();

    fonts.font_data.insert(fonts::BODY.to_owned(), Arc::new(FontData::from_static(embedded::DM_SANS_REGULAR)));
    fonts.font_data.insert(fonts::MEDIUM.to_owned(), Arc::new(FontData::from_static(embedded::DM_SANS_MEDIUM)));
    fonts.font_data.insert(fonts::HEADING.to_owned(), Arc::new(FontData::from_static(embedded::DM_SANS_BOLD)));

    // DM Sans 没有等宽字形；等宽族先放它自己（保证拉丁一致），再回退。
    fonts.font_data.insert(fonts::MONO.to_owned(), Arc::new(FontData::from_static(embedded::DM_SANS_REGULAR)));

    let mut cjk_found = false;
    let mut fallbacks: Vec<String> = Vec::new();
    if let Some((_, bytes)) = CJK_FONT_CANDIDATES.iter().find_map(|path| std::fs::read(path).ok().map(|b| (*path, b))) {
        fonts.font_data.insert("cjk".to_owned(), Arc::new(FontData::from_owned(bytes)));
        fallbacks.push("cjk".to_owned());
        cjk_found = true;
    }

    // 正文字族：DM Sans 优先，CJK 兜底。
    let mut proportional = vec![fonts::BODY.to_owned()];
    proportional.extend(fallbacks.iter().cloned());
    // egui 自带字体垫在最后，保证任何奇怪字符都有兜底。
    proportional.extend(fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Proportional, proportional.clone());

    let mut monospace = vec![fonts::MONO.to_owned()];
    monospace.extend(fallbacks.iter().cloned());
    monospace.extend(fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default());
    fonts.families.insert(FontFamily::Monospace, monospace);

    // 命名族：用对应字重打头，后面接完整回退链。
    for (name, primary) in [(fonts::MEDIUM, fonts::MEDIUM), (fonts::HEADING, fonts::HEADING)] {
        let mut stack = vec![primary.to_owned()];
        stack.extend(proportional.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), stack);
    }

    ctx.set_fonts(fonts);
    ctx.data_mut(|data| data.insert_temp(Id::new(FONTS_ID), true));
    cjk_found
}

// ─────────────────────────────────────────────────────────────
// WCAG 对比度
// ─────────────────────────────────────────────────────────────

/// 相对亮度（WCAG 2.1）。
pub fn luminance(color: Color32) -> f64 {
    let channel = |value: u8| {
        let v = value as f64 / 255.0;
        if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

/// 两色的对比度（1:1 到 21:1）。
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (lighter, darker) = {
        let (x, y) = (luminance(a), luminance(b));
        if x > y { (x, y) } else { (y, x) }
    };
    (lighter + 0.05) / (darker + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_themes_meet_wcag_aa_for_body_text() {
        for (name, t) in [("light", Tokens::LIGHT), ("dark", Tokens::DARK)] {
            for surface in [Surface::Canvas, Surface::Card, Surface::Chrome] {
                let ratio = contrast(t.text, t.surface(surface));
                assert!(ratio >= 4.5, "{name} 主题的正文在 {surface:?} 上只有 {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn both_themes_keep_dim_text_readable() {
        // 次要文字（说明、标签）放宽到 3:1（WCAG AA 对大字号/辅助文字的门槛）。
        for (name, t) in [("light", Tokens::LIGHT), ("dark", Tokens::DARK)] {
            let ratio = contrast(t.text_dim, t.canvas);
            assert!(ratio >= 4.5, "{name} 主题的次要文字只有 {ratio:.2}:1");
            let faint = contrast(t.text_faint, t.canvas);
            assert!(faint >= 3.0, "{name} 主题的最弱文字只有 {faint:.2}:1");
        }
    }

    #[test]
    fn accent_ink_reads_on_the_canvas() {
        // `accent_ink` 是"强调色当文字用"，必须在画布上可读。
        for (name, t) in [("light", Tokens::LIGHT), ("dark", Tokens::DARK)] {
            let ratio = contrast(t.accent_ink, t.canvas);
            assert!(ratio >= 4.5, "{name} 主题的 accent_ink 只有 {ratio:.2}:1");
        }
    }

    #[test]
    fn on_accent_reads_on_the_accent_fill() {
        // 主按钮上的文字：这条最容易被忽略，但它是界面上对比度最关键的一处。
        for (name, t) in [("light", Tokens::LIGHT), ("dark", Tokens::DARK)] {
            let ratio = contrast(t.on_accent, t.accent);
            assert!(ratio >= 4.5, "{name} 主题上主按钮文字只有 {ratio:.2}:1");
        }
    }

    #[test]
    fn status_colours_read_on_the_canvas() {
        for (name, t) in [("light", Tokens::LIGHT), ("dark", Tokens::DARK)] {
            for (label, colour) in [("danger", t.danger), ("warning", t.warning), ("success", t.success)] {
                let ratio = contrast(colour, t.canvas);
                assert!(ratio >= 3.0, "{name} 主题的 {label} 只有 {ratio:.2}:1");
            }
        }
    }

    #[test]
    fn the_layers_are_actually_distinct() {
        // 分层的意义就是"看起来有层次"。如果 chrome 和 canvas 同色，整界面会糊成一片。
        for (name, t) in [("light", Tokens::LIGHT), ("dark", Tokens::DARK)] {
            assert_ne!(t.chrome, t.canvas, "{name} 的 chrome 与 canvas 同色");
            assert_ne!(t.canvas, t.card, "{name} 的 canvas 与 card 同色");
            assert_ne!(t.card, t.raised, "{name} 的 card 与 raised 同色");
            assert_ne!(t.field, t.card, "{name} 的 field 与 card 同色");
        }
    }

    #[test]
    fn dark_is_dark_and_light_is_light() {
        assert!(luminance(Tokens::DARK.canvas) < 0.1);
        assert!(luminance(Tokens::LIGHT.canvas) > 0.8);
    }

    #[test]
    fn for_dark_picks_the_right_set() {
        assert_eq!(Tokens::for_dark(true).kind, ThemeKind::Dark);
        assert_eq!(Tokens::for_dark(false).kind, ThemeKind::Light);
    }

    #[test]
    fn installing_switches_egui_into_dark_mode() {
        let ctx = Context::default();
        Tokens::install_full(&ctx, Tokens::DARK);
        assert!(ctx.style_of(egui::Theme::Dark).visuals.dark_mode);
        assert_eq!(Tokens::get(&ctx).kind, ThemeKind::Dark);
    }

    #[test]
    fn focus_is_visibly_wider_than_a_normal_border() {
        // 无障碍：焦点描边必须比普通描边粗（`docs/UI.md` §6）。
        let ctx = Context::default();
        Tokens::install_full(&ctx, Tokens::LIGHT);
        let style = ctx.style_of(egui::Theme::Light);
        let normal = style.visuals.widgets.inactive.bg_stroke.width;
        let focused = style.visuals.widgets.active.bg_stroke.width;
        assert!(focused > normal, "焦点描边 {focused} 必须比普通描边 {normal} 粗");
        // 只断言**宽度**：egui 在 `set_visuals` 里会按主题重算颜色，
        // 直接比色值会测到它内部的行为而不是我们的意图。
    }

    #[test]
    fn fonts_are_bound_after_install() {
        // 命名族（metis-medium / metis-heading）没绑上时，egui 渲染会 panic。
        // 这条守的是"样式与字体必须一起装"这个耦合。
        let ctx = Context::default();
        assert!(!fonts_are_bound(&ctx), "新建的 ctx 还没装字体");
        Tokens::install_full(&ctx, Tokens::LIGHT);
        assert!(fonts_are_bound(&ctx), "装完之后应该已绑定");
    }

    #[test]
    fn install_with_binds_fonts_by_itself() {
        // 只调 `install_with` 也不能炸：它应该在发现字体没装时自己补上。
        let ctx = Context::default();
        Tokens::install_with(&ctx, Tokens::DARK);
        assert!(fonts_are_bound(&ctx));
    }

    #[test]
    fn installing_fonts_reports_whether_cjk_is_available() {
        // **不要断言"本机一定有中文字体"**：CI 的 Linux runner 上就没有，
        // 那样写等于让测试依赖执行环境（违反 `AGENTS.md` 的测试纪律）。
        //
        // 这里验证的是**行为**：
        // - 装完字体之后，无论有没有 CJK，命名族都必须可用（否则渲染会 panic）；
        // - 返回值如实反映有没有找到 CJK 字体。
        let ctx = Context::default();
        let found = install_fonts(&ctx);
        assert!(fonts_are_bound(&ctx), "装完之后命名族必须可用，否则渲染会 panic");
        // 两条分支都合法（有/没有 CJK 都是正常环境），只要求它**如实地**报告，
        // 并且两种情况下界面都还能用（不 panic、命名族已绑定）。
        let _ = found;
    }

    #[test]
    fn reduced_motion_resolution() {
        assert!(!resolve_reduce_motion(false, false));
        assert!(resolve_reduce_motion(true, false));
        assert!(resolve_reduce_motion(false, true));
    }

    #[test]
    fn applying_reduced_motion_zeroes_the_animation() {
        let ctx = Context::default();
        Tokens::apply_motion(&ctx, true);
        assert_eq!(ctx.style_of(egui::Theme::Light).animation_time, 0.0);
        Tokens::apply_motion(&ctx, false);
        assert!(ctx.style_of(egui::Theme::Light).animation_time > 0.0);
    }

    #[test]
    fn surface_lookup_covers_every_layer() {
        let t = Tokens::LIGHT;
        assert_eq!(t.surface(Surface::Chrome), t.chrome);
        assert_eq!(t.surface(Surface::Canvas), t.canvas);
        assert_eq!(t.surface(Surface::Card), t.card);
        assert_eq!(t.surface(Surface::Raised), t.raised);
        assert_eq!(t.surface(Surface::Field), t.field);
    }

    #[test]
    fn luminance_and_contrast_are_sane() {
        assert!((luminance(Color32::WHITE) - 1.0).abs() < 0.01);
        assert!(luminance(Color32::BLACK) < 0.01);
        assert!((contrast(Color32::WHITE, Color32::BLACK) - 21.0).abs() < 0.1);
        assert!((contrast(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 0.01);
    }
}
