//! 把一个 Hub `expert` 映射成本地人设。
//!
//! 规则：
//! - `content.prompt` → 正文主体；
//! - `content.knowledge` → 正文里的“参考资料”小节（可关）；
//! - `content.skills` → 逐个安装到 `~/.metis/skills/<slug>/`，再把本地路径写回正文；
//! - 远端来源信息 → front-matter 的 `hub-*` 键（未知键会被 `metis-persona` 原样保留）；
//! - **只写人设文件与技能目录，不自动 apply/arm**。

use std::collections::BTreeMap;

use metis_core::{ErrorKind, MetisError, PersonaId, Result};
use metis_paths::Paths;
use metis_persona::{Meta, Persona, PersonaStore};
use serde::Serialize;
use url::Url;

use crate::hub::{FetchRequest, HttpResponse, HubClient, HubDetail, single_line};
use crate::hub_skills::{SkillInstallOptions, SkillInstallReport, install_skill};
use crate::persona_import::ImportOutcome;

/// 单次导入最多安装多少个关联技能（公开读限流 60 次/分钟）。
pub const MAX_SKILLS_PER_IMPORT: usize = 10;

/// 关联技能的选择。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum SkillSelection {
    /// 安装全部关联技能（最多 [`MAX_SKILLS_PER_IMPORT`] 个）。
    #[default]
    All,
    /// 一个都不装。
    None,
    /// 只装指定的 slug。
    Only(Vec<String>),
}

/// 导入选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HubImportOptions {
    /// 覆盖同名的人设。
    pub overwrite: bool,
    /// 只预览，不写盘。
    pub dry_run: bool,
    /// 把 `knowledge` 追加到正文。
    pub include_knowledge: bool,
    /// 允许导入 `verified=false` 的条目（UI 会二次确认）。
    pub allow_unverified: bool,
    /// 绕过缓存新鲜期。
    pub refresh: bool,
    /// 指定人设 id；默认用 Hub slug。
    pub as_id: Option<String>,
    /// 关联技能的选择。
    pub skills: SkillSelection,
}

impl Default for HubImportOptions {
    fn default() -> Self {
        Self { overwrite: false, dry_run: false, include_knowledge: true, allow_unverified: false, refresh: false, as_id: None, skills: SkillSelection::All }
    }
}

/// 一次 Hub 导入的报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubImportReport {
    /// Hub 地址。
    pub source: String,
    /// 条目类型（`expert`）。
    pub kind: String,
    /// Hub slug。
    pub slug: String,
    /// 版本。
    pub version: String,
    /// 本地人设 id。
    pub persona_id: String,
    /// 人设的结局。
    pub outcome: ImportOutcome,
    /// 条目是否通过服务端哈希校验。
    pub verified: bool,
    /// 是否只是预览。
    pub dry_run: bool,
    /// 逐个技能的结局。
    pub skills: Vec<SkillInstallReport>,
    /// 汇总告警。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// 导入一个 expert，并按需安装关联技能。
