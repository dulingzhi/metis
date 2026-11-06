//! 注册表：15 家的静态事实全在这里，一处登记、别处只读。
//!
//! 对应 `docs/HARNESS-MATRIX.md`。改这里必须同步那张表，反之亦然（`xtask ci` 会核对）。

use metis_core::{Channel, ChannelState};

use crate::caps::{Capabilities, HotSwitch, HotSwitchKind};
use crate::spec::{Layer, PersistKind, PersistTarget, Probe, Spec};

pub use crate::spec::Spec as SpecExport;

/// 一家执行器的行为接口。实现放 [`all`] 里登记。
pub trait Harness: Send + Sync {
    /// 静态事实。
    fn spec(&self) -> &'static Spec;
}

/// 全部 15 家。顺序与 `docs/HARNESS-MATRIX.md` 的总览表一致。
///
/// 用 `LazyLock` 而不是 `static`：表里含 `Vec`，无法在常量上下文里构造。
static SPECS: std::sync::LazyLock<Vec<Spec>> = std::sync::LazyLock::new(build_specs);

fn build_specs() -> Vec<Spec> {
    vec![
        Spec {
            id: "pi",
            display: "Pi",
            probe: Probe {
                bins: vec!["pi"],
                home_env: "PI_HOME",
                home_relative: ".pi/agent",
                markers: vec!["extensions"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "AGENTS.md",
                layer: Layer::Sys,
                kind: PersistKind::ManagedBlock,
                note: "每次启动重读；正文是快照，换人设要重装",
            }),
            slash_file: Some("extensions/metis.ts"),
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::yes(HotSwitchKind::Command, Channel::Slash, "扩展钩子每轮改写 systemPrompt：/purge 或改设置，下一轮生效"),
            channels: vec![Channel::Shim, Channel::Slash, Channel::Install, Channel::Project],
        },
        Spec {
            id: "pi-desktop",
            display: "PI-Desktop",
            probe: Probe {
                bins: vec![],
                home_env: "PI_DESKTOP_DATA_DIR",
                home_relative: ".pi-desktop",
                markers: vec!["plugins", "pi.sqlite", "secrets"],
                version_args: None,
                desktop_only: true,
            },
            persist: Some(PersistTarget {
                relative: "SYSTEM.md",
                layer: Layer::Sys,
                kind: PersistKind::ManagedBlock,
                note: "与 pi CLI 共用 ~/.pi/agent，装了两边都变",
            }),
            slash_file: Some("plugins/imported/metis/main.cjs"),
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::yes(HotSwitchKind::Command, Channel::Slash, "imported 插件每轮读 state 并改写 systemPrompt（首次安装需重启一次）"),
            channels: vec![Channel::Slash, Channel::Install, Channel::Project],
        },
        Spec {
            id: "omp",
            display: "Oh My Pi",
            probe: Probe {
                bins: vec!["omp"],
                home_env: "OMP_HOME",
                home_relative: ".omp",
                markers: vec!["agent/config.yml"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "agent/AGENTS.md",
                layer: Layer::Sys,
                kind: PersistKind::ManagedBlock,
                note: "就是它的 rules 文件（--no-rules 关的就是这个）",
            }),
            slash_file: Some("agent/extensions/metis.ts"),
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::yes(HotSwitchKind::Command, Channel::Slash, "扩展钩子每轮改写 systemPrompt 数组：/purge 或改设置，下一轮生效"),
            channels: vec![Channel::Shim, Channel::Slash, Channel::Install, Channel::Project],
        },
        Spec {
            id: "opencode",
            display: "OpenCode",
            probe: Probe {
                bins: vec!["opencode"],
                home_env: "XDG_CONFIG_HOME",
                home_relative: ".config/opencode",
                markers: vec!["opencode.json", "plugins"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "plugins/metis.ts",
                layer: Layer::Sys,
                kind: PersistKind::WholeFile,
                note: "插件改 chat.system；每轮读 state 与 active.md，配置改完下一轮生效",
            }),
            slash_file: None,
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::yes(HotSwitchKind::State, Channel::Install, "插件钩子每轮读 state.json 与 active.md：改设置下一轮生效"),
            channels: vec![Channel::Shim, Channel::Install, Channel::Project],
        },
        Spec {
            id: "opencode2",
            display: "OpenCode 2",
            probe: Probe {
                bins: vec!["opencode2"],
                home_env: "XDG_CONFIG_HOME",
                home_relative: ".config/opencode",
                markers: vec!["opencode.json", "plugins"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "plugins/metis-v2.ts",
                layer: Layer::Sys,
                kind: PersistKind::WholeFile,
                note: "v2 插件 API 与 v1 不同，不能混用",
            }),
            slash_file: None,
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::yes(HotSwitchKind::State, Channel::Install, "v2 插件钩子每轮读 state：改设置下一轮生效"),
            channels: vec![Channel::Shim, Channel::Install, Channel::Project],
        },
        Spec {
            id: "dsh",
            display: "DeepSeek Harness",
            probe: Probe {
                bins: vec!["dsh"],
                home_env: "DSH_HOME",
                home_relative: ".dsh",
                markers: vec!["profiles"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "profiles",
                layer: Layer::Sys,
                kind: PersistKind::PerProfileDir,
                note: "cordis 插件包；装进每个已经存在的 profile（profiles/<name>/node_modules/metis）",
            }),
            slash_file: Some("plugins"),
            project_file: None,
            hot: HotSwitch::yes(HotSwitchKind::Command, Channel::Slash, "cordis 插件注册 /purge：改 state 下一句生效，不产生模型消息"),
            channels: vec![Channel::Slash, Channel::Install],
        },
        Spec {
            id: "codex",
            display: "Codex",
            probe: Probe {
                bins: vec!["codex"],
                home_env: "CODEX_HOME",
                home_relative: ".codex",
                markers: vec!["config.toml"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "config.toml",
                layer: Layer::Sys,
                kind: PersistKind::Field,
                note: "顶层键 model_instructions_file，必须写在第一个 [table] 之前；实际请求角色是 developer",
            }),
            slash_file: None,
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::no("config.toml 启动时读；会话内没有能重拼 system 的钩子"),
            channels: vec![Channel::Shim, Channel::Install, Channel::Project],
        },
        Spec {
            id: "claude",
            display: "Claude Code",
            probe: Probe {
                bins: vec!["claude"],
                home_env: "CLAUDE_CONFIG_DIR",
                home_relative: ".claude",
                markers: vec!["settings.json"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "CLAUDE.md",
                layer: Layer::User,
                kind: PersistKind::ManagedBlock,
                note: "进的是 <system-reminder> 上下文块（用户消息），压不住系统提示词",
            }),
            slash_file: None,
            project_file: Some("CLAUDE.md"),
            hot: HotSwitch::no("system 只在启动时拼一次；会话内只能往 user 消息里塞（实测无效）"),
            channels: vec![Channel::Shim, Channel::Project],
        },
        Spec {
            id: "grok",
            display: "Grok CLI",
            probe: Probe {
                bins: vec!["grok"],
                home_env: "GROK_HOME",
                home_relative: ".grok",
                markers: vec!["config.toml"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "AGENTS.md",
                layer: Layer::Sys,
                kind: PersistKind::ManagedBlock,
                note: "内容 append 进系统提示；单文件上限 10000 字符，超了会自动截断",
            }),
            slash_file: None,
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::no("AGENTS.md 启动时读一次"),
            channels: vec![Channel::Shim, Channel::Install, Channel::Project],
        },
        Spec {
            id: "codebuddy",
            display: "CodeBuddy",
            probe: Probe {
                bins: vec!["codebuddy"],
                home_env: "CODEBUDDY_HOME",
                home_relative: ".codebuddy",
                markers: vec!["history.jsonl"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: None,
            slash_file: None,
            project_file: None,
            hot: HotSwitch::no("只有启动参数"),
            channels: vec![Channel::Shim],
        },
        Spec {
            id: "hermes",
            display: "Hermes",
            probe: Probe {
                bins: vec!["hermes"],
                home_env: "HERMES_HOME",
                home_relative: ".hermes",
                markers: vec!["hermes-agent"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "SOUL.md",
                layer: Layer::Sys,
                kind: PersistKind::ManagedBlock,
                note: "身份槽位 #1，新会话才读（续会话沿用 session DB 的提示词）",
            }),
            slash_file: None,
            project_file: None,
            hot: HotSwitch::no("SOUL.md 只在会话首次/重建时读；续会话沿用 session DB 里的提示词"),
            channels: vec![Channel::Shim, Channel::Install],
        },
        Spec {
            id: "kilo",
            display: "Kilo Code",
            probe: Probe {
                bins: vec!["kilo", "kilocode"],
                home_env: "KILO_CONFIG_DIR",
                home_relative: ".config/kilo",
                markers: vec!["agent"],
                version_args: Some(vec!["--version"]),
                desktop_only: false,
            },
            persist: Some(PersistTarget {
                relative: "agent/code.md",
                layer: Layer::Sys,
                kind: PersistKind::WholeFile,
                note: "code agent Markdown（replace/strict）；AGENTS.md 托管块用于 append",
            }),
            slash_file: None,
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::no("code agent prompt 启动时读；AGENTS.md 会话启动时读"),
            channels: vec![Channel::Shim, Channel::Install, Channel::Project],
        },
        Spec {
            id: "workbuddy",
            display: "WorkBuddy",
            probe: Probe {
                bins: vec!["WorkBuddy", "workbuddy"],
                home_env: "WORKBUDDY_CONFIG_DIR",
                home_relative: ".workbuddy",
                markers: vec!["WorkBuddy.exe", "resources/app.asar"],
                version_args: None,
                desktop_only: true,
            },
            persist: Some(PersistTarget {
                relative: "settings.json",
                layer: Layer::Sys,
                kind: PersistKind::Field,
                note: "personalization.customPrompt（身份上下文），可逆",
            }),
            slash_file: None,
            project_file: None,
            hot: HotSwitch::no("桌面客户端，配置启动时读"),
            channels: vec![Channel::Install],
        },
        Spec {
            id: "zcode",
            display: "ZCode",
            probe: Probe {
                bins: vec!["zcode"],
                home_env: "ZCODE_HOME",
                home_relative: ".zcode",
                markers: vec!["v2/provider_config.json"],
                version_args: Some(vec!["--version"]),
                desktop_only: true,
            },
            persist: Some(PersistTarget {
                relative: "AGENTS.md",
                layer: Layer::Sys,
                kind: PersistKind::ManagedBlock,
                note: "官方文档写的默认 instructions 文件；桌面版闭源，无法抓包复核",
            }),
            slash_file: None,
            project_file: Some("AGENTS.md"),
            hot: HotSwitch::no("桌面版闭源，没有插件与命令钩子"),
            channels: vec![Channel::Install, Channel::Project],
        },
        Spec {
            id: "antigravity",
            display: "Antigravity",
            probe: Probe {
                bins: vec!["antigravity"],
                home_env: "ANTIGRAVITY_HOME",
                home_relative: "Programs/antigravity",
                markers: vec!["resources/bin/language_server.exe"],
                version_args: None,
                desktop_only: true,
            },
            persist: None,
            slash_file: None,
            project_file: None,
            hot: HotSwitch::no("客户端常量，只能二进制补丁（Metis 不提供）"),
            channels: vec![],
        },
    ]
}

/// 全部 15 家的静态事实。
pub fn all() -> &'static [Spec] {
    &SPECS
}

/// 按 id 查一家。
pub fn find(id: &str) -> Option<&'static Spec> {
    let lowered = id.trim().to_ascii_lowercase();
    all().iter().find(|spec| spec.id == lowered)
}

