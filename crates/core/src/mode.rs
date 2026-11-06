//! 提示词处理模式。字符串值与 agent-purge 逐字兼容（`state.json` 里存的就是这些）。

use serde::{Deserialize, Serialize};

/// 处理模式。
///
/// - [`Mode::Replace`] 换身份：优先替换宿主的身份行；没有读取口的入口按 strict 处理。
/// - [`Mode::Strict`] 只留人设：宿主原有说明一并去掉。
/// - [`Mode::Append`] 追加：宿主说明保留，人设另加一份（可能出现两份矛盾身份）。
/// - [`Mode::Off`] 不注入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// 只留人设。
    Strict,
    /// 换身份（默认）。
    #[default]
    Replace,
    /// 追加。
    Append,
    /// 不注入。
    Off,
}

impl Mode {
    /// 默认模式：`replace`（与基线一致）。
    pub const DEFAULT: Self = Self::Replace;

    /// 全部取值，顺序即界面展示顺序。
    pub const ALL: [Self; 4] = [Self::Replace, Self::Strict, Self::Append, Self::Off];

    /// 序列化用的字符串值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Replace => "replace",
            Self::Append => "append",
            Self::Off => "off",
        }
    }

    /// 解析字符串。大小写不敏感，但**不做模糊匹配**：认不出来就是认不出来。
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "strict" => Some(Self::Strict),
            "replace" => Some(Self::Replace),
            "append" => Some(Self::Append),
            "off" => Some(Self::Off),
            _ => None,
        }
    }

    /// 这个模式会不会真的注入内容。
    pub fn injects(self) -> bool {
        self != Self::Off
    }

    /// 这个模式需要人设正文吗（`off` 不需要，所以暂停时不需要人设也能撤）。
    pub fn needs_persona(self) -> bool {
        self.injects()
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_display_and_parse() {
        for mode in Mode::ALL {
            assert_eq!(Mode::parse(mode.as_str()), Some(mode), "{mode} 不能往返");
        }
    }

    #[test]
    fn parse_is_case_insensitive_but_not_fuzzy() {
        assert_eq!(Mode::parse("  APPEND "), Some(Mode::Append));
        assert_eq!(Mode::parse("Replace"), Some(Mode::Replace));
        for bad in ["", "appendd", "offf", "none", "0", "严格"] {
            assert_eq!(Mode::parse(bad), None, "{bad:?} 不该被接受");
        }
    }

    #[test]
    fn serde_uses_the_plain_strings() {
        assert_eq!(serde_json::to_string(&Mode::Replace).unwrap_or_default(), "\"replace\"");
        assert_eq!(serde_json::from_str::<Mode>("\"off\"").ok(), Some(Mode::Off));
        assert!(serde_json::from_str::<Mode>("\"OFF\"").is_err(), "serde 层必须严格，不偷偷归一化");
    }

    #[test]
    fn off_is_the_only_mode_that_does_not_inject() {
        assert!(!Mode::Off.injects());
        assert!(!Mode::Off.needs_persona());
        for mode in [Mode::Replace, Mode::Strict, Mode::Append] {
            assert!(mode.injects());
            assert!(mode.needs_persona());
        }
    }

    #[test]
    fn default_is_replace() {
        assert_eq!(Mode::DEFAULT, Mode::Replace);
        let parsed: Mode = serde_json::from_str("null").unwrap_or(Mode::DEFAULT);
        assert_eq!(parsed, Mode::Replace, "缺省字段应回落到 replace");
    }
}
