//! UI 状态：纯 serde 数据，不进 `state.json`。
//!
//! 持久化只走 eframe 的 storage（窗口偏好、上次屏幕），产品设置走引擎。

use metis_core::Mode;
use serde::{Deserialize, Serialize};

use crate::model::DraftChange;

/// 屏幕。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    /// 控制台。
    #[default]
    Console,
    /// 人设库。
    Library,
    /// 验证与诊断。
    Diagnostics,
    /// 关于。
    About,
}

impl Screen {
    /// 全部屏幕（导航顺序）。
    pub const ALL: [Self; 4] = [Self::Console, Self::Library, Self::Diagnostics, Self::About];

    /// 导航标签。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Console => "控制台",
            Self::Library => "人设库",
            Self::Diagnostics => "验证与诊断",
            Self::About => "关于",
        }
    }
}

/// 一次临时的顶部提示。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
    /// 文案。
    pub text: String,
    /// 色调。
    pub tone: NoticeTone,
}

/// 提示的色调。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoticeTone {
    /// 信息。
    Info,
    /// 成功。
    Success,
    /// 警告。
    Warning,
    /// 错误。
    Error,
}

/// 界面偏好（持久化到 eframe storage）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiState {
    /// 上次看的屏幕。
    pub screen: Screen,
    /// 侧栏宽度。
    pub sidebar_width: u32,
    /// 是否显示"本机没装"的执行器。
    pub show_uninstalled: bool,
    /// 执行器详情里展开了哪一家。
    pub expanded: Option<String>,
    /// 人设列表的搜索词。
    pub persona_query: String,
    /// 是否用深色主题。
    #[serde(default)]
    pub dark: bool,
    /// 下一帧是否要把焦点交给搜索框（Ctrl+K 用；消费后立即复位）。
    #[serde(skip)]
    pub focus_search: bool,
    /// 应用内的"减少动效"开关（系统偏好优先，见 `theme::apply_motion`）。
    #[serde(default)]
    pub reduce_motion: bool,
    /// 侧栏是否折叠成图标（窄窗口会强制折叠）。
    #[serde(default)]
    pub sidebar_folded: bool,
    /// GitHub 导入的仓库地址（`owner/repo`）。
    ///
    /// 默认填官方源 —— 用户点一下就能用；想导别人的直接改。
    ///
    /// **不持久化**：它属于"这次想导什么"，不是偏好，而且改它不该被记住。
    #[serde(skip)]
    pub import_source: String,
    /// 命令面板状态（`Ctrl+K`）。
    ///
    /// **不持久化**：这是"正在输入"的瞬时状态，跨进程保留没有意义，
    /// 而且会造出一个很怪的 bug —— 上次退出时面板开着，下次启动它就自己弹出来
    /// （而且关掉之后下次又回来，看起来像"关不掉"）。实测踩过。
    #[serde(skip)]
    pub palette: crate::palette::PaletteState,
}

/// 一个可选的导入源。
///
/// **数据而不是 if 链**（`AGENTS.md` 架构规则）：新增一个源只在这里加一行，
/// UI 自动多一个快捷按钮，不需要改渲染代码。
#[derive(Debug)]
pub struct SourceSuggestion {
    /// 仓库（`owner/repo`）。
    pub repo: &'static str,
    /// 按钮上的短名。
    pub label: &'static str,
    /// 悬停说明：它是什么、谁维护。
    pub hint: &'static str,
}

/// 界面上给出的源清单。
///
/// 只放**明确知道来源**的仓库。第三方源列在这里是"方便用户选择"，
/// 不表示 Metis 对其内容背书 —— `hint` 里要写清是谁的。
pub const SOURCE_SUGGESTIONS: &[SourceSuggestion] = &[
    SourceSuggestion { repo: OFFICIAL_SOURCE, label: "官方源", hint: "Metis 维护的起步人设（CC0）" },
    SourceSuggestion {
        repo: "ZiChuanLan/some-magic", label: "some-magic", hint: "第三方人设库（ZiChuanLan 维护，未声明许可；内容与 Metis 无关）"
    },
];

