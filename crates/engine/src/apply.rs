//! 应用编排：把"用户选了什么"落到每一条通道上。
//!
//! 顺序即行为（`docs/ARCHITECTURE.md` §5）：
//! 1. 先算模式；
//! 2. **先发布 off**（这样即使后面某一家失败，动态通道也已经停）；
//! 3. 逐 `harness × channel` 执行，每格一条结果；
//! 4. `off` 时额外清理磁盘上仍存在的托管块；
//! 5. 写回报告。

use metis_core::{Channel, Effect, HarnessId, MetisError, Mode, PersonaId, Result};
use serde::{Deserialize, Serialize};

use crate::Engine;

/// 单条结果的 status。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResultStatus {
    /// 写成功了。
    Written,
    /// 失败了。
    Failed,
}

impl ResultStatus {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Written => "written",
            Self::Failed => "failed",
        }
    }
}

/// 一条通道的执行结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelResult {
    /// 哪家。
    pub harness: String,
    /// 哪条通道。
    pub channel: Channel,
    /// 成功还是失败。
    pub status: ResultStatus,
    /// 具体做了什么。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    /// 生效时机。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
    /// 面向用户的一句话。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// 失败原因。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// 补救建议。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "remediation")]
    pub remediation: Option<String>,
    /// 用了哪份人设。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// 当时的模式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<Mode>,
    /// 目标文件。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// 运行时是否已确认生效。**apply 里恒为 false**（铁律 4）。
    #[serde(default, rename = "runtimeConfirmed")]
    pub runtime_confirmed: bool,
}

impl ChannelResult {
    /// 成功。
    pub fn written(harness: &HarnessId, channel: Channel, action: impl Into<String>) -> Self {
        Self {
            harness: harness.to_string(),
            channel,
            status: ResultStatus::Written,
            action: Some(action.into()),
            effect: Some(channel.effect()),
            message: Some(channel.effect().message().to_owned()),
            error: None,
            remediation: None,
            persona: None,
            mode: None,
            file: None,
            runtime_confirmed: false,
        }
    }

    /// 失败。
    pub fn failed(harness: &HarnessId, channel: Channel, error: &MetisError) -> Self {
        Self {
            harness: harness.to_string(),
            channel,
            status: ResultStatus::Failed,
            action: None,
            effect: Some(channel.effect()),
            message: None,
            error: Some(error.message().to_owned()),
            remediation: error.remediation().map(str::to_owned),
            persona: None,
            mode: None,
            file: None,
            runtime_confirmed: false,
        }
    }

    /// 附上文件路径。
    #[must_use]
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }

    /// 是否失败。
    pub fn is_failed(&self) -> bool {
        self.status == ResultStatus::Failed
    }
}

/// 一次应用的报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReport {
    /// 唯一 id。
    pub id: String,
    /// 时间（RFC 3339）。
    pub at: String,
    /// `sha256(persona, body, mode)[..16]`：这次报告对应哪版人设。
    pub revision: String,
    /// 用了哪份人设。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// 当时的模式。
    pub mode: Mode,
    /// 全部成功。
    pub ok: bool,
    /// 有失败也有成功。
    pub partial: bool,
    /// 逐条结果。
    pub results: Vec<ChannelResult>,
    /// 运行时确认。**恒为 false**：只有真机抓包才能把它变真。
    #[serde(rename = "runtimeConfirmed")]
    pub runtime_confirmed: bool,
}

impl ApplyReport {
    /// 计算 revision。
    pub fn revision_of(persona: Option<&PersonaId>, body: &str, mode: Mode) -> String {
        let material = format!("{}", serde_json::json!([persona.map(PersonaId::as_str), body.trim(), mode.as_str()]));
        metis_fs::hash_bytes(material.as_bytes())[..16].to_owned()
    }

    /// 失败的条目。
    pub fn failures(&self) -> Vec<&ChannelResult> {
        self.results.iter().filter(|r| r.is_failed()).collect()
    }

