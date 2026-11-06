//! `state.json`：唯一配置。
//!
//! 铁律（`docs/DATA-FORMAT.md` §2）：
//! - 原子写、`0600`、末尾换行；
//! - **损坏即停止**，原字节一个都不改；
//! - **未知字段保留**：`load → modify → save` 必须把读到的未知键原样带回去。

use std::collections::BTreeMap;

use metis_core::{Activation, ActivationContext, ActivationState, Channel, ErrorKind, HarnessId, MetisError, Mode, Override, PersonaId, Result, activation};
use metis_paths::Paths;
use serde::{Deserialize, Serialize};

/// 当前格式版本。
pub const STATE_VERSION: u32 = 1;

/// `default` 字段：裸敲命令时启动谁，同时 `mode` 是总电源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    /// 默认执行器。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub harness: Option<String>,
    /// 全局模式（总电源）。
    #[serde(default = "default_mode")]
    pub mode: Mode,
}

fn default_mode() -> Mode {
    Mode::DEFAULT
}

impl Default for Defaults {
    fn default() -> Self {
        Self { harness: None, mode: Mode::DEFAULT }
    }
}

/// `state.json` 的内存形态。
///
/// 未知字段进 `extra`，写回时原样输出（保证不破坏新版本写的配置）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateFile {
    /// 格式版本。
    #[serde(default = "default_version")]
    pub v: u32,
    /// 全局人设。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persona: Option<String>,
    /// 默认执行器与总电源。
    #[serde(default)]
    pub default: Defaults,
    /// 每家武装了哪些通道。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub armed: BTreeMap<String, Vec<Channel>>,
    /// 分家例外。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, Override>,
    /// 已登记的项目目录。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", rename = "projectDirs")]
    pub project_dirs: BTreeMap<String, Vec<String>>,
    /// 整体停用的家。
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "disabledHarnesses")]
    pub disabled_harnesses: Vec<String>,
    /// 暂停期间选择但未落盘的家。
    #[serde(default, skip_serializing_if = "Vec::is_empty", rename = "pausedInstalls")]
    pub paused_installs: Vec<String>,
    /// 恢复时回到哪个模式。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "prevMode")]
    pub prev_mode: Option<Mode>,
    /// 全局 off→on 时重新生成。
    #[serde(default = "initial_epoch", rename = "activationEpoch")]
    pub activation_epoch: String,
    /// 单家 off→on 时重新生成。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty", rename = "harnessEpochs")]
    pub harness_epochs: BTreeMap<String, String>,
    /// DSH 要处理的 profile 白名单。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "dshProfiles")]
    pub dsh_profiles: Option<Vec<String>>,
    /// `piAuto` / `ompAuto`。
    #[serde(default, skip_serializing_if = "is_false", rename = "piAuto")]
    pub pi_auto: bool,
    /// `ompAuto`。
    #[serde(default, skip_serializing_if = "is_false", rename = "ompAuto")]
    pub omp_auto: bool,
    /// 最近一次应用报告（原样保存，供诊断页显示）。
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "lastApply")]
    pub last_apply: Option<serde_json::Value>,
    /// 未知字段：原样保留。
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

fn default_version() -> u32 {
    STATE_VERSION
}

