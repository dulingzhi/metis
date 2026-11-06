//! **不依赖 egui** 的纯函数模型：筛选、排序、状态派生、文案。
//!
//! 放在这里而不是屏幕文件里，是因为它可以直接用普通单元测试覆盖
//! （`docs/DEVELOPMENT.md` §5：界面逻辑不该被"必须开窗口才能测"绑架）。

use metis_core::{Activation, Channel, HarnessId, Mode};
use metis_harness::Spec;

/// 一行执行器的显示数据（由命令的 JSON 结果算出）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRow {
    /// id。
    pub id: String,
    /// 显示名。
    pub display: String,
    /// 能否注入。
    pub injectable: bool,
    /// 四条通道的可用性（`ok` 与 `why`）。
    pub channels: Vec<(Channel, bool, Option<String>)>,
    /// 当前是否在注入。
    pub on: bool,
    /// 状态说明。
    pub explain: String,
    /// 热切说明。
    pub hot: String,
    /// 已武装的通道。
    pub armed: Vec<Channel>,
    /// 本机装没装（未知时为 `None`）。
    pub installed: Option<bool>,
    /// 找到了可执行文件（原版区分"已就绪"与"发现配置"）。
    pub has_binary: bool,
    /// 这一家单独指定的人设（`None` = 跟随全局）。
    pub persona: Option<String>,
    /// 这一家单独指定的模式（`None` = 跟随全局）。
    pub mode: Option<Mode>,
}

impl HarnessRow {
    /// 取一条通道的可用性。
    pub fn channel(&self, channel: Channel) -> (bool, Option<&str>) {
        self.channels.iter().find(|(c, _, _)| *c == channel).map(|(_, ok, why)| (*ok, why.as_deref())).unwrap_or((false, None))
    }

    /// 已武装吗。
    pub fn is_armed(&self, channel: Channel) -> bool {
        self.armed.contains(&channel)
    }

    /// 一行的人话状态。
    pub fn status_line(&self) -> String {
        if !self.injectable {
            return "不支持注入".to_owned();
        }
        if self.on { format!("注入中 · {}", self.explain) } else { self.explain.clone() }
    }
}

/// 从注册表的静态事实构造一行（不含本机探测结果）。
pub fn row_from_spec(spec: &'static Spec, armed: &[Channel], activation: Option<&Activation>) -> HarnessRow {
    let caps = metis_harness::registry::caps_of(spec);
    let channels = Channel::ALL
        .iter()
        .map(|channel| {
            let state = caps.channel(*channel);
            (*channel, state.ok, state.why.clone())
        })
        .collect();
    HarnessRow {
        id: spec.id.to_owned(),
        display: spec.display.to_owned(),
        injectable: caps.injectable,
        channels,
        on: activation.is_some_and(|a| a.on),
        explain: activation.map(Activation::explain).unwrap_or_else(|| "未启用".to_owned()),
        hot: caps.hot.how.clone(),
        armed: armed.to_vec(),
        installed: None,
        has_binary: false,
        persona: None,
        mode: None,
    }
}

/// 筛选选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Filter {
    /// 只显示能注入的。
    pub injectable_only: bool,
    /// 只显示已武装的。
    pub armed_only: bool,
    /// 只显示本机装了的（对应界面的「显示本机没装的」取消勾选）。
    pub installed_only: bool,
}

/// 按筛选条件过滤，保持注册表顺序（稳定，不随排序抖动）。
pub fn filter_rows(rows: &[HarnessRow], filter: Filter) -> Vec<HarnessRow> {
    rows.iter()
        .filter(|row| !filter.injectable_only || row.injectable)
        .filter(|row| !filter.armed_only || !row.armed.is_empty())
        // 未探测出结果（`None`）时保留：**不能因为"没探测"就把整行藏掉**，
        // 那会让用户以为这家不存在。
        .filter(|row| !filter.installed_only || row.installed.unwrap_or(true))
        .cloned()
        .collect()
}

/// 范围摘要："已接管 N 家 · 共 M 家"。
pub fn scope_summary(rows: &[HarnessRow]) -> String {
    let armed = rows.iter().filter(|row| !row.armed.is_empty()).count();
    let total = rows.iter().filter(|row| row.injectable).count();
    format!("已接管 {armed} 家 · 可接管 {total} 家")
}

/// 模式的**短名**（卡片里的小下拉用）。
///
/// 完整名见 [`mode_label`]；短名是为了在小尺寸下不挤。
pub fn mode_short(mode: Mode) -> &'static str {
    match mode {
        Mode::Replace => "换身份",
        Mode::Strict => "只留人设",
        Mode::Append => "追加",
        Mode::Off => "关闭",
    }
}

/// 每个模式对应的图标（选择瓦片上用）。
pub fn mode_icon(mode: Mode) -> &'static str {
    match mode {
        Mode::Replace => "shield-check",
        Mode::Strict => "message-square",
        Mode::Append => "plus",
        Mode::Off => "circle-stop",
    }
}

