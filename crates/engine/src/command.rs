//! 命令注册表（铁律 2：一切皆命令）。
//!
//! CLI 与 GUI 都按 id 分发；判定逻辑只在这里与它调用的引擎函数里，不在界面里。

use std::collections::BTreeMap;

use metis_core::{Channel, ErrorKind, HarnessId, MetisError, Mode, PersonaId, Result};
use serde_json::Value;

use crate::Engine;

/// 一个命令的输出。
#[derive(Debug, Clone, PartialEq)]
pub enum CommandOutput {
    /// 一句话结果。
    Message(String),
    /// 结构化数据（`--json` 与 GUI 用）。
    Json(Value),
    /// 一次应用的报告。
    Apply(Box<crate::ApplyReport>),
}

impl CommandOutput {
    /// 转成 JSON。
    pub fn to_json(&self) -> Value {
        match self {
            Self::Message(text) => serde_json::json!({ "message": text }),
            Self::Json(value) => value.clone(),
            Self::Apply(report) => serde_json::to_value(report.as_ref()).unwrap_or(Value::Null),
        }
    }
}

/// 一个命令。
pub trait Command: Send + Sync {
    /// 稳定 id（CLI 子命令、GUI 动作、文档共用）。
    fn id(&self) -> &'static str;

    /// 面向用户的标题键。
    fn title(&self) -> &'static str;

    /// 会不会改动磁盘 / 起进程 / 联网。
    fn effects(&self) -> Effects {
        Effects::none()
    }

    /// 执行。
    fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput>;
}

/// 副作用声明（供 UI 提示与 `--dry-run` 使用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Effects {
    /// 会写盘。
    pub writes_disk: bool,
    /// 会起外部进程。
    pub spawns_process: bool,
    /// 会联网。
    pub network: bool,
}

impl Effects {
    /// 没有副作用。
    pub const fn none() -> Self {
        Self { writes_disk: false, spawns_process: false, network: false }
    }

    /// 只写盘。
    pub const fn writes() -> Self {
        Self { writes_disk: true, spawns_process: false, network: false }
    }
}

/// 命令表。
pub struct Commands {
    entries: BTreeMap<&'static str, Box<dyn Command>>,
}

impl std::fmt::Debug for Commands {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Commands").field("ids", &self.entries.keys().collect::<Vec<_>>()).finish()
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::new()
    }
}

impl Commands {
    /// 空表。
    pub fn new() -> Self {
        // 不能写 Self::default()——它会回调 new()，互相递归把栈打穿。
        Self { entries: BTreeMap::new() }
    }

    /// 注册一个命令。
    pub fn register(&mut self, command: impl Command + 'static) {
        self.entries.insert(command.id(), Box::new(command));
    }