fn initial_epoch() -> String {
    // 缺省值是字符串 "initial"（不是空值）：它表达"还没有发生过 off→on"。
    "initial".to_owned()
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Default for StateFile {
    fn default() -> Self {
        Self {
            v: STATE_VERSION,
            persona: None,
            default: Defaults::default(),
            armed: BTreeMap::new(),
            overrides: BTreeMap::new(),
            project_dirs: BTreeMap::new(),
            disabled_harnesses: Vec::new(),
            paused_installs: Vec::new(),
            prev_mode: None,
            activation_epoch: initial_epoch(),
            harness_epochs: BTreeMap::new(),
            dsh_profiles: None,
            pi_auto: false,
            omp_auto: false,
            last_apply: None,
            extra: BTreeMap::new(),
        }
    }
}

impl StateFile {
    /// 读状态。文件不存在 → 默认值；损坏 → 错误且**原字节不动**。
    pub fn load(paths: &Paths) -> Result<Self> {
        let file = paths.state_file();
        let Some(bytes) = metis_fs::read_optional(&file)? else { return Ok(Self::default()) };
        match serde_json::from_slice::<Self>(&bytes) {
            Ok(state) => Ok(state),
            Err(error) => Err(MetisError::new(ErrorKind::StateCorrupt, format!("state.json 损坏，原文件已保留：{}", file.display()))
                .with_detail(error.to_string())
                .with_remediation("备份该文件后删除它，Metis 会重新生成默认配置")),
        }
    }

    /// 写状态（原子、`0600`、末尾换行）。
    pub fn save(&self, paths: &Paths) -> Result<()> {
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|error| MetisError::new(ErrorKind::Internal, "state 无法序列化").with_detail(error.to_string()))?;
        bytes.push(b'\n');
        metis_fs::write_atomic(&paths.state_file(), &bytes)
    }

    /// 解析出激活判定用的输入。
    pub fn activation_state(&self) -> Result<ActivationState> {
        let mut overrides = Vec::new();
        for (id, over) in &self.overrides {
            overrides.push((HarnessId::parse(id)?, over.clone()));
        }
        let mut armed = Vec::new();
        for (id, channels) in &self.armed {
            armed.push((HarnessId::parse(id)?, channels.clone()));
        }
        let mut disabled = Vec::new();
        for id in &self.disabled_harnesses {
            disabled.push(HarnessId::parse(id)?);
        }
        let mut auto_inject = Vec::new();
        if self.pi_auto {
            auto_inject.push((HarnessId::parse("pi")?, true));
        }
        if self.omp_auto {
            auto_inject.push((HarnessId::parse("omp")?, true));
        }
        Ok(ActivationState {
            persona: match &self.persona {
                Some(raw) => Some(PersonaId::parse(raw)?),
                None => None,
            },
            default_mode: self.default.mode,
            overrides,
            armed,
            disabled_harnesses: disabled,
            dsh_profiles: self.dsh_profiles.clone(),
            auto_inject,
        })
    }

    /// 设置总电源。关闭时记下 `prev_mode`，恢复时回到它。
    pub fn set_power(&mut self, on: bool) {
        if on {
            if self.default.mode == Mode::Off {
                let restore = self.prev_mode.filter(|m| m.injects()).unwrap_or(Mode::DEFAULT);
                self.default.mode = restore;
                self.activation_epoch = new_epoch();
            }
        } else if self.default.mode != Mode::Off {
            self.prev_mode = Some(self.default.mode);
            self.default.mode = Mode::Off;
            self.activation_epoch = new_epoch();
        }
    }

    /// 设某家的模式例外（`None` 表示恢复跟随全局）。
    pub fn set_override_mode(&mut self, harness: &HarnessId, mode: Option<Mode>) -> Result<()> {
        let was_off = self.overrides.get(harness.as_str()).and_then(|o| o.mode) == Some(Mode::Off);
        let entry = self.overrides.entry(harness.as_str().to_owned()).or_default();
        entry.mode = mode;
        if entry.is_empty() {
            self.overrides.remove(harness.as_str());
        }
        // off → 非 off 时重新生成 epoch，防止旧插件复活。
        if was_off && mode != Some(Mode::Off) {
            self.harness_epochs.insert(harness.as_str().to_owned(), new_epoch());
        }
        Ok(())
    }

    /// 设某家的人设例外。
    pub fn set_override_persona(&mut self, harness: &HarnessId, persona: Option<PersonaId>) {
        let entry = self.overrides.entry(harness.as_str().to_owned()).or_default();
        entry.persona = persona;
        if entry.is_empty() {
            self.overrides.remove(harness.as_str());
        }
    }

    /// 武装 / 解除一条通道。数组按 [`Channel::ALL`] 的规范顺序写，保证字节稳定。
    pub fn set_armed(&mut self, harness: &HarnessId, channel: Channel, on: bool) {
        let entry = self.armed.entry(harness.as_str().to_owned()).or_default();
        if on {
            if !entry.contains(&channel) {
                entry.push(channel);
            }
        } else {
            entry.retain(|c| *c != channel);
        }
        entry.sort();
        entry.dedup();
        if entry.is_empty() {
            self.armed.remove(harness.as_str());
        }
    }

    /// 某家武装了哪些通道。
    pub fn armed_for(&self, harness: &HarnessId) -> &[Channel] {
        self.armed.get(harness.as_str()).map(|c| c.as_slice()).unwrap_or(&[])
    }

    /// 解析某家的激活状态。
    pub fn activation_for(&self, harness: &HarnessId, ctx: &ActivationContext) -> Result<Activation> {
        let state = self.activation_state()?;
        Ok(activation::resolve_activation(&state, harness, ctx))
    }
}

