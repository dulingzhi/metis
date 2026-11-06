//! 激活解析：配置值 vs 运行时许可。
//!
//! 这是全项目最容易出错、也最重要的纯函数。**判定顺序即产品语义**，顺序改了就换了行为
//! （`docs/ARCHITECTURE.md` §4.2 / 不变式 I5）。
//!
//! 关键不变式：环境变量、命令行 token、任何外部输入**都不能**直接把 `on` 变成 true；
//! 只有经进程归属核验的显式启动路径可以传 `launch: true`。

use serde::{Deserialize, Serialize};

use crate::channel::Channel;
use crate::harness::HarnessId;
use crate::mode::Mode;
use crate::persona::PersonaId;

/// 分家例外（`state.overrides[harness]`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Override {
    /// 这一家单独用的人设。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub persona: Option<PersonaId>,
    /// 这一家单独用的模式。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub mode: Option<Mode>,
}

impl Override {
    /// 这个例外是否什么都没设（设了就不该留在 `state.overrides` 里）。
    pub fn is_empty(&self) -> bool {
        self.persona.is_none() && self.mode.is_none()
    }
}

/// 全局选中的配置（从 `state.json` 摘出来的最小集合）。
///
/// 刻意只带解析激活需要的东西：解析器不该知道人设正文、账本、通道实现。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Configured {
    /// 全局人设。
    pub persona: Option<PersonaId>,
    /// 全局模式（`state.default.mode`）。
    pub mode: Mode,
}

/// 解析后的配置值（还没判断"允不允许"）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// 生效的人设。
    pub persona: Option<PersonaId>,
    /// 生效的模式。
    pub mode: Mode,
    /// 人设来自哪里。
    pub persona_from: Source,
    /// 模式来自哪里。
    pub mode_from: Source,
}

/// 一个值来自全局还是分家例外。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    /// 分家例外。
    Override,
    /// 全局配置。
    Global,
}

impl Source {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Override => "override",
            Self::Global => "global",
        }
    }
}

/// 激活的许可来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActivationSource {
    /// 显式启动（`metis launch <h>`，且经过了进程归属核验）。
    ExplicitLaunch,
    /// 常驻配置已武装。
    Resident,
    /// 会话内命令（`/purge on`）。
    SessionCommand,
    /// 扩展的自动注入开关（`piAuto` / `ompAuto`）。
    Auto,
}

impl ActivationSource {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitLaunch => "explicit-launch",
            Self::Resident => "resident",
            Self::SessionCommand => "session-command",
            Self::Auto => "auto",
        }
    }
}

/// 被拦下的原因。**顺序即判定顺序**，展示层按这个顺序解释"为什么没生效"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BlockReason {
    /// 状态不存在或不是对象。
    MissingState,
    /// 全局暂停（总电源关着）。
    GlobalPause,
    /// 这一家被单独设成 off。
    HarnessOff,
    /// 这一家在 `disabledHarnesses` 里。
    HarnessDisabled,
    /// 会话层关闭。
    SessionOff,
    /// DSH profile 不在白名单里。
    ProfileOff,
    /// 模式是 off。
    ModeOff,
    /// 模式值非法。
    InvalidMode,
    /// 没有任何通道提供许可。
    NotEnabled,
}

impl BlockReason {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MissingState => "missing-state",
            Self::GlobalPause => "global-pause",
            Self::HarnessOff => "harness-off",
            Self::HarnessDisabled => "harness-disabled",
            Self::SessionOff => "session-off",
            Self::ProfileOff => "profile-off",
            Self::ModeOff => "mode-off",
            Self::InvalidMode => "invalid-mode",
            Self::NotEnabled => "not-enabled",
        }
    }

    /// 面向用户的一句话。
    pub const fn message(self) -> &'static str {
        match self {
            Self::MissingState => "读不到配置状态",
            Self::GlobalPause => "总电源关着（暂停中）",
            Self::HarnessOff => "这一家的模式被设为 off",
            Self::HarnessDisabled => "这一家被整体停用",
            Self::SessionOff => "本次会话关闭了注入",
            Self::ProfileOff => "这个 profile 不在处理范围内",
            Self::ModeOff => "当前模式是 off",
            Self::InvalidMode => "模式值非法",
            Self::NotEnabled => "没有已开启的通道",
        }
    }
}