    /// 成功了几条。
    pub fn written_count(&self) -> usize {
        self.results.iter().filter(|r| !r.is_failed()).count()
    }
}

/// 一次应用的输入。
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
    /// 只重跑上次失败的条目。
    pub only_failed: bool,
    /// 显式指定的人设。
    pub persona: Option<PersonaId>,
    /// 显式指定的模式。
    pub mode: Option<Mode>,
    /// 显式开关电源（`false` = 暂停）。
    pub enabled: Option<bool>,
}

/// 执行一次应用。
///
/// 全程持控制锁；返回报告并把它写回 `state.json` 的 `last_apply`。
pub fn apply(engine: &Engine, options: &ApplyOptions) -> Result<ApplyReport> {
    let paths = engine.paths().clone();
    metis_fs::lock::with_lock(paths.home(), || apply_locked(engine, options))
}

fn apply_locked(engine: &Engine, options: &ApplyOptions) -> Result<ApplyReport> {
    let paths = engine.paths();
    let mut state = crate::state::StateFile::load(paths)?;

    // ① 先算模式。
    let mut mode = options.mode.unwrap_or(state.default.mode);
    if options.enabled == Some(false) {
        mode = Mode::Off;
    }
    if state.default.mode == Mode::Off && options.enabled != Some(true) && mode != Mode::Off {
        mode = Mode::Off;
    }
    if options.enabled == Some(true) && mode == Mode::Off {
        mode = state.prev_mode.filter(|m| m.injects()).unwrap_or(Mode::DEFAULT);
    }

    // ② 先发布 off：即使后面有失败，动态通道也已经停。
    let previous_mode = state.default.mode;
    state.default.mode = mode;
    if previous_mode != Mode::Off && mode == Mode::Off {
        state.prev_mode = Some(previous_mode);
    }
    state.save(paths)?;

    // ③ 逐个通道执行。
    let store = metis_persona::PersonaStore::new(paths.clone());
    let (personas, _) = store.list()?;
    let persona = match options.persona.clone() {
        Some(id) => personas.iter().find(|p| p.id == id).cloned(),
        None => match state.persona.as_deref() {
            Some(raw) => personas.iter().find(|p| p.id.as_str() == raw).cloned(),
            None => personas.first().cloned(),
        },
    };
    if mode.injects() && persona.is_none() {
        return Err(MetisError::new(metis_core::ErrorKind::PersonaMissing, "没有可用人设，无法应用")
            .with_remediation("先在「人设库」新建一份，或用 metis import personas 导入"));
    }
    // 暂停（mode = off）不需要人设：它的语义是"把静态内容撤下来"，反而必须在没有人设时也能做。
    // 刷新 active.md 快照（宿主插件读它）。
    if let Some(current) = &persona {
        store.set_active(current)?;
        state.persona = Some(current.id.as_str().to_owned());
    }

    let mut results = Vec::new();
    let armed_pairs: Vec<(String, Vec<Channel>)> = state.armed.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    for (harness_id, channels) in armed_pairs {
        let harness = HarnessId::parse(&harness_id)?;
        let activation = state.activation_for(&harness, &metis_core::ActivationContext { launch: modes_launch(mode), ..Default::default() })?;
        for channel in channels {
            if options.only_failed && !was_failed(&state, &harness, channel) {
                continue;
            }
            let result = run_channel(engine, &harness, channel, mode, persona.as_ref(), activation.on);
            results.push(result);
        }
    }

    let failed = results.iter().filter(|r| r.is_failed()).count();
    let report = ApplyReport {
        id: uuid::Uuid::new_v4().to_string(),
        at: now_rfc3339(),
        revision: ApplyReport::revision_of(persona.as_ref().map(|p| &p.id), persona.as_ref().map(|p| p.body.as_str()).unwrap_or(""), mode),
        persona: persona.as_ref().map(|p| p.id.to_string()),
        mode,
        ok: failed == 0,
        partial: failed > 0 && failed < results.len(),
        results,
        // 铁律 4：写入成功 ≠ 运行时生效。这个字段只有真机抓包才能变真。
        runtime_confirmed: false,
    };

    // ④ 写变更日志。日志失败**不阻断**主流程：它只是诊断材料，
    //    但"没写成"这件事在被问到时要能回答（返回的 report 里已有 ok/失败项）。
    for result in &report.results {
        let mut entry = crate::journal::JournalEntry::new("cli", format!("apply.{}", result.channel.as_str()), !result.is_failed())
            .with_hashes(None, None)
            .with_revision(report.revision.clone());
        entry.harness = Some(result.harness.clone());
        entry.channel = Some(result.channel.as_str().to_owned());
        entry.path = result.file.clone();
        entry.action = result.action.clone();
        let _ = crate::journal::append(&paths.journal_dir(), &entry);
    }

    // ⑤ 写回报告。
    state.last_apply = serde_json::to_value(&report).ok();
    state.save(paths)?;
    Ok(report)
}

