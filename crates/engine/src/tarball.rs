//! 从 GitHub 的 tarball 里取出人设文件。
//!
//! 为什么用 tarball 而不是逐个下载：
//! - `raw.githubusercontent.com` 在部分网络环境下**不可达**（实测这台机器 3 次全超时）；
//! - `api.github.com` 可用但**未认证只有 60 次/小时**，逐个下载是 N+1 次；
//! - `codeload.github.com` 可用、无限流，**一次请求就是整个仓库**。
//!
//! 所以：一次 tarball → 内存里解压 → 从中挑出 `personas/*.md` + `personas.json`。
//! 请求数固定为 **1**，与仓库有多少份人设无关。

use std::collections::BTreeMap;
use std::io::Read;

use metis_core::{ErrorKind, MetisError, Result};

/// tarball 里我们关心的文件。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Tarball {
    /// `personas.json` 的内容（如果有）。
    pub index: Option<String>,
    /// 人设文件：**仓库相对路径** → 正文。
    ///
    /// 键是 `personas/concise.md` 这种（去掉 tarball 顶层的 `<repo>-<sha>/` 前缀）。
    pub personas: BTreeMap<String, String>,
}

/// 解压上限：防止一个恶意/异常的 tarball 把内存吃掉。
pub const MAX_BYTES: usize = 32 * 1024 * 1024;
/// 单个文件上限。
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

/// 从 `.tar.gz` 里取出索引与人设。
///
/// 规则：
/// - 只保留 `personas.json` 与 `personas/**/*.md`；
/// - 去掉 tarball 的顶层目录（GitHub 打的是 `<repo>-<ref>/...`）；
/// - 跳过文档文件（README/LICENSE 等），与人设目录的规则一致；
/// - 单文件超限直接跳过（不报错：那多半不是人设）。
pub fn extract(gz: &[u8]) -> Result<Tarball> {
    let mut out = Tarball::default();
    let mut collected: BTreeMap<String, String> = BTreeMap::new();
    let mut total = 0_usize;

    // `GzDecoder` 是**惰性**的：`tar::Archive::new` 不校验 gzip 头，错误要到
    // 第一次读条目时才出现。所以先单独解压一次，把坏包变成可诊断的错误 ——
    // 否则它表现为"里面什么都没有"，用户完全看不出是下载坏了。
    //
    // **不能靠 `entries.next()` 来探测**：那会消费掉第一个条目。
    validate_gzip(gz)?;

    let decoder = flate2::read::GzDecoder::new(gz);
    let mut archive = tar::Archive::new(decoder);
    let entries = archive
        .entries()
        .map_err(|error| MetisError::new(ErrorKind::Network, "tarball 打不开").with_detail(error.to_string()).with_remediation("仓库可能不是有效的 tar.gz"))?;

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            // 单个条目坏了就跳过，不让整包失败。
            Err(_) => continue,
        };
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let Ok(path) = entry.path() else { continue };
        let Some(relative) = strip_top_level(&path) else { continue };
        if !is_wanted(&relative) {
            continue;
        }
        let size = entry.header().size().unwrap_or(0) as usize;
        if size > MAX_FILE_BYTES || total + size > MAX_BYTES {
            continue;
        }
        let mut buffer = Vec::with_capacity(size.min(64 * 1024));
        if entry.take(MAX_FILE_BYTES as u64).read_to_end(&mut buffer).is_err() {
            continue;
        }
        total += buffer.len();
        // 非 UTF-8 的文件跳过：人设必须是文本。
        let Ok(text) = String::from_utf8(buffer) else { continue };

        if relative == "personas.json" {
            out.index = Some(text);
        } else {
            collected.insert(relative, text);
        }
    }

    out.personas = pick_layout(collected);
    Ok(out)
}