fn new_epoch() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// 运行时状态：把 `state.json` 与"本机装了什么"合并后的只读视图。
#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeState {
    /// 磁盘状态。
    pub state: StateFile,
    /// 每家的激活结果。
    pub activations: Vec<(HarnessId, Activation)>,
}

/// 解析全部执行器的激活状态（显式启动、非通道来源）。
pub fn resolve(engine: &Engine, ctx: &ActivationContext) -> Result<RuntimeState> {
    let state = engine.state()?;
    let activation_state = state.activation_state()?;
    let mut activations = Vec::new();
    for spec in metis_harness::registry::all() {
        let harness = HarnessId::parse(spec.id)?;
        let activation = activation::resolve_activation(&activation_state, &harness, ctx);
        activations.push((harness, activation));
    }
    Ok(RuntimeState { state, activations })
}

use crate::Engine;

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> (tempfile::TempDir, Paths) {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = Paths::with_home(dir.path().join("home"));
        (dir, paths)
    }

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("id")
    }

    #[test]
    fn missing_state_is_default() {
        let (_dir, paths) = paths();
        let state = StateFile::load(&paths).expect("load");
        assert_eq!(state.v, STATE_VERSION);
        assert_eq!(state.default.mode, Mode::Replace);
        assert_eq!(state.activation_epoch, "initial", "缺省 epoch 必须是字面量 initial");
    }

    #[test]
    fn save_then_load_round_trips() {
        let (_dir, paths) = paths();
        let mut state = StateFile { persona: Some("concise".to_owned()), ..StateFile::default() };
        state.set_armed(&id("codex"), Channel::Install, true);
        state.save(&paths).expect("save");

        let back = StateFile::load(&paths).expect("load");
        assert_eq!(back.persona.as_deref(), Some("concise"));
        assert_eq!(back.armed_for(&id("codex")), &[Channel::Install]);
    }

    #[test]
    fn saved_bytes_end_with_newline_and_are_atomic() {
        let (_dir, paths) = paths();
        StateFile::default().save(&paths).expect("save");
        let bytes = std::fs::read(paths.state_file()).expect("read");
        assert_eq!(bytes.last(), Some(&b'\n'));
        let text = String::from_utf8(bytes).expect("utf8");
        assert!(text.contains("\"v\": 1"));
    }

    #[test]
    fn corrupt_state_preserves_original_bytes() {
        let (_dir, paths) = paths();
        metis_fs::ensure_dir(paths.home()).expect("mkdir");
        std::fs::write(paths.state_file(), b"{ broken json").expect("seed");
        let before = std::fs::read(paths.state_file()).expect("read");

        let error = StateFile::load(&paths).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::StateCorrupt);
        assert!(error.remediation().is_some());
        assert_eq!(std::fs::read(paths.state_file()).expect("reread"), before);
    }

    #[test]
    fn unknown_fields_survive_a_load_modify_save_cycle() {
        let (_dir, paths) = paths();
        metis_fs::ensure_dir(paths.home()).expect("mkdir");
        std::fs::write(paths.state_file(), br#"{"v":1,"persona":"concise","futureField":{"nested":[1,2,3]},"anotherOne":true}"#).expect("seed");

        let mut state = StateFile::load(&paths).expect("load");
        state.persona = Some("other".to_owned());
        state.save(&paths).expect("save");

        let text = std::fs::read_to_string(paths.state_file()).expect("read");
        assert!(text.contains("futureField"), "未知字段必须保留：{text}");
        assert!(text.contains("anotherOne"));
        let parsed: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(parsed["futureField"]["nested"][2], 3);
    }

    #[test]
    fn power_off_records_prev_mode_and_restores_it() {
        let mut state = StateFile::default();
        state.default.mode = Mode::Append;
        let epoch_before = state.activation_epoch.clone();

        state.set_power(false);
        assert_eq!(state.default.mode, Mode::Off);
        assert_eq!(state.prev_mode, Some(Mode::Append));
        assert_ne!(state.activation_epoch, epoch_before, "关闭必须换 epoch");

        state.set_power(true);
        assert_eq!(state.default.mode, Mode::Append, "恢复应回到打开前的模式");
    }

    #[test]
    fn harness_off_then_on_regenerates_its_epoch() {
        let mut state = StateFile::default();
        state.set_override_mode(&id("codex"), Some(Mode::Off)).expect("off");
        let epoch = state.harness_epochs.get("codex").cloned();
        state.set_override_mode(&id("codex"), Some(Mode::Replace)).expect("on");
        let new = state.harness_epochs.get("codex").cloned();
        assert!(new.is_some(), "off→on 必须生成 epoch");
        if epoch.is_some() {
            assert_ne!(epoch, new);
        }
    }

    #[test]
    fn empty_override_is_dropped() {
        let mut state = StateFile::default();
        state.set_override_mode(&id("codex"), Some(Mode::Append)).expect("set");
        assert!(state.overrides.contains_key("codex"));
        state.set_override_mode(&id("codex"), None).expect("clear");
        assert!(!state.overrides.contains_key("codex"), "例外清空后不该留空对象");
    }

    #[test]
    fn armed_order_is_canonical() {
        let mut state = StateFile::default();
        state.set_armed(&id("codex"), Channel::Project, true);
        state.set_armed(&id("codex"), Channel::Install, true);
        state.set_armed(&id("codex"), Channel::Shim, true);
        // Channel 的 Ord 由枚举定义顺序决定：Shim < Slash < Install < Project
        assert_eq!(state.armed_for(&id("codex")), &[Channel::Shim, Channel::Install, Channel::Project]);
    }

    #[test]
    fn disarming_the_last_channel_drops_the_entry() {
        let mut state = StateFile::default();
        state.set_armed(&id("codex"), Channel::Install, true);
        state.set_armed(&id("codex"), Channel::Install, false);
        assert!(!state.armed.contains_key("codex"));
    }

    #[test]
    fn armed_assignment_is_idempotent() {
        let mut state = StateFile::default();
        state.set_armed(&id("codex"), Channel::Install, true);
        state.set_armed(&id("codex"), Channel::Install, true);
        assert_eq!(state.armed_for(&id("codex")).len(), 1);
    }

    #[test]
    fn activation_for_respects_global_pause() {
        let mut state = StateFile::default();
        state.set_power(false);
        let activation = state.activation_for(&id("codex"), &ActivationContext { launch: true, ..ActivationContext::default() }).expect("resolve");
        assert!(!activation.on);
        assert_eq!(activation.blocked_by, Some(metis_core::BlockReason::GlobalPause));
    }

    #[test]
    fn resolve_covers_every_registered_harness() {
        let (_dir, paths) = paths();
        let engine = Engine::new(paths);
        let runtime = resolve(&engine, &ActivationContext::default()).expect("resolve");
        assert_eq!(runtime.activations.len(), 15);
        for (harness, activation) in &runtime.activations {
            assert!(!activation.on, "{harness} 在没有许可时不该是 on");
        }
    }

    #[test]
    fn engine_update_state_writes_through_the_lock() {
        let (_dir, paths) = paths();
        let engine = Engine::new(paths.clone());
        engine
            .update_state(|state| {
                state.persona = Some("concise".to_owned());
                Ok(())
            })
            .expect("update");
        assert_eq!(engine.state().expect("state").persona.as_deref(), Some("concise"));
        assert!(!paths.lock_file().exists(), "更新完必须释放锁");
    }

    #[test]
    fn engine_update_state_propagates_errors_without_writing() {
        let (_dir, paths) = paths();
        let engine = Engine::new(paths.clone());
        let result: Result<()> = engine.update_state(|state| {
            state.persona = Some("should-not-persist".to_owned());
            Err(MetisError::new(ErrorKind::Internal, "boom"))
        });
        assert!(result.is_err());
        assert!(engine.state().expect("state").persona.is_none(), "闭包失败时不该写盘");
    }
}