fn modes_launch(mode: Mode) -> bool {
    mode.injects()
}

fn was_failed(state: &crate::state::StateFile, harness: &HarnessId, channel: Channel) -> bool {
    let Some(last) = &state.last_apply else { return false };
    let Some(results) = last.get("results").and_then(|r| r.as_array()) else { return false };
    results.iter().any(|entry| {
        entry.get("harness").and_then(|h| h.as_str()) == Some(harness.as_str())
            && entry.get("channel").and_then(|c| c.as_str()) == Some(channel.as_str())
            && entry.get("status").and_then(|s| s.as_str()) == Some("failed")
    })
}

fn run_channel(engine: &Engine, harness: &HarnessId, channel: Channel, mode: Mode, persona: Option<&metis_persona::Persona>, activated: bool) -> ChannelResult {
    let paths = engine.paths();
    let home = paths.home();
    let result = (|| -> Result<ChannelResult> {
        let Some(spec) = metis_harness::registry::find(harness.as_str()) else {
            return Err(MetisError::new(metis_core::ErrorKind::UnknownHarness, format!("未注册的执行器：{harness}"))
                .with_remediation("用 `metis doctor` 看本机支持哪些执行器"));
        };
        let caps = metis_harness::registry::caps_of(spec);
        if !caps.channel(channel).ok {
            let why = caps.channel(channel).why.clone().unwrap_or_else(|| "该通道不可用".to_owned());
            return Err(MetisError::new(metis_core::ErrorKind::UnsupportedChannel, why).with_remediation("换一条通道，或在界面里看这家的能力说明"));
        }
        if !activated && mode.injects() {
            return Err(MetisError::new(metis_core::ErrorKind::Refused, "这家当前没有获得注入许可")
                .with_remediation("先把它武装起来（metis arm），或检查是不是被暂停了"));
        }
        if !mode.injects() {
            // off：只撤不改。
            return match channel {
                Channel::Install => {
                    let Some(target) = &spec.persist else {
                        return Ok(ChannelResult::written(harness, channel, "disabled-by-state"));
                    };
                    let file = paths.harness_home(harness, env_get, user_home()).unwrap_or_else(|_| paths.home().to_path_buf()).join(target.relative);
                    let outcome = metis_channel::install::revert_block(home, harness.as_str(), "install", &file)?;
                    Ok(ChannelResult::written(harness, channel, outcome.action.as_str()).with_file(outcome.file.display().to_string()))
                }
                Channel::Project => Ok(ChannelResult::written(harness, channel, "disabled-by-state")),
                Channel::Shim => {
                    let removed = metis_channel::shim::revert(home, &paths.shim_dir(), harness.as_str())?;
                    Ok(ChannelResult::written(harness, channel, if removed { "removed" } else { "already-clean" }))
                }
                Channel::Slash => Ok(ChannelResult::written(harness, channel, "disabled-by-state")),
            };
        }

        let body = persona.map(|p| p.trimmed_body().to_owned()).unwrap_or_default();
        match channel {
            Channel::Install => {
                let Some(target) = &spec.persist else {
                    return Err(MetisError::new(metis_core::ErrorKind::UnsupportedChannel, "这家没有可写的配置文件").with_remediation("用启动参数或项目通道"));
                };
                let harness_home = paths.harness_home(harness, env_get, user_home())?;

                // 落点有两种形状：单个文件，或"每个 profile 一份"的目录展开。
                if target.kind == metis_harness::PersistKind::PerProfileDir {
                    let profiles = metis_channel::slash::profile_dirs(home, harness, &harness_home, target.relative);
                    if profiles.is_empty() {
                        return Err(MetisError::new(
                            metis_core::ErrorKind::Io,
                            format!("找不到任何 {harness} profile（{}/{}/<name>）", harness_home.display(), target.relative),
                        )
                        .with_remediation("先启动一次该宿主，让它把 profile 建好，再重试"));
                    }
                    let mut written = 0_usize;
                    let mut last = std::path::PathBuf::new();
                    for profile in &profiles {
                        let outcome = metis_channel::slash::install(home, harness, profile)?;
                        // 光把文件放进 node_modules 宿主不会加载：必须在 cordis.patch.yml 里点名。
                        for plugin in ["metis", "metis-slash"] {
                            metis_channel::dsh_patch::mount(home, profile, plugin)?;
                        }
                        last = outcome.file;
                        written += 1;
                    }
                    return Ok(ChannelResult::written(harness, channel, format!("installed-into-{written}-profiles")).with_file(last.display().to_string()));
                }

                let file = harness_home.join(target.relative);
                let outcome = metis_channel::install::apply_block(home, harness.as_str(), "install", &file, &body)?;
                Ok(ChannelResult::written(harness, channel, outcome.action.as_str()).with_file(outcome.file.display().to_string()))
            }
            Channel::Shim => {
                let exe = std::env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "metis".to_owned());
                let outcome = metis_channel::shim::install(home, &paths.shim_dir(), harness.as_str(), &exe)?;
                let mut result = ChannelResult::written(harness, channel, if outcome.written { "installed" } else { "unchanged" })
                    .with_file(outcome.file.display().to_string());
                // 文件写了 ≠ 裸命令被接管：shim 目录必须在 PATH 最前面，否则系统仍会
                // 先找到原来的 `claude`。把这件事说清楚（UI 也会显示 remediation）。
                if !metis_channel::shim::on_path(&paths.shim_dir()) {
                    let hint = metis_channel::shim::path_hint(&paths.shim_dir());
                    result.message = Some(format!("{}；{hint}", result.message.unwrap_or_default()));
                    result.remediation = Some(hint);
                }
                Ok(result)
            }
            Channel::Project => {
                let dirs = state_project_dirs(engine, harness)?;
                if dirs.is_empty() {
                    return Err(MetisError::new(metis_core::ErrorKind::InvalidArgument, "没有已登记的项目目录")
                        .with_remediation("用 metis project on 指定一个绝对路径"));
                }
                let mut last = None;
                for dir in &dirs {
                    let file_name = spec.project_file.unwrap_or("AGENTS.md");
                    let outcome = metis_channel::project::apply(home, harness.as_str(), std::path::Path::new(dir), file_name, &body)?;
                    last = Some(outcome);
                }
                let outcome = last.ok_or_else(|| MetisError::new(metis_core::ErrorKind::Internal, "项目目录列表为空"))?;
                Ok(ChannelResult::written(harness, channel, outcome.action.as_str()).with_file(outcome.file.display().to_string()))
            }
            Channel::Slash => {
                // 会话内 `/purge`：写宿主的扩展/插件产物。
                let harness_home = paths.harness_home(harness, env_get, user_home())?;
                let outcome = metis_channel::slash::install(home, harness, &harness_home)?;
                Ok(ChannelResult::written(harness, channel, if outcome.written { "installed" } else { "unchanged" })
                    .with_file(outcome.file.display().to_string()))
            }
        }
    })();

    match result {
        Ok(mut result) => {
            result.persona = persona.map(|p| p.id.to_string());
            result.mode = Some(mode);
            result
        }
        Err(error) => {
            let mut result = ChannelResult::failed(harness, channel, &error);
            result.mode = Some(mode);
            result
        }
    }
}

