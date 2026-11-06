//! 通道与生效时机。

use serde::{Deserialize, Serialize};

/// 注入通道。顺序即优先级顺序（与基线的 `ARMS` 一致）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    /// 裸命令接管：PATH 最前放同名命令。
    Shim,
    /// 会话内 `/purge` 扩展。
    Slash,
    /// 宿主配置文件 / 插件常驻。
    Install,
    /// 项目规则文件（`AGENTS.md` / `CLAUDE.md`）。
    Project,
}

impl Channel {
    /// 全部通道，顺序即界面展示与 `armed` 数组的规范顺序。
    pub const ALL: [Self; 4] = [Self::Shim, Self::Slash, Self::Install, Self::Project];

    /// 序列化用的字符串值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Shim => "shim",
            Self::Slash => "slash",
            Self::Install => "install",
            Self::Project => "project",
        }
    }

    /// 解析字符串。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "shim" => Some(Self::Shim),
            "slash" => Some(Self::Slash),
            "install" => Some(Self::Install),
            "project" => Some(Self::Project),
            _ => None,
        }
    }

    /// 这条通道写入后，什么时候才会被宿主读取。
    pub const fn effect(self) -> Effect {
        match self {
            Self::Shim => Effect::NextLaunch,
            Self::Slash => Effect::NextTurn,
            Self::Project => Effect::NextSession,
            Self::Install => Effect::NextSession,
        }
    }

    /// 这条通道是否会改动磁盘。
    pub const fn writes_disk(self) -> bool {
        true
    }
}

impl std::fmt::Display for Channel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 生效时机。**这是产品语义**：报告与界面都必须按它说话，不许把"已写入"说成"已生效"。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// 下一轮对话生效（动态插件每轮重读）。
    NextTurn,
    /// 下次从该命令启动生效。
    NextLaunch,
    /// 需要新会话。
    NextSession,
}

impl Effect {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NextTurn => "next-turn",
            Self::NextLaunch => "next-launch",
            Self::NextSession => "next-session",
        }
    }

    /// 面向用户的一句话。
    pub const fn message(self) -> &'static str {
        match self {
            Self::NextTurn => "已加载扩展下一轮读取；首次安装需重启",
            Self::NextLaunch => "新启动的命令生效；已运行会话不变",
            Self::NextSession => "配置已写入；新会话读取",
        }
    }
}

impl std::fmt::Display for Effect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 一条通道在某家执行器上的可用性。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelState {
    /// 能不能用。
    pub ok: bool,
    /// 不能用时的原因（面向用户的一句话）。`ok == true` 时通常为 `None`。
    pub why: Option<String>,
}

impl ChannelState {
    /// 可用。
    pub fn available() -> Self {
        Self { ok: true, why: None }
    }

    /// 不可用，并说明原因。
    pub fn unavailable(why: impl Into<String>) -> Self {
        Self { ok: false, why: Some(why.into()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_round_trips() {
        for channel in Channel::ALL {
            assert_eq!(Channel::parse(channel.as_str()), Some(channel));
        }
        assert_eq!(Channel::parse("bogus"), None);
        assert_eq!(Channel::parse(" SHIM "), Some(Channel::Shim));
    }

    #[test]
    fn channel_order_is_the_canonical_arm_order() {
        // armed 数组必须按这个顺序规范化，否则两次 apply 会产生不同的字节。
        assert_eq!(Channel::ALL, [Channel::Shim, Channel::Slash, Channel::Install, Channel::Project]);
    }

    #[test]
    fn effects_match_the_channel_semantics() {
        assert_eq!(Channel::Shim.effect(), Effect::NextLaunch);
        assert_eq!(Channel::Slash.effect(), Effect::NextTurn);
        assert_eq!(Channel::Project.effect(), Effect::NextSession);
        assert_eq!(Channel::Install.effect(), Effect::NextSession);
    }

    #[test]
    fn effect_strings_are_kebab_case() {
        assert_eq!(Effect::NextTurn.as_str(), "next-turn");
        assert_eq!(Effect::NextLaunch.as_str(), "next-launch");
        assert_eq!(Effect::NextSession.as_str(), "next-session");
        assert_eq!(serde_json::to_string(&Effect::NextTurn).unwrap_or_default(), "\"next-turn\"");
    }

    #[test]
    fn every_effect_has_a_user_facing_message() {
        for effect in [Effect::NextTurn, Effect::NextLaunch, Effect::NextSession] {
            assert!(!effect.message().is_empty(), "{effect} 缺少面向用户的说明");
        }
    }

    #[test]
    fn channel_state_constructors() {
        let ok = ChannelState::available();
        assert!(ok.ok);
        assert!(ok.why.is_none());

        let no = ChannelState::unavailable("本机 PATH 里找不到 codex");
        assert!(!no.ok);
        assert_eq!(no.why.as_deref(), Some("本机 PATH 里找不到 codex"));
    }
}
