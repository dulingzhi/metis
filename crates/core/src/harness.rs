//! 执行器标识。
//!
//! 是一个开放集合：内置 15 家有常量，但新增一家不需要改这个类型 —— 只要在 `metis-harness`
//! 的注册表里登记（AGENTS.md「扩展走接缝」）。

use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, MetisError, Result};

/// 执行器 id（例如 `codex`、`opencode2`、`pi-desktop`）。
///
/// 比较区分大小写（id 是稳定标识，不是用户输入），但**解析**时会先 trim + 转小写。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct HarnessId(String);

impl HarnessId {
    /// 内置的 15 家（与 `docs/HARNESS-MATRIX.md` 的总览表一一对应）。
    pub const BUILTIN: [&'static str; 15] = [
        "pi",
        "pi-desktop",
        "omp",
        "opencode",
        "opencode2",
        "dsh",
        "codex",
        "claude",
        "grok",
        "codebuddy",
        "hermes",
        "kilo",
        "workbuddy",
        "zcode",
        "antigravity",
    ];

    /// 从字符串构造，校验字符集。
    ///
    /// 允许：小写字母、数字、`-`、`_`。不允许为空、不允许大写、不允许路径分隔符
    /// （id 会进文件名与账本键，不能有歧义）。
    pub fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(MetisError::new(ErrorKind::InvalidArgument, "执行器 id 不能为空").with_remediation("用 metis doctor 查看本机已安装的执行器"));
        }
        let lowered = trimmed.to_ascii_lowercase();
        let valid = lowered.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
        if !valid || lowered.contains('/') || lowered.contains('\\') || lowered.contains("..") {
            return Err(
                MetisError::new(ErrorKind::InvalidArgument, format!("执行器 id 非法：{value:?}")).with_remediation("id 只能由小写字母、数字、- 和 _ 组成")
            );
        }
        Ok(Self(lowered))
    }

    /// 构造一个内置 id。调用方保证传入的是 `BUILTIN` 里的值（否则返回错误而不是 panic）。
    pub fn builtin(value: &str) -> Result<Self> {
        if !Self::BUILTIN.contains(&value) {
            return Err(
                MetisError::new(ErrorKind::UnknownHarness, format!("未知执行器：{value}")).with_remediation(format!("可用：{}", Self::BUILTIN.join(", ")))
            );
        }
        Self::parse(value)
    }

    /// 字符串视图。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 是内置的 15 家之一吗。
    pub fn is_builtin(&self) -> bool {
        Self::BUILTIN.contains(&self.0.as_str())
    }
}

impl std::fmt::Display for HarnessId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for HarnessId {
    type Err = MetisError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_list_has_no_duplicates() {
        let mut sorted = HarnessId::BUILTIN.to_vec();
        sorted.sort_unstable();
        let before = sorted.len();
        sorted.dedup();
        assert_eq!(before, sorted.len());
        assert_eq!(before, 15);
    }

    #[test]
    fn parse_normalises_case_and_whitespace() {
        assert_eq!(HarnessId::parse("  Codex ").map(|h| h.as_str().to_owned()), Ok("codex".to_owned()));
        assert_eq!(HarnessId::parse("PI-DESKTOP").map(|h| h.as_str().to_owned()), Ok("pi-desktop".to_owned()));
    }

    #[test]
    fn parse_rejects_path_like_and_empty_input() {
        for bad in ["", "   ", "a/b", "a\\b", "../x", "co dex", "codex!", "日本語"] {
            let result = HarnessId::parse(bad);
            assert!(result.is_err(), "{bad:?} 应该被拒绝");
            assert_eq!(result.err().map(|e| e.kind()), Some(ErrorKind::InvalidArgument), "{bad:?} 的错误类别应为 InvalidArgument");
        }
    }

    #[test]
    fn builtin_accepts_only_known_ids() {
        for id in HarnessId::BUILTIN {
            let parsed = HarnessId::builtin(id);
            assert!(parsed.is_ok(), "{id} 应被接受");
            assert!(parsed.map(|h| h.is_builtin()).unwrap_or(false));
        }
        let unknown = HarnessId::builtin("cursor");
        assert_eq!(unknown.as_ref().err().map(|e| e.kind()), Some(ErrorKind::UnknownHarness));
        // 补救建议里必须列出可用 id，否则用户不知道下一步做什么。
        let err = unknown.err().map(|e| e.remediation().unwrap_or_default().to_owned()).unwrap_or_default();
        assert!(err.contains("codex"));
    }

    #[test]
    fn serde_is_transparent() {
        let id = HarnessId::parse("opencode2").expect("id");
        let json = serde_json::to_string(&id).expect("serialize");
        assert_eq!(json, "\"opencode2\"");
        let back: HarnessId = serde_json::from_str("\"opencode2\"").expect("deserialize");
        assert_eq!(back, id);
    }

    #[test]
    fn unknown_ids_are_allowed_but_not_builtin() {
        // 未来新增一家不需要改这个类型。
        let custom = HarnessId::parse("cursor").expect("custom id");
        assert!(!custom.is_builtin());
    }

    #[test]
    fn from_str_works() {
        let id: HarnessId = "codex".parse().expect("parse");
        assert_eq!(id.as_str(), "codex");
    }
}