///
/// 网络通过 `fetch` 注入：生产传 [`crate::hub::ureq_fetch`]，测试传假响应。
pub fn import_expert<F>(paths: &Paths, base: &Url, fetch: F, slug: &str, version: Option<&str>, options: &HubImportOptions) -> Result<HubImportReport>
where
    F: Fn(&FetchRequest) -> Result<HttpResponse>,
{
    let client = HubClient::new(base.clone(), Some(paths.hub_cache_dir()), fetch);
    let detail = client.show_expert(slug, version, options.refresh)?;
    if !detail.item.verified && !options.allow_unverified && !options.dry_run {
        return Err(
            MetisError::new(ErrorKind::Refused, "这个 Hub 条目的内容哈希校验失败").with_remediation("确认来源后加 `--allow-unverified`；界面会要求二次确认")
        );
    }

    let persona_id = persona_id_for(&detail, options.as_id.as_deref())?;
    let mut skills = Vec::new();
    let selected = select_skill_slugs(&detail, &options.skills);
    for skill_slug in &selected {
        match client.show_skill(skill_slug, None, options.refresh) {
            Ok(skill) => match install_skill(paths, &skill, &SkillInstallOptions { overwrite: options.overwrite, dry_run: options.dry_run }) {
                Ok(report) => skills.push(report),
                Err(error) => skills.push(failed_skill_report(skill_slug, &error)),
            },
            Err(error) => skills.push(failed_skill_report(skill_slug, &error)),
        }
    }
    if let SkillSelection::Only(requested) = &options.skills {
        for skill_slug in requested {
            if !detail.content.skills.iter().any(|skill| &skill.slug == skill_slug) {
                skills.push(failed_skill_report(
                    skill_slug,
                    &MetisError::new(ErrorKind::InvalidArgument, "专家没有引用这个 skill").with_remediation("从详情里的技能列表选择；不要手填不存在的 slug"),
                ));
            }
        }
    }

    let body = build_persona_body(&detail, &skills, options.include_knowledge);
    let meta = build_meta(&detail, base, &skills);
    let store = PersonaStore::new(paths.clone());
    let existing = store.read(&persona_id)?;
    let (outcome, note) = decide_outcome(existing.as_ref(), &meta, &body, options);
    if !options.dry_run && matches!(outcome, ImportOutcome::Imported | ImportOutcome::Overwritten) {
        store.save(&persona_id, &meta, &body)?;
    }

    let mut warning = detail.warning.clone();
    let requested = match &options.skills {
        SkillSelection::All => detail.content.skills.len(),
        SkillSelection::None => 0,
        SkillSelection::Only(values) => values.len(),
    };
    if selected.len() >= MAX_SKILLS_PER_IMPORT && requested > MAX_SKILLS_PER_IMPORT {
        append_warning(&mut warning, format!("一次最多安装 {MAX_SKILLS_PER_IMPORT} 个技能，其余未安装；可用 `--skills` 分批指定"));
    }
    if let Some(note) = note {
        append_warning(&mut warning, note);
    }

    Ok(HubImportReport {
        source: base.as_str().trim_end_matches('/').to_owned(),
        kind: detail.item.kind.clone(),
        slug: detail.item.slug.clone(),
        version: detail.item.version.clone(),
        persona_id: persona_id.as_str().to_owned(),
        outcome,
        verified: detail.item.verified,
        dry_run: options.dry_run,
        skills,
        warning,
    })
}

/// 按选择挑出要安装的技能 slug（最多 `MAX_SKILLS_PER_IMPORT` 个）。
pub fn select_skill_slugs(detail: &HubDetail, selection: &SkillSelection) -> Vec<String> {
    let linked: Vec<String> = detail.content.skills.iter().map(|skill| skill.slug.clone()).filter(|slug| !slug.is_empty()).collect();
    let mut out: Vec<String> = Vec::new();
    match selection {
        SkillSelection::None => {}
        SkillSelection::All => {
            for slug in linked {
                if !out.contains(&slug) {
                    out.push(slug);
                }
                if out.len() >= MAX_SKILLS_PER_IMPORT {
                    break;
                }
            }
        }
        SkillSelection::Only(requested) => {
            for slug in linked {
                if requested.contains(&slug) && !out.contains(&slug) {
                    out.push(slug);
                    if out.len() >= MAX_SKILLS_PER_IMPORT {
                        break;
                    }
                }
            }
        }
    }
    out
}

/// 生成人设正文：prompt + 可选参考资料 + 已安装技能的本地路径。
pub fn build_persona_body(detail: &HubDetail, skills: &[SkillInstallReport], include_knowledge: bool) -> String {
    let mut body = detail.content.prompt.trim().to_owned();
    if include_knowledge {
        for file in &detail.content.knowledge {
            body.push_str("\n\n## 参考资料：");
            body.push_str(&single_line(&file.name, 120));
            body.push_str("\n\n");
            body.push_str(file.text.trim());
        }
    }

    let installed: Vec<&SkillInstallReport> = skills
        .iter()
        .filter(|skill| skill.entry.is_some() && matches!(skill.outcome, ImportOutcome::Imported | ImportOutcome::Overwritten | ImportOutcome::Identical))
        .collect();
    if !installed.is_empty() {
        body.push_str("\n\n## 本地技能\n\n以下技能已随人设安装到本机。需要时先读取对应文件，再按其说明操作：\n");
        for skill in installed {
            let entry = skill.entry.as_deref().unwrap_or_default().replace('`', "'");
            body.push_str(&format!("\n- {}（v{}）：`{}`", single_line(&skill.name, 80), single_line(&skill.version, 32), entry));
            if let Some(description) = skill.description.as_deref().map(|text| single_line(text, 200)).filter(|text| !text.is_empty()) {
                body.push_str(" — ");
                body.push_str(&description);
            }
        }
    }
    body
}