    /// 全部命令 id（排序后，稳定）。
    pub fn ids(&self) -> Vec<&'static str> {
        self.entries.keys().copied().collect()
    }

    /// 按 id 取命令本身（CLI/GUI 的一致性测试需要看它的 `title` 与 `effects`）。
    pub fn get(&self, id: &str) -> Option<&dyn Command> {
        self.entries.get(id).map(|entry| entry.as_ref())
    }

    /// 按 id 取效果声明。
    pub fn effects_of(&self, id: &str) -> Option<Effects> {
        self.get(id).map(Command::effects)
    }

    /// 按 id 执行。
    pub fn run(&self, id: &str, engine: &Engine, params: &Value) -> Result<CommandOutput> {
        let command = self.entries.get(id).ok_or_else(|| {
            MetisError::new(ErrorKind::InvalidArgument, format!("未知命令：{id}")).with_remediation(format!("可用命令：{}", self.ids().join(", ")))
        })?;
        command.run(engine, params)
    }

    /// 查一个命令的副作用声明。
    pub fn effects(&self, id: &str) -> Option<Effects> {
        self.entries.get(id).map(|command| command.effects())
    }

    /// 默认的完整命令表。
    pub fn default_set() -> Self {
        let mut commands = Self::new();
        commands.register(state::ShowStatus);
        commands.register(state::SetPersona);
        commands.register(state::SetMode);
        commands.register(state::SetPower);
        commands.register(state::SetArmed);
        commands.register(state::ArmChannel);
        commands.register(persona::ListPersonas);
        commands.register(persona::SavePersona);
        commands.register(persona::RemovePersona);
        commands.register(apply::ApplyNow);
        commands.register(harness::ListHarnesses);
        commands.register(crate::commands_probe::Doctor);
        commands.register(crate::commands_probe::Launch);
        commands.register(crate::commands_probe::Ask);
        commands.register(crate::commands_verify::Verify);
        commands.register(crate::commands_verify::VerifyReport);
        commands.register(crate::commands_maintenance::Clean);
        commands.register(crate::commands_maintenance::Uninstall);
        commands.register(crate::commands_maintenance::SelfUninstall);
        commands.register(crate::commands_maintenance::ImportAgentPurge);
        commands.register(crate::commands_update::CheckUpdate);
        commands.register(crate::commands_update::Journal);
        commands.register(crate::commands_update::PruneJournal);
        commands.register(crate::commands_import::ImportPersonas);
        commands.register(crate::commands_armor::ArmorPlan);
        commands.register(crate::commands_armor::ArmorPreflight);
        commands.register(crate::commands_armor::ArmorRecord);
        commands.register(crate::commands_armor::ArmorJudge);
        commands.register(crate::commands_armor::ArmorAdopt);
        commands.register(crate::commands_backups::BackupsUsage);
        commands.register(crate::commands_backups::BackupsPrune);
        commands.register(crate::commands_project::ProjectAdd);
        commands.register(crate::commands_project::ProjectRemove);
        commands.register(crate::commands_project::ProjectList);
        commands
    }
}

fn params_object(params: &Value) -> Result<&serde_json::Map<String, Value>> {
    params.as_object().ok_or_else(|| MetisError::new(ErrorKind::InvalidArgument, "命令参数必须是一个对象").with_remediation("检查调用方传参"))
}

pub(crate) fn required_str(params: &Value, key: &str) -> Result<String> {
    params_object(params)?
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .ok_or_else(|| MetisError::new(ErrorKind::InvalidArgument, format!("缺少参数 {key}")).with_remediation("补上这个参数"))
}

pub(crate) fn optional_str(params: &Value, key: &str) -> Option<String> {
    params.get(key).and_then(|v| v.as_str()).map(str::to_owned)
}

pub(crate) fn required_bool(params: &Value, key: &str) -> Result<bool> {
    params_object(params)?
        .get(key)
        .and_then(|v| v.as_bool())
        .ok_or_else(|| MetisError::new(ErrorKind::InvalidArgument, format!("缺少布尔参数 {key}")).with_remediation("补上这个参数（true / false）"))
}

fn harness_param(params: &Value) -> Result<HarnessId> {
    HarnessId::parse(&required_str(params, "harness")?)
}

fn channel_param(params: &Value) -> Result<Channel> {
    let raw = required_str(params, "channel")?;
    Channel::parse(&raw)
        .ok_or_else(|| MetisError::new(ErrorKind::UnknownChannel, format!("未知通道：{raw}")).with_remediation("可用：shim, slash, install, project"))
}

/// 状态相关命令。
pub mod state {
    use super::*;

    /// 显示当前状态。
    #[derive(Debug)]
    pub struct ShowStatus;

