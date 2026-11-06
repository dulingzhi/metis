//! 人设的 front-matter 元数据。
//!
//! 设计要点：**未知键必须原样保留**。用户可能用别的工具给同一个人设文件加了字段，
//! 我们不能在保存时把人家删掉（`DATA-FORMAT.md` §3）。

use std::collections::BTreeMap;

use metis_core::PersonaId;

/// 人设元数据。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    /// `id`：与文件名不一致时以它为准，并给出警告。
    pub id: Option<String>,
    /// `name`：仅用于管理界面。
    pub name: Option<String>,
    /// `description`（旧键 `desc` 读时兼容）。
    pub description: Option<String>,
    /// `harness`：标签，`["*"]` 表示不限。
    pub harness: Vec<String>,
    /// `model`：标签。
    pub model: Vec<String>,
    /// `order`：列表排序。
    pub order: Option<i64>,
    /// 我们不认识、但必须原样写回的键（按名字排序，保证输出稳定）。
    pub extra: BTreeMap<String, String>,
}

impl Meta {
    /// 从键值对构造。值已经过解析（标量或流式数组都归一成字符串列表）。
    pub fn from_pairs(pairs: Vec<(String, Value)>) -> Self {
        let mut meta = Self::default();
        for (key, value) in pairs {
            match key.as_str() {
                "id" => meta.id = value.as_scalar(),
                "name" => meta.name = value.as_scalar(),
                "description" => meta.description = value.as_scalar(),
                "desc" => {
                    // 旧键：只在没有新键时生效。
                    if meta.description.is_none() {
                        meta.description = value.as_scalar();
                    }
                }
                "harness" => meta.harness = value.as_list(),
                "model" => meta.model = value.as_list(),
                "order" => meta.order = value.as_scalar().and_then(|s| s.trim().parse().ok()),
                _ => {
                    meta.extra.insert(key, value.raw);
                }
            }
        }
        meta
    }

    /// 解析出有效的 id：元数据里的 `id` 优先，否则用文件名。
    ///
    /// 返回 `(id, used_fallback)`：`true` 表示元数据里没有 id 或它非法，用了文件名。
    pub fn effective_id(&self, file_stem: &str) -> (PersonaId, bool) {
        if let Some(raw) = &self.id
            && let Ok(id) = PersonaId::parse(raw)
        {
            return (id, false);
        }
        match PersonaId::parse(file_stem) {
            Ok(id) => (id, self.id.is_some()),
            // 文件名也不合法时只能兜一个安全值：调用方应在读列表时跳过它。
            Err(_) => (PersonaId::parse("unnamed").unwrap_or_else(|_| unreachable_id()), true),
        }
    }

    /// 显示名：`name` > id。
    pub fn display_name(&self, id: &PersonaId) -> String {
        self.name.clone().filter(|n| !n.trim().is_empty()).unwrap_or_else(|| id.to_string())
    }

    /// 是否对某家执行器适用（没写标签视为适用）。
    pub fn applies_to(&self, harness: &str) -> bool {
        if self.harness.is_empty() || self.harness.iter().any(|tag| tag == "*") {
            return true;
        }
        self.harness.iter().any(|tag| tag == harness)
    }
}

/// 一个 front-matter 值：要么是标量，要么是列表，同时保留原始文本用于写回。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Value {
    /// 原样的值文本（不含键名）。
    pub raw: String,
    /// 标量视图。
    pub scalar: Option<String>,
    /// 列表视图。
    pub list: Option<Vec<String>>,
}

impl Value {
    /// 标量构造。
    pub fn scalar(value: impl Into<String>) -> Self {
        let value = value.into();
        Self { raw: value.clone(), scalar: Some(value), list: None }
    }

    /// 列表构造。
    pub fn list(items: Vec<String>) -> Self {
        let raw = format!("[{}]", items.iter().map(|i| format!("\"{i}\"")).collect::<Vec<_>>().join(", "));
        Self { raw, scalar: None, list: Some(items) }
    }

    /// 原样文本构造（解析未知键时用）。
    pub fn raw(text: impl Into<String>) -> Self {
        Self { raw: text.into(), scalar: None, list: None }
    }

    fn as_scalar(&self) -> Option<String> {
        self.scalar.clone().or_else(|| self.list.as_ref().and_then(|l| l.first().cloned()))
    }

    fn as_list(&self) -> Vec<String> {
        match (&self.list, &self.scalar) {
            (Some(list), _) => list.clone(),
            (None, Some(scalar)) => vec![scalar.clone()],
            _ => Vec::new(),
        }
    }
}

