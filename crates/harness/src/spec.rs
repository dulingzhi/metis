//! 一家的静态事实：探测规则、落点、层级、启动参数形状。
//!
//! 这些是**数据**，不是 `if` 链（AGENTS.md「数据优先于代码」）。

use std::path::PathBuf;

use metis_core::Channel;

use crate::caps::HotSwitch;

/// 落点所在层。**只有 `Sys` 算可用能力**（`HARNESS-MATRIX.md` §0）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// 真的进系统提示词。
    Sys,
    /// 只进上下文块 / 用户消息，压不住身份。
    User,
}

impl Layer {
    /// 能不能算作"能改身份"的能力。
    pub fn is_authoritative(self) -> bool {
        self == Self::Sys
    }

    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sys => "sys",
            Self::User => "user",
        }
    }
}

/// 探测规则。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// PATH 里的可执行名（可能多个）。
    pub bins: Vec<&'static str>,
    /// 主目录环境变量。
    pub home_env: &'static str,
    /// 主目录默认相对路径。
    pub home_relative: &'static str,
    /// 标记文件（相对主目录）。
    pub markers: Vec<&'static str>,
    /// 版本命令参数。
    pub version_args: Option<Vec<&'static str>>,
    /// 只有桌面版、没有命令行入口。
    pub desktop_only: bool,
}

/// 常驻落点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersistTarget {
    /// 相对主目录的文件路径。
    pub relative: &'static str,
    /// 层级。
    pub layer: Layer,
    /// 写入方式。
    pub kind: PersistKind,
    /// 给用户看的一句话说明。
    pub note: &'static str,
}

/// 往文件里写的方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistKind {
    /// 在既有文件里插入带标记的块。
    ManagedBlock,
    /// 整文件替换（回滚凭据完整时才允许）。
    WholeFile,
    /// 改结构化文件的某个字段。
    Field,
    /// 按目录展开：落点是**每个子目录一个插件包**（DSH 的每个 profile 各一份）。
    ///
    /// 这类落点不能用"一个文件"来表达：目录名是运行期枚举出来的，不是固定的。
    PerProfileDir,
}

/// 一家执行器的全部静态事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// id。
    pub id: &'static str,
    /// 界面显示名。
    pub display: &'static str,
    /// 探测规则。
    pub probe: Probe,
    /// 常驻落点（`None` 表示没有可写配置）。
    pub persist: Option<PersistTarget>,
    /// 会话内 `/purge` 扩展落点。
    pub slash_file: Option<&'static str>,
    /// 项目规则文件名。
    pub project_file: Option<&'static str>,
    /// 热切能力。
    pub hot: HotSwitch,
    /// 支持哪些通道（静态判定；运行时还要看本机装没装）。
    pub channels: Vec<Channel>,
}

impl Spec {
    /// 主目录。
    pub fn home(&self, env: impl Fn(&str) -> Option<String>, user_home: &std::path::Path) -> Option<PathBuf> {
        let layout = metis_paths::HarnessLayout::of(&metis_core::HarnessId::parse(self.id).ok()?)?;
        Some(layout.resolve(env, user_home))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_core::HarnessId;

    #[test]
    fn only_sys_is_authoritative() {
        // 这条守着一个产品判断：user 层落点压不住身份，不能当能力提供。
        assert!(Layer::Sys.is_authoritative());
        assert!(!Layer::User.is_authoritative());
    }

    #[test]
    fn layer_strings_are_stable() {
        assert_eq!(Layer::Sys.as_str(), "sys");
        assert_eq!(Layer::User.as_str(), "user");
        // 与 `docs/HARNESS-MATRIX.md` 的列名一致。
        assert_ne!(Layer::Sys.as_str(), Layer::User.as_str());
    }

    #[test]
    fn spec_home_uses_the_layout_table() {
        let spec = crate::registry::find("codex").expect("codex");
        let home = spec.home(|_| None, std::path::Path::new("C:/Users/example")).expect("home");
        assert!(home.ends_with(".codex"), "{}", home.display());
    }

    #[test]
    fn spec_home_honours_the_env_override() {
        let spec = crate::registry::find("codex").expect("codex");
        let home = spec.home(|key| (key == "CODEX_HOME").then(|| "D:/codex".to_owned()), std::path::Path::new("C:/Users/example")).expect("home");
        assert_eq!(home, std::path::Path::new("D:/codex"));
    }

    #[test]
    fn spec_home_is_none_for_an_unknown_id() {
        // 造一个不在布局表里的 Spec：`home()` 必须返回 None 而不是 panic。
        use metis_core::{Channel, ChannelState};
        let _ = ChannelState::available();
        let synthetic = Spec {
            id: "definitely-not-a-real-harness-xyz",
            display: "X",
            probe: Probe { bins: vec![], home_env: "X_HOME", home_relative: ".x", markers: vec![], version_args: None, desktop_only: false },
            persist: None,
            slash_file: None,
            project_file: None,
            hot: crate::caps::HotSwitch::no("n/a"),
            channels: vec![Channel::Shim],
        };
        assert!(synthetic.home(|_| None, std::path::Path::new("C:/")).is_none());
        assert!(HarnessId::parse(synthetic.id).is_ok(), "id 本身是合法的，只是不在布局表里");
    }

    #[test]
    fn every_registry_spec_resolves_a_home() {
        // 15 家都必须能在布局表里找到主目录，否则 `apply` 会静默失败。
        for spec in crate::registry::all() {
            assert!(spec.home(|_| None, std::path::Path::new("C:/Users/example")).is_some(), "{} 没有主目录", spec.id);
        }
    }
}
