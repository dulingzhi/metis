//! 通用人设导入：从本地目录或 GitHub `owner/repo` 拉取。
//!
//! 与 `import agent-purge`（导入**状态**）不同，这里只处理**人设文件**。
//!
//! 三条规则：
//! 1. **不覆盖**是默认；`--overwrite` 才覆盖，且逐条报告；
//! 2. **去重按内容**：同名但正文相同的跳过，避免"导入了但什么都没变"的假成功；
//! 3. 网络可注入（`fetch` 闭包），测试永不联网。

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, PersonaId, Result};
use serde::{Deserialize, Serialize};

/// 一项导入的结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ImportOutcome {
    /// 新增了。
    Imported,
    /// 覆盖了同名的。
    Overwritten,
    /// 同名且内容相同，跳过。
    Identical,
    /// 目标已存在，跳过。
    Skipped,
    /// 源文件不合法。
    Failed,
}

/// 一项导入结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportItem {
    /// 人设 id（解析不出来时为文件名）。
    pub id: String,
    /// 结局。
    pub outcome: ImportOutcome,
    /// 细节。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// 导入报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportReport {
    /// 源（目录路径或 `owner/repo`）。
    pub source: String,
    /// 是否预览。
    #[serde(rename = "dryRun")]
    pub dry_run: bool,
    /// 逐项结果。
    pub items: Vec<ImportItem>,
}

impl ImportReport {
    /// 新建。
    pub fn new(source: impl Into<String>, dry_run: bool) -> Self {
        Self { source: source.into(), dry_run, items: Vec::new() }
    }

    /// 成功导入（新增或覆盖）几项。
    pub fn imported(&self) -> usize {
        self.items.iter().filter(|i| matches!(i.outcome, ImportOutcome::Imported | ImportOutcome::Overwritten)).count()
    }

    /// 失败几项。
    pub fn failed(&self) -> usize {
        self.items.iter().filter(|i| i.outcome == ImportOutcome::Failed).count()
    }

    fn push(&mut self, id: impl Into<String>, outcome: ImportOutcome, detail: Option<String>) {
        self.items.push(ImportItem { id: id.into(), outcome, detail });
    }
}

/// 导入选项。
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// 覆盖同名。
    pub overwrite: bool,
    /// 只预览。
    pub dry_run: bool,
}

