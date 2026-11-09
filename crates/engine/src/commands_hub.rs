//! Hub 相关的命令：导入 expert、安装/移除 skill。
//!
//! 业务逻辑在 `hub_import` 与 `hub_skills`；这里只做参数解析、URL 解析与结果序列化。
//! CLI/GUI 都按命令 id 分发（`AGENTS.md` 铁律 2）。

use serde_json::Value;
use url::Url;

use metis_core::{ErrorKind, MetisError, Result};

use crate::Engine;
use crate::command::{Command, CommandOutput, Effects, required_str};
use crate::hub::{HUB_DEFAULT_URL, HubClient, HubKind, HubProvider, HubQuery, parse_hub_url, ureq_fetch};
use crate::hub_import::{HubImportOptions, SkillSelection, import_expert};
use crate::hub_skills::{SkillInstallOptions, install_skill, remove_skill};

/// 列出远端 Hub 的 expert（分页）。
#[derive(Debug)]
pub struct HubList;

impl Command for HubList {
    fn id(&self) -> &'static str {
        "metis.hub-list"
    }
    fn title(&self) -> &'static str {
        "metis.hub-list.title"
    }
    fn effects(&self) -> Effects {
        Effects { writes_disk: true, spawns_process: false, network: true }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let base = resolve_hub_url(params)?;
        let provider = match string_param(params, "provider")? {
            Some(value) => Some(parse_provider(&value)?),
            None => None,
        };
        let query = HubQuery {
            kind: Some(HubKind::Expert),
            q: string_param(params, "query")?,
            tag: string_param(params, "tag")?,
            provider,
            cursor: string_param(params, "cursor")?,
            limit: usize_param(params, "limit", 20)?,
            refresh: bool_param(params, "refresh", false)?,
        };
        let client = HubClient::new(base, Some(engine.paths().hub_cache_dir()), ureq_fetch);
        let page = client.list(&query)?;
        Ok(CommandOutput::Json(serde_json::to_value(&page).unwrap_or(Value::Null)))
    }
}

/// 查看远端 Hub 一个 expert 的完整详情。
#[derive(Debug)]
pub struct HubShow;

impl Command for HubShow {
    fn id(&self) -> &'static str {
        "metis.hub-show"
    }
    fn title(&self) -> &'static str {
        "metis.hub-show.title"
    }
    fn effects(&self) -> Effects {
        Effects { writes_disk: true, spawns_process: false, network: true }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let base = resolve_hub_url(params)?;
        let slug = required_str(params, "slug")?;
        let version = string_param(params, "version")?;
        let refresh = bool_param(params, "refresh", false)?;
        let client = HubClient::new(base, Some(engine.paths().hub_cache_dir()), ureq_fetch);
        let detail = client.show_expert(&slug, version.as_deref(), refresh)?;
        Ok(CommandOutput::Json(serde_json::to_value(&detail).unwrap_or(Value::Null)))
    }
}

/// 从远端 Hub 导入一个 expert。
#[derive(Debug)]
pub struct HubImport;