/// 把「坏 gzip」变成一个明确的错误。
///
/// 只读前几十个字节就够：足以触发 gzip 头校验，又不浪费带宽/CPU。
fn validate_gzip(gz: &[u8]) -> Result<()> {
    // **先确认 gzip 魔数**：`1f 8b` 是 gzip 的标志。
    //
    // 这一步比"试着解压一下"更可靠：一个短到无法解压的垃圾
    // （比如 `b"not gzip"`）在 `GzDecoder` 里会得到 `UnexpectedEof`，
    // 而 `UnexpectedEof` 既可能是"垃圾数据"也可能是"极小的合法包" ——
    // 单看错误类型分不清。查魔数就没有这个歧义。
    if gz.len() < 2 || gz[0] != 0x1f || gz[1] != 0x8b {
        return Err(MetisError::new(ErrorKind::Network, "下载到的不是 gzip 数据").with_remediation("仓库地址可能不对；确认 `owner/repo` 与分支名"));
    }

    // 再用一次解压确认头部没坏（魔数对但内容烂的情况）。
    let mut decoder = flate2::read::GzDecoder::new(gz);
    let mut probe = [0_u8; 16];
    let mut filled = 0_usize;
    while filled < probe.len() {
        match decoder.read(&mut probe[filled..]) {
            Ok(0) => break,
            Ok(read) => filled += read,
            Err(error) => {
                return Err(MetisError::new(ErrorKind::Network, "下载到的 gzip 头部损坏")
                    .with_detail(error.to_string())
                    .with_remediation("仓库地址可能不对；确认 `owner/repo` 与分支名"));
            }
        }
    }
    Ok(())
}

/// 去掉 tarball 的顶层目录（GitHub 打的是 `<repo>-<ref>/...`）。
fn strip_top_level(path: &std::path::Path) -> Option<String> {
    let mut parts = path.components().filter_map(|c| match c {
        std::path::Component::Normal(part) => part.to_str(),
        _ => None,
    });
    // 第一个是顶层目录，丢掉。
    let _top = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    if rest.is_empty() { None } else { Some(rest.join("/")) }
}

/// 是不是我们要的文件。
fn is_wanted(relative: &str) -> bool {
    if relative == "personas.json" {
        return true;
    }
    if !relative.ends_with(".md") {
        return false;
    }
    // 收两类：`personas/**` 与**仓库根目录的 `*.md`**。
    //
    // 很多第三方人设库就是把文件平铺在根目录（`ZiChuanLan/some-magic`：
    // `dafeiyu.md` / `xiaomajiang.md` / `CTF.md`）。只认 `personas/` 会把它们**全漏掉**。
    //
    // 根目录之外的其他子目录（`characters/`、`presets/`…）也收：
    // 第三方布局五花八门，只认一种会漏。但**排除**明显的非人设目录
    // （依赖、构建产物、版本控制），否则 `node_modules/**/*.md` 会灌进来。
    let under_personas = relative.starts_with("personas/");
    let at_root = !relative.contains('/');
    if !under_personas && !at_root && hits_ignored_dir(relative) {
        return false;
    }
    // 深度上限：和本地导入一致（`MAX_IMPORT_DEPTH` 的语义）。
    if relative.split('/').count() > 4 {
        return false;
    }
    let stem = relative.trim_end_matches(".md").rsplit('/').next().unwrap_or_default().to_ascii_lowercase();
    !is_documentation(&stem)
}

/// 路径里是否出现"不是人设"的目录。
fn hits_ignored_dir(relative: &str) -> bool {
    const IGNORED: &[&str] = &[".git", "node_modules", "target", "dist", "vendor", ".github", ".vscode", ".idea"];
    relative.split('/').any(|part| IGNORED.contains(&part))
}

/// 是不是"文档"而不是"人设"。
fn is_documentation(stem: &str) -> bool {
    matches!(
        stem,
        "readme" | "license" | "licence" | "changelog" | "contributing" | "notice" | "authors" | "code_of_conduct" | "security" | "index" | "summary"
    )
}

/// 从"两类都收了"的结果里挑出真正要用的那一类。
///
/// 规则：**有 `personas/` 目录就只用它**（那是明确的约定，根目录的 `.md`
/// 多半是说明或配置）；否则用根目录的。
fn pick_layout(mut collected: BTreeMap<String, String>) -> BTreeMap<String, String> {
    // 有 `personas/` 就用它（那是明确约定）。
    //
    // 但**只有在根目录没有直接放人设时**才这么做：有些库既有 `personas/`
    // 又在根目录放了几份，若一刀切丢掉根目录的会漏。
    // 判据：根目录那份如果文档名都排除了还剩下东西，就两边都留。
    let has_personas_dir = collected.keys().any(|k| k.starts_with("personas/"));
    let has_root_md = collected.keys().any(|k| !k.contains('/'));
    if has_personas_dir && !has_root_md {
        collected.retain(|k, _| k.starts_with("personas/"));
    }
    collected
}