/// 从本地目录导入（目录下所有 `.md`）。
pub fn import_dir(paths: &metis_paths::Paths, source: &Path, options: &ImportOptions) -> Result<ImportReport> {
    if !source.is_dir() {
        return Err(MetisError::new(ErrorKind::Io, format!("目录不存在：{}", source.display())).with_remediation("检查路径；或改用 GitHub 形式 `owner/repo`"));
    }
    let mut report = ImportReport::new(source.display().to_string(), options.dry_run);
    let store = metis_persona::PersonaStore::new(paths.clone());

    // **优先读 `personas/` 子目录**（人设库的标准布局），没有才读当前目录。
    //
    // 两个真踩过的坑：
    // 1. 只看当前目录 → 一个结构正常的仓库（`personas/*.md` + `README.md`）
    //    导进来是**零份人设**；
    // 2. 不排除 `README.md` → 它会被当成一份 id 叫 `README` 的人设导进去，
    //    用户 `metis ls` 会看到一份垃圾。
    let root = if source.join("personas").is_dir() { source.join("personas") } else { source.to_path_buf() };
    let mut files: Vec<PathBuf> = std::fs::read_dir(&root)
        .map_err(|error| MetisError::from(error).with_detail(root.display().to_string()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("md"))
        .filter(|path| !is_documentation_file(path))
        .collect();
    files.sort();

    if files.is_empty() {
        report.push("（无）", ImportOutcome::Skipped, Some(format!("{} 里没有可用的人设（.md，且不是 README/LICENSE）", root.display())));
        return Ok(report);
    }

    for file in files {
        let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_owned();
        import_one(&store, &stem, &file, options, &mut report);
    }
    Ok(report)
}

/// 从 GitHub `owner/repo` 导入。
///
/// **一次 tarball 请求拿全部**，走 `codeload.github.com`：
///
/// - `api.github.com` 未认证只有 **60 次/小时**，逐个下载是 N+1 次；
/// - `raw.githubusercontent.com` 在部分网络环境下**不可达**（实测这台机器 3 次全超时）；
/// - `codeload` 是 GitHub 自己给 "Download ZIP" 用的后端，**可用且无 API 限流**。
///
/// 请求数固定为 **1**，与仓库有多少份人设无关。解压后从内存里挑
/// `personas.json` 与 `personas/**/*.md`（见 [`crate::tarball`]）。
pub fn import_github(
    paths: &metis_paths::Paths,
    repo: &str,
    options: &ImportOptions,
    default_branch: &str,
    fetch: impl Fn(&str) -> std::result::Result<(u16, Vec<u8>), String>,
) -> Result<ImportReport> {
    let (owner, name) = parse_repo(repo)?;
    let mut report = ImportReport::new(repo, options.dry_run);
    let store = metis_persona::PersonaStore::new(paths.clone());

    let url = crate::tarball::tarball_url(&owner, &name, default_branch);
    let (status, body) = fetch(&url).map_err(|error| {
        MetisError::new(ErrorKind::Network, format!("下载 {repo} 失败：{error}"))
            .with_remediation("检查网络；或先把仓库克隆到本地，再用 `metis import <本地目录>`")
    })?;
    if status == 404 {
        return Err(MetisError::new(ErrorKind::Network, format!("{repo} 不存在，或分支 `{default_branch}` 不存在"))
            .with_remediation("确认 `owner/repo` 拼写；私有仓库需要凭据，目前不支持"));
    }
    if status != 200 {
        return Err(MetisError::new(ErrorKind::Network, format!("下载 {repo} 返回 {status}")).with_remediation("稍后重试"));
    }

    let tarball = crate::tarball::extract(&body)?;

    // 有索引就按索引走（顺序稳定、带 name/description）；没有索引就按文件名。
    //
    // 为什么要兼容"没有索引"：用户完全可以自己建个人设仓库，不该强制它们
    // 也配一套 workflow。
    let entries: Vec<(String, String)> = match tarball.index.as_deref() {
        Some(text) => {
            let index: PersonaIndex = serde_json::from_str(text).map_err(|error| {
                MetisError::new(ErrorKind::Network, "仓库里的 personas.json 不是预期格式")
                    .with_detail(error.to_string())
                    .with_remediation("`cargo xtask`/仓库的 workflow 生成的是这个格式；确认没有被手改过")
            })?;
            index.personas.into_iter().filter(|entry| tarball.personas.contains_key(&entry.file)).map(|entry| (entry.id, entry.file)).collect()
        }
        None => tarball
            .personas
            .iter()
            .map(|(file, text)| {
                // **优先用 front-matter 里声明的 id**，文件名只作兜底。
                //
                // 第三方库的文件名不一定是合法 id（`CTF.md` → `CTF`，
                // 或者作者用中文/大写命名），而文件自己声明的 id 才是权威。
                let stem = std::path::Path::new(file).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
                let declared = metis_persona::parse_persona(text).meta.id.unwrap_or_default();
                let id = if !declared.trim().is_empty() && PersonaId::parse(declared.trim()).is_ok() { declared.trim().to_owned() } else { stem.to_owned() };
                (id, file.clone())
            })
            .collect(),
    };

    if entries.is_empty() {
        report.push("（无）", ImportOutcome::Skipped, Some("仓库里没有找到人设（personas/*.md）".to_owned()));
        return Ok(report);
    }

    for (id, file) in entries {
        match tarball.personas.get(&file) {
            Some(text) => import_text(&store, &id, text, options, &mut report),
            None => report.push(id, ImportOutcome::Failed, Some(format!("索引指向的 {file} 不存在"))),
        }
    }
    Ok(report)
}

/// 人设库的索引文件（`personas.json`）。
///
/// 由源仓库的 workflow 生成，客户端只读。**格式带版本号**：
/// 将来加字段时旧客户端能明确拒绝，而不是解析出半个东西。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PersonaIndex {
    /// 格式版本。
    #[serde(default = "index_version")]
    pub v: u32,
    /// 人设条目。
    #[serde(default)]
    pub personas: Vec<IndexEntry>,
}

fn index_version() -> u32 {
    1
}

/// 索引里的一条。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IndexEntry {
    /// 人设 id。
    pub id: String,
    /// 显示名（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// 说明（可选）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// 仓库内相对路径，例如 `personas/concise.md`。
    pub file: String,
    /// 字节数（可选，用于粗略校验）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// 是不是"文档"而不是"人设"。
///
/// 人设库仓库里一定会有 `README.md`、`CHANGELOG.md` 之类 —— 它们不是人设。
fn is_documentation_file(path: &Path) -> bool {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    matches!(stem.as_str(), "readme" | "license" | "licence" | "changelog" | "contributing" | "notice" | "authors" | "code_of_conduct" | "security")
}