/// 模式的中文短名（与 `docs/UI.md` §2.1 的三选一一致）。
pub fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Replace => "换身份（默认）",
        Mode::Strict => "只留人设",
        Mode::Append => "追加",
        Mode::Off => "关闭",
    }
}

/// 模式的一行说明。
pub fn mode_hint(mode: Mode) -> &'static str {
    match mode {
        Mode::Replace => "优先替换身份行；无法读取宿主文本的启动入口按「只留人设」处理",
        Mode::Strict => "替换可控入口中的宿主文本；宿主仍可能额外添加身份与运行说明",
        Mode::Append => "宿主提示词原样保留，人设另加一份（两份身份可能打架）",
        Mode::Off => "停止注入；已写入的静态内容会被撤下",
    }
}

/// 通道的中文名。
pub fn channel_label(channel: Channel) -> &'static str {
    match channel {
        Channel::Shim => "接管命令",
        Channel::Install => "常驻",
        Channel::Slash => "会话内",
        Channel::Project => "项目",
    }
}

/// 通道的一行说明（悬停用）。
pub fn channel_hint(channel: Channel) -> &'static str {
    match channel {
        Channel::Shim => "在 PATH 最前放同名命令，裸敲即经 Metis 启动",
        Channel::Install => "改宿主的配置或插件，连别人启动的它也带人设",
        Channel::Slash => "会话里用 /purge 热切换，下一轮生效",
        Channel::Project => "写项目根部的规则文件，只影响这个项目",
    }
}

/// 把"配置已写"与"运行时生效"分开的固定提示（`docs/UI.md` §2.1）。
pub const WRITE_NOT_EFFECT: &str = "配置写入不代表现有会话已生效。";

/// 顶层开关的文案。
pub fn power_label(on: bool) -> &'static str {
    if on { "已开启 · 点击暂停" } else { "已暂停 · 点击恢复" }
}

/// 草稿与已保存的比较结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DraftChange {
    /// 没有改动。
    Clean,
    /// 改了人设。
    Persona,
    /// 改了模式。
    Mode,
    /// 两者都改了。
    Both,
}

impl DraftChange {
    /// 有没有改动。
    pub fn is_dirty(self) -> bool {
        self != Self::Clean
    }

    /// 界面上的状态文字。
    pub fn label(self) -> &'static str {
        match self {
            Self::Clean => "未修改",
            Self::Persona => "人设待应用",
            Self::Mode => "模式待应用",
            Self::Both => "人设与模式待应用",
        }
    }
}

/// 比较草稿与已保存值。
pub fn draft_change(draft_persona: Option<&str>, draft_mode: Mode, saved_persona: Option<&str>, saved_mode: Mode) -> DraftChange {
    let persona_changed = draft_persona != saved_persona;
    let mode_changed = draft_mode != saved_mode;
    match (persona_changed, mode_changed) {
        (false, false) => DraftChange::Clean,
        (true, false) => DraftChange::Persona,
        (false, true) => DraftChange::Mode,
        (true, true) => DraftChange::Both,
    }
}

/// 人设列表的一项（供列表渲染与搜索）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonaRow {
    /// id。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 描述。
    pub description: Option<String>,
    /// 字数。
    pub chars: usize,
    /// 是否当前生效。
    pub active: bool,
}

/// 按关键字搜索（id 与名字都不区分大小写）。
pub fn search_personas(rows: &[PersonaRow], query: &str) -> Vec<PersonaRow> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return rows.to_vec();
    }
    rows.iter()
        .filter(|row| {
            row.id.to_lowercase().contains(&needle)
                || row.name.to_lowercase().contains(&needle)
                || row.description.as_deref().is_some_and(|d| d.to_lowercase().contains(&needle))
        })
        .cloned()
        .collect()
}

