//! 托管块：往用户既有文件里插入一段"我们管的"内容，并能精确切除。
//!
//! 规则（`PITFALLS.md` B6）：
//! - 只有**去掉行尾换行并 trim 后与标记完全相等**的行才算边界；行中间出现的字符串不算；
//! - 切除时从 BEGIN 切到**下一个** END（含）；找不到 END 就把剩下的都当块内容；
//! - **块外一个字节都不改**。

/// 一对起止标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockMarker {
    /// 起始行。
    pub begin: &'static str,
    /// 结束行。
    pub end: &'static str,
}

impl BlockMarker {
    /// Markdown 用的标记。
    pub const MARKDOWN: Self = Self { begin: crate::BLOCK_BEGIN_MD, end: crate::BLOCK_END_MD };
    /// TOML 用的标记。
    pub const TOML: Self = Self { begin: crate::BLOCK_BEGIN_TOML, end: crate::BLOCK_END_TOML };

    /// 按文件扩展名挑标记。
    pub fn for_path(path: &std::path::Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("toml") => Self::TOML,
            _ => Self::MARKDOWN,
        }
    }
}

/// 切除的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StripOutcome {
    /// 切除后的文件内容。
    pub content: String,
    /// 是否真的切掉了东西。
    pub removed: bool,
    /// 有没有找到 BEGIN 却缺 END（说明文件被人手改坏了）。
    pub unterminated: bool,
}

/// 文件里是否已经有我们的托管块。
pub fn has_block(content: &str, marker: BlockMarker) -> bool {
    content.lines().any(|line| is_marker(line, marker.begin))
}

/// 把块插进文件（已有则替换）。
///
/// 保持文件原有的换行风格：原文含 `\r\n` 就用 `\r\n`。
pub fn upsert_block(content: &str, marker: BlockMarker, body: &str) -> String {
    let newline = if content.contains("\r\n") { "\r\n" } else { "\n" };
    let stripped = strip_block(content, marker).content;
    let block = render_block(marker, body, newline);

    if stripped.trim().is_empty() {
        return block;
    }
    let mut out = stripped;
    if !out.ends_with('\n') {
        out.push_str(newline);
    }
    out.push_str(newline);
    out.push_str(&block);
    out
}

/// 渲染一个块（不含前后空行）。
pub fn render_block(marker: BlockMarker, body: &str, newline: &str) -> String {
    let mut out = String::new();
    out.push_str(marker.begin);
    out.push_str(newline);
    let trimmed = body.trim();
    if !trimmed.is_empty() {
        for line in trimmed.lines() {
            out.push_str(line.trim_end());
            out.push_str(newline);
        }
    }
    out.push_str(marker.end);
    out.push_str(newline);
    out
}

/// 切除托管块。块外的内容逐字节保留。
pub fn strip_block(content: &str, marker: BlockMarker) -> StripOutcome {
    let mut out = String::with_capacity(content.len());
    let mut removed = false;
    let mut unterminated = false;
    let mut inside = false;

    for line in content.split_inclusive('\n') {
        if inside {
            if is_marker(line, marker.end) {
                inside = false;
            }
            removed = true;
            continue;
        }
        if is_marker(line, marker.begin) {
            inside = true;
            removed = true;
            continue;
        }
        out.push_str(line);
    }
    if inside {
        unterminated = true;
    }
    StripOutcome { content: tidy(out), removed, unterminated }
}

/// 切除后清掉块留下的多余空行（最多留一个行尾换行）。
///
/// **换行风格必须跟随块前面的内容**：用户文件是 CRLF 就还它 CRLF
/// （`PITFALLS.md`：块外一个字节都不改，包括换行）。
fn tidy(content: String) -> String {
    if content.trim().is_empty() {
        return String::new();
    }
    let uses_crlf = content.contains("\r\n");
    let mut out = content.trim_end_matches(['\n', '\r']).to_owned();
    if uses_crlf {
        out.push_str("\r\n");
    } else {
        out.push('\n');
    }
    out
}