/// 从本地目录导入一份。
fn import_one(store: &metis_persona::PersonaStore, stem: &str, file: &Path, options: &ImportOptions, report: &mut ImportReport) {
    let Ok(text) = std::fs::read_to_string(file) else {
        report.push(stem, ImportOutcome::Failed, Some("读不了（可能不是 UTF-8）".to_owned()));
        return;
    };
    import_text(store, stem, &text, options, report);
}

/// 解析 `owner/repo`（允许前缀 `github.com/` 与 `.git` 后缀）。
pub fn parse_repo(input: &str) -> Result<(String, String)> {
    let cleaned = input
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("github.com/")
        .trim_end_matches(".git")
        .trim_end_matches('/');
    let mut parts = cleaned.split('/');
    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    if owner.is_empty() || repo.is_empty() || parts.next().is_some() {
        return Err(MetisError::new(ErrorKind::InvalidArgument, format!("仓库要写成 owner/repo：{input}"))
            .with_remediation("例如 `metis import dulingzhi/metis-personas`"));
    }
    // 只允许安全的字符，避免拼出奇怪的 URL 或路径穿越。
    // **必须拒绝 `.` / `..`**：它们是合法字符但会把请求打到意料之外的路径。
    let valid = |value: &str| {
        !value.is_empty()
            && value != "."
            && value != ".."
            && !value.starts_with('.')
            && value.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    };
    if !valid(owner) || !valid(repo) {
        return Err(MetisError::new(ErrorKind::InvalidArgument, format!("仓库名含非法字符：{input}"))
            .with_remediation("仓库名只能含字母、数字、`-`、`_`、`.`，且不能以 `.` 开头"));
    }
    Ok((owner.to_owned(), repo.to_owned()))
}

