//! 图标：内嵌 Lucide SVG，按需光栅化并着色。
//!
//! 做法（借鉴同一作者项目的**思路**，实现是 Metis 自己的）：
//! Lucide 的 SVG 用 `currentColor` 描边，所以我们在加载前把它替换成目标颜色，
//! 再由 `egui_extras` 的 SVG loader 光栅化到**当前屏幕像素尺寸**——
//! 这样放大不会糊，也不需要预渲染一堆尺寸。

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use egui::{Color32, Rect, Response, Sense, Vec2};

use crate::icon_data::ICONS;

/// 把某个颜色的图标字节准备好（同色只准备一次）。
fn svg_for(tint: Color32) -> HashMap<&'static str, Arc<[u8]>> {
    ICONS
        .iter()
        .map(|(name, bytes)| {
            let svg = String::from_utf8_lossy(bytes)
                .replace("currentColor", &format!("#{:02x}{:02x}{:02x}", tint.r(), tint.g(), tint.b()))
                // Lucide 默认 2px 描边在 14–18px 的界面里偏粗，压到 1.75 更利落。
                .replace("stroke-width=\"2\"", "stroke-width=\"1.75\"");
            (*name, Arc::from(svg.into_bytes().into_boxed_slice()))
        })
        .collect()
}

/// 白色版本的图标表（供 tint 使用）。
fn white() -> &'static HashMap<&'static str, Arc<[u8]>> {
    static MAP: OnceLock<HashMap<&'static str, Arc<[u8]>>> = OnceLock::new();
    MAP.get_or_init(|| svg_for(Color32::WHITE))
}

/// 有没有这个图标。
pub fn exists(name: &str) -> bool {
    white().contains_key(name)
}

/// 全部可用的图标名（测试与文档用）。
pub fn names() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = ICONS.iter().map(|(name, _)| *name).collect();
    out.sort_unstable();
    out
}

/// 构造一个图标 `Image`。
///
/// 未知名字回退到 `square` —— 界面上出现一个方块是明显的信号，
/// 比"什么都没画"更容易发现。
pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    let table = white();
    let (name, bytes) = match table.get(name).or_else(|| table.get("square")) {
        Some(bytes) => (name, bytes.clone()),
        None => ("missing", Arc::from(Vec::new().into_boxed_slice())),
    };
    egui::Image::from_bytes(format!("bytes://icons/{name}.svg"), egui::load::Bytes::Shared(bytes)).fit_to_exact_size(Vec2::splat(size)).tint(tint)
}

/// 在 `rect` 里居中画一个图标。
pub fn paint(ui: &egui::Ui, rect: Rect, name: &str, size: f32, tint: Color32) {
    let r = Rect::from_center_size(rect.center(), Vec2::splat(size));
    image(name, size, tint).paint_at(ui, r);
}

/// 一个图标按钮（无背景，悬停有底色）。
pub fn button(ui: &mut egui::Ui, name: &str, size: f32, tint: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size + 12.0), Sense::click());
    let t = crate::tokens::Tokens::get(ui.ctx());
    if response.hovered() {
        ui.painter().rect_filled(rect, egui::CornerRadius::same(6), t.hover);
    }
    paint(ui, rect, name, size, tint);
    response
}

/// 图标 + 文字的按钮。
pub fn labeled(ui: &mut egui::Ui, name: &str, label: &str, tint: Color32) -> Response {
    let t = crate::tokens::Tokens::get(ui.ctx());
    let text = egui::RichText::new(label).size(crate::tokens::font::BODY).color(tint);
    let response = ui.add(egui::Button::image_and_text(image(name, 15.0, tint), text).frame(false));
    if response.hovered() {
        ui.painter().rect_filled(response.rect.expand(2.0), egui::CornerRadius::same(6), t.hover);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_table_is_not_empty() {
        assert!(ICONS.len() >= 50, "图标太少：{}", ICONS.len());
    }

    #[test]
    fn every_icon_name_is_kebab_case_and_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for name in names() {
            assert!(!name.is_empty());
            assert!(name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'), "图标名要 kebab-case：{name}");
            assert!(seen.insert(name), "图标名重复：{name}");
        }
    }

    #[test]
    fn every_icon_is_valid_svg_with_current_colour() {
        for (name, bytes) in ICONS {
            let text = String::from_utf8_lossy(bytes);
            assert!(text.contains("<svg"), "{name} 不是 SVG");
            assert!(text.contains("currentColor"), "{name} 没有 currentColor，无法着色");
            assert!(text.contains("viewBox"), "{name} 缺 viewBox，缩放会出错");
        }
    }

    #[test]
    fn the_icons_we_use_actually_exist() {
        // 界面里用到的名字必须都在表里 —— 否则界面上会出现"missing"方块。
        for name in [
            "house",
            "bot",
            "activity",
            "info",
            "settings",
            "search",
            "plus",
            "check",
            "x",
            "trash-2",
            "refresh-cw",
            "shield-check",
            "circle-help",
            "chevron-right",
            "chevron-down",
            "layers",
            "play",
            "moon",
            "sun",
            "square",
        ] {
            assert!(exists(name), "缺少图标：{name}");
        }
    }

    #[test]
    fn tinting_replaces_current_colour() {
        // 白色版本应该已经把 currentColor 替换掉了 —— 这样 `Image::tint` 才有意义。
        let table = white();
        let svg = table.get("house").expect("house");
        let text = String::from_utf8_lossy(svg);
        assert!(!text.contains("currentColor"), "白色版本应已替换 currentColor");
        assert!(text.contains("#ffffff"), "应替换成白色");
    }

    #[test]
    fn an_unknown_name_falls_back_to_a_square() {
        assert!(!exists("definitely-not-an-icon"));
        // 构造不应 panic（画的时候会退到 square）。
        let _ = image("definitely-not-an-icon", 16.0, Color32::WHITE);
    }
}