    impl Command for ShowStatus {
        fn id(&self) -> &'static str {
            "state.status"
        }
        fn title(&self) -> &'static str {
            "state.status.title"
        }
        fn run(&self, engine: &Engine, _params: &Value) -> Result<CommandOutput> {
            // 读命令也要保证数据目录存在：否则第一次跑 `metis status` 之后
            // 目录仍然不存在，用户找不到自己的配置文件（也找不到日志去排查）。
            metis_fs::ensure_dir(engine.paths().home())?;
            let state = engine.state()?;
            let snapshot = serde_json::json!({
                "v": state.v,
                "persona": state.persona,
                "default": state.default,
                "armed": state.armed,
                "overrides": state.overrides,
                "disabledHarnesses": state.disabled_harnesses,
                "prevMode": state.prev_mode,
                "activationEpoch": state.activation_epoch,
                "statePath": engine.paths().state_file().display().to_string(),
            });
            Ok(CommandOutput::Json(snapshot))
        }
    }

    /// 换人设。
    #[derive(Debug)]
    pub struct SetPersona;

    impl Command for SetPersona {
        fn id(&self) -> &'static str {
            "persona.use"
        }
        fn title(&self) -> &'static str {
            "persona.use.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let id = PersonaId::parse(&required_str(params, "persona")?)?;
            let store = metis_persona::PersonaStore::new(engine.paths().clone());
            if store.read(&id)?.is_none() {
                // 这里是人名写错了（调用方传参问题），不是运行时故障：
                // 用 InvalidArgument，让 CLI 以退出码 2 区分"你传错了"。
                return Err(MetisError::new(ErrorKind::InvalidArgument, format!("没有人设：{id}")).with_remediation("用 metis ls 查看已有的，或先新建一份"));
            }
            match optional_str(params, "harness") {
                Some(harness) => {
                    let harness = HarnessId::parse(&harness)?;
                    engine.update_state(|state| {
                        state.set_override_persona(&harness, Some(id.clone()));
                        Ok(())
                    })?;
                    Ok(CommandOutput::Message(format!("{harness} 改用人设 {id}")))
                }
                None => {
                    engine.update_state(|state| {
                        state.persona = Some(id.to_string());
                        Ok(())
                    })?;
                    Ok(CommandOutput::Message(format!("当前人设：{id}")))
                }
            }
        }
    }

    /// 换模式。
    #[derive(Debug)]
    pub struct SetMode;

    impl Command for SetMode {
        fn id(&self) -> &'static str {
            "state.mode"
        }
        fn title(&self) -> &'static str {
            "state.mode.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let raw = required_str(params, "mode")?;
            let mode = Mode::parse(&raw).ok_or_else(|| {
                MetisError::new(ErrorKind::InvalidArgument, format!("未知模式：{raw}")).with_remediation("可用：replace, strict, append, off")
            })?;
            match optional_str(params, "harness") {
                Some(harness) => {
                    let harness = HarnessId::parse(&harness)?;
                    engine.update_state(|state| state.set_override_mode(&harness, Some(mode)))?;
                    Ok(CommandOutput::Message(format!("{harness} 模式改为 {mode}")))
                }
                None => {
                    engine.update_state(|state| {
                        state.default.mode = mode;
                        Ok(())
                    })?;
                    Ok(CommandOutput::Message(format!("全局模式：{mode}")))
                }
            }
        }
    }

    /// 开关总电源**并立即应用**。
    ///
    /// 只写 `state.json` 是不够的：暂停的语义是"把静态托管内容也撤下来"，
    /// 否则用户看到开关关了、宿主里却还留着人设（`PITFALLS.md` A4）。
    #[derive(Debug)]
    pub struct SetPower;

    impl Command for SetPower {
        fn id(&self) -> &'static str {
            "state.power"
        }
        fn title(&self) -> &'static str {
            "state.power.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let on = required_bool(params, "on")?;
            engine.update_state(|state| {
                state.set_power(on);
                Ok(())
            })?;
            // 立刻把状态同步到磁盘，让"暂停"真的撤下静态内容。
            let report = crate::apply::apply(engine, &crate::apply::ApplyOptions { enabled: Some(on), ..Default::default() })?;
            Ok(CommandOutput::Apply(Box::new(report)))
        }
    }

    /// 武装一条通道。
    #[derive(Debug)]
    pub struct ArmChannel;

    impl Command for ArmChannel {
        fn id(&self) -> &'static str {
            "channel.arm"
        }
        fn title(&self) -> &'static str {
            "channel.arm.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let harness = harness_param(params)?;
            let channel = channel_param(params)?;
            let on = required_bool(params, "on")?;
            engine.update_state(|state| {
                state.set_armed(&harness, channel, on);
                Ok(())
            })?;
            Ok(CommandOutput::Message(format!("{harness} 的 {} 通道已{}", channel, if on { "开启" } else { "关闭" })))
        }
    }

    /// 一次性设置某家的全部通道（`arm` / `disarm` 用），并立即同步磁盘。
    ///
    /// 关闭通道时**必须把已经写下去的内容撤掉**：否则账本里记着"我写过"，
    /// 磁盘上却没人再认得它，用户会看到一个再也撤不掉的残留块
    /// （`ARCHITECTURE.md` 不变式 I1 / `PITFALLS.md` A4）。
    #[derive(Debug)]
    pub struct SetArmed;

    impl Command for SetArmed {
        fn id(&self) -> &'static str {
            "channel.set"
        }
        fn title(&self) -> &'static str {
            "channel.set.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let harness = harness_param(params)?;
            let channels: Vec<Channel> = params
                .get("channels")
                .and_then(|v| v.as_array())
                .map(|items| items.iter().filter_map(|v| v.as_str()).filter_map(Channel::parse).collect())
                .unwrap_or_default();

            // 记下关闭前的武装状态：关闭之后才知道要撤哪些落点。
            let previously_armed = engine.state()?.armed_for(&harness).to_vec();

            engine.update_state(|state| {
                for channel in Channel::ALL {
                    state.set_armed(&harness, channel, channels.contains(&channel));
                }
                Ok(())
            })?;

            for channel in previously_armed.iter().filter(|c| !channels.contains(c)) {
                revert_channel(engine, &harness, *channel)?;
            }

            Ok(CommandOutput::Message(format!("{harness} 已开启 {} 条通道", channels.len())))
        }
    }

    /// 撤下一条通道已经写到磁盘上的内容。没有落点或没写过时是空操作。
    fn revert_channel(engine: &Engine, harness: &HarnessId, channel: Channel) -> Result<()> {
        let paths = engine.paths();
        let home = paths.home();
        match channel {
            Channel::Install => {
                let Some(spec) = metis_harness::registry::find(harness.as_str()) else { return Ok(()) };
                let Some(target) = &spec.persist else { return Ok(()) };

                // 按目录展开的落点（DSH 每个 profile 一份）：产物是 `slash::install` 写的，
                // 所以撤销也要走 `slash::revert` —— 走 `revert_block` 会一个文件都删不掉。
                if target.kind == metis_harness::PersistKind::PerProfileDir {
                    // 先摘挂载块（否则宿主会去找一个已经不存在的插件），再删产物。
                    if let Ok(harness_home) = paths.harness_home(harness, |key| std::env::var(key).ok(), user_home_path().as_path()) {
                        for profile in metis_channel::slash::profile_dirs(home, harness, &harness_home, target.relative) {
                            for plugin in ["metis", "metis-slash"] {
                                let _ = metis_channel::dsh_patch::unmount(home, &profile, plugin);
                            }
                        }
                    }
                    metis_channel::slash::revert(home, harness)?;
                    return Ok(());
                }

                let Ok(harness_home) = paths.harness_home(harness, |key| std::env::var(key).ok(), user_home_path().as_path()) else {
                    return Ok(());
                };
                let file = harness_home.join(target.relative.replace('/', std::path::MAIN_SEPARATOR_STR));
                // 只有账本里确实有这笔记录时才动文件。
                if let Ok(Some(entry)) = metis_fs::footprint::find_at(home, &file)
                    && entry.channel == "install"
                    && entry.harness == harness.as_str()
                {
                    metis_channel::install::revert_block(home, harness.as_str(), "install", &file)?;
                }
                Ok(())
            }
            Channel::Shim => {
                metis_channel::shim::revert(home, &paths.shim_dir(), harness.as_str())?;
                Ok(())
            }
            Channel::Project => {
                let dirs = engine.state()?.project_dirs.get(harness.as_str()).cloned().unwrap_or_default();
                let Some(spec) = metis_harness::registry::find(harness.as_str()) else { return Ok(()) };
                for dir in dirs {
                    metis_channel::project::revert(home, harness.as_str(), std::path::Path::new(&dir), spec.project_file.unwrap_or("AGENTS.md"))?;
                }
                Ok(())
            }
            Channel::Slash => {
                metis_channel::slash::revert(home, harness)?;
                Ok(())
            }
        }
    }

    /// 用户主目录（宿主配置的默认根）。
    fn user_home_path() -> std::path::PathBuf {
        dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."))
    }
}

