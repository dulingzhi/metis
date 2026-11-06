//! front-matter 的解析与序列化。
//!
//! 只支持 `---` 分隔的简单 `key: value` 与流式数组 `[a, b]`。**这不是通用 YAML 解析器**，
//! 是给人设文件用的最小语法 —— 认不出来就当正文（宁可少解析，也不能猜错把正文吃掉）。

use metis_core::PersonaId;

use crate::meta::{Meta, Value};

/// front-matter 分隔线。
const FENCE: &str = "---";

/// 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPersona {
    /// 元数据。
    pub meta: Meta,
    /// 正文（已去掉首尾空白行，保留了内部换行）。
    pub body: String,
    /// 解析过程中的告警（例如元数据 id 与文件名不一致）。
    pub warnings: Vec<String>,
}

/// 解析一份人设文件内容。
pub fn parse_persona(source: &str) -> ParsedPersona {
    // 容忍 BOM 与 CRLF（`AGENTS.md` 单测契约里明确要求）。
    let text = source.strip_prefix('\u{feff}').unwrap_or(source);
    let normalised = text.replace("\r\n", "\n").replace('\r', "\n");
    let mut warnings = Vec::new();

    let mut lines = normalised.split('\n');
    let first = lines.next().unwrap_or("").trim();
    if first != FENCE {
        // 没有 front-matter：整份都是正文。
        return ParsedPersona { meta: Meta::default(), body: trim_body(&normalised), warnings };
    }

    let mut pairs: Vec<(String, Value)> = Vec::new();
    let mut body_start: Option<usize> = None;
    let collected: Vec<&str> = normalised.split('\n').collect();
    let mut index = 1;
    while index < collected.len() {
        let line = collected[index];
        if line.trim() == FENCE {
            body_start = Some(index + 1);
            break;
        }
        if !line.trim().is_empty() {
            match parse_pair(line) {
                Some(pair) => pairs.push(pair),
                None => warnings.push(format!("忽略无法解析的元数据行：{}", line.trim())),
            }
        }
        index += 1;
    }

    let body = match body_start {
        Some(start) => trim_body(&collected[start.min(collected.len())..].join("\n")),
        None => {
            warnings.push("front-matter 缺少结束的 ---，已按无头文件处理".to_owned());
            return ParsedPersona { meta: Meta::from_pairs(pairs), body: String::new(), warnings };
        }
    };

    ParsedPersona { meta: Meta::from_pairs(pairs), body, warnings }
}

fn parse_pair(line: &str) -> Option<(String, Value)> {
    let (key, rest) = line.split_once(':')?;
    let key = key.trim();
    if key.is_empty() || key.contains(' ') {
        return None;
    }
    let rest = rest.trim();
    if rest.starts_with('[') && rest.ends_with(']') {
        let inner = &rest[1..rest.len() - 1];
        let items: Vec<String> =
            inner.split(',').map(|item| item.trim().trim_matches('"').trim_matches('\'').trim().to_owned()).filter(|item| !item.is_empty()).collect();
        return Some((key.to_owned(), Value::list(items)));
    }
    Some((key.to_owned(), Value::scalar(rest.trim_matches('"').trim_matches('\'').trim().to_owned())))
}

fn trim_body(body: &str) -> String {
    body.trim_matches('\n').trim_end().to_owned()
}

/// 序列化回文件内容：`---` 头 + 正文 + 末尾换行。
///
/// 未知键原样写回（`meta.extra` 按名字排序，保证输出稳定）。
pub fn render_persona(id: &PersonaId, meta: &Meta, body: &str) -> String {
    let mut out = String::new();
    out.push_str(FENCE);
    out.push('\n');
    out.push_str(&format!("id: {id}\n"));
    if let Some(name) = meta.name.as_deref().filter(|n| !n.trim().is_empty()) {
        out.push_str(&format!("name: {name}\n"));
    }
    if let Some(description) = meta.description.as_deref().filter(|d| !d.trim().is_empty()) {
        out.push_str(&format!("description: {description}\n"));
    }
    if !meta.harness.is_empty() {
        out.push_str(&format!("harness: {}\n", render_list(&meta.harness)));
    }
    if !meta.model.is_empty() {
        out.push_str(&format!("model: {}\n", render_list(&meta.model)));
    }
    if let Some(order) = meta.order {
        out.push_str(&format!("order: {order}\n"));
    }
    for (key, value) in &meta.extra {
        out.push_str(&format!("{key}: {value}\n"));
    }
    out.push_str(FENCE);
    out.push('\n');
    out.push('\n');
    out.push_str(body.trim_end());
    out.push('\n');
    out
}