/// 按 id 查一家的能力表。
pub fn capabilities(id: &str) -> Option<Capabilities> {
    let spec = find(id)?;
    Some(caps_of(spec))
}

/// 从静态事实推导能力表。
///
/// 这里的"能不能用"只考虑**静态**约束（层级、有没有落点、有没有桌面版问题）；
/// 运行时还要叠加"本机有没有装"（见 `metis-harness` 的 detect）。
pub fn caps_of(spec: &'static Spec) -> Capabilities {
    let injectable = !spec.channels.is_empty();
    let has = |channel: Channel| spec.channels.contains(&channel);

    let install = if !injectable {
        ChannelState::unavailable(format!("{} 只能二进制补丁，配置层没有可写入口", spec.display))
    } else if spec.persist.is_none() {
        ChannelState::unavailable(format!("{} 没有可写的配置文件", spec.display))
    } else if let Some(target) = &spec.persist {
        if target.layer.is_authoritative() {
            ChannelState::available()
        } else {
            // user 层不算能力：写下去也压不住身份。
            ChannelState::unavailable(format!("{} 的常驻文件只落到 {} 层（{}）—— 压不住系统提示词", spec.display, target.layer.as_str(), target.note))
        }
    } else {
        ChannelState::unavailable("没有可写的配置文件")
    };

    let shim = if !injectable {
        ChannelState::unavailable(format!("{} 注入不进去", spec.display))
    } else if spec.probe.desktop_only && spec.probe.bins.is_empty() {
        ChannelState::unavailable("本机没有命令行入口；桌面版双击启动也不经过 PATH —— 裸命令接管对它无效")
    } else if !has(Channel::Shim) {
        ChannelState::unavailable(match spec.id {
            "dsh" => "dsh 命令行只能起 headless：接管裸命令会让双击/网页入口变成终端会话；要三形态就装常驻插件".to_owned(),
            _ => format!("{} 不走命令行启动，裸命令接管无效", spec.display),
        })
    } else {
        ChannelState::available()
    };

    let slash = if !injectable {
        ChannelState::unavailable(format!("{} 注入不进去", spec.display))
    } else if !has(Channel::Slash) {
        ChannelState::unavailable(format!("{} 没有能改系统提示词的 /purge（/purge 展开成用户消息，实测无效）", spec.display))
    } else {
        ChannelState::available()
    };

    let project = if !injectable {
        ChannelState::unavailable(format!("{} 注入不进去", spec.display))
    } else if spec.project_file.is_none() {
        ChannelState::unavailable(match spec.id {
            "workbuddy" => "WorkBuddy 只把 CODEBUDDY.md / AGENTS.md 当首轮用户上下文，压不住系统身份".to_owned(),
            _ => format!("{} 没有项目级规则文件（AGENTS.md/CLAUDE.md）可写", spec.display),
        })
    } else {
        ChannelState::available()
    };

    Capabilities { injectable, shim, install, slash, project, hot: spec.hot.clone(), layer: spec.persist.as_ref().map(|t| t.layer.as_str()) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metis_core::HarnessId;

    #[test]
    fn table_covers_all_builtin_harnesses() {
        assert_eq!(all().len(), 15);
        for id in HarnessId::BUILTIN {
            assert!(find(id).is_some(), "注册表里缺少 {id}");
        }
    }

    #[test]
    fn ids_are_unique_and_lowercase() {
        let mut ids: Vec<&str> = all().iter().map(|s| s.id).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(count, ids.len(), "注册表里有重复 id");
        for id in ids {
            assert_eq!(id, id.to_ascii_lowercase());
            assert!(!id.is_empty());
        }
    }

    #[test]
    fn every_display_name_is_present() {
        for spec in all() {
            assert!(!spec.display.trim().is_empty(), "{} 缺少显示名", spec.id);
        }
    }

    #[test]
    fn every_hot_switch_has_a_reason() {
        for spec in all() {
            assert!(!spec.hot.how.trim().is_empty(), "{} 的热切能力缺少说明", spec.id);
            if spec.hot.ok {
                assert!(spec.hot.kind.is_some(), "{} 声称支持热切却没给触发方式", spec.id);
                assert!(spec.hot.needs.is_some(), "{} 声称支持热切却没给依赖通道", spec.id);
            }
        }
    }

    #[test]
    fn every_channel_a_spec_declares_has_a_persist_support() {
        for spec in all() {
            if spec.channels.contains(&Channel::Install) {
                assert!(spec.persist.is_some(), "{} 声明了 install 通道却没有落点", spec.id);
            }
            if spec.channels.contains(&Channel::Slash) {
                assert!(spec.slash_file.is_some(), "{} 声明了 slash 通道却没有扩展落点", spec.id);
            }
            if spec.channels.contains(&Channel::Project) {
                assert!(spec.project_file.is_some(), "{} 声明了 project 通道却没有目标文件", spec.id);
            }
        }
    }

    #[test]
    fn antigravity_is_not_injectable_and_says_why() {
        let caps = capabilities("antigravity").expect("caps");
        assert!(!caps.injectable);
        assert!(!caps.any(), "antigravity 不该有任何可用通道");
        let why = caps.install.why.expect("必须说明原因");
        assert!(why.contains("二进制补丁"), "原因应指出只能打补丁：{why}");
    }

    #[test]
    fn claude_install_is_unavailable_because_it_is_a_user_layer() {
        let caps = capabilities("claude").expect("caps");
        assert!(!caps.install.ok, "user 层不能算能力");
        assert_eq!(caps.layer, Some("user"));
        let why = caps.install.why.expect("why");
        assert!(why.contains("压不住系统提示词"), "{why}");
        // 但启动参数与项目通道仍然可用。
        assert!(caps.project.ok);
    }

    #[test]
    fn codebuddy_has_no_install_target() {
        let caps = capabilities("codebuddy").expect("caps");
        assert!(!caps.install.ok);
        assert!(caps.shim.ok, "codebuddy 仍可裸命令接管");
        assert!(caps.install.why.is_some());
    }

    #[test]
    fn dsh_shim_is_blocked_with_a_specific_reason() {
        let caps = capabilities("dsh").expect("caps");
        assert!(!caps.shim.ok);
        let why = caps.shim.why.expect("why");
        assert!(why.contains("headless"), "{why}");
        assert!(caps.install.ok);
        assert!(caps.slash.ok);
    }

    #[test]
    fn desktop_only_harnesses_explain_shim_block() {
        for id in ["pi-desktop", "workbuddy", "zcode"] {
            let caps = capabilities(id).expect("caps");
            if !caps.shim.ok {
                let why = caps.shim.why.unwrap_or_default();
                assert!(!why.is_empty(), "{id} 的 shim 不可用必须说明原因");
            }
        }
    }

    #[test]
    fn hot_switch_requires_its_channel_to_work() {
        for spec in all() {
            let caps = caps_of(spec);
            if caps.hot.ok {
                let needs = caps.hot.needs.expect("needs");
                assert!(caps.channel(needs).ok, "{} 的热切依赖 {needs}，但那条通道不可用", spec.id);
            }
        }
    }

    #[test]
    fn every_unavailable_channel_has_a_reason() {
        // 界面要显示 why，空字符串等于没解释。
        for spec in all() {
            let caps = caps_of(spec);
            for channel in Channel::ALL {
                let state = caps.channel(channel);
                if !state.ok {
                    let why = state.why.as_deref().unwrap_or_default();
                    assert!(!why.trim().is_empty(), "{} 的 {channel} 不可用但没给原因", spec.id);
                }
            }
        }
    }

    #[test]
    fn find_is_case_insensitive_and_trims() {
        assert_eq!(find("  CODEX ").map(|s| s.id), Some("codex"));
        assert!(find("nope").is_none());
    }

    #[test]
    fn pi_and_omp_and_dsh_and_pi_desktop_support_hot_switch() {
        for id in ["pi", "omp", "dsh", "pi-desktop"] {
            assert!(capabilities(id).expect("caps").hot.ok, "{id} 应支持热切");
        }
        for id in ["codex", "claude", "grok", "hermes", "kilo", "zcode", "codebuddy", "workbuddy"] {
            assert!(!capabilities(id).expect("caps").hot.ok, "{id} 不该声称支持热切");
        }
    }
}