/// 人设相关命令。
pub mod persona {
    use super::*;

    /// 列出人设。
    #[derive(Debug)]
    pub struct ListPersonas;

    impl Command for ListPersonas {
        fn id(&self) -> &'static str {
            "persona.list"
        }
        fn title(&self) -> &'static str {
            "persona.list.title"
        }
        fn run(&self, engine: &Engine, _params: &Value) -> Result<CommandOutput> {
            let store = metis_persona::PersonaStore::new(engine.paths().clone());
            let (personas, errors) = store.list()?;
            let active = engine.state()?.persona;
            let items: Vec<Value> = personas
                .iter()
                .map(|persona| {
                    serde_json::json!({
                        "id": persona.id.as_str(),
                        "name": persona.display_name(),
                        "description": persona.description(),
                        "chars": persona.char_count(),
                        "active": active.as_deref() == Some(persona.id.as_str()),
                        "warnings": persona.warnings,
                    })
                })
                .collect();
            Ok(CommandOutput::Json(serde_json::json!({
                "personas": items,
                "errors": errors.iter().map(|e| serde_json::json!({ "path": e.path.display().to_string(), "message": e.message })).collect::<Vec<_>>(),
            })))
        }
    }

    /// 保存人设。
    #[derive(Debug)]
    pub struct SavePersona;

    impl Command for SavePersona {
        fn id(&self) -> &'static str {
            "persona.save"
        }
        fn title(&self) -> &'static str {
            "persona.save.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let id = PersonaId::parse(&required_str(params, "persona")?)?;
            let body = required_str(params, "body")?;
            let mut meta =
                metis_persona::Meta { name: optional_str(params, "name"), description: optional_str(params, "description"), ..metis_persona::Meta::default() };

            // 保留原有元数据里表单没有覆盖的键。
            let store = metis_persona::PersonaStore::new(engine.paths().clone());
            if let Some(existing) = store.read(&id)? {
                meta.extra = existing.meta.extra;
                if meta.harness.is_empty() {
                    meta.harness = existing.meta.harness;
                }
                if meta.model.is_empty() {
                    meta.model = existing.meta.model;
                }
                if meta.order.is_none() {
                    meta.order = existing.meta.order;
                }
            }
            let path = store.save(&id, &meta, &body)?;
            Ok(CommandOutput::Message(format!("已保存人设 {id}（{}）", path.display())))
        }
    }

    /// 删除人设。
    #[derive(Debug)]
    pub struct RemovePersona;

    impl Command for RemovePersona {
        fn id(&self) -> &'static str {
            "persona.remove"
        }
        fn title(&self) -> &'static str {
            "persona.remove.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let id = PersonaId::parse(&required_str(params, "persona")?)?;
            let store = metis_persona::PersonaStore::new(engine.paths().clone());
            let removed = store.delete(&id)?;
            Ok(CommandOutput::Message(if removed { format!("已删除人设 {id}") } else { format!("人设 {id} 不存在") }))
        }
    }
}

