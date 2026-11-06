//! Metis 引擎：状态、激活、应用编排、命令注册表。
//!
//! 铁律 6（Headless 优先）：这一层不依赖任何 UI 类型。CLI 与 GUI 都以它为准。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod apply;
pub mod armor;
pub mod backups;
pub mod clean;
pub mod command;
pub mod commands_armor;
pub mod commands_backups;
pub mod commands_import;
pub mod commands_maintenance;
pub mod commands_probe;
pub mod commands_project;
pub mod commands_update;
pub mod commands_verify;
pub mod import;
pub mod journal;
pub mod persona_import;
pub mod state;
pub mod tarball;
pub mod update;
pub mod verify;

pub use apply::{ApplyReport, ChannelResult, ResultStatus};
pub use clean::{CleanReport, CleanScope, clean};
pub use command::{Command, CommandOutput, Commands};
pub use state::{RuntimeState, StateFile, resolve};

use metis_paths::Paths;

/// 引擎上下文：一切副作用都从这里出发。
#[derive(Debug, Clone)]
pub struct Engine {
    paths: Paths,
}

impl Engine {
    /// 用给定的路径集合建引擎（测试注入临时目录）。
    pub fn new(paths: Paths) -> Self {
        Self { paths }
    }

    /// 路径集合。
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// 读状态。文件不存在 → 默认状态。
    pub fn state(&self) -> metis_core::Result<StateFile> {
        StateFile::load(&self.paths)
    }

    /// 在控制锁里修改状态。
    pub fn update_state<T>(&self, update: impl FnOnce(&mut StateFile) -> metis_core::Result<T>) -> metis_core::Result<T> {
        metis_fs::lock::with_lock(self.paths.home(), || {
            let mut state = StateFile::load(&self.paths)?;
            let result = update(&mut state)?;
            state.save(&self.paths)?;
            Ok(result)
        })
    }

    /// 按 id 跑一个命令（界面与后台任务用）。
    ///
    /// 界面不自己拼业务逻辑：所有按钮最终都走这里，和 CLI 是同一条路。
    pub fn run_command(&self, id: &str, params: &serde_json::Value) -> metis_core::Result<CommandOutput> {
        Commands::default_set().run(id, self, params)
    }
}