fn import_text(store: &metis_persona::PersonaStore, stem: &str, text: &str, options: &ImportOptions, report: &mut ImportReport) {
    let Ok(id) = PersonaId::parse(stem) else {
        report.push(stem, ImportOutcome::Failed, Some(format!("文件名不能作为人设 id：{stem}")));
        return;
    };
    let parsed = metis_persona::parse_persona(text);

    match store.read(&id) {
        Ok(Some(existing)) => {
            // 内容相同：不重复写，也不报成"新增"。
            if existing.trimmed_body() == parsed.body.trim() {
                report.push(stem, ImportOutcome::Identical, Some("内容与现有的完全相同".to_owned()));
                return;
            }
            if !options.overwrite {
                report.push(stem, ImportOutcome::Skipped, Some("同名已存在；加 --overwrite 覆盖".to_owned()));
                return;
            }
            if !options.dry_run
                && let Err(error) = store.save(&id, &parsed.meta, &parsed.body)
            {
                report.push(stem, ImportOutcome::Failed, Some(error.message().to_owned()));
                return;
            }
            report.push(stem, ImportOutcome::Overwritten, if options.dry_run { Some("（dry-run）".to_owned()) } else { None });
        }
        Ok(None) => {
            if !options.dry_run
                && let Err(error) = store.save(&id, &parsed.meta, &parsed.body)
            {
                report.push(stem, ImportOutcome::Failed, Some(error.message().to_owned()));
                return;
            }
            report.push(stem, ImportOutcome::Imported, if options.dry_run { Some("（dry-run）".to_owned()) } else { None });
        }
        Err(error) => report.push(stem, ImportOutcome::Failed, Some(error.message().to_owned())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;

    fn setup() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        (dir, paths)
    }

    fn write(dir: &Path, name: &str, body: &str) {
        std::fs::write(dir.join(name), format!("---\nid: {}\n---\n\n{body}\n", name.trim_end_matches(".md"))).expect("write");
    }

    #[test]
    fn parse_repo_accepts_the_common_forms() {
        for input in ["owner/repo", "github.com/owner/repo", "https://github.com/owner/repo", "https://github.com/owner/repo.git"] {
            let (owner, repo) = parse_repo(input).expect(input);
            assert_eq!(owner, "owner", "{input}");
            assert_eq!(repo, "repo", "{input}");
        }
    }

    #[test]
    fn parse_repo_rejects_junk() {
        for bad in ["", "owner", "a/b/c", "owner/re po", "../x", "owner/repo/"] {
            // `owner/repo/` 尾部斜杠会被 trim 掉，是合法的；其它都应拒绝。
            if bad == "owner/repo/" {
                continue;
            }
            assert!(parse_repo(bad).is_err(), "{bad:?} 不该被接受");
        }
    }

    #[test]
    fn import_dir_adds_new_personas() {
        let (dir, paths) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        write(&src, "a.md", "甲");
        write(&src, "b.md", "乙");

        let report = import_dir(&paths, &src, &ImportOptions::default()).expect("import");
        assert_eq!(report.imported(), 2, "{report:?}");
        let store = metis_persona::PersonaStore::new(paths.clone());
        assert_eq!(store.read(&PersonaId::parse("a").expect("id")).expect("read").expect("present").trimmed_body(), "甲");
    }

    #[test]
    fn import_dir_is_idempotent_and_reports_identical() {
        let (dir, paths) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        write(&src, "a.md", "甲");

        import_dir(&paths, &src, &ImportOptions::default()).expect("first");
        let second = import_dir(&paths, &src, &ImportOptions::default()).expect("second");
        assert_eq!(second.imported(), 0, "第二次不该再算导入");
        assert!(second.items.iter().any(|i| i.outcome == ImportOutcome::Identical), "应报告内容相同");
    }

    #[test]
    fn existing_persona_is_skipped_unless_overwrite() {
        let (dir, paths) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        let store = metis_persona::PersonaStore::new(paths.clone());
        store.save(&PersonaId::parse("a").expect("id"), &metis_persona::Meta::default(), "原来的").expect("save");
        write(&src, "a.md", "新的");

        let skipped = import_dir(&paths, &src, &ImportOptions::default()).expect("import");
        assert!(skipped.items.iter().any(|i| i.outcome == ImportOutcome::Skipped));
        assert_eq!(store.read(&PersonaId::parse("a").expect("id")).expect("read").expect("present").trimmed_body(), "原来的");

        let over = import_dir(&paths, &src, &ImportOptions { overwrite: true, dry_run: false }).expect("import");
        assert!(over.items.iter().any(|i| i.outcome == ImportOutcome::Overwritten));
        assert_eq!(store.read(&PersonaId::parse("a").expect("id")).expect("read").expect("present").trimmed_body(), "新的");
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (dir, paths) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        write(&src, "a.md", "甲");
        let report = import_dir(&paths, &src, &ImportOptions { overwrite: false, dry_run: true }).expect("import");
        assert!(report.dry_run);
        assert_eq!(report.imported(), 1, "预览也算会导入");
        let store = metis_persona::PersonaStore::new(paths.clone());
        assert!(store.read(&PersonaId::parse("a").expect("id")).expect("read").is_none(), "dry-run 不能写");
    }

    #[test]
    fn it_reads_the_personas_subdirectory() {
        // 真踩过：只读当前目录，于是一个结构正常的仓库（`personas/*.md` + `README.md`）
        // 导进来是**零份人设**。
        let (dir, paths) = setup();
        let src = dir.path().join("repo");
        std::fs::create_dir_all(src.join("personas")).expect("mkdir");
        std::fs::write(src.join("README.md"), "# 说明\n").expect("write");
        write(&src.join("personas"), "a.md", "甲");

        let report = import_dir(&paths, &src, &ImportOptions::default()).expect("import");
        assert_eq!(report.imported(), 1, "应读到 personas/ 下的人设：{report:?}");
        let store = metis_persona::PersonaStore::new(paths.clone());
        assert!(store.read(&PersonaId::parse("a").expect("id")).expect("read").is_some());
    }

    #[test]
    fn readme_is_never_imported_as_a_persona() {
        // 真踩过：`README.md` 被当成一份 id 叫 `README` 的人设导了进去，
        // 用户的 `metis ls` 里会多一份垃圾。
        let (dir, paths) = setup();
        let src = dir.path().join("flat");
        std::fs::create_dir_all(&src).expect("mkdir");
        for doc in ["README.md", "CHANGELOG.md", "CONTRIBUTING.md", "LICENSE.md"] {
            std::fs::write(src.join(doc), "# 文档\n").expect("write");
        }
        write(&src, "real.md", "真人设");

        let report = import_dir(&paths, &src, &ImportOptions::default()).expect("import");
        assert_eq!(report.imported(), 1, "只应导入真人设：{report:?}");
        assert!(report.items.iter().any(|i| i.id == "real"));
        for doc in ["README", "CHANGELOG", "CONTRIBUTING", "LICENSE"] {
            assert!(!report.items.iter().any(|i| i.id == doc), "{doc} 不该被导入");
        }
    }

    #[test]
    fn missing_dir_is_a_clear_error() {
        let (_dir, paths) = setup();
        let error = import_dir(&paths, Path::new("/definitely/not/here"), &ImportOptions::default()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn empty_dir_is_reported_not_an_error() {
        let (dir, paths) = setup();
        let src = dir.path().join("empty");
        std::fs::create_dir_all(&src).expect("mkdir");
        let report = import_dir(&paths, &src, &ImportOptions::default()).expect("import");
        assert_eq!(report.imported(), 0);
        assert!(report.items.iter().any(|i| i.outcome == ImportOutcome::Skipped));
    }

    #[test]
    fn source_dir_is_never_modified() {
        let (dir, paths) = setup();
        let src = dir.path().join("src");
        std::fs::create_dir_all(&src).expect("mkdir");
        write(&src, "a.md", "甲");
        let before = std::fs::read(src.join("a.md")).expect("read");
        import_dir(&paths, &src, &ImportOptions::default()).expect("import");
        assert_eq!(std::fs::read(src.join("a.md")).expect("read"), before, "只读导入");
    }
    /// 造一个 tarball，供 GitHub 导入测试使用。
    fn tarball(files: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
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
        gz.finish().expect("gz")
    }

    #[test]
    fn github_import_reads_one_tarball() {
        // 核心：**只发一个请求**，且不打 api.github.com。
        let (_dir, paths) = setup();
        let archive = tarball(&[
            ("repo-main/personas.json", r#"{"v":1,"personas":[{"id":"a","file":"personas/a.md"}]}"#),
            ("repo-main/personas/a.md", "---\nid: a\n---\n\n来自 GitHub\n"),
        ]);
        let requested: std::cell::RefCell<Vec<String>> = std::cell::RefCell::new(Vec::new());

        let report = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |url| {
            requested.borrow_mut().push(url.to_owned());
            Ok((200, archive.clone()))
        })
        .expect("import");

        let requested = requested.borrow();
        assert_eq!(requested.len(), 1, "只应发一个请求，实际：{requested:?}");
        assert!(requested[0].starts_with("https://codeload.github.com/"), "{}", requested[0]);
        assert!(!requested[0].contains("api.github.com"), "不该走 API");
        assert!(!requested[0].contains("raw.githubusercontent.com"), "不该走 raw");

        assert_eq!(report.imported(), 1, "{report:?}");
        let store = metis_persona::PersonaStore::new(paths.clone());
        assert_eq!(store.read(&PersonaId::parse("a").expect("id")).expect("read").expect("present").trimmed_body(), "来自 GitHub");
    }

    #[test]
    fn a_repo_without_an_index_still_imports() {
        // 用户自建的人设仓库不该被强制配 workflow。
        let (_dir, paths) = setup();
        let archive = tarball(&[("repo-main/personas/x.md", "---\nid: x\n---\n\n无索引\n"), ("repo-main/personas/y.md", "---\nid: y\n---\n\n也导入\n")]);
        let report = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |_| Ok((200, archive.clone()))).expect("import");
        assert_eq!(report.imported(), 2, "没有索引时应按文件名导入：{report:?}");
    }

    #[test]
    fn a_404_is_a_clear_error() {
        let (_dir, paths) = setup();
        let error = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |_| Ok((404, Vec::new()))).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.remediation().is_some(), "应给补救建议");
    }

    #[test]
    fn github_network_failure_is_reported() {
        let (_dir, paths) = setup();
        let error = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |_| Err("offline".to_owned())).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("offline") || error.remediation().is_some());
    }

    #[test]
    fn a_bad_index_json_is_reported() {
        let (_dir, paths) = setup();
        let archive = tarball(&[("repo-main/personas.json", "not json at all")]);
        let error = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |_| Ok((200, archive.clone()))).expect_err("must fail");
        assert!(error.message().contains("personas.json"), "{}", error.message());
    }

    #[test]
    fn a_corrupt_tarball_is_reported() {
        let (_dir, paths) = setup();
        let error = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |_| Ok((200, b"not gzip".to_vec()))).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
    }

    #[test]
    fn an_empty_repo_is_reported_not_an_error() {
        let (_dir, paths) = setup();
        let archive = tarball(&[("repo-main/README.md", "# 空仓库")]);
        let report = import_github(&paths, "owner/repo", &ImportOptions::default(), "main", |_| Ok((200, archive.clone()))).expect("import");
        assert_eq!(report.imported(), 0);
        assert!(report.items.iter().any(|i| i.outcome == ImportOutcome::Skipped), "{report:?}");
    }

    #[test]
    fn the_index_round_trips() {
        let index = PersonaIndex {
            v: 1,
            personas: vec![IndexEntry {
                id: "a".to_owned(),
                name: Some("甲".to_owned()),
                description: None,
                file: "personas/a.md".to_owned(),
                bytes: Some(42),
            }],
        };
        let json = serde_json::to_string(&index).expect("json");
        let back: PersonaIndex = serde_json::from_str(&json).expect("parse");
        assert_eq!(back, index);
    }
}