/// 应用命令。
pub mod apply {
    use super::*;

    /// 应用当前配置。
    #[derive(Debug)]
    pub struct ApplyNow;

    impl Command for ApplyNow {
        fn id(&self) -> &'static str {
            "config.apply"
        }
        fn title(&self) -> &'static str {
            "config.apply.title"
        }
        fn effects(&self) -> Effects {
            Effects::writes()
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let options = crate::apply::ApplyOptions {
                only_failed: params.get("onlyFailed").and_then(|v| v.as_bool()).unwrap_or(false),
                persona: optional_str(params, "persona").map(|raw| PersonaId::parse(&raw)).transpose()?,
                mode: optional_str(params, "mode")
                    .map(|raw| {
                        Mode::parse(&raw).ok_or_else(|| {
                            MetisError::new(ErrorKind::InvalidArgument, format!("未知模式：{raw}")).with_remediation("可用：replace / strict / append / off")
                        })
                    })
                    .transpose()?,
                enabled: params.get("enabled").and_then(|v| v.as_bool()),
            };
            let report = crate::apply::apply(engine, &options)?;
            Ok(CommandOutput::Apply(Box::new(report)))
        }
    }
}

/// 执行器相关命令。
pub mod harness {
    use super::*;

    /// 列出执行器与能力。
    #[derive(Debug)]
    pub struct ListHarnesses;