/// `codeload` 的 tarball 地址。
///
/// 这是 GitHub 自己给 "Download ZIP" 用的后端，**没有 API 限流**。
pub fn tarball_url(owner: &str, repo: &str, branch: &str) -> String {
    format!("https://codeload.github.com/{owner}/{repo}/tar.gz/refs/heads/{branch}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// 造一个 tarball，验证解包逻辑（不联网）。
    fn make_tarball(files: &[(&str, &str)]) -> Vec<u8> {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut gz);
            for (path, content) in files {
                let mut header = tar::Header::new_gnu();
                header.set_size(content.len() as u64);
                header.set_mode(0o644);
                header.set_cksum();
                builder.append_data(&mut header, path, content.as_bytes()).expect("append");
            }
            builder.finish().expect("finish");
        }
        gz.write_all(&[]).ok();
        gz.finish().expect("gz finish")
    }

    #[test]
    fn extracts_the_index_and_personas() {
        let gz = make_tarball(&[
            ("metis-personas-main/personas.json", r#"{"v":1,"personas":[]}"#),
            ("metis-personas-main/personas/a.md", "---\nid: a\n---\n\n甲\n"),
            ("metis-personas-main/personas/b.md", "---\nid: b\n---\n\n乙\n"),
            ("metis-personas-main/README.md", "# 说明\n"),
        ]);
        let tarball = extract(&gz).expect("extract");
        assert!(tarball.index.is_some(), "应取到索引");
        assert_eq!(tarball.personas.len(), 2, "{:?}", tarball.personas.keys());
        assert!(tarball.personas.contains_key("personas/a.md"));
        assert!(tarball.personas.contains_key("personas/b.md"));
    }

    #[test]
    fn the_top_level_directory_is_stripped() {
        // GitHub 打的是 `<repo>-<ref>/...`，前缀必须去掉，否则路径对不上索引。
        let gz = make_tarball(&[("anything-at-all/personas/x.md", "x")]);
        let tarball = extract(&gz).expect("extract");
        assert!(tarball.personas.contains_key("personas/x.md"), "{:?}", tarball.personas.keys());
    }

    #[test]
    fn readme_outside_and_inside_personas_is_skipped() {
        let gz =
            make_tarball(&[("repo-main/README.md", "根目录说明"), ("repo-main/personas/README.md", "人设目录说明"), ("repo-main/personas/real.md", "真人设")]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 1);
        assert!(tarball.personas.contains_key("personas/real.md"));
    }

    #[test]
    fn files_outside_personas_are_ignored() {
        let gz = make_tarball(&[("repo-main/src/main.rs", "fn main() {}"), ("repo-main/docs/guide.md", "# 指南"), ("repo-main/personas/ok.md", "ok")]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 1, "只应保留 personas/ 下的：{:?}", tarball.personas.keys());
    }

    #[test]
    fn root_level_personas_are_collected() {
        // 第三方人设库常常把文件平铺在根目录（`ZiChuanLan/some-magic`）。
        // 只认 `personas/` 会把它们**全漏掉** —— 实测导入结果是 0 份。
        let gz = make_tarball(&[
            ("some-magic-main/README.md", "# 说明"),
            ("some-magic-main/dafeiyu.md", "---\nid: dafeiyu\n---\n\n甲"),
            ("some-magic-main/xiaomajiang.md", "---\nid: xiaomajiang\n---\n\n乙"),
            ("some-magic-main/CTF.md", "---\nid: CTF\n---\n\n丙"),
        ]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 3, "{:?}", tarball.personas.keys());
        assert!(tarball.personas.contains_key("CTF.md"), "大写文件名也要收");
        assert!(!tarball.personas.keys().any(|k| k.contains("README")), "README 不该进来");
    }

    #[test]
    fn the_personas_directory_wins_when_the_root_has_no_personas() {
        // `personas/` 是明确约定，用它。根目录只有文档时不该混进来。
        let gz = make_tarball(&[("repo-main/README.md", "说明"), ("repo-main/personas/real.md", "真人设")]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 1, "{:?}", tarball.personas.keys());
        assert!(tarball.personas.contains_key("personas/real.md"));
    }

    #[test]
    fn a_mixed_layout_keeps_both_sides() {
        // 既有人设目录又在根目录放了人设时，**两边都留**。
        // 一刀切只留 `personas/` 会漏掉根目录那几份（第三方库常见这种混放）。
        let gz = make_tarball(&[("repo-main/NOTES.md", "随手记"), ("repo-main/personas/real.md", "真人设")]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 2, "{:?}", tarball.personas.keys());
        assert!(tarball.personas.contains_key("NOTES.md"));
        assert!(tarball.personas.contains_key("personas/real.md"));
    }

    #[test]
    fn nested_personas_are_collected_but_ignored_dirs_are_not() {
        // 第三方库的布局五花八门（`characters/`、`presets/`）—— 子目录也要收。
        // 但依赖/构建产物目录必须排除，否则 `node_modules/**/*.md` 会灌进来。
        let gz = make_tarball(&[
            ("repo-main/characters/a.md", "甲"),
            ("repo-main/presets/b.md", "乙"),
            ("repo-main/root.md", "根"),
            ("repo-main/node_modules/pkg/README.md", "依赖的文档"),
            ("repo-main/target/doc/x.md", "构建产物"),
            ("repo-main/README.md", "根说明"),
        ]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 3, "{:?}", tarball.personas.keys());
        assert!(tarball.personas.contains_key("characters/a.md"));
        assert!(tarball.personas.contains_key("presets/b.md"));
        assert!(tarball.personas.contains_key("root.md"));
        assert!(!tarball.personas.keys().any(|k| k.contains("node_modules")), "依赖目录不该被收");
        assert!(!tarball.personas.keys().any(|k| k.contains("target")), "构建产物不该被收");
    }

    #[test]
    fn very_deep_paths_are_ignored() {
        // 深度上限：防止有人在很深的目录里放大量 `.md`。
        let gz = make_tarball(&[("repo-main/a/b/c/d/e.md", "太深了"), ("repo-main/ok.md", "正常")]);
        let tarball = extract(&gz).expect("extract");
        assert_eq!(tarball.personas.len(), 1, "{:?}", tarball.personas.keys());
        assert!(tarball.personas.contains_key("ok.md"));
    }

    #[test]
    fn more_documentation_names_are_excluded() {
        // 逐个验证：加上这个文档文件之后，人设数量不变。
        for doc in ["INDEX", "SUMMARY", "LICENSE", "CODE_OF_CONDUCT", "NOTICE", "CHANGELOG"] {
            let gz = make_tarball(&[("repo-main/real.md", "真的"), (&format!("repo-main/{doc}.md"), "文档")]);
            let tarball = extract(&gz).expect("extract");
            assert_eq!(tarball.personas.len(), 1, "{doc}.md 不该被收进来：{:?}", tarball.personas.keys());
        }
    }

    #[test]
    fn non_utf8_files_are_skipped() {
        // 二进制文件不该让人设导入失败。
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut gz);
            let bad = [0xff_u8, 0xfe, 0xfd];
            let mut header = tar::Header::new_gnu();
            header.set_size(bad.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, "repo-main/personas/bad.md", &bad[..]).expect("append");
            builder.finish().expect("finish");
        }
        let bytes = gz.finish().expect("finish");
        let tarball = extract(&bytes).expect("extract");
        assert!(tarball.personas.is_empty(), "非 UTF-8 的文件应被跳过");
    }

    #[test]
    fn an_invalid_gzip_is_a_clear_error() {
        let error = extract(b"definitely not a gzip").expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn an_empty_tarball_yields_nothing() {
        let gz = make_tarball(&[]);
        let tarball = extract(&gz).expect("extract");
        assert!(tarball.index.is_none());
        assert!(tarball.personas.is_empty());
    }

    #[test]
    fn the_tarball_url_avoids_the_api_host() {
        let url = tarball_url("owner", "repo", "main");
        assert_eq!(url, "https://codeload.github.com/owner/repo/tar.gz/refs/heads/main");
        assert!(!url.contains("api.github.com"), "不该走 API");
        assert!(!url.contains("raw.githubusercontent.com"), "不该走 raw");
    }

    #[test]
    fn directories_are_not_files() {
        // tarball 里有目录条目；它们不该被当成文件读。
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        {
            let mut builder = tar::Builder::new(&mut gz);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Directory);
            header.set_size(0);
            header.set_mode(0o755);
            header.set_cksum();
            builder.append_data(&mut header, "repo-main/personas/", &[][..]).expect("append");
            builder.finish().expect("finish");
        }
        let bytes = gz.finish().expect("finish");
        let tarball = extract(&bytes).expect("extract");
        assert!(tarball.personas.is_empty());
    }
}