fn unreachable_id() -> PersonaId {
    // `"unnamed"` 是合法 id，所以这里永远走 Ok 分支。
    // 但类型上仍要给出一个不 panic 的兜底：用 `map_or` 而不是 `unwrap_or_else` +
    // `unreachable!()`——后者会违反"永不崩溃"的铁律，也会被 clippy 拦住。
    PersonaId::parse("unnamed").unwrap_or_else(|_| PersonaId::from_static_unchecked("unnamed"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, value: Value) -> (String, Value) {
        (key.to_owned(), value)
    }

    #[test]
    fn known_keys_are_parsed() {
        let meta = Meta::from_pairs(vec![
            pair("id", Value::scalar("concise")),
            pair("name", Value::scalar("简洁协作")),
            pair("description", Value::scalar("先结论")),
            pair("harness", Value::list(vec!["*".to_owned()])),
            pair("order", Value::scalar("10")),
        ]);
        assert_eq!(meta.id.as_deref(), Some("concise"));
        assert_eq!(meta.name.as_deref(), Some("简洁协作"));
        assert_eq!(meta.description.as_deref(), Some("先结论"));
        assert_eq!(meta.harness, vec!["*".to_owned()]);
        assert_eq!(meta.order, Some(10));
    }

    #[test]
    fn legacy_desc_is_accepted_but_description_wins() {
        let legacy = Meta::from_pairs(vec![pair("desc", Value::scalar("旧说明"))]);
        assert_eq!(legacy.description.as_deref(), Some("旧说明"));

        let both = Meta::from_pairs(vec![pair("description", Value::scalar("新说明")), pair("desc", Value::scalar("旧说明"))]);
        assert_eq!(both.description.as_deref(), Some("新说明"));
    }

    #[test]
    fn unknown_keys_are_preserved() {
        let meta =
            Meta::from_pairs(vec![pair("id", Value::scalar("x")), pair("custom-key", Value::raw("some value")), pair("another", Value::raw("[\"a\", \"b\"]"))]);
        assert_eq!(meta.extra.get("custom-key").map(String::as_str), Some("some value"));
        assert_eq!(meta.extra.get("another").map(String::as_str), Some("[\"a\", \"b\"]"));
    }

    #[test]
    fn effective_id_prefers_metadata_then_file_stem() {
        let with_id = Meta::from_pairs(vec![pair("id", Value::scalar("chosen"))]);
        let (id, fallback) = with_id.effective_id("file-name");
        assert_eq!(id.as_str(), "chosen");
        assert!(!fallback);

        let without = Meta::default();
        let (id, fallback) = without.effective_id("file-name");
        assert_eq!(id.as_str(), "file-name");
        assert!(!fallback);
    }

    #[test]
    fn invalid_metadata_id_falls_back_to_file_stem_and_reports() {
        let bad = Meta::from_pairs(vec![pair("id", Value::scalar("a/b"))]);
        let (id, fallback) = bad.effective_id("good-name");
        assert_eq!(id.as_str(), "good-name");
        assert!(fallback, "需要用文件名兜底时必须报告");
    }

    #[test]
    fn display_name_falls_back_to_id() {
        let id = PersonaId::parse("concise").expect("id");
        assert_eq!(Meta::default().display_name(&id), "concise");
        let named = Meta { name: Some("简洁".to_owned()), ..Meta::default() };
        assert_eq!(named.display_name(&id), "简洁");
        let blank = Meta { name: Some("   ".to_owned()), ..Meta::default() };
        assert_eq!(blank.display_name(&id), "concise", "空白名称应回落到 id");
    }

    #[test]
    fn applies_to_handles_wildcards_and_empty() {
        let none = Meta::default();
        assert!(none.applies_to("codex"), "没写标签视为不限");

        let star = Meta { harness: vec!["*".to_owned()], ..Meta::default() };
        assert!(star.applies_to("anything"));

        let specific = Meta { harness: vec!["codex".to_owned(), "claude".to_owned()], ..Meta::default() };
        assert!(specific.applies_to("codex"));
        assert!(!specific.applies_to("grok"));
    }

    #[test]
    fn scalar_value_can_back_a_list() {
        let single = Meta::from_pairs(vec![pair("harness", Value::scalar("codex"))]);
        assert_eq!(single.harness, vec!["codex".to_owned()], "标量标签应被当成单元素列表");
    }

    #[test]
    fn order_must_parse() {
        let bad = Meta::from_pairs(vec![pair("order", Value::scalar("first"))]);
        assert_eq!(bad.order, None, "解析不了的 order 应被忽略，而不是崩溃");
    }
}
