//! 人设标识。

use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, MetisError, Result};

/// 人设 id。
///
/// 会直接变成文件名 `<id>.md`，因此字符集受限；`active` 是保留字（它是基线里的快照文件名）。
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersonaId(String);

impl PersonaId {
    /// 保留 id：`active.md` 是"当前生效人设的快照"，不能被人设占用。
    pub const RESERVED: [&'static str; 1] = ["active"];

    /// 单个人设正文的防御性上限（4 MiB）。超过直接拒绝，不做部分加载。
    pub const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

    /// 解析并校验一个 id。
    ///
    /// 规则：非空；只含字母、数字、`.`、`-`、`_`；不得以 `.` 开头；不得是 `.` / `..`；
    /// 不得以 `.migrated` 结尾（迁移备份的保留后缀）；不得是保留字。
    pub fn parse(value: &str) -> Result<Self> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, "人设 id 不能为空").with_remediation("用 metis ls 查看已有的人设"));
        }
        if trimmed == "." || trimmed == ".." {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, format!("人设 id 非法：{value:?}")).with_remediation("id 不能是 . 或 ..；换一个正常名字"));
        }
        if trimmed.starts_with('.') {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, format!("人设 id 不能以 . 开头：{value:?}")).with_remediation("改一个不隐藏的名字"));
        }
        let valid = trimmed.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
        if !valid {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, format!("人设 id 含非法字符：{value:?}")).with_remediation("只允许字母、数字、. - _"));
        }
        if trimmed.ends_with(".migrated") {
            return Err(
                MetisError::new(ErrorKind::PersonaInvalid, format!("人设 id 不能以 .migrated 结尾：{value:?}")).with_remediation("这是迁移备份的保留后缀")
            );
        }
        if Self::RESERVED.contains(&trimmed) {
            return Err(MetisError::new(ErrorKind::PersonaInvalid, format!("{value:?} 是保留标识")).with_remediation("换一个名字，例如 concise"));
        }
        Ok(Self(trimmed.to_owned()))
    }

    /// 字符串视图。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 对应的文件名。
    pub fn file_name(&self) -> String {
        format!("{}.md", self.0)
    }

    /// 从一个**已知合法**的常量构造。
    ///
    /// 只给内部兜底用：调用方必须传编译期就确定的合法字面量。
    /// 非法输入会被替换成 `"unnamed"` 而不是 panic —— 比起崩溃，宁可名字难看一点。
    pub fn from_static_unchecked(value: &'static str) -> Self {
        let valid = !value.is_empty()
            && !Self::RESERVED.contains(&value)
            && !value.starts_with('.')
            && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_');
        if valid { Self(value.to_owned()) } else { Self("unnamed".to_owned()) }
    }
}

impl std::fmt::Display for PersonaId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::str::FromStr for PersonaId {
    type Err = MetisError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_reasonable_ids() {
        for ok in ["concise", "zh-CN.work", "a", "persona_1", "0123"] {
            let parsed = PersonaId::parse(ok);
            assert!(parsed.is_ok(), "{ok} 应被接受");
        }
    }

    #[test]
    fn rejects_path_traversal_and_separators() {
        for bad in ["a/b", "a\\b", "../etc", "..", ".", "a/../b"] {
            assert!(PersonaId::parse(bad).is_err(), "{bad:?} 必须被拒绝");
        }
    }

    #[test]
    fn rejects_hidden_and_reserved() {
        assert!(PersonaId::parse(".hidden").is_err());
        let reserved = PersonaId::parse("active");
        assert_eq!(reserved.err().map(|e| e.kind()), Some(ErrorKind::PersonaInvalid));
        assert!(PersonaId::parse("x.migrated").is_err());
    }

    #[test]
    fn rejects_unicode_and_spaces() {
        for bad in ["中文", "a b", "a\tb", "a:b"] {
            assert!(PersonaId::parse(bad).is_err(), "{bad:?} 必须被拒绝");
        }
    }

    #[test]
    fn file_name_is_md() {
        let id = PersonaId::parse("concise").expect("id");
        assert_eq!(id.file_name(), "concise.md");
    }

    #[test]
    fn trim_is_applied() {
        let id = PersonaId::parse("  concise  ").expect("id");
        assert_eq!(id.as_str(), "concise");
    }

    #[test]
    fn max_body_is_four_mebibytes() {
        assert_eq!(PersonaId::MAX_BODY_BYTES, 4 * 1024 * 1024);
    }
}
