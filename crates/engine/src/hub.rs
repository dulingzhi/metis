//! 远端人设库（worldd / KK Hub）的只读客户端。
//!
//! 这里只做**协议、分页、缓存与解析**，不碰人设落盘：
//!
//! - 网络通过注入的 `fetch` 闭包（`AGENTS.md`；`PITFALLS.md` G42）；
//! - 列表分页用服务端给的不透明 `nextCursor`，**绝不解析**；
//! - 响应带 `ETag` 时用 `If-None-Match` 条件请求，命中 `304` 直接用缓存；
//! - 网络失败/5xx 时回退缓存并标 `cache-stale`；4xx 不回退，避免把“下架/参数错”伪装成旧数据；
//! - 远端内容是**不可信输入**：字段限长、限数量，越界只截断并告警，不 panic。
//!
//! 对端契约见 `docs/world/hub-api.md`（参考项目 habblaud）。生产地址只在这个文件里出现一次。

use std::io::Read;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use metis_core::{ErrorKind, MetisError, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::{Host, Url};

/// 默认 Hub：worldd 的公开来源。
///
/// `/hub` 是网页，API 在 origin 的 `/v1/hub/*`。`parse_hub_url` 同时接受两种写法。
pub const HUB_DEFAULT_URL: &str = "https://n.kkpt.tech:8443";

/// 单次响应的防御性上限（列表与详情共用）。
pub const HUB_MAX_RESPONSE_BYTES: usize = 1024 * 1024;

/// 分页默认条数（服务端默认也是 20）。
pub const DEFAULT_PAGE_SIZE: usize = 20;

/// 分页最大条数（服务端会把 `limit` 夹到 1..=100）。
pub const MAX_PAGE_SIZE: usize = 100;

/// 列表缓存的新鲜期（秒）。期内直接读缓存，不发请求。
pub const LIST_TTL_SECS: i64 = 60;

/// 详情缓存的新鲜期（秒）。
pub const DETAIL_TTL_SECS: i64 = 10 * 60;

/// 知识文件的条数上限。
pub const MAX_KNOWLEDGE: usize = 16;

/// 知识文件正文的合计上限（字节）。
pub const MAX_KNOWLEDGE_BYTES: usize = 32 * 1024;

/// 关联技能的上限。
pub const MAX_SKILLS: usize = 128;

/// 工具名的上限。
pub const MAX_TOOLS: usize = 32;

/// Hub 条目类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HubKind {
    /// 专家（对应 Metis 的人设）。
    Expert,
    /// 技能（`SKILL.md` + 附件）。
    Skill,
    /// MCP 服务。
    Mcp,
}

impl HubKind {
    /// 机器可读字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Expert => "expert",
            Self::Skill => "skill",
            Self::Mcp => "mcp",
        }
    }

    /// 解析；未知值返回 `None`。
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "expert" => Some(Self::Expert),
            "skill" => Some(Self::Skill),
            "mcp" => Some(Self::Mcp),
            _ => None,
        }
    }
}

/// Hub 的 provider 过滤值。
///
/// **不要把它映射成人设的 `harness` 标签**：provider 是模型厂商，不是执行器。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubProvider {
    /// Claude。
    Claude,
    /// Codex。
    Codex,
    /// DeepSeek。
    Deepseek,
}

impl HubProvider {
    /// 机器可读字符串。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Deepseek => "deepseek",
        }
    }
}

/// 条目的来源描述。
///
/// 用宽松结构而不是封闭枚举：服务端加新来源时旧客户端仍能解析（未知字段忽略）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSource {
    /// `inline` / `github` / `npx` / `url`。
    #[serde(default)]
    pub kind: String,
    /// GitHub 仓库 `owner/name`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// GitHub 提交 SHA。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// GitHub 内的相对路径。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// npx 包名。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// npx 精确版本。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// npx 可执行名。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
    /// 远端 URL。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

/// 列表里的条目摘要（不含 `content`）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HubSummary {
    /// `expert` / `skill` / `mcp`。
    #[serde(default)]
    pub kind: String,
    /// 稳定标识。
    #[serde(default)]
    pub slug: String,
    /// 显示名。
    #[serde(default)]
    pub name: String,
    /// 一句话说明。
    #[serde(default)]
    pub description: String,
    /// 语义版本。
    #[serde(default)]
    pub version: String,
    /// `draft` / `pending` / `approved` / `rejected` / `hidden`。
    #[serde(default)]
    pub status: String,
    /// `public` / `unlisted` / `private`。
    #[serde(default)]
    pub visibility: String,
    /// 作者账号 id；官方条目为 `null`。
    #[serde(default)]
    pub author: Option<String>,
    /// 是否官方条目。
    #[serde(default)]
    pub official: bool,
    /// 标签。
    #[serde(default)]
    pub tags: Vec<String>,
    /// 支持的 provider；空数组 = 全部。
    #[serde(default)]
    pub providers: Vec<String>,
    /// 来源。
    #[serde(default)]
    pub source: Option<HubSource>,
    /// `sha256:<hex>`，服务端对规范化内容计算。
    #[serde(default)]
    pub hash: String,
    /// 创建时间（毫秒）。
    #[serde(default)]
    pub created_at: i64,
    /// 更新时间（毫秒）。
    #[serde(default)]
    pub updated_at: i64,
    /// 下载/安装计数。
    #[serde(default)]
    pub downloads: u64,
    /// 服务端重算哈希是否一致。
    #[serde(default)]
    pub verified: bool,
}

impl HubSummary {
    /// 最少要有 slug、name、version 才能进 UI。
    fn is_usable(&self) -> bool {
        !self.slug.trim().is_empty() && !self.name.trim().is_empty() && !self.version.trim().is_empty()
    }

    /// 把远端字符串裁到安全范围。只截断，不报错。
    fn sanitize(&mut self) {
        self.kind = single_line(&self.kind, 16);
        self.slug = single_line(&self.slug, 64);
        self.name = single_line(&self.name, 80);
        self.description = single_line(&self.description, 300);
        self.version = single_line(&self.version, 32);
        self.status = single_line(&self.status, 16);
        self.visibility = single_line(&self.visibility, 16);
        self.author = self.author.as_deref().map(|value| single_line(value, 64)).filter(|value| !value.is_empty());
        self.tags = clean_list(&self.tags, 10, 24);
        self.providers = clean_list(&self.providers, 3, 16);
        self.hash = single_line(&self.hash, 80);
        if let Some(source) = &mut self.source {
            source.kind = single_line(&source.kind, 16);
            source.repo = normalize_optional(source.repo.take(), 120);
            source.commit = normalize_optional(source.commit.take(), 40);
            source.path = normalize_optional(source.path.take(), 256);
            source.package = normalize_optional(source.package.take(), 120);
            source.version = normalize_optional(source.version.take(), 32);
            source.bin = normalize_optional(source.bin.take(), 64);
            source.url = normalize_optional(source.url.take(), 2048);
        }
    }
}