fn state_project_dirs(engine: &Engine, harness: &HarnessId) -> Result<Vec<String>> {
    let state = crate::state::StateFile::load(engine.paths())?;
    Ok(state.project_dirs.get(harness.as_str()).cloned().unwrap_or_default())
}

fn env_get(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn user_home() -> &'static std::path::Path {
    // 只在需要拼宿主主目录时用到；失败时回落到临时目录由调用方兜底。
    static HOME: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
    HOME.get_or_init(|| dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")))
}

fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap_or_else(|_| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;

    fn engine() -> (tempfile::TempDir, Engine) {
        let dir = tempfile::tempdir().expect("tempdir");
        // 宿主主目录也重定向到临时目录：apply 会真的写文件，绝不能落到 ~/.pi。
        let paths = Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harness-homes"));
        (dir, Engine::new(paths))
    }

    fn seed_persona(engine: &Engine, id: &str) {
        let store = metis_persona::PersonaStore::new(engine.paths().clone());
        let persona_id = PersonaId::parse(id).expect("id");
        store.save(&persona_id, &metis_persona::Meta::default(), "人设正文").expect("save");
        let mut state = crate::state::StateFile::load(engine.paths()).expect("load");
        state.persona = Some(id.to_owned());
        state.default.harness = Some("codex".to_owned());
        state.save(engine.paths()).expect("save");
    }

    #[test]
    fn slash_channel_writes_host_products_into_the_isolated_home() {
        // slash 通道最终要落到"宿主的扩展目录"里：这条测试证明整条链路通，
        // 而且**只**落在注入的目录里（不会碰真实的 ~/.pi）。
        let (_dir, engine) = engine();
        seed_persona(&engine, "concise");

        let harness = HarnessId::parse("pi").expect("id");
        engine
            .update_state(|state| {
                state.set_armed(&harness, Channel::Slash, true);
                Ok(())
            })
            .expect("arm");

        let report = apply(&engine, &ApplyOptions::default()).expect("apply");
        assert!(report.ok, "slash 通道应写入成功：{:?}", report.failures());
        let result = report.results.iter().find(|r| r.channel == Channel::Slash).expect("slash 结果");
        assert_eq!(result.status, ResultStatus::Written);

        let file = result.file.as_deref().expect("必须报告文件路径");
        assert!(std::path::Path::new(file).is_file(), "产物必须真的落盘：{file}");
        assert!(file.contains("harness-homes"), "必须落在注入的宿主目录里，实际 {file}");
        assert!(file.ends_with("metis.mjs"));

        // 产物是可撤销的：disarm 之后文件消失、账本清空。
        let removed = metis_channel::slash::revert(engine.paths().home(), &harness).expect("revert");
        assert!(removed >= 1);
        assert!(!std::path::Path::new(file).exists(), "撤销后产物必须被删除");
    }

    #[test]
    fn report_revision_is_deterministic() {
        let a = ApplyReport::revision_of(None, "body", Mode::Replace);
        let b = ApplyReport::revision_of(None, "body", Mode::Replace);
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert_ne!(a, ApplyReport::revision_of(None, "other", Mode::Replace));
        assert_ne!(a, ApplyReport::revision_of(None, "body", Mode::Append));
    }

    #[test]
    fn apply_without_persona_and_injecting_mode_is_an_error() {
        let (_dir, engine) = engine();
        let error = apply(&engine, &ApplyOptions::default()).expect_err("must fail without persona");
        assert_eq!(error.kind(), metis_core::ErrorKind::PersonaMissing);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn apply_writes_active_snapshot_and_report() {
        let (_dir, engine) = engine();
        seed_persona(&engine, "concise");
        let report = apply(&engine, &ApplyOptions::default()).expect("apply");
        assert!(report.ok, "没有武装任何通道时应成功且无事可做");
        assert!(!report.runtime_confirmed, "apply 里 runtime_confirmed 必须恒为 false");

        let store = metis_persona::PersonaStore::new(engine.paths().clone());
        assert_eq!(store.active_body().expect("body").as_deref(), Some("人设正文\n"));

        let state = crate::state::StateFile::load(engine.paths()).expect("load");
        assert!(state.last_apply.is_some(), "报告必须写回 state");
    }

    #[test]
    fn apply_publishes_off_before_touching_channels() {
        let (_dir, engine) = engine();
        seed_persona(&engine, "concise");
        let report = apply(&engine, &ApplyOptions { enabled: Some(false), ..ApplyOptions::default() }).expect("apply");
        assert_eq!(report.mode, Mode::Off);
        let state = crate::state::StateFile::load(engine.paths()).expect("load");
        assert_eq!(state.default.mode, Mode::Off, "总电源必须已经关掉");
        assert_eq!(state.prev_mode, Some(Mode::Replace));
    }

    #[test]
    fn apply_resumes_previous_mode() {
        let (_dir, engine) = engine();
        seed_persona(&engine, "concise");
        apply(&engine, &ApplyOptions { enabled: Some(false), ..ApplyOptions::default() }).expect("pause");
        let report = apply(&engine, &ApplyOptions { enabled: Some(true), ..ApplyOptions::default() }).expect("resume");
        assert_eq!(report.mode, Mode::Replace, "恢复应回到暂停前的模式");
    }

    #[test]
    fn report_helpers_count_correctly() {
        let harness = HarnessId::parse("codex").expect("id");
        let ok = ChannelResult::written(&harness, Channel::Install, "installed");
        let bad = ChannelResult::failed(&harness, Channel::Shim, &MetisError::new(metis_core::ErrorKind::Io, "boom").with_remediation("重试"));
        let report = ApplyReport {
            id: "x".to_owned(),
            at: "now".to_owned(),
            revision: "rev".to_owned(),
            persona: None,
            mode: Mode::Replace,
            ok: false,
            partial: true,
            results: vec![ok, bad],
            runtime_confirmed: false,
        };
        assert_eq!(report.failures().len(), 1);
        assert_eq!(report.written_count(), 1);
        assert!(report.partial);
    }

    #[test]
    fn channel_result_carries_effect_and_never_claims_runtime_confirmation() {
        let harness = HarnessId::parse("pi").expect("id");
        let result = ChannelResult::written(&harness, Channel::Slash, "installed");
        assert_eq!(result.effect, Some(Effect::NextTurn));
        assert!(!result.runtime_confirmed);
        let failed = ChannelResult::failed(&harness, Channel::Install, &MetisError::new(metis_core::ErrorKind::Io, "x").with_remediation("重试"));
        assert!(failed.is_failed());
        assert!(!failed.runtime_confirmed);
    }

    #[test]
    fn report_serialises_with_camel_case_keys() {
        let harness = HarnessId::parse("codex").expect("id");
        let report = ApplyReport {
            id: "x".to_owned(),
            at: "now".to_owned(),
            revision: "rev".to_owned(),
            persona: Some("concise".to_owned()),
            mode: Mode::Replace,
            ok: true,
            partial: false,
            results: vec![ChannelResult::written(&harness, Channel::Install, "installed")],
            runtime_confirmed: false,
        };
        let json = serde_json::to_value(&report).expect("json");
        assert_eq!(json["runtimeConfirmed"], false);
        assert_eq!(json["results"][0]["channel"], "install");
        assert_eq!(json["results"][0]["status"], "written");
    }
}
