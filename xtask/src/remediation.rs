//! 补救建议覆盖检查（`M8.4`）。
//!
//! 规则（`AGENTS.md` 铁律 1）：**面向用户的失败必须给出"照做就能修"的补救建议**。
//! 例外只有一类：`ErrorKind::Internal`（程序缺陷，用户确实无事可做）——
//! 这类仍应在 `PITFALLS.md` 里被记录，但不强制 `remediation`。
//!
//! 这个检查是**源码扫描**：找到每个 `MetisError::new(...)`，看它周围有没有
//! `with_remediation`。它不是编译器级别的精确分析，但对"漏写补救建议"够用。

use std::path::Path;

/// 扫描结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// 文件（相对工作区根）。
    pub file: String,
    /// 行号（1 起）。
    pub line: usize,
    /// 错误类别（从源码里读到的）。
    pub kind: String,
}

/// 扫描整个工作区，返回**缺少补救建议**的调用点。
pub fn scan(root: &Path) -> Vec<Finding> {
    let mut findings = Vec::new();
    for base in ["crates", "apps"] {
        let dir = root.join(base);
        if !dir.is_dir() {
            continue;
        }
        for path in walk(&dir) {
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let relative = path.strip_prefix(root).unwrap_or(&path).display().to_string();
            scan_file(&relative, &text, &mut findings);
        }
    }
    findings
}

/// 扫描一份源码。
pub fn scan_file(file: &str, text: &str, findings: &mut Vec<Finding>) {
    let bytes = text.as_bytes();
    let needle = "MetisError::new(";
    let mut search_from = 0;
    while let Some(found) = text[search_from..].find(needle) {
        let start = search_from + found;
        let after = start + needle.len();
        let line = text[..start].bytes().filter(|b| *b == b'\n').count() + 1;

        // 读到本次调用的**结尾**：按括号配平。
        let mut depth = 1_i32;
        let mut index = after;
        while index < bytes.len() && depth > 0 {
            match bytes[index] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                _ => {}
            }
            index += 1;
        }

        // 调用链可能延续（`.with_detail(...).with_remediation(...)`）：
        // 取到**语句结束**（`;` 或下一个 `MetisError::new`）为止。
        let tail_start = index;
        let next_call = text[index..].find(needle).map(|offset| index + offset).unwrap_or(text.len());
        let statement_end = text[index..].find(';').map(|offset| index + offset).unwrap_or(next_call);
        let tail = &text[tail_start..statement_end.min(next_call)];

        let kind = extract_kind(&text[start..index]);
        // `Internal` 是唯一允许没有补救建议的类别。
        let exempt = kind.as_deref() == Some("Internal") || kind.as_deref() == Some("internal");
        let has_remediation = tail.contains("with_remediation") || text[start..index].contains("with_remediation");

        if !exempt && !has_remediation {
            findings.push(Finding { file: file.to_owned(), line, kind: kind.unwrap_or_else(|| "?".to_owned()) });
        }
        search_from = after;
    }
}

/// 从 `MetisError::new(kind, …)` 的第一个参数里读出类别。
fn extract_kind(call: &str) -> Option<String> {
    // 形如 `MetisError::new(ErrorKind::Io, …)` 或 `metis_core::ErrorKind::Io`
    let after = call.split("ErrorKind::").nth(1)?;
    let ident: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
    if ident.is_empty() { None } else { Some(ident) }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan_text(text: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        scan_file("test.rs", text, &mut findings);
        findings
    }

    #[test]
    fn flags_a_user_facing_error_without_remediation() {
        let found = scan_text(r#"let e = MetisError::new(ErrorKind::Io, "boom");"#);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "Io");
        assert_eq!(found[0].line, 1);
    }

    #[test]
    fn accepts_an_error_with_remediation() {
        let found = scan_text(r#"let e = MetisError::new(ErrorKind::Io, "boom").with_remediation("重试");"#);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn accepts_remediation_on_the_next_line() {
        let found = scan_text(
            r#"let e = MetisError::new(
                ErrorKind::Refused,
                "拒绝",
            )
            .with_remediation("换一个目录");"#,
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn internal_errors_are_exempt() {
        // `Internal` = 程序缺陷，用户无事可做；不强制补救建议。
        let found = scan_text(r#"let e = MetisError::new(ErrorKind::Internal, "bug");"#);
        assert!(found.is_empty(), "Internal 应豁免：{found:?}");
    }

    #[test]
    fn fully_qualified_kind_paths_work() {
        let found = scan_text(r#"let e = MetisError::new(metis_core::ErrorKind::LockHeld, "x");"#);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, "LockHeld");
    }

    #[test]
    fn finds_every_site_in_a_file() {
        let found = scan_text(
            r#"
            let a = MetisError::new(ErrorKind::Io, "a");
            let b = MetisError::new(ErrorKind::Io, "b").with_remediation("x");
            let c = MetisError::new(ErrorKind::Refused, "c");
            "#,
        );
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[0].line, 2);
        assert_eq!(found[1].line, 4);
    }

    #[test]
    fn the_real_workspace_is_clean() {
        // 这条是整个 `M8.4` 的验收：仓库里不该有面向用户却没给补救建议的错误。
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("workspace root");
        let findings = scan(root);
        assert!(
            findings.is_empty(),
            "缺少补救建议的错误（{} 处）：\n{}",
            findings.len(),
            findings.iter().map(|f| format!("  {}:{} ({})", f.file, f.line, f.kind)).collect::<Vec<_>>().join("\n")
        );
    }
}