/// 生成人设的 front-matter：名称/说明 + `hub-*` 来源信息。
pub fn build_meta(detail: &HubDetail, base: &Url, skills: &[SkillInstallReport]) -> Meta {
    let mut meta =
        Meta { name: front_matter_scalar(&detail.item.name, 80), description: front_matter_scalar(&detail.item.description, 300), ..Meta::default() };
    let mut extra = BTreeMap::new();
    extra.insert("hub-url".to_owned(), base.as_str().trim_end_matches('/').to_owned());
    extra.insert("hub-kind".to_owned(), single_line(&detail.item.kind, 16));
    extra.insert("hub-slug".to_owned(), single_line(&detail.item.slug, 64));
    extra.insert("hub-version".to_owned(), single_line(&detail.item.version, 32));
    extra.insert("hub-hash".to_owned(), single_line(&detail.item.hash, 80));
    extra.insert("hub-verified".to_owned(), detail.item.verified.to_string());
    extra.insert("hub-official".to_owned(), detail.item.official.to_string());
    if let Some(author) = detail.item.author.as_deref().map(|value| single_line(value, 64)).filter(|value| !value.is_empty()) {
        extra.insert("hub-author".to_owned(), author);
    }
    if let Some(tags) = flow_list(&detail.item.tags) {
        extra.insert("hub-tags".to_owned(), tags);
    }
    if let Some(providers) = flow_list(&detail.item.providers) {
        extra.insert("hub-providers".to_owned(), providers);
    }
    let installed: Vec<String> = skills
        .iter()
        .filter(|skill| skill.entry.is_some() && matches!(skill.outcome, ImportOutcome::Imported | ImportOutcome::Overwritten | ImportOutcome::Identical))
        .map(|skill| format!("{}@{}", single_line(&skill.slug, 64), single_line(&skill.version, 32)))
        .collect();
    if let Some(list) = flow_list(&installed) {
        extra.insert("hub-skills".to_owned(), list);
    }
    if let Some(model) = detail.content.effective_model().map(|value| single_line(value, 64)).filter(|value| !value.is_empty()) {
        extra.insert("hub-model".to_owned(), model);
    }
    if let Some(tools) = flow_list(detail.content.effective_tools()) {
        extra.insert("hub-tools".to_owned(), tools);
    }
    if let Some(notes) = detail.content.notes.as_deref().map(|value| single_line(value, 300)).filter(|value| !value.is_empty()) {
        extra.insert("hub-notes".to_owned(), notes);
    }
    extra.insert("hub-imported-at".to_owned(), now_rfc3339());
    meta.extra = extra;
    meta
}

fn persona_id_for(detail: &HubDetail, requested: Option<&str>) -> Result<PersonaId> {
    let raw = requested.map(str::trim).filter(|value| !value.is_empty()).unwrap_or(detail.item.slug.as_str());
    PersonaId::parse(raw).map_err(|error| {
        MetisError::new(ErrorKind::InvalidArgument, format!("Hub 条目不能直接作为人设 id：{raw}"))
            .with_detail(error.to_string())
            .with_remediation("用 `--as <id>` 指定一个合法 id（字母、数字、`.`、`-`、`_`；不能是 active）")
    })
}

fn decide_outcome(existing: Option<&Persona>, meta: &Meta, body: &str, options: &HubImportOptions) -> (ImportOutcome, Option<String>) {
    let Some(persona) = existing else {
        return (ImportOutcome::Imported, options.dry_run.then(|| "（dry-run）".to_owned()));
    };
    let same_body = persona.trimmed_body() == body.trim();
    let same_hash = persona.meta.extra.get("hub-hash") == meta.extra.get("hub-hash");
    let same_version = persona.meta.extra.get("hub-version") == meta.extra.get("hub-version");
    if same_body && same_hash && same_version {
        return (ImportOutcome::Identical, None);
    }
    if !options.overwrite {
        return (ImportOutcome::Skipped, Some("同名已存在；加 --overwrite 覆盖".to_owned()));
    }
    (ImportOutcome::Overwritten, options.dry_run.then(|| "（dry-run）".to_owned()))
}

fn failed_skill_report(slug: &str, error: &MetisError) -> SkillInstallReport {
    SkillInstallReport {
        slug: slug.to_owned(),
        name: slug.to_owned(),
        description: None,
        version: String::new(),
        outcome: ImportOutcome::Failed,
        entry: None,
        detail: Some(error.message().to_owned()),
    }
}

fn flow_list(values: &[String]) -> Option<String> {
    let items: Vec<String> = values.iter().map(|value| single_line(value, 64).replace(['[', ']', ','], "")).filter(|value| !value.is_empty()).collect();
    (!items.is_empty()).then(|| format!("[{}]", items.join(", ")))
}