/// 一条知识文件。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeFile {
    /// 文件名（可能是 `references/x.md` 这种相对路径）。
    #[serde(default)]
    pub name: String,
    /// 正文。
    #[serde(default)]
    pub text: String,
}

/// 一个关联技能引用。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRef {
    /// 技能 slug。
    #[serde(default)]
    pub slug: String,
    /// 技能显示名。
    #[serde(default)]
    pub name: Option<String>,
}

/// `expert.content`：宽容解析。
///
/// 服务端的文档与实现有细微差异（`modelHint` vs `model`、`toolsHint` vs `tools`），
/// 所以两组字段都收；未知字段忽略。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExpertContent {
    /// 系统提示词（必需）。
    #[serde(default)]
    pub prompt: String,
    /// 参考资料。
    #[serde(default)]
    pub knowledge: Vec<KnowledgeFile>,
    /// 关联技能引用。
    #[serde(default)]
    pub skills: Vec<SkillRef>,
    /// 备注。
    #[serde(default)]
    pub notes: Option<String>,
    /// 头像 emoji。
    #[serde(default)]
    pub avatar: Option<String>,
    /// 语言。
    #[serde(default)]
    pub language: Option<String>,
    /// 模型提示（实现字段）。
    #[serde(default)]
    pub model: Option<String>,
    /// 模型提示（文档字段）。
    #[serde(default)]
    pub model_hint: Option<String>,
    /// 工具提示（实现字段）。
    #[serde(default)]
    pub tools: Vec<String>,
    /// 工具提示（文档字段）。
    #[serde(default)]
    pub tools_hint: Vec<String>,
    /// 最大回合数。
    #[serde(default)]
    pub max_turns: Option<u32>,
}

impl ExpertContent {
    /// 有效的模型提示：实现字段优先，文档字段兜底。
    pub fn effective_model(&self) -> Option<&str> {
        self.model.as_deref().or(self.model_hint.as_deref())
    }

    /// 有效的工具提示：实现字段优先，文档字段兜底。
    pub fn effective_tools(&self) -> &[String] {
        if self.tools.is_empty() { &self.tools_hint } else { &self.tools }
    }

    /// 裁剪到安全范围，返回给用户看的告警。
    fn sanitize(&mut self) -> Vec<String> {
        let mut warnings = Vec::new();

        self.prompt = self.prompt.trim().to_owned();
        if self.prompt.chars().count() > 64 * 1024 {
            self.prompt = truncate_chars(&self.prompt, 64 * 1024);
            warnings.push("prompt 超过 64 KiB，已截断".to_owned());
        }

        let mut knowledge = Vec::new();
        let mut bytes = 0_usize;
        for entry in self.knowledge.drain(..) {
            if entry.name.trim().is_empty() || entry.text.trim().is_empty() {
                continue;
            }
            if knowledge.len() >= MAX_KNOWLEDGE {
                warnings.push(format!("knowledge 超过 {MAX_KNOWLEDGE} 条，已截断"));
                break;
            }
            let text = entry.text;
            if bytes.saturating_add(text.len()) > MAX_KNOWLEDGE_BYTES {
                warnings.push(format!("knowledge 合计超过 {} KiB，已截断", MAX_KNOWLEDGE_BYTES / 1024));
                break;
            }
            bytes = bytes.saturating_add(text.len());
            knowledge.push(KnowledgeFile { name: single_line(&entry.name, 120), text });
        }
        self.knowledge = knowledge;

        let mut skills = Vec::new();
        let mut seen = Vec::new();
        for entry in self.skills.drain(..) {
            let slug = single_line(&entry.slug, 64);
            if slug.is_empty() || seen.contains(&slug) {
                continue;
            }
            if skills.len() >= MAX_SKILLS {
                warnings.push(format!("skills 超过 {MAX_SKILLS} 个，已截断"));
                break;
            }
            seen.push(slug.clone());
            skills.push(SkillRef { slug, name: entry.name.as_deref().map(|name| single_line(name, 80)).filter(|name| !name.is_empty()) });
        }
        self.skills = skills;

        self.notes = normalize_optional(self.notes.take(), 2000);
        self.avatar = normalize_optional(self.avatar.take(), 16);
        self.language = normalize_optional(self.language.take(), 32);
        self.model = normalize_optional(self.model.take(), 64);
        self.model_hint = normalize_optional(self.model_hint.take(), 64);
        self.tools = clean_list(&self.tools, MAX_TOOLS, 128);
        self.tools_hint = clean_list(&self.tools_hint, MAX_TOOLS, 128);
        if let Some(turns) = self.max_turns
            && !(1..=50).contains(&turns)
        {
            self.max_turns = None;
            warnings.push(format!("maxTurns={turns} 超出 1..50，已忽略"));
        }

        warnings
    }
}

/// 一页列表。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubPage {
    /// 条目摘要。
    pub items: Vec<HubSummary>,
    /// 下一页游标；没有更多时为 `None`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
    /// 这份数据的取得时间（Unix 秒）。
    pub fetched_at: i64,
    /// 来自网络还是缓存。
    pub source: DataSource,
    /// 解析时的告警（坏条目、截断等）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// 一个 expert 的完整详情。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HubDetail {
    /// 摘要字段。
    #[serde(flatten)]
    pub item: HubSummary,
    /// 完整内容。
    pub content: ExpertContent,
    /// 这份数据的取得时间（Unix 秒）。
    pub fetched_at: i64,
    /// 来自网络还是缓存。
    pub source: DataSource,
    /// 解析时的告警。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

/// 数据来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataSource {
    /// 本次从网络取得。
    Network,
    /// 命中新鲜缓存（或 304）。
    Cache,
    /// 网络不可用/5xx，回退到旧缓存。
    CacheStale,
}

/// 列表查询。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HubQuery {
    /// 条目类型；`None` = 全部。
    pub kind: Option<HubKind>,
    /// 搜索词（匹配 name/description/tags）。
    pub q: Option<String>,
    /// 标签过滤。
    pub tag: Option<String>,
    /// provider 过滤。
    pub provider: Option<HubProvider>,
    /// 不透明游标，原样回传。
    pub cursor: Option<String>,
    /// 每页条数；0 = 默认 20，超过 100 会被夹到 100。
    pub limit: usize,
    /// 绕过新鲜期，强制条件请求。
    pub refresh: bool,
}