/// 官方人设源的仓库地址。
///
/// 这是**唯一**一处硬编码的源；改它等于改"点一下导入什么"。
/// 它必须在 `PARITY.md` / README 里有对应说明，不能只藏在代码里。
pub const OFFICIAL_SOURCE: &str = "dulingzhi/metis-personas";

fn default_import_source() -> String {
    OFFICIAL_SOURCE.to_owned()
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            screen: Screen::Console,
            sidebar_width: 200,
            show_uninstalled: true,
            expanded: None,
            persona_query: String::new(),
            dark: false,
            focus_search: false,
            reduce_motion: false,
            sidebar_folded: false,
            import_source: default_import_source(),
            palette: crate::palette::PaletteState::default(),
        }
    }
}

impl UiState {
    /// eframe storage 的键。
    pub const STORAGE_KEY: &'static str = "metis-ui";

    /// 从 storage 读；读不出来用默认值（**不报错**：偏好坏了不该让应用起不来）。
    ///
    /// 直接用 `Storage` trait 的字符串读写 + JSON，不依赖 eframe 的可选序列化 feature：
    /// 界面偏好的格式完全由我们控制，没必要把它绑死在第三方编码上。
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        // 优先级：已保存的偏好 > 环境变量 `METIS_DARK` > 默认值。
        // 环境变量只影响**首次启动**（之后用户点按钮的偏好更权威）。
        let mut state =
            storage.and_then(|storage| storage.get_string(Self::STORAGE_KEY)).and_then(|text| serde_json::from_str::<Self>(&text).ok()).unwrap_or_else(|| {
                Self { dark: std::env::var("METIS_DARK").map(|value| value == "1" || value.eq_ignore_ascii_case("true")).unwrap_or(false), ..Self::default() }
            });
        // `serde(skip)` 的字段在反序列化后是 `Default::default()`（空串），
        // **不是** `UiState::default()` 里的那个值。不补这一次，输入框会显示
        // 占位符而不是官方源 —— 用户会以为"默认没填"（真机截图确认过）。
        if state.import_source.trim().is_empty() {
            state.import_source = default_import_source();
        }
        state
    }

    /// 写回 storage。
    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        if let Ok(text) = serde_json::to_string(self) {
            storage.set_string(Self::STORAGE_KEY, text);
        }
    }

    /// 展开/收起某家执行器（一次只展开一个）。
    pub fn toggle_expanded(&mut self, harness: &str) {
        if self.expanded.as_deref() == Some(harness) {
            self.expanded = None;
        } else {
            self.expanded = Some(harness.to_owned());
        }
    }
}

/// 控制台上的表单草稿。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Draft {
    /// 选中的人设 id。
    pub persona: Option<String>,
    /// 选中的模式。
    pub mode: Mode,
    /// 是否已经和磁盘同步过（第一次加载时置真）。
    pub loaded: bool,
}

impl Draft {
    /// 从已保存的值初始化。
    pub fn from_saved(persona: Option<String>, mode: Mode) -> Self {
        Self { persona, mode, loaded: true }
    }