/// front-matter 的单行标量。以 `[` 开头并以 `]` 结尾的值会被误读成 flow 数组，改成圆括号。
fn front_matter_scalar(value: &str, max: usize) -> Option<String> {
    let mut text = single_line(value, max);
    if text.starts_with('[') && text.ends_with(']') {
        text.replace_range(..1, "(");
        let last = text.len().saturating_sub(1);
        text.replace_range(last.., ")");
    }
    (!text.is_empty()).then_some(text)
}

fn append_warning(warning: &mut Option<String>, text: String) {
    if text.trim().is_empty() {
        return;
    }
    *warning = Some(match warning.take() {
        Some(existing) => format!("{existing}；{text}"),
        None => text,
    });
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{DataSource, ExpertContent, HubSummary, SkillRef};

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        (dir, paths)
    }

    fn base() -> Url {
        crate::hub::parse_hub_url("https://hub.example.com").expect("base")
    }

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse { status, etag: None, content_encoding: None, body: body.as_bytes().to_vec() }
    }

    fn expert_json(prompt: &str, knowledge: &str, skills: &str) -> String {
        format!(
            r#"{{"kind":"expert","slug":"security-reviewer","name":"安全审查专家","description":"desc","version":"1.2.0","status":"approved","visibility":"public","author":"acc_1","official":false,"tags":["security"],"providers":["claude"],"source":{{"kind":"inline"}},"hash":"sha256:expert","verified":true,"content":{{"prompt":"{prompt}","knowledge":[{knowledge}],"skills":[{skills}]}}}}"#
        )
    }

    fn skill_json(slug: &str, version: &str, files: &str) -> String {
        format!(
            r#"{{"kind":"skill","slug":"{slug}","name":"技能 {slug}","description":"d","version":"{version}","status":"approved","visibility":"public","verified":true,"hash":"sha256:{version}","content":{{"entry":"SKILL.md","body":"技能正文 {slug}","files":[{files}]}}}}"#
        )
    }

    fn fetch_from(expert: String, skills: Vec<(String, String)>) -> impl Fn(&FetchRequest) -> Result<HttpResponse> {
        move |request| {
            if request.url.contains("/v1/hub/items/expert/security-reviewer") || request.url.contains("/v1/hub/items/expert/active") {
                return Ok(response(200, &expert));
            }
            for (slug, body) in &skills {
                if request.url.contains(&format!("/v1/hub/items/skill/{slug}")) {
                    return Ok(response(200, body));
                }
            }
            Err(MetisError::new(ErrorKind::Internal, format!("测试没有预置这个 URL：{}", request.url)))
        }
    }

    fn read_persona(paths: &Paths, id: &str) -> Persona {
        PersonaStore::new(paths.clone()).read(&PersonaId::parse(id).expect("id")).expect("read").expect("present")
    }

    #[test]
    fn imports_expert_and_installs_linked_skills() {
        let (_dir, paths) = paths();
        let expert = expert_json("你是安全审查专家。", r#"{"name":"checklist.md","text":"检查清单"}"#, r#"{"slug":"wb-apiz","name":"Apiz"}"#);
        let skill = skill_json("wb-apiz", "1.0.0", r#"{"path":"references/x.md","text":"附件"}"#);
        let report =
            import_expert(&paths, &base(), fetch_from(expert, vec![("wb-apiz".to_owned(), skill)]), "security-reviewer", None, &HubImportOptions::default())
                .expect("import");

        assert_eq!(report.outcome, ImportOutcome::Imported);
        assert_eq!(report.persona_id, "security-reviewer");
        assert_eq!(report.skills.len(), 1);
        assert_eq!(report.skills[0].outcome, ImportOutcome::Imported);

        let persona = read_persona(&paths, "security-reviewer");
        assert!(persona.body.contains("## 参考资料：checklist.md"), "{}", persona.body);
        assert!(persona.body.contains("## 本地技能"), "{}", persona.body);
        assert!(persona.body.contains("SKILL.md"), "{}", persona.body);
        assert_eq!(persona.meta.extra.get("hub-slug").map(String::as_str), Some("security-reviewer"));
        assert_eq!(persona.meta.extra.get("hub-version").map(String::as_str), Some("1.2.0"));
        assert!(persona.meta.extra.get("hub-skills").is_some_and(|value| value.contains("wb-apiz@1.0.0")), "{:?}", persona.meta.extra);
        assert!(paths.skill_dir("wb-apiz").join("SKILL.md").is_file());
        assert!(paths.skill_dir("wb-apiz").join("references/x.md").is_file());
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (_dir, paths) = paths();
        let expert = expert_json("prompt", r#"{"name":"k.md","text":"x"}"#, r#"{"slug":"wb-apiz","name":"Apiz"}"#);
        let skill = skill_json("wb-apiz", "1.0.0", "");
        let options = HubImportOptions { dry_run: true, ..HubImportOptions::default() };
        let report =
            import_expert(&paths, &base(), fetch_from(expert, vec![("wb-apiz".to_owned(), skill)]), "security-reviewer", None, &options).expect("import");
        assert!(report.dry_run);
        assert!(!paths.personas_dir().join("security-reviewer.md").exists());
        assert!(!paths.skill_dir("wb-apiz").exists());
    }

    #[test]
    fn an_identical_import_is_reported_as_identical() {
        let (_dir, paths) = paths();
        let options = HubImportOptions::default();
        let expert = expert_json("prompt", "", "");
        import_expert(&paths, &base(), fetch_from(expert.clone(), Vec::new()), "security-reviewer", None, &options).expect("first");
        let second = import_expert(&paths, &base(), fetch_from(expert, Vec::new()), "security-reviewer", None, &options).expect("second");
        assert_eq!(second.outcome, ImportOutcome::Identical);
    }

    #[test]
    fn an_existing_persona_is_skipped_unless_overwrite() {
        let (_dir, paths) = paths();
        let options = HubImportOptions::default();
        import_expert(&paths, &base(), fetch_from(expert_json("旧", "", ""), Vec::new()), "security-reviewer", None, &options).expect("first");
        // 同一 URL 的详情有 10 分钟缓存；模拟“Hub 内容变了”时必须显式 refresh。
        let changed = HubImportOptions { refresh: true, ..options.clone() };
        let skipped = import_expert(&paths, &base(), fetch_from(expert_json("新", "", ""), Vec::new()), "security-reviewer", None, &changed).expect("skipped");
        assert_eq!(skipped.outcome, ImportOutcome::Skipped);
        assert_eq!(read_persona(&paths, "security-reviewer").trimmed_body(), "旧");
        let overwritten = import_expert(
            &paths,
            &base(),
            fetch_from(expert_json("新", "", ""), Vec::new()),
            "security-reviewer",
            None,
            &HubImportOptions { overwrite: true, refresh: true, ..options },
        )
        .expect("overwritten");
        assert_eq!(overwritten.outcome, ImportOutcome::Overwritten);
        assert_eq!(read_persona(&paths, "security-reviewer").trimmed_body(), "新");
    }

    #[test]
    fn unverified_is_refused_without_the_flag_but_allowed_for_dry_run() {
        let (_dir, paths) = paths();
        let expert = expert_json("prompt", "", "").replace(r#""verified":true"#, r#""verified":false"#);
        let error = import_expert(&paths, &base(), fetch_from(expert.clone(), Vec::new()), "security-reviewer", None, &HubImportOptions::default())
            .expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Refused);
        assert!(error.remediation().is_some());
        let dry = import_expert(
            &paths,
            &base(),
            fetch_from(expert, Vec::new()),
            "security-reviewer",
            None,
            &HubImportOptions { dry_run: true, ..HubImportOptions::default() },
        )
        .expect("dry");
        assert!(!dry.verified);
    }

    #[test]
    fn knowledge_can_be_disabled() {
        let (_dir, paths) = paths();
        let expert = expert_json("prompt", r#"{"name":"k.md","text":"x"}"#, "");
        import_expert(
            &paths,
            &base(),
            fetch_from(expert, Vec::new()),
            "security-reviewer",
            None,
            &HubImportOptions { include_knowledge: false, ..HubImportOptions::default() },
        )
        .expect("import");
        let persona = read_persona(&paths, "security-reviewer");
        assert!(!persona.body.contains("参考资料"), "{}", persona.body);
    }

    #[test]
    fn as_id_overrides_the_persona_id_and_reserved_slugs_need_it() {
        let (_dir, paths) = paths();
        let expert = expert_json("prompt", "", "").replace(r#""slug":"security-reviewer""#, r#""slug":"active""#);
        let bare = import_expert(&paths, &base(), fetch_from(expert.clone(), Vec::new()), "active", None, &HubImportOptions::default());
        assert_eq!(bare.expect_err("reserved").kind(), ErrorKind::InvalidArgument);
        let report = import_expert(
            &paths,
            &base(),
            fetch_from(expert, Vec::new()),
            "active",
            None,
            &HubImportOptions { as_id: Some("my-active".to_owned()), ..HubImportOptions::default() },
        )
        .expect("with --as");
        assert_eq!(report.persona_id, "my-active");
    }

    #[test]
    fn skill_selection_caps_and_filters() {
        let detail = HubDetail {
            item: HubSummary { kind: "expert".to_owned(), slug: "x".to_owned(), name: "X".to_owned(), version: "1.0.0".to_owned(), ..Default::default() },
            content: ExpertContent {
                prompt: "p".to_owned(),
                skills: (0..15).map(|i| SkillRef { slug: format!("s{i}"), name: None }).collect(),
                ..Default::default()
            },
            fetched_at: 0,
            source: DataSource::Network,
            warning: None,
        };
        assert_eq!(select_skill_slugs(&detail, &SkillSelection::All).len(), MAX_SKILLS_PER_IMPORT);
        assert!(select_skill_slugs(&detail, &SkillSelection::None).is_empty());
        assert_eq!(select_skill_slugs(&detail, &SkillSelection::Only(vec!["s3".to_owned(), "missing".to_owned()])), vec!["s3".to_owned()]);
    }

    #[test]
    fn front_matter_values_do_not_look_like_flow_arrays() {
        let meta = build_meta(
            &HubDetail {
                item: HubSummary {
                    kind: "expert".to_owned(),
                    slug: "x".to_owned(),
                    name: "[Preview]".to_owned(),
                    description: "a\nb".to_owned(),
                    version: "1.0.0".to_owned(),
                    hash: "sha256:x".to_owned(),
                    verified: true,
                    tags: vec!["a".to_owned(), "b".to_owned()],
                    ..Default::default()
                },
                content: ExpertContent { prompt: "p".to_owned(), ..Default::default() },
                fetched_at: 0,
                source: DataSource::Network,
                warning: None,
            },
            &base(),
            &[],
        );
        assert_eq!(meta.name.as_deref(), Some("(Preview)"));
        assert_eq!(meta.description.as_deref(), Some("a b"));
        assert_eq!(meta.extra.get("hub-tags").map(String::as_str), Some("[a, b]"));
        let rendered = metis_persona::render_persona(&PersonaId::parse("x").expect("id"), &meta, "p");
        let parsed = metis_persona::parse_persona(&rendered);
        assert_eq!(parsed.meta.name.as_deref(), Some("(Preview)"));
        assert_eq!(parsed.meta.description.as_deref(), Some("a b"));
    }

    #[test]
    #[ignore = "需要真实 worldd hub（https://n.kkpt.tech:8443）"]
    fn live_import_of_an_expert_with_skills() {
        let (_dir, paths) = paths();
        let base = crate::hub::parse_hub_url(crate::hub::HUB_DEFAULT_URL).expect("url");
        let client = crate::hub::HubClient::new(base.clone(), Some(paths.hub_cache_dir()), crate::hub::ureq_fetch);
        let page = client.list(&crate::hub::HubQuery::experts()).expect("list");
        assert!(!page.items.is_empty(), "真实 Hub 的 expert 列表不应为空");

        // 列表摘要不含 content；取前几个详情，挑一个带关联技能的条目。
        let mut picked = None;
        for item in page.items.iter().take(5) {
            if let Ok(detail) = client.show_expert(&item.slug, None, true)
                && !detail.content.skills.is_empty()
            {
                picked = Some(item.slug.clone());
                break;
            }
        }
        let slug = picked.unwrap_or_else(|| page.items[0].slug.clone());
        let options = HubImportOptions { allow_unverified: true, ..HubImportOptions::default() };
        let report = import_expert(&paths, &base, crate::hub::ureq_fetch, &slug, None, &options).expect("import");

        assert_eq!(report.outcome, ImportOutcome::Imported);
        let persona = read_persona(&paths, &report.persona_id);
        assert!(!persona.trimmed_body().is_empty());
        assert!(paths.personas_dir().join(format!("{}.md", report.persona_id)).is_file());
        if !report.skills.is_empty() {
            assert!(paths.skills_dir().is_dir());
            assert!(
                report.skills.iter().any(|skill| skill.entry.as_deref().is_some_and(|entry| std::path::Path::new(entry).is_file())),
                "至少有一个技能落到了本地：{:?}",
                report.skills
            );
            assert!(persona.body.contains("## 本地技能"), "{}", persona.body);
        }
    }
}