impl HubQuery {
    /// 人设库用的默认查询：只要 expert，第一页。
    pub fn experts() -> Self {
        Self { kind: Some(HubKind::Expert), limit: DEFAULT_PAGE_SIZE, ..Self::default() }
    }

    fn effective_limit(&self) -> usize {
        if self.limit == 0 { DEFAULT_PAGE_SIZE } else { self.limit.clamp(1, MAX_PAGE_SIZE) }
    }
}

/// 一次 HTTP 请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchRequest {
    /// 完整 URL。
    pub url: String,
    /// 请求头（名称、值）。
    pub headers: Vec<(String, String)>,
    /// 响应体上限。
    pub max_bytes: usize,
}

/// 一次 HTTP 响应。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// 状态码。
    pub status: u16,
    /// `ETag` 响应头（原样保留，可能带引号与后缀）。
    pub etag: Option<String>,
    /// `Content-Encoding` 响应头。
    pub content_encoding: Option<String>,
    /// 响应体。
    pub body: Vec<u8>,
}

/// 可注入的时钟（Unix 秒）。测试用固定值，生产用系统时间。
pub type Clock = fn() -> i64;

fn now_secs() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

/// Hub 客户端。
///
/// `F` 是注入的 fetch 闭包：生产传 [`ureq_fetch`]，测试传假响应。
pub struct HubClient<F> {
    base: Url,
    cache_dir: Option<PathBuf>,
    fetch: F,
    clock: Clock,
}

impl<F> std::fmt::Debug for HubClient<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubClient").field("base", &self.base).field("cache_dir", &self.cache_dir).finish_non_exhaustive()
    }
}

impl<F> HubClient<F>
where
    F: Fn(&FetchRequest) -> Result<HttpResponse>,
{
    /// 用已归一化的 base URL、可选缓存目录与 fetch 闭包构造。
    pub fn new(base: Url, cache_dir: Option<PathBuf>, fetch: F) -> Self {
        Self { base, cache_dir, fetch, clock: now_secs }
    }

    /// 替换时钟（测试用）。
    #[must_use]
    pub fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// 取一页列表。
    pub fn list(&self, query: &HubQuery) -> Result<HubPage> {
        let url = self.list_url(query)?;
        let fetched = self.fetch_with_cache(&url, LIST_TTL_SECS, query.refresh)?;
        parse_page(&fetched.body, fetched.fetched_at, fetched.source, fetched.warning)
    }

    /// 取一个 expert 的完整详情。
    pub fn show_expert(&self, slug: &str, version: Option<&str>, refresh: bool) -> Result<HubDetail> {
        validate_slug(slug)?;
        if let Some(version) = version {
            validate_version(version)?;
        }
        let url = self.detail_url(HubKind::Expert, slug, version)?;
        let fetched = self.fetch_with_cache(&url, DETAIL_TTL_SECS, refresh)?;
        parse_detail(&fetched.body, fetched.fetched_at, fetched.source, fetched.warning)
    }

    fn list_url(&self, query: &HubQuery) -> Result<String> {
        let mut url = self.base.clone();
        url.set_path("/v1/hub/items");
        url.set_query(None);
        url.set_fragment(None);
        {
            let mut pairs = url.query_pairs_mut();
            if let Some(kind) = query.kind {
                pairs.append_pair("kind", kind.as_str());
            }
            if let Some(value) = cleaned(query.q.as_deref()) {
                pairs.append_pair("q", &value);
            }
            if let Some(value) = cleaned(query.tag.as_deref()) {
                pairs.append_pair("tag", &value);
            }
            if let Some(provider) = query.provider {
                pairs.append_pair("provider", provider.as_str());
            }
            pairs.append_pair("limit", &query.effective_limit().to_string());
            if let Some(cursor) = cleaned(query.cursor.as_deref()) {
                pairs.append_pair("cursor", &cursor);
            }
        }
        Ok(url.to_string())
    }

    fn detail_url(&self, kind: HubKind, slug: &str, version: Option<&str>) -> Result<String> {
        let mut url = self.base.clone();
        url.set_path("/v1/hub/items");
        url.set_query(None);
        url.set_fragment(None);
        {
            let mut segments = url.path_segments_mut().map_err(|_| {
                MetisError::new(ErrorKind::InvalidArgument, "Hub 地址不能作为路径基址")
                    .with_remediation("填 `https://n.kkpt.tech:8443` 或 `https://n.kkpt.tech:8443/hub`")
            })?;
            segments.push(kind.as_str());
            segments.push(slug);
            if let Some(version) = version {
                segments.push(version);
            }
        }
        Ok(url.to_string())
    }

    fn fetch_with_cache(&self, url: &str, ttl_secs: i64, refresh: bool) -> Result<FetchedBody> {
        let now = (self.clock)();
        let cached = self.read_cache(url);
        if let Some(entry) = &cached
            && !refresh
            && now.saturating_sub(entry.fetched_at) <= ttl_secs
        {
            return Ok(FetchedBody { body: entry.body.clone(), fetched_at: entry.fetched_at, source: DataSource::Cache, warning: None });
        }

        let mut headers = vec![
            ("accept".to_owned(), "application/json".to_owned()),
            // 固定 identity：对端 Caddy 会按协商返回 zstd，带 `-zstd` 的 ETag；
            // 不压缩时 ETag 稳定，客户端也不需要额外的解码器。
            ("accept-encoding".to_owned(), "identity".to_owned()),
            ("user-agent".to_owned(), format!("Metis/{}", env!("CARGO_PKG_VERSION"))),
        ];
        if let Some(etag) = cached.as_ref().and_then(|entry| entry.etag.as_deref()) {
            headers.push(("if-none-match".to_owned(), etag.to_owned()));
        }

        let response = match (self.fetch)(&FetchRequest { url: url.to_owned(), headers, max_bytes: HUB_MAX_RESPONSE_BYTES }) {
            Ok(response) => response,
            Err(error) => {
                if let Some(entry) = cached {
                    return Ok(FetchedBody {
                        body: entry.body,
                        fetched_at: entry.fetched_at,
                        source: DataSource::CacheStale,
                        warning: Some(format!("网络不可用，正在使用缓存：{}", error.message())),
                    });
                }
                return Err(MetisError::new(ErrorKind::Network, format!("连不上 Hub：{}", error.message()))
                    .with_detail(error.detail().unwrap_or_default().to_owned())
                    .with_remediation("检查网络与 Hub 地址；如果是内网服务，确认本机能访问"));
            }
        };

        if let Some(encoding) = response.content_encoding.as_deref().map(str::trim).filter(|value| !value.is_empty() && !value.eq_ignore_ascii_case("identity"))
        {
            return Err(MetisError::new(ErrorKind::Network, format!("Hub 返回了未预期的压缩格式：{encoding}"))
                .with_remediation("客户端固定请求 identity；请确认中间代理没有强制压缩"));
        }

        if response.status == 304 {
            if let Some(mut entry) = cached {
                entry.fetched_at = now;
                self.write_cache(url, &entry);
                return Ok(FetchedBody { body: entry.body, fetched_at: now, source: DataSource::Cache, warning: None });
            }
            return Err(MetisError::new(ErrorKind::Network, "Hub 返回 304，但本地没有缓存").with_remediation("清空 `~/.metis/hub-cache` 后重试"));
        }

        if response.status == 200 {
            if response.body.len() > HUB_MAX_RESPONSE_BYTES {
                return Err(MetisError::new(ErrorKind::Network, format!("Hub 响应超过 {} KiB", HUB_MAX_RESPONSE_BYTES / 1024))
                    .with_remediation("稍后重试；如果持续，检查 Hub 是否返回了异常数据"));
            }
            let body = String::from_utf8(response.body).map_err(|error| {
                MetisError::new(ErrorKind::Network, "Hub 响应不是 UTF-8")
                    .with_detail(error.to_string())
                    .with_remediation("稍后重试；如果持续，报告 Hub 服务端问题")
            })?;
            let entry = CacheEntry { v: 1, url: url.to_owned(), etag: response.etag, fetched_at: now, body: body.clone() };
            self.write_cache(url, &entry);
            return Ok(FetchedBody { body, fetched_at: now, source: DataSource::Network, warning: None });
        }

        if response.status >= 500
            && let Some(entry) = cached
        {
            return Ok(FetchedBody {
                body: entry.body,
                fetched_at: entry.fetched_at,
                source: DataSource::CacheStale,
                warning: Some(format!("Hub 暂时不可用（HTTP {}），正在使用缓存", response.status)),
            });
        }

        Err(hub_error(response.status, &response.body))
    }

    fn cache_path(&self, url: &str) -> Option<PathBuf> {
        let dir = self.cache_dir.as_deref()?;
        Some(dir.join(format!("{}.json", metis_fs::hash_bytes(url.as_bytes()))))
    }

    fn read_cache(&self, url: &str) -> Option<CacheEntry> {
        let path = self.cache_path(url)?;
        let text = std::fs::read_to_string(path).ok()?;
        let entry: CacheEntry = serde_json::from_str(&text).ok()?;
        if entry.v != 1 || entry.url != url || entry.body.len() > HUB_MAX_RESPONSE_BYTES {
            return None;
        }
        Some(entry)
    }

    /// 缓存是优化，不是契约：写失败不阻断浏览。
    fn write_cache(&self, url: &str, entry: &CacheEntry) {
        let Some(path) = self.cache_path(url) else { return };
        let Ok(bytes) = serde_json::to_vec(entry) else { return };
        let _ = metis_fs::write_atomic(&path, &bytes);
    }
}