    /// 与已保存值相比改了什么。
    pub fn change(&self, saved_persona: Option<&str>, saved_mode: Mode) -> DraftChange {
        crate::model::draft_change(self.persona.as_deref(), self.mode, saved_persona, saved_mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_screen_is_console() {
        assert_eq!(UiState::default().screen, Screen::Console);
    }

    #[test]
    fn every_screen_has_a_label() {
        for screen in Screen::ALL {
            assert!(!screen.label().is_empty());
        }
        assert_eq!(Screen::ALL.len(), 4);
    }

    #[test]
    fn screen_serde_round_trips() {
        for screen in Screen::ALL {
            let json = serde_json::to_string(&screen).expect("serialize");
            let back: Screen = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, screen);
        }
    }

    #[test]
    fn toggle_expanded_keeps_only_one_open() {
        let mut ui = UiState::default();
        ui.toggle_expanded("codex");
        assert_eq!(ui.expanded.as_deref(), Some("codex"));
        ui.toggle_expanded("claude");
        assert_eq!(ui.expanded.as_deref(), Some("claude"), "一次只展开一个");
        ui.toggle_expanded("claude");
        assert_eq!(ui.expanded, None, "再点一次收起");
    }

    #[test]
    fn draft_reports_clean_until_it_differs() {
        let draft = Draft::from_saved(Some("a".to_owned()), Mode::Replace);
        assert_eq!(draft.change(Some("a"), Mode::Replace), DraftChange::Clean);
        let mut changed = draft.clone();
        changed.mode = Mode::Append;
        assert_eq!(changed.change(Some("a"), Mode::Replace), DraftChange::Mode);
    }

    #[test]
    fn load_without_storage_uses_defaults() {
        assert_eq!(UiState::load(None), UiState::default());
    }

    #[test]
    fn the_command_palette_is_never_persisted() {
        // 真机 bug：上次退出时面板开着，下次启动它自己弹出来，而且关掉之后
        // 下次又回来 —— 看起来像"关不掉"。根因是 `open` 被写进了 storage。
        let mut state = UiState::default();
        state.palette.open();
        state.palette.query = "验证".to_owned();

        let json = serde_json::to_string(&state).expect("json");
        assert!(!json.contains("\"open\""), "面板的开关不该进 storage：{json}");
        assert!(!json.contains("验证"), "搜索词也不该进 storage：{json}");
        assert!(!json.contains("palette"), "整个 palette 字段都该被跳过：{json}");
    }

    #[test]
    fn a_persisted_state_reopens_with_a_closed_palette() {
        // 反序列化之后面板必须是关着的（`skip` 会给默认值）。
        let text = r#"{"screen":"console","sidebar_width":200,"show_uninstalled":true,"persona_query":"","dark":false}"#;
        let state: UiState = serde_json::from_str(text).expect("parse");
        assert!(!state.palette.open, "启动时面板必须是关着的");
        assert!(state.palette.query.is_empty());
    }

    #[test]
    fn storage_key_is_stable() {
        assert_eq!(UiState::STORAGE_KEY, "metis-ui");
    }
}

#[cfg(test)]
mod source_tests {
    use super::*;

    #[test]
    fn the_official_source_is_always_offered() {
        // 官方源必须在清单里 —— 那是"点一下就能用"的默认路径。
        assert!(SOURCE_SUGGESTIONS.iter().any(|s| s.repo == OFFICIAL_SOURCE), "官方源必须在快捷源里");
    }

    #[test]
    fn every_suggestion_is_a_valid_repo_string() {
        for source in SOURCE_SUGGESTIONS {
            // `owner/repo`：恰好一个斜杠，两边都非空，不含空白。
            assert_eq!(source.repo.matches('/').count(), 1, "{} 不是 owner/repo 形式", source.repo);
            let (owner, repo) = source.repo.split_once('/').unwrap_or(("", ""));
            assert!(!owner.is_empty() && !repo.is_empty(), "{} 的 owner/repo 不能为空", source.repo);
            assert!(!source.repo.contains(char::is_whitespace), "{} 含空白", source.repo);
        }
    }

    #[test]
    fn every_suggestion_has_a_label_and_a_provenance_hint() {
        for source in SOURCE_SUGGESTIONS {
            assert!(!source.label.is_empty(), "{} 缺少按钮文字", source.repo);
            // **hint 必须说明是谁的源**：第三方源列在界面上不等于 Metis 背书。
            assert!(!source.hint.is_empty(), "{} 缺少说明", source.repo);
        }
    }

    #[test]
    fn third_party_sources_say_they_are_third_party() {
        // 非官方源的说明里要能看出"不是我维护的"。
        for source in SOURCE_SUGGESTIONS.iter().filter(|s| s.repo != OFFICIAL_SOURCE) {
            let hint = source.hint;
            let says_third_party = hint.contains("第三方") || hint.contains("未声明许可") || hint.contains("无关");
            assert!(says_third_party, "{} 的说明没写清它是第三方：{hint}", source.repo);
        }
    }

    #[test]
    fn suggestions_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for source in SOURCE_SUGGESTIONS {
            assert!(seen.insert(source.repo), "{} 重复出现", source.repo);
        }
    }
}