    impl Command for ListHarnesses {
        fn id(&self) -> &'static str {
            "harness.list"
        }
        fn title(&self) -> &'static str {
            "harness.list.title"
        }
        fn run(&self, engine: &Engine, params: &Value) -> Result<CommandOutput> {
            let state = engine.state()?;
            let activation_state = state.activation_state()?;
            // GUI 的「显示本机没装的」需要一个"装没装"的判定；这里顺手带上，
            // 让界面不必自己再起一次探测（那会让首屏变慢、也让判定散落两处）。
            let detect = params.get("detect").and_then(|v| v.as_bool()).unwrap_or(false);
            let detected: Vec<metis_harness::Detected> = if detect {
                match dirs::home_dir() {
                    Some(user_home) => {
                        metis_harness::detect_all(|k| std::env::var(k).ok(), &user_home, &metis_harness::DetectOptions { probe_versions: false })
                    }
                    None => Vec::new(),
                }
            } else {
                Vec::new()
            };
            let items: Vec<Value> = metis_harness::registry::all()
                .iter()
                .map(|spec| {
                    let caps = metis_harness::registry::caps_of(spec);
                    let found = detected.iter().find(|d| d.harness.as_str() == spec.id);
                    let harness = HarnessId::parse(spec.id).ok();
                    let activation = harness.as_ref().map(|h| metis_core::activation::resolve_activation(&activation_state, h, &Default::default()));
                    serde_json::json!({
                        "id": spec.id,
                        "display": spec.display,
                        "injectable": caps.injectable,
                        "layer": caps.layer,
                        "hot": { "ok": caps.hot.ok, "how": caps.hot.how },
                        "channels": {
                            "shim": { "ok": caps.shim.ok, "why": caps.shim.why },
                            "install": { "ok": caps.install.ok, "why": caps.install.why },
                            "slash": { "ok": caps.slash.ok, "why": caps.slash.why },
                            "project": { "ok": caps.project.ok, "why": caps.project.why },
                        },
                        "armed": state.armed.get(spec.id).cloned().unwrap_or_default(),
                        "installed": found.map(|d| d.installed()),
                        "installedSummary": found.map(|d| d.summary()),
                        "activation": activation.map(|a| serde_json::json!({
                            "on": a.on,
                            "blockedBy": a.blocked_by.map(|b| b.as_str()),
                            "explain": a.explain(),
                        })),
                    })
                })
                .collect();
            Ok(CommandOutput::Json(serde_json::json!({ "harnesses": items })))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_paths::Paths;
    use serde_json::json;

    fn setup() -> (tempfile::TempDir, Engine, Commands) {
        let dir = tempfile::tempdir().expect("tempdir");
        let engine = Engine::new(Paths::with_home(dir.path().join("home")));
        (dir, engine, Commands::default_set())
    }

    #[test]
    fn every_command_has_a_unique_id() {
        let (_dir, _engine, commands) = setup();
        let ids = commands.ids();
        assert!(ids.len() >= 10, "命令表太小：{ids:?}");
        let mut sorted = ids.clone();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }

    #[test]
    fn unknown_command_lists_available_ones() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("nope", &engine, &json!({})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.remediation().unwrap_or_default().contains("state.status"));
    }

    #[test]
    fn status_reports_a_fresh_state() {
        let (_dir, engine, commands) = setup();
        let output = commands.run("state.status", &engine, &json!({})).expect("status");
        let json = output.to_json();
        assert_eq!(json["default"]["mode"], "replace");
        assert_eq!(json["activationEpoch"], "initial");
        assert!(json["statePath"].as_str().unwrap_or_default().contains("state.json"));
    }

    #[test]
    fn persona_use_requires_an_existing_persona() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("persona.use", &engine, &json!({"persona": "ghost"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument, "名字写错属于调用方传参问题");
    }

    #[test]
    fn persona_save_then_use_then_list() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "concise", "body": "正文", "name": "简洁"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "concise"})).expect("use");

        let output = commands.run("persona.list", &engine, &json!({})).expect("list");
        let json = output.to_json();
        assert_eq!(json["personas"][0]["id"], "concise");
        assert_eq!(json["personas"][0]["name"], "简洁");
        assert_eq!(json["personas"][0]["active"], true);
    }

    #[test]
    fn persona_save_preserves_unknown_metadata_from_disk() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "x", "body": "一"})).expect("save");
        // 手工加一个我们不认识的键（模拟别的工具写的）。
        let file = engine.paths().persona_file(&PersonaId::parse("x").expect("id"));
        let text = std::fs::read_to_string(&file).expect("read");
        std::fs::write(&file, text.replace("id: x\n", "id: x\ncustom: keep\n")).expect("write");

        commands.run("persona.save", &engine, &json!({"persona": "x", "body": "二"})).expect("save again");
        let after = std::fs::read_to_string(&file).expect("read");
        assert!(after.contains("custom: keep"), "未知元数据必须保留：{after}");
    }

    #[test]
    fn mode_command_rejects_unknown_values() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("state.mode", &engine, &json!({"mode": "bogus"})).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
        assert!(error.remediation().unwrap_or_default().contains("replace"));
    }

    #[test]
    fn mode_and_power_round_trip() {
        let (_dir, engine, commands) = setup();
        // 恢复（on）需要人设：先放一份，让"恢复注入"有东西可注入。
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");

        commands.run("state.mode", &engine, &json!({"mode": "append"})).expect("mode");
        commands.run("state.power", &engine, &json!({"on": false})).expect("off");
        let state = engine.state().expect("state");
        assert_eq!(state.default.mode, Mode::Off);
        assert_eq!(state.prev_mode, Some(Mode::Append));

        commands.run("state.power", &engine, &json!({"on": true})).expect("on");
        assert_eq!(engine.state().expect("state").default.mode, Mode::Append);
    }

    #[test]
    fn pausing_works_even_without_any_persona() {
        // 暂停的语义是"把静态内容撤下来"，没有人设时也必须能做。
        let (_dir, engine, commands) = setup();
        commands.run("state.power", &engine, &json!({"on": false})).expect("off");
        assert_eq!(engine.state().expect("state").default.mode, Mode::Off);
    }

    #[test]
    fn dsh_install_mounts_the_cordis_patch_block() {
        // DSH 光有 node_modules 里的文件不会生效：必须在 cordis.patch.yml 里点名。
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = metis_paths::Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harnesses"));
        let engine = Engine::new(paths);
        let commands = Commands::default_set();

        let dsh_home = engine.paths().harness_home(&HarnessId::parse("dsh").expect("id"), |_| None, std::path::Path::new("C:\\Users\\example")).expect("home");
        let profile = dsh_home.join("profiles").join("desktop");
        std::fs::create_dir_all(&profile).expect("mkdir");
        std::fs::write(profile.join("package.json"), "{}").expect("write");
        std::fs::write(profile.join("cordis.patch.yml"), "[]\n").expect("seed patch");

        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("channel.set", &engine, &json!({"harness": "dsh", "channels": ["install"]})).expect("arm");
        commands.run("config.apply", &engine, &json!({})).expect("apply");

        let patch = std::fs::read_to_string(profile.join("cordis.patch.yml")).expect("read");
        assert!(patch.contains("- insert:"), "必须挂上 insert 条目：{patch}");
        assert!(patch.contains("id: metis"), "必须在 patch 里点名插件：{patch}");
        assert!(!patch.trim_start().starts_with("[]"), "空 flow 数组必须被摊平：{patch}");

        commands.run("channel.set", &engine, &json!({"harness": "dsh", "channels": []})).expect("disarm");
        let after = std::fs::read_to_string(profile.join("cordis.patch.yml")).expect("read");
        assert!(!after.contains("metis"), "撤销后不该还挂着：{after}");
    }

    #[test]
    fn disarm_removes_per_profile_products() {
        // 回归：DSH 的落点是"每个 profile 一份"，撤销必须走 slash::revert。
        // 走 revert_block 会一个文件都删不掉，留下撤不掉的孤儿。
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = metis_paths::Paths::with_home(dir.path().join("home")).with_harness_root(dir.path().join("harnesses"));
        let engine = Engine::new(paths);
        let commands = Commands::default_set();

        // 造两个像模像样的 profile。
        let dsh_home = engine.paths().harness_home(&HarnessId::parse("dsh").expect("id"), |_| None, std::path::Path::new("C:\\Users\\example")).expect("home");
        for name in ["desktop", "web"] {
            let profile = dsh_home.join("profiles").join(name);
            std::fs::create_dir_all(&profile).expect("mkdir");
            std::fs::write(profile.join("package.json"), "{}").expect("write");
        }

        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        commands.run("channel.set", &engine, &json!({"harness": "dsh", "channels": ["install"]})).expect("arm");
        commands.run("config.apply", &engine, &json!({})).expect("apply");

        let product = dsh_home.join("profiles").join("desktop").join("node_modules").join("metis").join("index.mjs");
        assert!(product.is_file(), "apply 应该把插件写进 profile：{}", product.display());

        commands.run("channel.set", &engine, &json!({"harness": "dsh", "channels": []})).expect("disarm");
        assert!(!product.exists(), "撤销后插件必须被删除");
        assert!(!dsh_home.join("profiles").join("desktop").join("node_modules").join("metis").exists(), "我们建的空目录也该清掉");
    }

    #[test]
    fn channel_arm_requires_a_known_channel() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("channel.arm", &engine, &json!({"harness": "codex", "channel": "nope", "on": true})).expect_err("fail");
        assert_eq!(error.kind(), ErrorKind::UnknownChannel);
    }

    #[test]
    fn channel_set_replaces_the_whole_list() {
        let (_dir, engine, commands) = setup();
        commands.run("channel.set", &engine, &json!({"harness": "codex", "channels": ["install", "shim"]})).expect("set");
        let harness = HarnessId::parse("codex").expect("id");
        let state = engine.state().expect("state");
        assert_eq!(state.armed_for(&harness), &[Channel::Shim, Channel::Install]);
    }

    #[test]
    fn harness_list_covers_fifteen_and_includes_why() {
        let (_dir, engine, commands) = setup();
        let output = commands.run("harness.list", &engine, &json!({})).expect("list");
        let json = output.to_json();
        let items = json["harnesses"].as_array().expect("array");
        assert_eq!(items.len(), 15);
        let claude = items.iter().find(|h| h["id"] == "claude").expect("claude");
        assert_eq!(claude["channels"]["install"]["ok"], false);
        assert!(claude["channels"]["install"]["why"].as_str().unwrap_or_default().contains("压不住"));
    }

    #[test]
    fn apply_command_returns_a_report() {
        let (_dir, engine, commands) = setup();
        commands.run("persona.save", &engine, &json!({"persona": "p", "body": "正文"})).expect("save");
        commands.run("persona.use", &engine, &json!({"persona": "p"})).expect("use");
        let output = commands.run("config.apply", &engine, &json!({})).expect("apply");
        match output {
            CommandOutput::Apply(report) => {
                assert!(report.ok);
                assert!(!report.runtime_confirmed);
            }
            other => panic!("expected apply report, got {other:?}"),
        }
    }

    #[test]
    fn effects_are_declared_for_writing_commands() {
        let (_dir, _engine, commands) = setup();
        for id in ["persona.use", "state.mode", "state.power", "channel.arm", "persona.save", "config.apply"] {
            let effects = commands.effects(id).expect("effects");
            assert!(effects.writes_disk, "{id} 会写盘，必须声明");
        }
        let read_only = commands.effects("state.status").expect("effects");
        assert!(!read_only.writes_disk, "读命令不该声明写盘");
    }

    #[test]
    fn params_must_be_an_object() {
        let (_dir, engine, commands) = setup();
        let error = commands.run("persona.use", &engine, &json!("not an object")).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::InvalidArgument);
    }
}