struct FetchedBody {
    body: String,
    fetched_at: i64,
    source: DataSource,
    warning: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CacheEntry {
    v: u32,
    url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    etag: Option<String>,
    fetched_at: i64,
    body: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubListResponse {
    #[serde(default)]
    items: Vec<HubSummary>,
    #[serde(default)]
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HubDetailResponse {
    #[serde(flatten)]
    item: HubSummary,
    #[serde(default)]
    content: ExpertContent,
}

fn parse_page(body: &str, fetched_at: i64, source: DataSource, mut warning: Option<String>) -> Result<HubPage> {
    let response: HubListResponse = serde_json::from_str(body).map_err(|error| {
        MetisError::new(ErrorKind::Network, "Hub 返回的列表不是预期 JSON")
            .with_detail(format!("{error}；{}", preview(body)))
            .with_remediation("确认地址指向 worldd 的 `/v1/hub`；稍后重试")
    })?;

    let mut items = Vec::new();
    let mut skipped = 0_usize;
    for mut item in response.items {
        if !item.is_usable() {
            skipped += 1;
            continue;
        }
        item.sanitize();
        items.push(item);
    }
    if skipped > 0 {
        let text = format!("{skipped} 个条目的字段不完整，已跳过");
        warning = Some(match warning {
            Some(existing) => format!("{existing}；{text}"),
            None => text,
        });
    }

    Ok(HubPage { items, next_cursor: response.next_cursor.filter(|cursor| !cursor.trim().is_empty()), fetched_at, source, warning })
}

fn parse_detail(body: &str, fetched_at: i64, source: DataSource, mut warning: Option<String>) -> Result<HubDetail> {
    let mut response: HubDetailResponse = serde_json::from_str(body).map_err(|error| {
        MetisError::new(ErrorKind::Network, "Hub 返回的详情不是预期 JSON")
            .with_detail(format!("{error}；{}", preview(body)))
            .with_remediation("确认地址指向 worldd 的 `/v1/hub`；稍后重试")
    })?;

    response.item.sanitize();
    if !response.item.is_usable() {
        return Err(MetisError::new(ErrorKind::Network, "Hub 详情缺少 slug、name 或 version").with_remediation("回到列表刷新；如果条目刚被下架，换一个条目"));
    }
    if response.item.kind != HubKind::Expert.as_str() {
        return Err(
            MetisError::new(ErrorKind::Network, format!("条目类型是 {}，不是 expert", response.item.kind)).with_remediation("从专家列表选择一个 expert 条目")
        );
    }

    let content_warnings = response.content.sanitize();
    if !content_warnings.is_empty() {
        let text = content_warnings.join("；");
        warning = Some(match warning {
            Some(existing) => format!("{existing}；{text}"),
            None => text,
        });
    }
    if response.content.prompt.trim().is_empty() {
        return Err(MetisError::new(ErrorKind::Network, "这个专家详情缺少 prompt").with_remediation("换一个条目；如果这是你自己的条目，请在 Hub 上补全 prompt"));
    }
    if !response.item.verified {
        let text = "Hub 校验失败：内容哈希与记录不一致".to_owned();
        warning = Some(match warning {
            Some(existing) => format!("{existing}；{text}"),
            None => text,
        });
    }

    Ok(HubDetail { item: response.item, content: response.content, fetched_at, source, warning })
}

fn hub_error(status: u16, body: &[u8]) -> MetisError {
    let code = error_code(body);
    let suffix = code.as_deref().map(|code| format!("（{code}）")).unwrap_or_default();
    let detail = preview(&String::from_utf8_lossy(body));
    match status {
        400 => MetisError::new(ErrorKind::InvalidArgument, format!("Hub 拒绝了请求{suffix}"))
            .with_detail(detail)
            .with_remediation("检查 kind、slug 与筛选条件；kind 目前只支持 expert / skill / mcp"),
        404 => MetisError::new(ErrorKind::Network, format!("Hub 上找不到这个条目{suffix}"))
            .with_detail(detail)
            .with_remediation("回到列表刷新；确认 slug 与版本；条目可能已被下架"),
        429 => MetisError::new(ErrorKind::Network, format!("Hub 请求太频繁{suffix}")).with_detail(detail).with_remediation("等一分钟再试；或减少刷新频率"),
        500..=599 => MetisError::new(ErrorKind::Network, format!("Hub 暂时不可用（HTTP {status}{suffix}）"))
            .with_detail(detail)
            .with_remediation("稍后重试；如果持续，检查 Hub 服务"),
        _ => MetisError::new(ErrorKind::Network, format!("Hub 返回 HTTP {status}{suffix}"))
            .with_detail(detail)
            .with_remediation("稍后重试；确认地址指向 worldd 的 `/v1/hub`"),
    }
}

fn error_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    value.get("error")?.get("code")?.as_str().map(str::to_owned)
}

/// 把输入归一化成 Hub 的 origin。
///
/// 接受 `https://host:port` 与 `https://host:port/hub`（网页地址）。
/// 只允许 https；本机的 `http://127.0.0.1` / `localhost` 给本地 worldd 开发用。
pub fn parse_hub_url(input: &str) -> Result<Url> {
    let raw = input.trim();
    if raw.is_empty() {
        return Err(MetisError::new(ErrorKind::InvalidArgument, "Hub 地址不能为空").with_remediation(format!("填 `{HUB_DEFAULT_URL}`，或它的 `/hub` 网页地址")));
    }
    let mut url = Url::parse(raw).map_err(|error| {
        MetisError::new(ErrorKind::InvalidArgument, format!("Hub 地址不合法：{raw}"))
            .with_detail(error.to_string())
            .with_remediation(format!("例如 `{HUB_DEFAULT_URL}` 或 `{HUB_DEFAULT_URL}/hub`"))
    })?;

    match url.scheme() {
        "https" => {}
        "http" if is_loopback(&url) => {}
        "http" => {
            return Err(MetisError::new(ErrorKind::InvalidArgument, "Hub 地址必须是 https（本机开发除外）")
                .with_remediation("把 `http://` 改成 `https://`；只有 127.0.0.1 / localhost 允许 http"));
        }
        other => {
            return Err(MetisError::new(ErrorKind::InvalidArgument, format!("Hub 地址的协议不支持：{other}")).with_remediation("用 http 或 https"));
        }
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(
            MetisError::new(ErrorKind::InvalidArgument, "Hub 地址不能带用户名或密码").with_remediation("删掉地址里的 `user:pass@`；Metis 不保存 Hub 凭据")
        );
    }
    if url.query().is_some() || url.fragment().is_some() {
        return Err(MetisError::new(ErrorKind::InvalidArgument, "Hub 地址不能带查询参数或片段")
            .with_remediation(format!("只填 `{HUB_DEFAULT_URL}` 或 `{HUB_DEFAULT_URL}/hub`")));
    }

    let trimmed = url.path().trim_end_matches('/');
    let kept = match trimmed {
        "" | "/" | "/hub" => "/",
        _ => {
            return Err(MetisError::new(ErrorKind::InvalidArgument, format!("Hub 地址的路径不支持：{}", url.path()))
                .with_remediation(format!("只填 origin 或 `/hub` 页面地址，例如 `{HUB_DEFAULT_URL}`")));
        }
    };
    url.set_path(kept);
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(ip)) => ip.is_loopback(),
        Some(Host::Ipv6(ip)) => ip.is_loopback(),
        None => false,
    }
}