impl Command for HubImport {
    fn id(&self) -> &'static str {
        "metis.hub-import"
    }
    fn title(&self) -> &'static str {
        "metis.hub-import.title"
    }
    fn effects(&self) -> Effects {
        Effects { writes_disk: true, spawns_process: false, network: true }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let base = resolve_hub_url(params)?;
        let slug = required_str(params, "slug")?;
        let version = string_param(params, "version")?;
        let options = HubImportOptions {
            overwrite: bool_param(params, "overwrite", false)?,
            dry_run: bool_param(params, "dryRun", false)?,
            include_knowledge: bool_param(params, "includeKnowledge", true)?,
            allow_unverified: bool_param(params, "allowUnverified", false)?,
            refresh: bool_param(params, "refresh", false)?,
            as_id: string_param(params, "as")?,
            skills: skill_selection(params.get("skills"))?,
        };
        let report = import_expert(engine.paths(), &base, ureq_fetch, &slug, version.as_deref(), &options)?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

/// 单独安装一个 Hub skill。
#[derive(Debug)]
pub struct HubSkillInstall;

impl Command for HubSkillInstall {
    fn id(&self) -> &'static str {
        "metis.hub-skill-install"
    }
    fn title(&self) -> &'static str {
        "metis.hub-skill-install.title"
    }
    fn effects(&self) -> Effects {
        Effects { writes_disk: true, spawns_process: false, network: true }
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let base = resolve_hub_url(params)?;
        let slug = required_str(params, "slug")?;
        let version = string_param(params, "version")?;
        let dry_run = bool_param(params, "dryRun", false)?;
        let overwrite = bool_param(params, "overwrite", false)?;
        let allow_unverified = bool_param(params, "allowUnverified", false)?;
        let refresh = bool_param(params, "refresh", false)?;

        let client = HubClient::new(base, Some(engine.paths().hub_cache_dir()), ureq_fetch);
        let detail = client.show_skill(&slug, version.as_deref(), refresh)?;
        if !detail.item.verified && !allow_unverified && !dry_run {
            return Err(MetisError::new(ErrorKind::Refused, "这个 Hub skill 的内容哈希校验失败")
                .with_remediation("确认来源后加 `--allow-unverified`；界面会要求二次确认"));
        }
        let report = install_skill(engine.paths(), &detail, &SkillInstallOptions { overwrite, dry_run })?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

/// 移除一个由 Metis 安装的 Hub skill。
#[derive(Debug)]
pub struct HubSkillRemove;

impl Command for HubSkillRemove {
    fn id(&self) -> &'static str {
        "metis.hub-skill-remove"
    }
    fn title(&self) -> &'static str {
        "metis.hub-skill-remove.title"
    }
    fn effects(&self) -> Effects {
        Effects::writes()
    }
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let slug = required_str(params, "slug")?;
        let report = remove_skill(engine.paths(), &slug)?;
        Ok(CommandOutput::Json(serde_json::to_value(&report).unwrap_or(Value::Null)))
    }
}

fn resolve_hub_url(params: &Value) -> Result<Url> {
    let raw = match string_param(params, "url")? {
        Some(url) => url,
        None => std::env::var("METIS_HUB_URL").ok().unwrap_or_else(|| HUB_DEFAULT_URL.to_owned()),
    };
    parse_hub_url(&raw)
}

fn string_param(params: &Value, key: &str) -> Result<Option<String>> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(MetisError::new(ErrorKind::InvalidArgument, format!("参数 {key} 必须是字符串")).with_remediation("检查调用方传参")),
    }
}

fn bool_param(params: &Value, key: &str, default: bool) -> Result<bool> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(MetisError::new(ErrorKind::InvalidArgument, format!("参数 {key} 必须是布尔值")).with_remediation("传 true 或 false")),
    }
}

fn usize_param(params: &Value, key: &str, default: usize) -> Result<usize> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Number(value)) => value
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| MetisError::new(ErrorKind::InvalidArgument, format!("参数 {key} 必须是非负整数")).with_remediation("例如 20")),
        Some(_) => Err(MetisError::new(ErrorKind::InvalidArgument, format!("参数 {key} 必须是非负整数")).with_remediation("例如 20")),
    }
}

fn parse_provider(value: &str) -> Result<HubProvider> {
    match value {
        "claude" => Ok(HubProvider::Claude),
        "codex" => Ok(HubProvider::Codex),
        "deepseek" => Ok(HubProvider::Deepseek),
        other => Err(MetisError::new(ErrorKind::InvalidArgument, format!("未知 provider：{other}")).with_remediation("可用：claude / codex / deepseek")),
    }
}