/// 判定某一行是不是标记：**整行相等**（去掉行尾换行与首尾空白）。
fn is_marker(line: &str, marker: &str) -> bool {
    line.trim() == marker
}

#[cfg(test)]
mod brand_tests {
    //! 决策 7（不字节兼容）的守卫：Metis 的标记里**不得出现上游品牌名**。
    //!
    //! 这条测试存在的意义：将来若有人"顺手"把标记改成与 agent-purge 一样以求兼容，
    //! 会在这里立刻红掉 —— 而那是维护者明确否决的方向（`docs/PARITY.md` §8 决策 7）。
    use super::{BlockMarker, has_block, strip_block};
    use crate::{BLOCK_BEGIN_MD, BLOCK_BEGIN_TOML, BLOCK_END_MD, BLOCK_END_TOML};

    #[test]
    fn markers_never_mention_the_upstream_brand() {
        for marker in [BLOCK_BEGIN_MD, BLOCK_END_MD, BLOCK_BEGIN_TOML, BLOCK_END_TOML] {
            let lowered = marker.to_ascii_lowercase();
            assert!(!lowered.contains("agent-purge"), "标记里出现了上游品牌名：{marker}");
            assert!(!lowered.contains("agent_purge"), "标记里出现了上游品牌名：{marker}");
            assert!(lowered.contains("metis"), "标记必须带 Metis 自己的名字：{marker}");
        }
    }

    #[test]
    fn dsh_markers_never_mention_the_upstream_brand() {
        for plugin in ["metis", "desktop", "web"] {
            let (begin, end) = crate::dsh_patch::markers_for_test(plugin);
            for marker in [begin, end] {
                let lowered = marker.to_ascii_lowercase();
                assert!(!lowered.contains("agent-purge"), "dsh 标记里出现了上游品牌名：{marker}");
                assert!(lowered.contains("metis"), "dsh 标记必须带 Metis 的名字：{marker}");
            }
        }
    }

    #[test]
    fn upstream_markers_are_not_recognised() {
        // Metis **不认**上游的块：扫描时不该把它当成"自己的块"。
        let upstream = "# >>> agent-purge\n人设内容\n# <<< agent-purge\n";
        assert!(!has_block(upstream, BlockMarker::MARKDOWN), "不该把上游块认成自己的");
        assert!(!has_block(upstream, BlockMarker::TOML), "不该把上游块认成自己的");

        // 而自己的块必须认出来。
        let ours = "# >>> metis\n人设内容\n# <<< metis\n";
        assert!(has_block(ours, BlockMarker::TOML));
    }