/// 解析的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Activation {
    /// 配置值（未考虑许可）。
    pub resolved: Resolved,
    /// 是否真的注入。
    pub on: bool,
    /// 许可来源；`on == false` 时为 `None`。
    pub source: Option<ActivationSource>,
    /// 被拦下的原因；`on == true` 时为 `None`。
    pub blocked_by: Option<BlockReason>,
}

impl Activation {
    /// 生效的模式：没被许可时对外一律表现为 `off`。
    ///
    /// 这条很重要：调用方拿到 `Activation` 后不该再自己判断"要不要当 off 处理"。
    pub fn effective_mode(&self) -> Mode {
        if self.on { self.resolved.mode } else { Mode::Off }
    }

    /// 一句人话解释当前状态。
    pub fn explain(&self) -> String {
        if self.on {
            match self.source {
                Some(ActivationSource::ExplicitLaunch) => "本次启动注入".to_owned(),
                Some(ActivationSource::Resident) => "常驻配置生效".to_owned(),
                Some(ActivationSource::SessionCommand) => "会话内命令开启".to_owned(),
                Some(ActivationSource::Auto) => "扩展自动注入".to_owned(),
                None => "注入中".to_owned(),
            }
        } else {
            self.blocked_by.map(|r| r.message().to_owned()).unwrap_or_else(|| "未开启".to_owned())
        }
    }
}

/// 状态里与激活有关的部分。调用方从 `state.json` 填进来。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivationState {
    /// 全局人设。
    pub persona: Option<PersonaId>,
    /// `default.mode`：同时是总电源。
    pub default_mode: Mode,
    /// 分家例外。
    pub overrides: Vec<(HarnessId, Override)>,
    /// 每家武装了哪些通道。
    pub armed: Vec<(HarnessId, Vec<Channel>)>,
    /// 整体停用的家。
    pub disabled_harnesses: Vec<HarnessId>,
    /// DSH 要处理的 profile 白名单；`None` 表示不限。
    pub dsh_profiles: Option<Vec<String>>,
    /// `piAuto` / `ompAuto`：按 harness id 存。
    pub auto_inject: Vec<(HarnessId, bool)>,
}

impl ActivationState {
    fn override_for(&self, harness: &HarnessId) -> Option<&Override> {
        self.overrides.iter().find(|(id, _)| id == harness).map(|(_, o)| o)
    }

    fn armed_for(&self, harness: &HarnessId) -> &[Channel] {
        self.armed.iter().find(|(id, _)| id == harness).map(|(_, c)| c.as_slice()).unwrap_or(&[])
    }

    fn auto_for(&self, harness: &HarnessId) -> bool {
        self.auto_inject.iter().find(|(id, _)| id == harness).is_some_and(|(_, on)| *on)
    }
}

/// 解析激活时的上下文。**全部默认 `None`/`true`，即"什么都不额外声明"**。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ActivationContext {
    /// 显式启动。只有经过进程归属核验的调用方可以传 `true`。
    pub launch: bool,
    /// 本次调用来自哪条通道（`install` / `slash`）。
    pub channel: Option<Channel>,
    /// 会话层开关。
    pub session: Option<bool>,
    /// 整体开关（`--enabled` 之类的调用方声明）。
    pub enabled: Option<bool>,
    /// DSH profile。
    pub profile: Option<String>,
    /// 本次启动临时覆盖的模式。
    pub mode: Option<Mode>,
    /// 本次启动临时覆盖的人设。
    pub persona: Option<PersonaId>,
}

/// 只解析配置值：分家例外优先，否则跟随全局。
pub fn resolve_configured(state: &ActivationState, harness: &HarnessId) -> Resolved {
    let over = state.override_for(harness);
    let persona = over.and_then(|o| o.persona.clone());
    let mode = over.and_then(|o| o.mode);
    Resolved {
        persona_from: if persona.is_some() { Source::Override } else { Source::Global },
        mode_from: if mode.is_some() { Source::Override } else { Source::Global },
        persona: persona.or_else(|| state.persona.clone()),
        mode: mode.unwrap_or(state.default_mode),
    }
}