fn skill_selection(value: Option<&Value>) -> Result<SkillSelection> {
    match value {
        None | Some(Value::Null) => Ok(SkillSelection::All),
        Some(Value::String(value)) => {
            match value.as_str() {
                "all" => Ok(SkillSelection::All),
                "none" => Ok(SkillSelection::None),
                other => Err(MetisError::new(ErrorKind::InvalidArgument, format!("未知的 skills 值：{other}"))
                    .with_remediation("用 `all`、`none`，或一个 skill slug 数组")),
            }
        }
        Some(Value::Array(values)) => {
            let mut slugs = Vec::new();
            for value in values {
                let Some(slug) = value.as_str() else {
                    return Err(MetisError::new(ErrorKind::InvalidArgument, "skills 数组里必须全是字符串")
                        .with_remediation("例如 [\"wb-apiz\", \"wb-claude-agent-sdk\"]"));
                };
                crate::hub::validate_slug(slug)?;
                if !slugs.contains(&slug.to_owned()) {
                    slugs.push(slug.to_owned());
                }
            }
            Ok(SkillSelection::Only(slugs))
        }
        Some(_) => {
            Err(MetisError::new(ErrorKind::InvalidArgument, "skills 必须是 all、none 或字符串数组").with_remediation("例如 \"all\"、\"none\" 或 [\"wb-apiz\"]"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;
    use serde_json::json;

    fn engine() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().expect("tmp");
        let paths = Paths::with_home(dir.path().join("home"));
        std::fs::create_dir_all(paths.home()).expect("mkdir");
        let engine = Engine::new(paths);
        (dir, engine)
    }

    #[test]
    fn a_bad_url_is_rejected_before_network() {
        let (_dir, engine) = engine();
        for bad in ["http://example.com", "https://example.com/path", "nope"] {
            let error = HubImport.run(&engine, &json!({ "url": bad, "slug": "security-reviewer" })).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
            let error = HubList.run(&engine, &json!({ "url": bad })).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
        }
    }

    #[test]
    fn hub_list_rejects_a_bad_provider_before_network() {
        let (_dir, engine) = engine();
        let error = HubList.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443", "provider": "nope" })).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.remediation().unwrap_or_default().contains("claude"));
    }

    #[test]
    fn hub_list_rejects_a_non_numeric_limit_before_network() {
        let (_dir, engine) = engine();
        for limit in [json!("20"), json!(-1), json!(1.5)] {
            let error = HubList.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443", "limit": limit })).expect_err("must fail");
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{limit}");
        }
    }

    #[test]
    fn hub_show_rejects_a_bad_slug_before_network() {
        let (_dir, engine) = engine();
        for bad in ["", "../x", "A", "a/b"] {
            let error = HubShow.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443", "slug": bad })).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
        }
    }

    #[test]
    fn a_missing_slug_is_rejected_before_network() {
        let (_dir, engine) = engine();
        let error = HubImport.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443" })).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.message().contains("slug"), "{}", error.message());
    }

    #[test]
    fn a_non_boolean_flag_is_rejected() {
        let (_dir, engine) = engine();
        let error = HubImport.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443", "slug": "security-reviewer", "dryRun": "yes" })).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.message().contains("dryRun"), "{}", error.message());
    }

    #[test]
    fn a_non_string_flag_is_rejected() {
        let (_dir, engine) = engine();
        let error = HubImport.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443", "slug": "security-reviewer", "as": 42 })).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.message().contains("as"), "{}", error.message());
    }

    #[test]
    fn skill_selection_parses_all_none_and_only() {
        assert_eq!(skill_selection(None).expect("none"), SkillSelection::All);
        assert_eq!(skill_selection(Some(&json!("all"))).expect("all"), SkillSelection::All);
        assert_eq!(skill_selection(Some(&json!("none"))).expect("none"), SkillSelection::None);
        assert_eq!(skill_selection(Some(&json!(["wb-apiz", "wb-apiz"]))).expect("only"), SkillSelection::Only(vec!["wb-apiz".to_owned()]));
        assert!(skill_selection(Some(&json!("bogus"))).is_err());
        assert!(skill_selection(Some(&json!(42))).is_err());
        assert!(skill_selection(Some(&json!([42]))).is_err());
        assert!(skill_selection(Some(&json!(["BAD SLUG"]))).is_err());
    }

    #[test]
    fn a_non_boolean_flag_on_skill_install_is_rejected() {
        let (_dir, engine) = engine();
        let error = HubSkillInstall.run(&engine, &json!({ "url": "https://n.kkpt.tech:8443", "slug": "wb-apiz", "overwrite": 1 })).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    }

    #[test]
    fn a_bad_slug_on_remove_is_rejected_before_touching_disk() {
        let (_dir, engine) = engine();
        for bad in ["", "../x", "A", "a/b"] {
            let error = HubSkillRemove.run(&engine, &json!({ "slug": bad })).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
        }
    }
}
