//! 能力表：这家能用哪条通道、为什么不能用。
//!
//! **唯一真相**：`docs/HARNESS-MATRIX.md`。本文件是它的代码形态。
//! 每个 `why` 都是给用户看的解释，必须有测试（AGENTS.md 单测契约）。

use metis_core::{Channel, ChannelState};

/// 热切能力。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotSwitch {
    /// 能不能同会话热切。
    pub ok: bool,
    /// 哪种触发方式。
    pub kind: Option<HotSwitchKind>,
    /// 依赖哪条通道才成立。
    pub needs: Option<Channel>,
    /// 面向用户的说明：怎么生效、为什么不能。
    pub how: String,
}

impl HotSwitch {
    /// 不支持热切，并说明为什么。
    pub fn no(why: impl Into<String>) -> Self {
        Self { ok: false, kind: None, needs: None, how: why.into() }
    }

    /// 支持热切。
    pub fn yes(kind: HotSwitchKind, needs: Channel, how: impl Into<String>) -> Self {
        Self { ok: true, kind: Some(kind), needs: Some(needs), how: how.into() }
    }
}

/// 热切的触发方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotSwitchKind {
    /// 会话里打 `/purge`。
    Command,
    /// 改设置，下一轮读到。
    State,
}

impl HotSwitchKind {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Command => "command",
            Self::State => "state",
        }
    }
}

/// 一家的四条通道能力 + 热切。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    /// 这家整家能不能注入。
    pub injectable: bool,
    /// 裸命令接管。
    pub shim: ChannelState,
    /// 常驻配置 / 插件。
    pub install: ChannelState,
    /// 会话内 `/purge`。
    pub slash: ChannelState,
    /// 项目规则文件。
    pub project: ChannelState,
    /// 热切。
    pub hot: HotSwitch,
    /// 安装后的层级（若适用）。
    pub layer: Option<&'static str>,
}

impl Capabilities {
    /// 取某条通道的状态。
    pub fn channel(&self, channel: Channel) -> &ChannelState {
        match channel {
            Channel::Shim => &self.shim,
            Channel::Slash => &self.slash,
            Channel::Install => &self.install,
            Channel::Project => &self.project,
        }
    }

    /// 至少有一条通道能用吗（全不能用 = 只能原样启动）。
    pub fn any(&self) -> bool {
        self.shim.ok || self.install.ok || self.slash.ok || self.project.ok
    }

    /// 全部不可用的原因摘要（用于"这家注入不进去"的提示）。
    pub fn blocked_summary(&self) -> Option<String> {
        if self.any() {
            return None;
        }
        Some(self.install.why.clone().or_else(|| self.shim.why.clone()).unwrap_or_else(|| "没有可用通道".to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(injectable: bool) -> Capabilities {
        Capabilities {
            injectable,
            shim: ChannelState::available(),
            install: ChannelState::unavailable("没有可写的配置文件"),
            slash: ChannelState::unavailable("不支持 /purge"),
            project: ChannelState::available(),
            hot: HotSwitch::no("system 只在启动时拼一次"),
            layer: Some("sys"),
        }
    }

    #[test]
    fn channel_accessor_maps_each_variant() {
        let capabilities = caps(true);
        assert!(capabilities.channel(Channel::Shim).ok);
        assert!(!capabilities.channel(Channel::Install).ok);
        assert_eq!(capabilities.channel(Channel::Install).why.as_deref(), Some("没有可写的配置文件"));
    }

    #[test]
    fn any_is_true_when_at_least_one_channel_works() {
        assert!(caps(true).any());
    }

    #[test]
    fn blocked_summary_only_when_nothing_works() {
        let mut all_blocked = caps(false);
        all_blocked.shim = ChannelState::unavailable("桌面版绕开 PATH");
        all_blocked.project = ChannelState::unavailable("没有项目文件");
        let summary = all_blocked.blocked_summary().expect("should be blocked");
        assert!(summary.contains("配置") || summary.contains("PATH"), "摘要应说清原因：{summary}");

        assert!(caps(true).blocked_summary().is_none());
    }

    #[test]
    fn hot_switch_constructors_carry_their_reason() {
        let no = HotSwitch::no("AGENTS.md 启动时读一次");
        assert!(!no.ok);
        assert_eq!(no.kind, None);
        assert!(no.how.contains("启动时读一次"));

        let yes = HotSwitch::yes(HotSwitchKind::State, Channel::Install, "插件每轮读 state");
        assert!(yes.ok);
        assert_eq!(yes.kind, Some(HotSwitchKind::State));
        assert_eq!(yes.needs, Some(Channel::Install));
    }

    #[test]
    fn hot_switch_kind_strings() {
        assert_eq!(HotSwitchKind::Command.as_str(), "command");
        assert_eq!(HotSwitchKind::State.as_str(), "state");
    }
}
