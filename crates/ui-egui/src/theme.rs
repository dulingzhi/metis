//! 兼容层：主题在这套重构里搬到了 [`crate::tokens`]。
//!
//! 这个模块只做重导出，保证既有调用点（`theme::Tokens` / `theme::install_cjk_font`）
//! 在迁移期间继续可用。**新代码请直接用 `crate::tokens`**。
//!
//! 之所以保留而不是删掉：CJK 字体的候选表与安装函数被测试直接引用（
//! `cjk_font_is_installed_on_this_machine`），一次性全改会牵动太多测试，
//! 而迁移期的稳定性比"目录好看"重要。

pub use crate::tokens::{
    CJK_FONT_CANDIDATES, ThemeKind, Tokens, contrast, font, fonts, install_fonts, luminance, resolve_reduce_motion, space, system_prefers_reduced_motion,
};

/// 装 CJK 字体（旧名，等价于 [`crate::tokens::install_fonts`]）。
pub fn install_cjk_font(ctx: &egui::Context) -> bool {
    crate::tokens::install_fonts(ctx)
}

/// 旧版的字号尺度名（`size::BODY` 等）。新代码用 [`crate::tokens::font`]。
pub mod size {
    pub use crate::tokens::font::{BODY, CAPTION, H1 as TITLE, H2, SECTION, SMALL};
}
