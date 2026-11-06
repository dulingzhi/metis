//! Metis 的 egui 界面。
//!
//! 铁律 7（UI 是数据，不是逻辑）：判定全部来自引擎命令，界面只负责渲染与收集输入。
//!
//! 结构（扁平单层，`docs/DEVELOPMENT.md` §5）：
//! - [`model`]：**不依赖 egui** 的纯函数（列表筛选、排序、状态派生），因此可以直接单测；
//! - [`theme`]：主题 token 与字号尺度；
//! - [`state`]：UI 状态（serde 数据）；
//! - [`app`]：`eframe::App` 实现与屏幕分派；
//! - [`screens`]：每个屏幕一个 `show` 函数。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod actions;
pub mod app;
pub mod chrome;
pub mod icon_data;
pub mod icons;
pub mod jobs;
pub mod model;
pub mod palette;
pub mod screens;
pub mod sidebar;
pub mod state;
pub mod theme;
pub mod tokens;
pub mod widgets;

pub use app::MetisApp;
pub use state::{Screen, UiState};
// 兼容旧名字：`Tokens` 现在住在 `tokens`，但外部仍按 `theme::Tokens` 引用过。
pub use tokens::{ThemeKind, Tokens};

/// 一次性的界面初始化：字体、图标加载器、主题。
///
/// **必须在创建窗口后、画第一帧前调用**。少任何一步的表现是：
/// 少字体 → 中文全是方框；少 `install_image_loaders` → 图标全是空白。
/// 返回 `true` 表示中文字体可用；`false` 时界面仍可用，但中文会显示成方框。
///
/// **不在库里 `eprintln!`**：这是库代码，往 stderr 打字是应用的职责。
/// 把结果返回给调用方，由它决定怎么告诉用户（状态栏、日志、弹窗都行）。
pub fn setup(ctx: &egui::Context, dark: bool, reduce_motion: bool) -> bool {
    let cjk = tokens::install_fonts(ctx);
    egui_extras::install_image_loaders(ctx);
    Tokens::install_with(ctx, Tokens::for_dark(dark));
    Tokens::apply_motion(ctx, reduce_motion);
    cjk
}

/// 应用显示名。
pub const APP_TITLE: &str = "Metis";

/// 应用 id（窗口管理器用）。
pub const APP_ID: &str = "dev.metis.console";

/// 窗口默认尺寸。
pub const DEFAULT_WINDOW: [f32; 2] = [1280.0, 860.0];

/// 窗口最小尺寸。
pub const MIN_WINDOW: [f32; 2] = [900.0, 620.0];
