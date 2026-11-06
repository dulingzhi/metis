//! 启动参数生成。**纯函数**：只描述"怎么启动"，不 spawn、不写盘（基线的好设计，保留）。

use metis_core::Mode;

/// 一次交互式启动的计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// 可执行名。
    pub program: String,
    /// 参数。
    pub args: Vec<String>,
    /// 给用户看的备注（例如"注入靠扩展，不占命令行"）。
    pub note: Option<String>,
    /// 残留说明（例如"不补丁时残留 62 字身份句"）。
    pub residue: Option<String>,
}

impl LaunchPlan {
    /// 直接透传。
    pub fn passthrough(program: impl Into<String>, args: Vec<String>) -> Self {
        Self { program: program.into(), args, note: None, residue: None }
    }

    /// 附备注。
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// 附残留说明。
    #[must_use]
    pub fn with_residue(mut self, residue: impl Into<String>) -> Self {
        self.residue = Some(residue.into());
        self
    }
}

/// 一次一次性问答（`--ask`）的计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneshootPlan {
    /// 可执行名。
    pub program: String,
    /// 参数。
    pub args: Vec<String>,
    /// 给用户看的备注。
    pub note: Option<String>,
}

/// 把文本包成 TOML basic string（codex 的 `-c` 值按 TOML 解析，不包会解析失败）。
pub fn toml_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// 去掉尾随空白，用于"一行式"注入。
pub fn inline(text: &str) -> String {
    text.trim_end().to_owned()
}

/// 把 Windows 路径转成正斜杠（codex 的 TOML 值需要）。
pub fn forward_slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// 模式是否要求注入。
pub fn injects(mode: Mode) -> bool {
    mode.injects()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toml_string_escapes_specials() {
        assert_eq!(toml_string("plain"), "\"plain\"");
        assert_eq!(toml_string("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(toml_string("a\nb"), "\"a\\nb\"");
        assert_eq!(toml_string("C:\\x\\y"), "\"C:\\\\x\\\\y\"");
        assert_eq!(toml_string("tab\there"), "\"tab\\there\"");
    }

    #[test]
    fn toml_string_round_trips_through_a_real_parser() {
        // 用 toml 自己的解析器验证转义是对的，而不是自我一致。
        let source = "多行\n带 \"引号\" 与 \\ 反斜杠";
        // TOML 文档以换行结尾：不给的话解析器会报 "expected nothing"。
        let mut document = format!("value = {}", toml_string(source));
        document.push('\n');
        // 用 `toml::from_str`（文档级解析）而不是 `str::parse`：
        // 后者走的是另一条路径，对 `value = "..."` 这种单键文档会报 "expected nothing"。
        let parsed: toml::Value = toml::from_str(&document).expect("TOML must parse");
        assert_eq!(parsed.get("value").and_then(|v| v.as_str()), Some(source));
    }

    #[test]
    fn inline_strips_trailing_whitespace_only() {
        assert_eq!(inline("  hello  \n\n"), "  hello");
        assert_eq!(inline("keep   inner"), "keep   inner");
    }

    #[test]
    fn forward_slashes_converts_windows_paths() {
        assert_eq!(forward_slashes("C:\\Users\\x\\persona.md"), "C:/Users/x/persona.md");
        assert_eq!(forward_slashes("/already/unix"), "/already/unix");
    }

    #[test]
    fn passthrough_has_no_notes() {
        let plan = LaunchPlan::passthrough("pi", vec!["--flag".to_owned()]);
        assert_eq!(plan.program, "pi");
        assert_eq!(plan.args, vec!["--flag".to_owned()]);
        assert!(plan.note.is_none());
        assert!(plan.residue.is_none());
    }

    #[test]
    fn builders_attach_notes() {
        let plan = LaunchPlan::passthrough("x", vec![]).with_note("注入靠扩展").with_residue("残留 62 字");
        assert_eq!(plan.note.as_deref(), Some("注入靠扩展"));
        assert_eq!(plan.residue.as_deref(), Some("残留 62 字"));
    }
}