fn validate_slug(slug: &str) -> Result<()> {
    let valid = !slug.is_empty()
        && slug.len() <= 64
        && slug.chars().next().is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && slug.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
    if valid {
        return Ok(());
    }
    Err(MetisError::new(ErrorKind::InvalidArgument, format!("Hub slug 不合法：{slug}"))
        .with_remediation("slug 只能含小写字母、数字与连字符，最长 64；从列表里选择条目"))
}

fn validate_version(version: &str) -> Result<()> {
    let valid = !version.is_empty()
        && version.len() <= 32
        && version.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' || ch == '_')
        && !version.contains("..");
    if valid {
        return Ok(());
    }
    Err(MetisError::new(ErrorKind::InvalidArgument, format!("Hub 版本号不合法：{version}"))
        .with_remediation("版本号形如 `1.2.3` 或 `1.2.3-beta.1`；不填则取当前 approved 版本"))
}

fn cleaned(value: Option<&str>) -> Option<String> {
    value.map(str::trim).filter(|value| !value.is_empty()).map(str::to_owned)
}

fn single_line(value: &str, max: usize) -> String {
    value.replace(['\r', '\n'], " ").trim().chars().take(max).collect()
}

fn truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

fn normalize_optional(value: Option<String>, max: usize) -> Option<String> {
    value.map(|value| single_line(&value, max)).filter(|value| !value.is_empty())
}

fn clean_list(values: &[String], max_items: usize, max_chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    for value in values {
        let value = single_line(value, max_chars);
        if value.is_empty() || out.contains(&value) {
            continue;
        }
        if out.len() >= max_items {
            break;
        }
        out.push(value);
    }
    out
}

fn preview(body: &str) -> String {
    let text = body.replace(['\r', '\n'], " ");
    text.chars().take(300).collect()
}