    #[test]
    fn stripping_leaves_upstream_blocks_untouched() {
        // 撤回时绝不能动上游留下的块 —— 那是基线的东西，Metis 无权处置。
        let content = "# >>> agent-purge\n基线的人设\n# <<< agent-purge\n用户自己的内容\n";
        let stripped = strip_block(content, BlockMarker::TOML).content;
        assert!(stripped.contains("agent-purge"), "上游块必须原样保留：{stripped}");
        assert!(stripped.contains("用户自己的内容"));
        assert_eq!(stripped, content, "没有自己的块时，撤回必须逐字节不变");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MARK: BlockMarker = BlockMarker::MARKDOWN;

    #[test]
    fn inserts_into_empty_file() {
        let out = upsert_block("", MARK, "人设正文");
        assert!(out.contains(crate::BLOCK_BEGIN_MD));
        assert!(out.contains("人设正文"));
        assert!(out.contains(crate::BLOCK_END_MD));
    }

    #[test]
    fn inserts_after_existing_content_without_touching_it() {
        let original = "# 用户的内容\n不要动我\n";
        let out = upsert_block(original, MARK, "人设");
        assert!(out.starts_with(original), "块外内容必须原样保留");
        assert!(out.contains("人设"));
    }

    #[test]
    fn upsert_replaces_rather_than_duplicating() {
        let once = upsert_block("base\n", MARK, "第一版");
        let twice = upsert_block(&once, MARK, "第二版");
        assert!(!twice.contains("第一版"), "旧块必须被替换");
        assert!(twice.contains("第二版"));
        assert_eq!(twice.matches(crate::BLOCK_BEGIN_MD).count(), 1, "只该有一个块");
    }

    #[test]
    fn upsert_is_idempotent() {
        let once = upsert_block("base\n", MARK, "同一版");
        let twice = upsert_block(&once, MARK, "同一版");
        assert_eq!(once, twice, "同样的内容重复写必须得到同样的字节");
    }

    #[test]
    fn strip_restores_the_original_exactly() {
        let original = "# 用户的内容\n不要动我\n保留尾随空行\n\n";
        let with_block = upsert_block(original, MARK, "人设");
        let stripped = strip_block(&with_block, MARK);
        assert!(stripped.removed);
        assert!(!stripped.unterminated);
        assert_eq!(stripped.content, "# 用户的内容\n不要动我\n保留尾随空行\n");
        // 反复切除幂等。
        assert_eq!(strip_block(&stripped.content, MARK).content, stripped.content);
    }

    #[test]
    fn strip_of_a_file_without_a_block_is_a_noop() {
        let content = "just\ncontent\n";
        let outcome = strip_block(content, MARK);
        assert!(!outcome.removed);
        assert_eq!(outcome.content, content);
    }

    #[test]
    fn marker_in_the_middle_of_a_line_does_not_count() {
        let content = "before <!-- metis:begin --> inline\nkeep me\n";
        let outcome = strip_block(content, MARK);
        assert!(!outcome.removed, "行中间的标记不算边界");
        assert_eq!(outcome.content, content);
    }

    #[test]
    fn unterminated_block_consumes_the_rest_and_reports() {
        let content = "keep\n<!-- metis:begin -->\nswallowed\nmore\n";
        let outcome = strip_block(content, MARK);
        assert!(outcome.removed);
        assert!(outcome.unterminated, "缺 END 必须报告");
        assert!(!outcome.content.contains("swallowed"), "未闭合块的内容应被切除");
        assert!(outcome.content.contains("keep"));
    }

    #[test]
    fn crlf_is_preserved() {
        let original = "line1\r\nline2\r\n";
        let out = upsert_block(original, MARK, "body");
        assert!(out.contains("\r\n"), "原文是 CRLF 就该继续用 CRLF");
        let stripped = strip_block(&out, MARK);
        assert_eq!(stripped.content, original);
    }

    #[test]
    fn mixed_newlines_strip_back_to_the_original_exactly() {
        // 文件里同时有 CRLF 与 LF 时，切除不能让任何一行换行符漂移。
        let original = "a\r\nb\nc\r\n";
        let out = upsert_block(original, MARK, "body");
        let stripped = strip_block(&out, MARK);
        assert_eq!(stripped.content, original, "块外字节必须一个都不变");
    }

    #[test]
    fn toml_marker_is_used_for_toml_files() {
        assert_eq!(BlockMarker::for_path(std::path::Path::new("config.toml")), BlockMarker::TOML);
        assert_eq!(BlockMarker::for_path(std::path::Path::new("AGENTS.md")), BlockMarker::MARKDOWN);
        let out = upsert_block("key = 1\n", BlockMarker::TOML, "body");
        assert!(out.contains(crate::BLOCK_BEGIN_TOML));
    }

    #[test]
    fn has_block_detects_our_marker_only_at_line_start() {
        assert!(has_block("<!-- metis:begin -->\n", MARK));
        assert!(!has_block("prefix <!-- metis:begin -->\n", MARK));
        assert!(!has_block("", MARK));
    }

    #[test]
    fn body_whitespace_is_trimmed_inside_the_block() {
        let out = upsert_block("", MARK, "  \n  正文  \n\n");
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines.contains(&"正文"));
        assert!(!lines.iter().any(|l| l.is_empty() && l != lines.first().unwrap_or(&"")), "块内不该留空行：{lines:?}");
    }

    #[test]
    fn strip_handles_an_empty_block() {
        let out = upsert_block("keep\n", MARK, "");
        let stripped = strip_block(&out, MARK);
        assert!(stripped.removed);
        assert_eq!(stripped.content, "keep\n");
    }
}