/// 解析用户输入的 harness id（界面上的输入框）。
pub fn parse_harness_id(raw: &str) -> Result<HarnessId, String> {
    HarnessId::parse(raw).map_err(|error| error.message().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_core::ActivationContext;

    fn spec(id: &str) -> &'static Spec {
        metis_harness::registry::find(id).expect("spec")
    }

    fn row(id: &str) -> HarnessRow {
        row_from_spec(spec(id), &[], None)
    }

    #[test]
    fn rows_cover_every_channel() {
        let row = row("codex");
        assert_eq!(row.channels.len(), 4);
        for channel in Channel::ALL {
            let _ = row.channel(channel);
        }
    }

    #[test]
    fn claude_install_row_is_marked_unavailable_with_reason() {
        let row = row("claude");
        let (ok, why) = row.channel(Channel::Install);
        assert!(!ok);
        assert!(why.unwrap_or_default().contains("压不住"));
    }

    #[test]
    fn antigravity_is_not_injectable() {
        let row = row("antigravity");
        assert!(!row.injectable);
        assert_eq!(row.status_line(), "不支持注入");
    }

    #[test]
    fn status_line_reflects_activation() {
        let state = metis_core::ActivationState { persona: None, default_mode: Mode::Replace, ..Default::default() };
        let harness = HarnessId::parse("codex").expect("id");
        let on = metis_core::activation::resolve_activation(&state, &harness, &ActivationContext { launch: true, ..Default::default() });
        let row = row_from_spec(spec("codex"), &[Channel::Install], Some(&on));
        assert!(row.on);
        assert!(row.status_line().contains("注入中"));
        assert_eq!(row.armed, vec![Channel::Install]);
        assert!(row.is_armed(Channel::Install));
        assert!(!row.is_armed(Channel::Shim));
    }

    #[test]
    fn filter_narrows_rows() {
        let rows = vec![row("codex"), row("antigravity"), row("claude")];
        let injectable = filter_rows(&rows, Filter { injectable_only: true, ..Filter::default() });
        assert_eq!(injectable.len(), 2, "antigravity 不可注入");

        let armed = filter_rows(&rows, Filter { armed_only: true, ..Filter::default() });
        assert!(armed.is_empty(), "都没武装时不该有行");

        let all = filter_rows(&rows, Filter::default());
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn installed_only_filter_keeps_unknown_rows_visible() {
        // 没探测出结果（None）时不能把行藏掉：用户会以为这家不存在。
        let mut rows = vec![row("codex"), row("claude")];
        rows[0].installed = Some(true);
        rows[1].installed = None;
        let kept = filter_rows(&rows, Filter { installed_only: true, ..Filter::default() });
        assert_eq!(kept.len(), 2, "未知安装状态的应保留");

        // 两个都确认没装时，都不显示。
        rows[0].installed = Some(false);
        rows[1].installed = Some(false);
        let kept = filter_rows(&rows, Filter { installed_only: true, ..Filter::default() });
        assert!(kept.is_empty(), "确认没装的应全部过滤掉");

        // 一个装了一个没装：只留装了那个。
        rows[0].installed = Some(true);
        let kept = filter_rows(&rows, Filter { installed_only: true, ..Filter::default() });
        assert_eq!(kept.len(), 1, "只留装了的");
        assert_eq!(kept[0].id, "codex");
    }

    #[test]
    fn scope_summary_counts_correctly() {
        let mut rows = vec![row("codex"), row("claude"), row("antigravity")];
        rows[0].armed = vec![Channel::Install];
        let summary = scope_summary(&rows);
        assert!(summary.contains("已接管 1 家"));
        assert!(summary.contains("可接管 2 家"), "antigravity 不算可接管：{summary}");
    }

    #[test]
    fn mode_labels_and_hints_are_complete() {
        for mode in Mode::ALL {
            assert!(!mode_label(mode).is_empty());
            assert!(!mode_hint(mode).is_empty());
        }
    }

    #[test]
    fn channel_labels_and_hints_are_complete() {
        for channel in Channel::ALL {
            assert!(!channel_label(channel).is_empty());
            assert!(!channel_hint(channel).is_empty());
        }
    }

    #[test]
    fn draft_change_detects_each_combination() {
        assert_eq!(draft_change(Some("a"), Mode::Replace, Some("a"), Mode::Replace), DraftChange::Clean);
        assert_eq!(draft_change(Some("b"), Mode::Replace, Some("a"), Mode::Replace), DraftChange::Persona);
        assert_eq!(draft_change(Some("a"), Mode::Append, Some("a"), Mode::Replace), DraftChange::Mode);
        assert_eq!(draft_change(Some("b"), Mode::Append, Some("a"), Mode::Replace), DraftChange::Both);
        assert!(!DraftChange::Clean.is_dirty());
        assert!(DraftChange::Both.is_dirty());
        assert!(!DraftChange::Both.label().is_empty());
    }

    #[test]
    fn power_label_is_actionable() {
        assert!(power_label(true).contains("暂停"));
        assert!(power_label(false).contains("恢复"));
    }

    #[test]
    fn search_matches_id_name_and_description() {
        let rows = vec![
            PersonaRow { id: "concise".to_owned(), name: "简洁协作".to_owned(), description: Some("先结论".to_owned()), chars: 10, active: true },
            PersonaRow { id: "strict".to_owned(), name: "严格".to_owned(), description: None, chars: 5, active: false },
        ];
        assert_eq!(search_personas(&rows, "").len(), 2, "空查询返回全部");
        assert_eq!(search_personas(&rows, "CONC").len(), 1, "id 搜索不区分大小写");
        assert_eq!(search_personas(&rows, "简洁").len(), 1, "名字可搜");
        assert_eq!(search_personas(&rows, "先结论").len(), 1, "描述可搜");
        assert!(search_personas(&rows, "nope").is_empty());
    }

    #[test]
    fn parse_harness_id_reports_a_message() {
        assert!(parse_harness_id("codex").is_ok());
        assert!(parse_harness_id("a/b").is_err());
    }

    #[test]
    fn write_not_effect_hint_exists() {
        assert!(WRITE_NOT_EFFECT.contains("不代表"));
    }
}