/// 生产用的 fetch：`ureq` + 10s 总超时 + 不跟随重定向 + 响应体上限。
///
/// 对外部世界的调用集中在这里，测试可以完全避开网络（`PITFALLS.md` G42）。
pub fn ureq_fetch(request: &FetchRequest) -> Result<HttpResponse> {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .max_redirects(0)
            .http_status_as_error(false)
            .accept_encoding("identity")
            .build()
            .into()
    });

    let mut builder = AGENT.get(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let mut response = builder.call().map_err(|error| {
        MetisError::new(ErrorKind::Network, format!("请求 Hub 失败：{error}"))
            .with_detail(request.url.clone())
            .with_remediation("检查网络、代理与 Hub 地址；内网服务确认本机能访问")
    })?;

    let status = response.status().as_u16();
    let etag = response.headers().get("etag").and_then(|value| value.to_str().ok()).map(str::to_owned);
    let content_encoding = response.headers().get("content-encoding").and_then(|value| value.to_str().ok()).map(str::to_owned);

    let limit = (request.max_bytes as u64).saturating_add(1);
    let mut body = Vec::new();
    response.body_mut().as_reader().take(limit).read_to_end(&mut body).map_err(|error| {
        MetisError::new(ErrorKind::Network, format!("读 Hub 响应失败：{error}"))
            .with_detail(request.url.clone())
            .with_remediation("稍后重试；如果持续，检查网络是否稳定")
    })?;
    if body.len() > request.max_bytes {
        return Err(MetisError::new(ErrorKind::Network, format!("Hub 响应超过 {} KiB", request.max_bytes / 1024))
            .with_detail(request.url.clone())
            .with_remediation("稍后重试；如果持续，检查 Hub 是否返回了异常数据"));
    }

    Ok(HttpResponse { status, etag, content_encoding, body })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;

    fn base() -> Url {
        parse_hub_url("https://hub.example.com").expect("base")
    }

    fn response(status: u16, body: &str) -> HttpResponse {
        HttpResponse { status, etag: None, content_encoding: None, body: body.as_bytes().to_vec() }
    }

    fn client(
        responses: Rc<RefCell<VecDeque<HttpResponse>>>,
        requests: Rc<RefCell<Vec<FetchRequest>>>,
        cache: Option<PathBuf>,
    ) -> HubClient<impl Fn(&FetchRequest) -> Result<HttpResponse>> {
        HubClient::new(base(), cache, move |request| {
            requests.borrow_mut().push(request.clone());
            responses.borrow_mut().pop_front().ok_or_else(|| MetisError::new(ErrorKind::Internal, "没有预置响应"))
        })
    }

    fn fixed_clock(now: i64) -> Clock {
        // 函数指针不能捕获环境；测试里用一个进程内静态值保存“当前时间”。
        static NOW: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
        NOW.store(now, std::sync::atomic::Ordering::SeqCst);
        fn read() -> i64 {
            NOW.load(std::sync::atomic::Ordering::SeqCst)
        }
        read as Clock
    }

    fn page_json(items: &str, next: Option<&str>) -> String {
        match next {
            Some(next) => format!(r#"{{"items":[{items}],"nextCursor":"{next}"}}"#),
            None => format!(r#"{{"items":[{items}],"nextCursor":null}}"#),
        }
    }

    fn expert_json(prompt: &str, extra: &str) -> String {
        format!(
            r#"{{"kind":"expert","slug":"security-reviewer","name":"安全审查专家","description":"desc","version":"1.2.0","status":"approved","visibility":"public","author":"acc_1","official":false,"tags":["security"],"providers":["claude"],"source":{{"kind":"inline"}},"hash":"sha256:abc","createdAt":1,"updatedAt":2,"downloads":3,"verified":true,"content":{{"prompt":"{prompt}"{extra}}}}}"#
        )
    }

    #[test]
    fn parse_hub_url_accepts_origin_and_hub_page() {
        for input in ["https://n.kkpt.tech:8443", "https://n.kkpt.tech:8443/", "https://n.kkpt.tech:8443/hub", "https://n.kkpt.tech:8443/hub/"] {
            let url = parse_hub_url(input).expect(input);
            assert_eq!(url.as_str(), "https://n.kkpt.tech:8443/", "{input}");
        }
    }

    #[test]
    fn parse_hub_url_allows_loopback_http() {
        for input in ["http://127.0.0.1:8080", "http://localhost:8080", "http://[::1]:8080"] {
            let url = parse_hub_url(input).expect(input);
            assert_eq!(url.scheme(), "http", "{input}");
        }
    }

    #[test]
    fn parse_hub_url_rejects_junk() {
        for bad in [
            "",
            "n.kkpt.tech:8443",
            "http://example.com",
            "ftp://example.com",
            "https://user:pass@example.com",
            "https://example.com/path",
            "https://example.com/?q=1",
            "https://example.com/#x",
        ] {
            let error = parse_hub_url(bad).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
            assert!(error.remediation().is_some(), "{bad} 需要补救建议");
        }
    }

    #[test]
    fn list_url_encodes_query_and_clamps_limit() {
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json("", None))])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests.clone(), None);
        client
            .list(&HubQuery {
                kind: Some(HubKind::Expert),
                q: Some(" 安全 & x ".to_owned()),
                tag: Some("review".to_owned()),
                provider: Some(HubProvider::Claude),
                cursor: Some("2".to_owned()),
                limit: 500,
                refresh: false,
            })
            .expect("list");
        let requests = requests.borrow();
        let url = &requests[0].url;
        assert!(url.contains("kind=expert"), "{url}");
        assert!(url.contains("q=%E5%AE%89%E5%85%A8+%26+x"), "查询必须被编码：{url}");
        assert!(url.contains("tag=review"), "{url}");
        assert!(url.contains("provider=claude"), "{url}");
        assert!(url.contains("limit=100"), "limit 必须夹到 100：{url}");
        assert!(url.contains("cursor=2"), "{url}");
    }

    #[test]
    fn list_parses_items_and_next_cursor() {
        let item = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0","verified":true}"#;
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(item, Some("20")))])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let page = client.list(&HubQuery::experts()).expect("list");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].slug, "a");
        assert_eq!(page.next_cursor.as_deref(), Some("20"));
        assert_eq!(page.source, DataSource::Network);
    }

    #[test]
    fn list_skips_incomplete_items_with_a_warning() {
        let good = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0"}"#;
        let bad = r#"{"kind":"expert","slug":"","name":"","version":""}"#;
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(&format!("{good},{bad}"), None))])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let page = client.list(&HubQuery::experts()).expect("list");
        assert_eq!(page.items.len(), 1);
        assert!(page.warning.as_deref().unwrap_or_default().contains("已跳过"), "{:?}", page.warning);
    }

    #[test]
    fn detail_parses_expert_content() {
        let body = expert_json(
            "你是一名安全审查专家。",
            r#","knowledge":[{"name":"checklist.md","text":"检查清单"}],"skills":[{"slug":"wb-apiz","name":"Apiz"}],"modelHint":"sonnet","toolsHint":["Read"],"notes":"n""#,
        );
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let detail = client.show_expert("security-reviewer", None, false).expect("detail");
        assert_eq!(detail.item.slug, "security-reviewer");
        assert_eq!(detail.content.prompt, "你是一名安全审查专家。");
        assert_eq!(detail.content.knowledge.len(), 1);
        assert_eq!(detail.content.skills[0].slug, "wb-apiz");
        assert_eq!(detail.content.effective_model(), Some("sonnet"));
        assert_eq!(detail.content.effective_tools().to_vec(), vec!["Read".to_owned()]);
    }

    #[test]
    fn detail_accepts_model_and_tools_fields() {
        let body = expert_json("p", r#","model":"opus","tools":["Grep"]"#);
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let detail = client.show_expert("security-reviewer", None, false).expect("detail");
        assert_eq!(detail.content.effective_model(), Some("opus"));
        assert_eq!(detail.content.effective_tools().to_vec(), vec!["Grep".to_owned()]);
    }

    #[test]
    fn detail_rejects_missing_prompt() {
        let body = expert_json("", "");
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let error = client.show_expert("security-reviewer", None, false).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn detail_rejects_a_kind_mismatch() {
        let body = expert_json("p", "").replace(r#""kind":"expert""#, r#""kind":"skill""#);
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let error = client.show_expert("security-reviewer", None, false).expect_err("must fail");
        assert!(error.message().contains("不是 expert"), "{}", error.message());
    }

    #[test]
    fn detail_caps_knowledge_and_skills() {
        let knowledge: Vec<String> = (0..20).map(|i| format!(r#"{{"name":"k{i}.md","text":"x"}}"#)).collect();
        let skills: Vec<String> = (0..140).map(|i| format!(r#"{{"slug":"s{i}","name":"S{i}"}}"#)).collect();
        let body = expert_json("p", &format!(r#","knowledge":[{}],"skills":[{}]"#, knowledge.join(","), skills.join(",")));
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let detail = client.show_expert("security-reviewer", None, false).expect("detail");
        assert_eq!(detail.content.knowledge.len(), MAX_KNOWLEDGE);
        assert_eq!(detail.content.skills.len(), MAX_SKILLS);
        assert!(detail.warning.as_deref().unwrap_or_default().contains("截断"), "{:?}", detail.warning);
    }

    #[test]
    fn conditional_request_uses_etag_and_304_returns_cache() {
        let dir = tempfile::tempdir().expect("tmp");
        let item = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0"}"#;
        let first = HttpResponse { status: 200, etag: Some("\"v1\"".to_owned()), content_encoding: None, body: page_json(item, None).into_bytes() };
        let second = HttpResponse { status: 304, etag: Some("\"v1\"".to_owned()), content_encoding: None, body: Vec::new() };
        let responses = Rc::new(RefCell::new(VecDeque::from([first, second])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests.clone(), Some(dir.path().to_path_buf()));

        let page = client.list(&HubQuery { refresh: true, ..HubQuery::experts() }).expect("first");
        assert_eq!(page.source, DataSource::Network);
        let page = client.list(&HubQuery { refresh: true, ..HubQuery::experts() }).expect("second");
        assert_eq!(page.source, DataSource::Cache);
        assert_eq!(page.items.len(), 1);

        let requests = requests.borrow();
        assert_eq!(requests[1].headers.iter().find(|(name, _)| name.as_str() == "if-none-match").map(|(_, value)| value.as_str()), Some("\"v1\""));
        assert!(requests[1].headers.iter().any(|(name, value)| name.as_str() == "accept-encoding" && value.as_str() == "identity"));
    }

    #[test]
    fn fresh_cache_is_used_without_a_request() {
        let dir = tempfile::tempdir().expect("tmp");
        let item = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0"}"#;
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(item, None))])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests.clone(), Some(dir.path().to_path_buf())).with_clock(fixed_clock(1_000));

        client.list(&HubQuery::experts()).expect("first");
        let page = client.list(&HubQuery::experts()).expect("second");
        assert_eq!(page.source, DataSource::Cache);
        assert_eq!(requests.borrow().len(), 1, "新鲜期内不应再发请求");
    }

    #[test]
    fn server_error_falls_back_to_cache() {
        let dir = tempfile::tempdir().expect("tmp");
        let item = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0"}"#;
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(item, None)), response(503, "{}")])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, Some(dir.path().to_path_buf()));

        client.list(&HubQuery { refresh: true, ..HubQuery::experts() }).expect("first");
        let page = client.list(&HubQuery { refresh: true, ..HubQuery::experts() }).expect("fallback");
        assert_eq!(page.source, DataSource::CacheStale);
        assert_eq!(page.items.len(), 1);
        assert!(page.warning.as_deref().unwrap_or_default().contains("缓存"));
    }

    #[test]
    fn client_error_does_not_fall_back_to_cache() {
        let dir = tempfile::tempdir().expect("tmp");
        let item = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0"}"#;
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(item, None)), response(404, r#"{"error":{"code":"not_found"}}"#)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, Some(dir.path().to_path_buf()));

        client.list(&HubQuery { refresh: true, ..HubQuery::experts() }).expect("first");
        let error = client.list(&HubQuery { refresh: true, ..HubQuery::experts() }).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("找不到"), "{}", error.message());
        assert!(error.remediation().is_some());
    }

    #[test]
    fn rate_limit_has_a_remediation() {
        let responses = Rc::new(RefCell::new(VecDeque::from([response(429, r#"{"error":{"code":"rate_limited"}}"#)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let error = client.list(&HubQuery::experts()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("too") || error.message().contains("频繁"), "{}", error.message());
        assert!(error.remediation().unwrap_or_default().contains("一分钟"));
    }

    #[test]
    fn network_error_without_cache_is_reported() {
        let client = HubClient::new(base(), None, |_| Err(MetisError::new(ErrorKind::Network, "offline").with_remediation("测试用假错误")));
        let error = client.list(&HubQuery::experts()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("连不上") || error.message().contains("offline"));
        assert!(error.remediation().is_some());
    }

    #[test]
    fn corrupt_cache_is_ignored() {
        let dir = tempfile::tempdir().expect("tmp");
        let url = "https://hub.example.com/v1/hub/items?kind=expert&limit=20";
        let path = dir.path().join(format!("{}.json", metis_fs::hash_bytes(url.as_bytes())));
        std::fs::write(&path, "not json").expect("seed");
        let item = r#"{"kind":"expert","slug":"a","name":"A","version":"1.0.0"}"#;
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(item, None))])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, Some(dir.path().to_path_buf()));
        let page = client.list(&HubQuery::experts()).expect("list");
        assert_eq!(page.source, DataSource::Network);
    }

    #[test]
    fn oversized_body_is_reported() {
        let responses = Rc::new(RefCell::new(VecDeque::from([HttpResponse {
            status: 200,
            etag: None,
            content_encoding: None,
            body: vec![b'x'; HUB_MAX_RESPONSE_BYTES + 1],
        }])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let error = client.list(&HubQuery::experts()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("超过"), "{}", error.message());
    }

    #[test]
    fn unexpected_content_encoding_is_reported() {
        let responses =
            Rc::new(RefCell::new(VecDeque::from([HttpResponse { status: 200, etag: None, content_encoding: Some("zstd".to_owned()), body: Vec::new() }])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let error = client.list(&HubQuery::experts()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("zstd"), "{}", error.message());
    }

    #[test]
    fn bad_slug_is_rejected_before_any_request() {
        let responses = Rc::new(RefCell::new(VecDeque::new()));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests.clone(), None);
        for bad in ["", "A", "a/b", "../x", "a b", "-a"] {
            let error = client.show_expert(bad, None, false).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
        }
        assert!(requests.borrow().is_empty(), "坏 slug 不能发请求");
    }

    #[test]
    fn bad_version_is_rejected_before_any_request() {
        let responses = Rc::new(RefCell::new(VecDeque::new()));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests.clone(), None);
        for bad in ["", "..", "1/2", "a b"] {
            let error = client.show_expert("security-reviewer", Some(bad), false).expect_err(bad);
            assert_eq!(error.kind(), ErrorKind::InvalidArgument, "{bad}");
        }
        assert!(requests.borrow().is_empty(), "坏版本不能发请求");
    }

    #[test]
    fn kind_and_provider_strings_are_stable() {
        assert_eq!(HubKind::parse("expert"), Some(HubKind::Expert));
        assert_eq!(HubKind::parse("skill"), Some(HubKind::Skill));
        assert_eq!(HubKind::parse("mcp"), Some(HubKind::Mcp));
        assert_eq!(HubKind::parse("nope"), None);
        assert_eq!(HubKind::Skill.as_str(), "skill");
        assert_eq!(HubKind::Mcp.as_str(), "mcp");
        assert_eq!(HubProvider::Claude.as_str(), "claude");
        assert_eq!(HubProvider::Codex.as_str(), "codex");
        assert_eq!(HubProvider::Deepseek.as_str(), "deepseek");
    }

    #[test]
    fn a_version_is_appended_to_the_detail_url() {
        let body = expert_json("p", "");
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests.clone(), None);
        client.show_expert("security-reviewer", Some("1.2.0"), false).expect("detail");
        let requests = requests.borrow();
        assert!(requests[0].url.ends_with("/v1/hub/items/expert/security-reviewer/1.2.0"), "{}", requests[0].url);
    }

    #[test]
    fn a_304_without_cache_is_reported() {
        let responses = Rc::new(RefCell::new(VecDeque::from([response(304, "")])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let error = client.list(&HubQuery::experts()).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("304"), "{}", error.message());
        assert!(error.remediation().is_some());
    }

    #[test]
    fn summaries_are_trimmed_and_deduplicated() {
        let name = "N".repeat(200);
        let item = format!(r#"{{"kind":"expert","slug":"a","name":"{name}","version":"1.0.0","tags":["x","x","y"],"providers":["claude","claude"]}}"#);
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &page_json(&item, None))])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let page = client.list(&HubQuery::experts()).expect("list");
        let item = &page.items[0];
        assert_eq!(item.name.chars().count(), 80);
        assert_eq!(item.tags, vec!["x".to_owned(), "y".to_owned()]);
        assert_eq!(item.providers, vec!["claude".to_owned()]);
    }

    #[test]
    fn unverified_content_is_flagged_but_not_rejected() {
        let body = expert_json("p", "").replace(r#""verified":true"#, r#""verified":false"#);
        let responses = Rc::new(RefCell::new(VecDeque::from([response(200, &body)])));
        let requests = Rc::new(RefCell::new(Vec::new()));
        let client = client(responses, requests, None);
        let detail = client.show_expert("security-reviewer", None, false).expect("detail");
        assert!(detail.warning.as_deref().unwrap_or_default().contains("校验失败"), "{:?}", detail.warning);
    }

    #[test]
    fn hub_error_maps_statuses_and_keeps_remediation() {
        let bad = hub_error(400, br#"{"error":{"code":"invalid"}}"#);
        assert_eq!(bad.kind(), ErrorKind::InvalidArgument);
        assert!(bad.remediation().is_some());
        let down = hub_error(503, b"{}");
        assert_eq!(down.kind(), ErrorKind::Network);
        assert!(down.remediation().is_some());
        let other = hub_error(418, b"{}");
        assert_eq!(other.kind(), ErrorKind::Network);
        assert!(other.remediation().is_some());
    }

    /// 只起一个**本机回环** HTTP 服务，验证生产 fetch 的请求头、响应头与上限。
    /// 每连接一个线程（`PITFALLS.md` G57），不碰外网。
    fn serve_once(response: Vec<u8>) -> (String, std::thread::JoinHandle<String>) {
        use std::io::Write as _;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                match stream.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&buffer[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = stream.write_all(&response);
            let _ = stream.flush();
            String::from_utf8_lossy(&request).to_string()
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn ureq_fetch_reads_a_local_response_and_sends_identity() {
        let body = br#"{"items":[],"nextCursor":null}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nETag: \"abc\"\r\nConnection: close\r\n\r\n{}",
            body.len(),
            String::from_utf8_lossy(body)
        )
        .into_bytes();
        let (base, handle) = serve_once(response);
        let fetched = ureq_fetch(&FetchRequest {
            url: format!("{base}/v1/hub/items"),
            headers: vec![("accept".to_owned(), "application/json".to_owned()), ("accept-encoding".to_owned(), "identity".to_owned())],
            max_bytes: 1024,
        })
        .expect("fetch");
        assert_eq!(fetched.status, 200);
        assert_eq!(fetched.etag.as_deref(), Some("\"abc\""));
        assert_eq!(fetched.body, body.to_vec());
        let request = handle.join().expect("join");
        assert!(request.to_ascii_lowercase().contains("accept-encoding: identity"), "{request}");
    }

    #[test]
    fn ureq_fetch_reports_an_oversized_body() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\nConnection: close\r\n\r\n0123456789".to_vec();
        let (base, handle) = serve_once(response);
        let error = ureq_fetch(&FetchRequest { url: format!("{base}/x"), headers: Vec::new(), max_bytes: 4 }).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::Network);
        assert!(error.message().contains("超过"), "{}", error.message());
        let _ = handle.join();
    }

    #[test]
    fn ureq_fetch_does_not_follow_redirects() {
        let response = b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
        let (base, handle) = serve_once(response);
        let fetched = ureq_fetch(&FetchRequest { url: format!("{base}/x"), headers: Vec::new(), max_bytes: 1024 }).expect("fetch");
        assert_eq!(fetched.status, 302);
        let _ = handle.join();
    }

    #[test]
    #[ignore = "需要真实 worldd hub（https://n.kkpt.tech:8443）"]
    fn live_hub_list_and_detail_match_the_contract() {
        let base = parse_hub_url(HUB_DEFAULT_URL).expect("url");
        let client = HubClient::new(base, None, ureq_fetch);
        let page = client.list(&HubQuery::experts()).expect("list");
        assert!(!page.items.is_empty(), "真实 Hub 的 expert 列表不应为空");
        let first = &page.items[0];
        let detail = client.show_expert(&first.slug, None, true).expect("detail");
        assert!(!detail.content.prompt.trim().is_empty());
        assert_eq!(detail.item.slug, first.slug);
    }
}