fn render_list(items: &[String]) -> String {
    format!("[{}]", items.iter().map(|item| format!("\"{item}\"")).collect::<Vec<_>>().join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "---\nid: concise\nname: 简洁协作\ndescription: 先结论\nharness: [\"*\"]\norder: 10\n---\n\n用简体中文。\n第二行。\n";

    #[test]
    fn parses_metadata_and_body() {
        let parsed = parse_persona(SAMPLE);
        assert_eq!(parsed.meta.id.as_deref(), Some("concise"));
        assert_eq!(parsed.meta.name.as_deref(), Some("简洁协作"));
        assert_eq!(parsed.meta.order, Some(10));
        assert_eq!(parsed.body, "用简体中文。\n第二行。");
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn file_without_front_matter_is_all_body() {
        let parsed = parse_persona("就是一段正文\n第二行");
        assert!(parsed.meta.id.is_none());
        assert_eq!(parsed.body, "就是一段正文\n第二行");
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn handles_bom_and_crlf() {
        let source = "\u{feff}---\r\nid: x\r\n---\r\n\r\n正文\r\n";
        let parsed = parse_persona(source);
        assert_eq!(parsed.meta.id.as_deref(), Some("x"));
        assert_eq!(parsed.body, "正文");
    }

    #[test]
    fn unclosed_front_matter_is_reported_and_body_is_empty() {
        let parsed = parse_persona("---\nid: x\n没有结束线\n");
        assert!(!parsed.warnings.is_empty(), "缺结束线必须报警");
        assert!(parsed.warnings.iter().any(|w| w.contains("缺少结束")), "{}", parsed.warnings.join(" / "));
        assert!(parsed.body.is_empty(), "没有结束线时不能猜正文边界");
    }

    #[test]
    fn unparsable_metadata_lines_are_warned_but_skipped() {
        let parsed = parse_persona("---\nid: x\n这一行没有冒号\n---\n正文\n");
        assert_eq!(parsed.meta.id.as_deref(), Some("x"));
        assert_eq!(parsed.body, "正文");
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn quoted_scalars_are_unquoted() {
        let parsed = parse_persona("---\nname: \"带引号的名字\"\ndescription: '单引号'\n---\n正文");
        assert_eq!(parsed.meta.name.as_deref(), Some("带引号的名字"));
        assert_eq!(parsed.meta.description.as_deref(), Some("单引号"));
    }

    #[test]
    fn list_values_are_split_and_trimmed() {
        let parsed = parse_persona("---\nharness: [codex, claude ,\"grok\"]\n---\n正文");
        assert_eq!(parsed.meta.harness, vec!["codex".to_owned(), "claude".to_owned(), "grok".to_owned()]);
    }

    #[test]
    fn render_then_parse_round_trips() {
        let id = PersonaId::parse("concise").expect("id");
        let meta = Meta::from_pairs(vec![
            ("name".to_owned(), Value::scalar("简洁协作")),
            ("description".to_owned(), Value::scalar("先结论")),
            ("harness".to_owned(), Value::list(vec!["*".to_owned()])),
            ("order".to_owned(), Value::scalar("10")),
            ("custom".to_owned(), Value::raw("keep me")),
        ]);
        let rendered = render_persona(&id, &meta, "正文第一行\n正文第二行");
        let parsed = parse_persona(&rendered);
        assert_eq!(parsed.meta.id.as_deref(), Some("concise"));
        assert_eq!(parsed.meta.name.as_deref(), Some("简洁协作"));
        assert_eq!(parsed.meta.description.as_deref(), Some("先结论"));
        assert_eq!(parsed.meta.harness, vec!["*".to_owned()]);
        assert_eq!(parsed.meta.order, Some(10));
        assert_eq!(parsed.meta.extra.get("custom").map(String::as_str), Some("keep me"));
        assert_eq!(parsed.body, "正文第一行\n正文第二行");
    }

    #[test]
    fn rendered_output_is_lf_only_and_ends_with_newline() {
        let id = PersonaId::parse("x").expect("id");
        let rendered = render_persona(&id, &Meta::default(), "body");
        assert!(!rendered.contains('\r'));
        assert!(rendered.ends_with('\n'));
        assert!(!rendered.starts_with('\u{feff}'));
    }

    #[test]
    fn extra_keys_are_rendered_in_sorted_order() {
        let id = PersonaId::parse("x").expect("id");
        let mut meta = Meta::default();
        meta.extra.insert("zeta".to_owned(), "1".to_owned());
        meta.extra.insert("alpha".to_owned(), "2".to_owned());
        let rendered = render_persona(&id, &meta, "body");
        let alpha = rendered.find("alpha").expect("alpha");
        let zeta = rendered.find("zeta").expect("zeta");
        assert!(alpha < zeta, "未知键必须按名字排序，保证输出稳定");
    }

    #[test]
    fn empty_body_renders_and_parses() {
        let id = PersonaId::parse("x").expect("id");
        let rendered = render_persona(&id, &Meta::default(), "");
        let parsed = parse_persona(&rendered);
        assert_eq!(parsed.body, "");
    }

    #[test]
    fn body_keeps_internal_blank_lines() {
        let parsed = parse_persona("---\nid: x\n---\n第一段\n\n第二段\n");
        assert_eq!(parsed.body, "第一段\n\n第二段");
    }
}