/// 解析激活：**九步短路，顺序不可改**。
pub fn resolve_activation(state: &ActivationState, harness: &HarnessId, ctx: &ActivationContext) -> Activation {
    let resolved = resolve_configured(state, harness);
    // 本次启动的临时覆盖只影响解析结果，不写盘、不授予许可。
    let resolved = Resolved { persona: ctx.persona.clone().or(resolved.persona), mode: ctx.mode.unwrap_or(resolved.mode), ..resolved };

    let stop = |reason: BlockReason, resolved: Resolved| Activation {
        resolved: Resolved { mode: Mode::Off, ..resolved },
        on: false,
        source: None,
        blocked_by: Some(reason),
    };

    // ① 全局暂停：压过任何分家例外。
    if state.default_mode == Mode::Off || resolved.mode == Mode::Off && state.default_mode == Mode::Off {
        return stop(BlockReason::GlobalPause, resolved);
    }
    // ② 这一家被单独设成 off。
    if state.override_for(harness).and_then(|o| o.mode) == Some(Mode::Off) {
        return stop(BlockReason::HarnessOff, resolved);
    }
    // ③ 这一家在 disabledHarnesses 里。
    if state.disabled_harnesses.iter().any(|id| id == harness) {
        return stop(BlockReason::HarnessDisabled, resolved);
    }
    // ④ 会话层关闭。
    if ctx.session == Some(false) || ctx.enabled == Some(false) {
        return stop(BlockReason::SessionOff, resolved);
    }
    // ⑤ DSH profile 不在白名单里。
    if let (Some(profile), Some(allowed)) = (&ctx.profile, &state.dsh_profiles)
        && !allowed.iter().any(|p| p == profile)
    {
        return stop(BlockReason::ProfileOff, resolved);
    }
    // ⑥ 模式必须是三个"注入"值之一。
    if !resolved.mode.injects() {
        return stop(BlockReason::ModeOff, resolved);
    }

    // ⑦ 找许可来源：显式启动 > install 已武装 > slash 已武装且（会话命令 或 Auto）。
    //
    // `ctx.channel == None` 表示"问这家整体在不在注入"（界面的状态行、`harness.list`），
    // 此时**任意一条已武装的注入通道**都算许可 —— 这正是用户看到"接管开关是亮的"想要的意思。
    // 判定单条通道时（apply / doctor）仍然由调用方显式传 `channel`，语义不变。
    let channels = state.armed_for(harness);
    let aggregates = ctx.channel.is_none();
    let source = if ctx.launch {
        Some(ActivationSource::ExplicitLaunch)
    } else if (aggregates || ctx.channel == Some(Channel::Install)) && channels.contains(&Channel::Install) {
        Some(ActivationSource::Resident)
    } else if (aggregates || ctx.channel == Some(Channel::Slash)) && channels.contains(&Channel::Slash) {
        if ctx.session == Some(true) {
            Some(ActivationSource::SessionCommand)
        } else if state.auto_for(harness) {
            Some(ActivationSource::Auto)
        } else {
            None
        }
    } else {
        None
    };

    match source {
        Some(source) => Activation { resolved, on: true, source: Some(source), blocked_by: None },
        None => stop(BlockReason::NotEnabled, resolved),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("harness id")
    }

    fn persona(value: &str) -> PersonaId {
        PersonaId::parse(value).expect("persona id")
    }

    fn base() -> ActivationState {
        ActivationState { persona: Some(persona("concise")), default_mode: Mode::Replace, ..ActivationState::default() }
    }

    #[test]
    fn override_wins_over_global_for_mode_and_persona() {
        let mut state = base();
        state.overrides = vec![(id("codex"), Override { persona: Some(persona("strict-one")), mode: Some(Mode::Append) })];
        let resolved = resolve_configured(&state, &id("codex"));
        assert_eq!(resolved.mode, Mode::Append);
        assert_eq!(resolved.persona.as_ref().map(PersonaId::as_str), Some("strict-one"));
        assert_eq!(resolved.mode_from, Source::Override);
        assert_eq!(resolved.persona_from, Source::Override);
    }

    #[test]
    fn other_harness_follows_global() {
        let mut state = base();
        state.overrides = vec![(id("codex"), Override { mode: Some(Mode::Append), persona: None })];
        let resolved = resolve_configured(&state, &id("claude"));
        assert_eq!(resolved.mode, Mode::Replace);
        assert_eq!(resolved.mode_from, Source::Global);
    }

    #[test]
    fn explicit_launch_is_enough_even_with_no_channels() {
        let state = base();
        let act = resolve_activation(&state, &id("codex"), &ActivationContext { launch: true, ..ActivationContext::default() });
        assert!(act.on);
        assert_eq!(act.source, Some(ActivationSource::ExplicitLaunch));
        assert_eq!(act.blocked_by, None);
        assert_eq!(act.effective_mode(), Mode::Replace);
    }

    #[test]
    fn without_any_permission_it_is_not_enabled() {
        let state = base();
        let act = resolve_activation(&state, &id("codex"), &ActivationContext::default());
        assert!(!act.on);
        assert_eq!(act.blocked_by, Some(BlockReason::NotEnabled));
        assert_eq!(act.effective_mode(), Mode::Off, "没被许可时对外一律 off");
    }

    #[test]
    fn resident_channel_grants_permission() {
        let mut state = base();
        state.armed = vec![(id("codex"), vec![Channel::Install])];
        let act = resolve_activation(&state, &id("codex"), &ActivationContext { channel: Some(Channel::Install), ..ActivationContext::default() });
        assert!(act.on);
        assert_eq!(act.source, Some(ActivationSource::Resident));
    }

    #[test]
    fn slash_needs_a_session_or_auto() {
        let mut state = base();
        state.armed = vec![(id("pi"), vec![Channel::Slash])];
        // 没有 session、没有 auto → 不授予
        let idle = resolve_activation(&state, &id("pi"), &ActivationContext { channel: Some(Channel::Slash), ..ActivationContext::default() });
        assert!(!idle.on);
        assert_eq!(idle.blocked_by, Some(BlockReason::NotEnabled));

        // 会话内命令 → 授予
        let session =
            resolve_activation(&state, &id("pi"), &ActivationContext { channel: Some(Channel::Slash), session: Some(true), ..ActivationContext::default() });
        assert_eq!(session.source, Some(ActivationSource::SessionCommand));

        // Auto → 授予
        state.auto_inject = vec![(id("pi"), true)];
        let auto = resolve_activation(&state, &id("pi"), &ActivationContext { channel: Some(Channel::Slash), ..ActivationContext::default() });
        assert_eq!(auto.source, Some(ActivationSource::Auto));
    }

    #[test]
    fn without_a_channel_context_any_armed_channel_counts() {
        // 界面上的状态行问的是"这家整体在不在注入"。
        // 已经武装 install、但没指定 channel 时，必须报"在注入"，否则开关亮着、状态却说没开启。
        let mut state = base();
        state.armed = vec![(id("codex"), vec![Channel::Install])];
        let aggregate = resolve_activation(&state, &id("codex"), &ActivationContext::default());
        assert!(aggregate.on, "没有 channel 上下文时应按聚合判定");
        assert_eq!(aggregate.source, Some(ActivationSource::Resident));

        // 指定了 channel 且没武装时，仍然是否定的 —— 单通道语义没被改坏。
        let single = resolve_activation(&state, &id("codex"), &ActivationContext { channel: Some(Channel::Shim), ..ActivationContext::default() });
        assert!(!single.on);
    }

    #[test]
    fn aggregate_slash_registration_alone_is_not_enough() {
        // slash 是"开关"：光武装 slash、没有会话命令也没有 Auto，不算在注入。
        let mut state = base();
        state.armed = vec![(id("pi"), vec![Channel::Slash])];
        let aggregate = resolve_activation(&state, &id("pi"), &ActivationContext::default());
        assert!(!aggregate.on, "slash 单独武装不该被判成注入中");
    }

    #[test]
    fn global_pause_beats_every_override() {
        let mut state = base();
        state.default_mode = Mode::Off;
        state.overrides = vec![(id("codex"), Override { mode: Some(Mode::Replace), persona: None })];
        state.armed = vec![(id("codex"), vec![Channel::Install])];
        let act = resolve_activation(&state, &id("codex"), &ActivationContext { launch: true, ..ActivationContext::default() });
        assert!(!act.on, "总电源关着时连显式启动也不注入");
        assert_eq!(act.blocked_by, Some(BlockReason::GlobalPause));
    }

    #[test]
    fn harness_off_and_disabled_are_distinct_reasons() {
        let mut state = base();
        state.overrides = vec![(id("claude"), Override { mode: Some(Mode::Off), persona: None })];
        let off = resolve_activation(&state, &id("claude"), &ActivationContext { launch: true, ..ActivationContext::default() });
        assert_eq!(off.blocked_by, Some(BlockReason::HarnessOff));

        let mut state2 = base();
        state2.disabled_harnesses = vec![id("grok")];
        let disabled = resolve_activation(&state2, &id("grok"), &ActivationContext { launch: true, ..ActivationContext::default() });
        assert_eq!(disabled.blocked_by, Some(BlockReason::HarnessDisabled));
    }

    #[test]
    fn session_off_and_profile_off_follow() {
        let mut state = base();
        state.armed = vec![(id("dsh"), vec![Channel::Install])];
        let ctx = ActivationContext { channel: Some(Channel::Install), session: Some(false), ..ActivationContext::default() };
        assert_eq!(resolve_activation(&state, &id("dsh"), &ctx).blocked_by, Some(BlockReason::SessionOff));

        state.dsh_profiles = Some(vec!["desktop".to_owned()]);
        let ctx = ActivationContext { channel: Some(Channel::Install), profile: Some("web".to_owned()), ..ActivationContext::default() };
        assert_eq!(resolve_activation(&state, &id("dsh"), &ctx).blocked_by, Some(BlockReason::ProfileOff));

        let ctx = ActivationContext { channel: Some(Channel::Install), profile: Some("desktop".to_owned()), ..ActivationContext::default() };
        assert!(resolve_activation(&state, &id("dsh"), &ctx).on, "白名单内的 profile 应放行");
    }

    #[test]
    fn mode_off_blocks_even_with_launch() {
        let mut state = base();
        state.mode_is_off_via_ctx();
        // 通过 ctx.mode 传 off
        let act = resolve_activation(&state, &id("codex"), &ActivationContext { launch: true, mode: Some(Mode::Off), ..ActivationContext::default() });
        assert_eq!(act.blocked_by, Some(BlockReason::ModeOff));
        assert!(!act.on);
    }

    impl ActivationState {
        fn mode_is_off_via_ctx(&mut self) {}
    }

    #[test]
    fn context_persona_and_mode_override_without_granting_permission() {
        let state = base();
        let ctx = ActivationContext { persona: Some(persona("temp")), mode: Some(Mode::Append), ..ActivationContext::default() };
        let act = resolve_activation(&state, &id("codex"), &ctx);
        assert!(!act.on, "临时覆盖不得授予许可");
        assert_eq!(act.resolved.persona.as_ref().map(PersonaId::as_str), Some("temp"));
        assert_eq!(act.resolved.mode, Mode::Off, "被拦下时对外模式是 off");
    }

    #[test]
    fn explain_is_always_human_readable() {
        let state = base();
        let blocked = resolve_activation(&state, &id("codex"), &ActivationContext::default());
        assert!(!blocked.explain().is_empty());

        let on = resolve_activation(&state, &id("codex"), &ActivationContext { launch: true, ..ActivationContext::default() });
        assert!(on.explain().contains("启动"));
    }

    #[test]
    fn every_block_reason_has_a_message() {
        let reasons = [
            BlockReason::MissingState,
            BlockReason::GlobalPause,
            BlockReason::HarnessOff,
            BlockReason::HarnessDisabled,
            BlockReason::SessionOff,
            BlockReason::ProfileOff,
            BlockReason::ModeOff,
            BlockReason::InvalidMode,
            BlockReason::NotEnabled,
        ];
        for reason in reasons {
            assert!(!reason.message().is_empty(), "{reason:?} 缺少面向用户的说明");
            assert!(!reason.as_str().is_empty());
        }
    }

    #[test]
    fn override_is_empty_when_nothing_set() {
        assert!(Override::default().is_empty());
        assert!(!Override { mode: Some(Mode::Append), persona: None }.is_empty());
    }
}
