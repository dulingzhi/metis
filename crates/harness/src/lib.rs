//! 执行器适配层。
//!
//! 一家的全部事实都在这里：怎么探测、落在哪、能开哪些通道、怎么启动、能不能热切。
//! 新增一家 = 加一个 [`Harness`] 实现并在 [`registry`] 里登记一行（AGENTS.md「扩展走接缝」）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

pub mod caps;
pub mod detect;
pub mod launch;
pub mod launch_spec;
pub mod registry;
pub mod spec;

pub use caps::{Capabilities, HotSwitch, HotSwitchKind};
pub use detect::{DetectOptions, Detected, detect, detect_all};
pub use launch::{LaunchPlan, OneshootPlan};
pub use launch_spec::{STRICT_REPLACE, oneshot, plan, replace_downgrade_note};
pub use registry::{Harness, all, find};
pub use spec::{Layer, PersistKind, PersistTarget, Probe, Spec};
