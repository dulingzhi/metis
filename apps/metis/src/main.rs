//! Metis CLI。
//!
//! 只做三件事：解析命令行 → 组装参数 → 调引擎命令。**不写业务分支**（AGENTS.md 铁律 6）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]
// CLI 的职责就是把结果打印到 stdout/stderr。
#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use metis_engine::{CommandOutput, Commands, Engine};
use metis_paths::Paths;

/// 一份人设，统一管理多个 coding agent。
#[derive(Debug, Parser)]
#[command(name = "metis", version, about, long_about = None)]
pub struct Cli {
    /// 输出机器可读的 JSON。
    #[arg(long, global = true)]
    pub json: bool,

    /// 覆盖数据目录（默认 ~/.metis）。
    #[arg(long, global = true, value_name = "DIR")]
    pub home: Option<String>,

    /// 界面语言：`zh-Hans` 或 `en`（也认 `zh` / `en-US`）。
    #[arg(long, global = true, value_name = "LANG")]
    pub lang: Option<String>,

    /// 子命令。
    #[command(subcommand)]
    pub command: Option<Command>,
}

/// 子命令。
#[derive(Debug, Subcommand)]
pub enum Command {
    /// 显示当前配置状态。
    Status,
    /// 列出人设。
    Ls,
    /// 换人设。
    Use {
        /// 人设 id。**不写就打印当前人设与全部例外**（与基线一致）。
        persona: Option<String>,
        /// 只给这一家换（不写就是全局）。
        #[arg(long, value_name = "HARNESS")]
        r#for: Option<String>,
    },
    /// 只刷新落点，不换人设（等价于 `apply`；`--failed` 只重试上次失败的）。
    Sync {
        /// 只重试上次失败的通道。
        #[arg(long)]
        failed: bool,
    },
    /// 只开 install 通道（`arm <h> --channel install` 的快捷方式）。
    Install {
        /// 执行器（不写就是全部可注入的）。
        harnesses: Vec<String>,
    },
    /// `/purge` 扩展（slash 通道）的装 / 卸 / 查。
    Slash {
        /// 子动作：status / on / off。
        #[arg(default_value = "status")]
        action: String,
        /// 执行器（不写就是全部支持 slash 的）。
        harnesses: Vec<String>,
    },
    /// 换模式：replace / strict / append / off。
    Mode {
        /// 模式。
        mode: String,
        /// 只给这一家换。
        #[arg(long, value_name = "HARNESS")]
        r#for: Option<String>,
    },
    /// 暂停注入（保留通道选择）。
    Pause,
    /// 恢复注入。
    Resume,
    /// 开启某家的通道。
    Arm {
        /// 执行器 id。
        harness: String,
        /// 只开某条通道（可重复）。
        #[arg(long)]
        channel: Vec<String>,
    },
    /// 关闭某家的通道。
    Disarm {
        /// 执行器 id。
        harness: String,
    },
    /// 应用当前配置。
    Apply {
        /// 只重试上次失败的项。
        #[arg(long)]
        failed: bool,
    },
    /// 列出执行器与能力。
    Harnesses {
        /// 顺便探测本机装没装、版本号（会起一次进程，慢一点）。
        #[arg(long)]
        detect: bool,
    },
    /// 探测本机装了哪些执行器（含版本与主目录）。
    Doctor {
        /// 跳过版本探测（CI 里快一点）。
        #[arg(long)]
        no_version: bool,
    },
    /// 启动一个宿主（默认只展示命令；加 --exec 真的启动）。
    Launch {
        /// 执行器 id。
        harness: String,
        /// 真的启动（默认只打印将执行的命令）。
        #[arg(long)]
        exec: bool,
        /// 本次的模式。
        #[arg(long)]
        mode: Option<String>,
        /// 透传给宿主的参数（写在 `--` 之后）。
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// 一次性问答（`--ask`）。
    Ask {
        /// 执行器 id。
        harness: String,
        /// 要问的问题。
        prompt: String,
        /// 真的执行（默认只打印将执行的命令）。
        #[arg(long)]
        exec: bool,
    },
    /// 实测：人设到底进没进 system 层（本地假 provider 抓真实请求）。
    Verify {
        /// 只验这几家（不写就验全部）。
        harnesses: Vec<String>,
        /// 每家的超时（毫秒）。
        #[arg(long, default_value = "20000")]
        timeout_ms: u64,
    },
    /// 读最近一次验证报告。
    Report,
    /// 撤回全部接管控件（保留人设与状态）。
    Clean {
        /// 只预览，不动磁盘。
        #[arg(long)]
        dry_run: bool,
    },
    /// 撤回该家的全部通道并删掉已写入的内容。
    Uninstall {
        /// 执行器 id。
        harness: String,
    },
    /// 撤回写入**并删除数据目录**（不可逆）。
    SelfUninstall {
        /// 真的删除。
        #[arg(long)]
        yes: bool,
        /// 只预览。
        #[arg(long)]
        dry_run: bool,
    },
    /// 项目目录的登记与摘除（`project` 通道）。
    Project {
        /// 子动作：list / add / remove。
        #[arg(default_value = "list")]
        action: String,
        /// 执行器 id（add / remove 用）。
        #[arg(long)]
        harness: Option<String>,
        /// 项目目录的绝对路径（add / remove 用）。
        #[arg(long)]
        dir: Option<String>,
    },
    /// 备份目录的体积管理。
    Backups {
        /// 子动作：usage / prune。
        #[arg(default_value = "usage")]
        action: String,
        /// 真的删除（默认只预览）。
        #[arg(long)]
        yes: bool,
    },
    /// 把人设注入的实测结果翻译成策略。
    Armor {
        /// 子动作：plan / preflight / run / judge / adopt。
        #[arg(default_value = "plan")]
        action: String,
        /// 目标执行器（可多个）。
        harnesses: Vec<String>,
        /// preflight / adopt 用：指定一家。
        #[arg(long)]
        harness: Option<String>,
        /// run / record 用：哪一层。
        #[arg(long)]
        layer: Option<String>,
        /// record 用：命中几轮。
        #[arg(long)]
        hits: Option<u64>,
        /// record 用：跑了几轮。
        #[arg(long)]
        rounds: Option<u64>,
    },
    /// 查看界面文案表（`--lang` 决定用哪一列显示）。
    I18n {
        /// 列出所有键与文案。
        #[arg(long)]
        all: bool,
    },
    /// 检查有没有新版本（默认只检查，不改任何文件）。
    Update,
    /// 查变更日志：不带参数列月份，带 --month 读那个月。
    Journal {
        /// 月份，例如 2026-11。
        #[arg(long)]
        month: Option<String>,
    },
    /// 按保留期裁剪变更日志。
    JournalPrune {
        /// 保留几个月（默认 6）。
        #[arg(long, default_value = "6")]
        keep: usize,
    },
    /// 从本地目录或 GitHub `owner/repo` 导入人设。
    Import {
        /// 目录路径，或 `owner/repo`。
        source: String,
        /// 覆盖同名。
        #[arg(long)]
        overwrite: bool,
        /// 只预览。
        #[arg(long)]
        dry_run: bool,
        /// 强制来源类型：dir 或 github（默认按路径是否存在猜）。
        #[arg(long)]
        kind: Option<String>,
    },
    /// 从 Node 版 agent-purge 只读导入人设与状态。
    ImportAgentPurge {
        /// 源数据目录（默认 ~/.agent-purge 或 AGENT_PURGE_HOME）。
        #[arg(long)]
        from: Option<String>,
        /// 覆盖同名。
        #[arg(long)]
        overwrite: bool,
        /// 只预览。
        #[arg(long)]
        dry_run: bool,
    },
    /// 远端人设库（worldd / KK Hub）。
    Hub {
        /// 子动作：list / show / import / skill。
        #[command(subcommand)]
        action: HubAction,
    },
    /// 新建或修改一份人设。
    Persona {
        /// 人设 id。
        id: String,
        /// 正文（不传则从标准输入读）。
        #[arg(long)]
        body: Option<String>,
        /// 显示名。
        #[arg(long)]
        name: Option<String>,
        /// 描述。
        #[arg(long)]
        description: Option<String>,
    },
    /// 删除一份人设。
    Rm {
        /// 人设 id。
        id: String,
    },
}

/// `metis hub` 的子动作。
#[derive(Debug, Subcommand)]
pub enum HubAction {
    /// 列出远端 expert（分页）。
    List {
        /// 搜索词（匹配名称/说明/标签）。
        #[arg(long, value_name = "TEXT")]
        query: Option<String>,
        /// 标签过滤。
        #[arg(long)]
        tag: Option<String>,
        /// provider：claude / codex / deepseek。
        #[arg(long, value_name = "NAME")]
        provider: Option<String>,
        /// 不透明游标（上一页返回的 nextCursor）。
        #[arg(long)]
        cursor: Option<String>,
        /// 每页条数（默认 20，最大 100）。
        #[arg(long, default_value = "20")]
        limit: usize,
        /// 绕过缓存新鲜期。
        #[arg(long)]
        refresh: bool,
        /// Hub 地址（默认 METIS_HUB_URL 或内置）。
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// 查看一个 expert 的完整详情。
    Show {
        /// `expert/<slug>` 或 `<slug>`。
        target: String,
        /// 版本（默认当前 approved）。
        #[arg(long)]
        version: Option<String>,
        /// 绕过缓存新鲜期。
        #[arg(long)]
        refresh: bool,
        /// Hub 地址（默认 METIS_HUB_URL 或内置）。
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// 导入一个 expert，并按需安装关联技能。
    Import {
        /// `expert/<slug>` 或 `<slug>`。
        target: String,
        /// 版本（默认当前 approved）。
        #[arg(long)]
        version: Option<String>,
        /// 人设 id（默认 Hub slug）。
        #[arg(long = "as", value_name = "ID")]
        as_id: Option<String>,
        /// 覆盖同名的人设。
        #[arg(long)]
        overwrite: bool,
        /// 只预览，不写盘。
        #[arg(long)]
        dry_run: bool,
        /// 不把 knowledge 追加到正文。
        #[arg(long)]
        no_knowledge: bool,
        /// 关联技能：all / none / slug,slug。
        #[arg(long, default_value = "all")]
        skills: String,
        /// 允许导入 verified=false 的条目。
        #[arg(long)]
        allow_unverified: bool,
        /// 绕过缓存新鲜期。
        #[arg(long)]
        refresh: bool,
        /// Hub 地址（默认 METIS_HUB_URL 或内置）。
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// 单独管理已安装的 skill。
    Skill {
        /// 子动作：install / remove。
        #[command(subcommand)]
        action: HubSkillAction,
    },
}

/// `metis hub skill` 的子动作。
#[derive(Debug, Subcommand)]
pub enum HubSkillAction {
    /// 安装一个 skill。
    Install {
        /// skill slug。
        slug: String,
        /// 版本（默认当前 approved）。
        #[arg(long)]
        version: Option<String>,
        /// 覆盖已有安装（包括没有清单的目录）。
        #[arg(long)]
        overwrite: bool,
        /// 只预览，不写盘。
        #[arg(long)]
        dry_run: bool,
        /// 允许安装 verified=false 的条目。
        #[arg(long)]
        allow_unverified: bool,
        /// 绕过缓存新鲜期。
        #[arg(long)]
        refresh: bool,
        /// Hub 地址（默认 METIS_HUB_URL 或内置）。
        #[arg(long, value_name = "URL")]
        url: Option<String>,
    },
    /// 移除一个由 Metis 安装的 skill（只删清单里的文件）。
    Remove {
        /// skill slug。
        slug: String,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("✗ {}", error.message());
            if let Some(remediation) = error.remediation() {
                eprintln!("  → {remediation}");
            }
            if let Some(detail) = error.detail() {
                eprintln!("  ({detail})");
            }
            ExitCode::from(exit_code_for(error.kind()))
        }
    }
}

/// 退出码约定（与基线一致）：`0` 成功，`1` 执行失败，`2` 语义错误（调用方用错了）。
///
/// 判据是**错误类别**，不是错误文案 —— 文案会改，类别是契约（`docs/PARITY.md` §1.2）。
fn exit_code_for(kind: metis_core::ErrorKind) -> u8 {
    use metis_core::ErrorKind;
    match kind {
        // 调用方用错了：参数非法、名字不存在、通道不支持、被安全闸门拒绝。
        ErrorKind::InvalidArgument
        | ErrorKind::UnknownHarness
        | ErrorKind::UnknownChannel
        | ErrorKind::UnsupportedChannel
        | ErrorKind::PersonaInvalid
        | ErrorKind::Refused => 2,
        // 其余都是"跑起来之后才失败"：缺人设、IO、进程、网络、恢复冲突。
        // 用 1，让脚本能区分"我传错了"和"它没做成"。
        _ => 1,
    }
}

fn run(cli: Cli) -> metis_core::Result<()> {
    let paths = match &cli.home {
        Some(home) => Paths::with_home(home),
        None => Paths::from_env()?,
    };
    let engine = Engine::new(paths);
    let commands = Commands::default_set();

    let Some(command) = cli.command else {
        print_help();
        return Ok(());
    };

    // `--lang` 只接受已知语言；拼错要报错，而不是静默用默认（PITFALLS G12）。
    if let Some(lang) = &cli.lang
        && metis_core::i18n::Lang::parse(lang).is_none()
    {
        eprintln!("✗ 不认识的语言：{lang}");
        eprintln!("  → 可用：zh-Hans、en（也认 zh / zh-CN / en-US）");
        std::process::exit(2);
    }

    // `i18n` 子命令读的是编译进来的文案表，不走命令注册表。
    if let Command::I18n { all } = &command {
        print_i18n(*all, cli.lang.as_deref());
        return Ok(());
    }

    // 有些子命令不产生命令调用（`metis use` 不带 id 就是"查"）。
    if early_exit(&command, &commands, &engine)? {
        return Ok(());
    }

    let (id, params) = translate(command)?;
    let output = commands.run(id, &engine, &params)?;
    emit(&output, cli.json);
    Ok(())
}

fn print_help() {
    println!("Metis —— 一份人设，统一管理多个 coding agent");
    println!();
    println!("  metis status            查看当前配置");
    println!("  metis harnesses         查看本机可接管的执行器");
    println!("  metis ls                列出人设");
    println!("  metis hub list|show|import  远端人设库（worldd / KK Hub）");
    println!("  metis use <id>          换人设");
    println!("  metis mode <m>          换模式（replace / strict / append / off）");
    println!("  metis arm <harness>     开启通道");
    println!("  metis disarm <harness>  关闭该家的全部通道并撤下已写入的内容");
    println!("  metis apply             应用配置");
    println!("  metis pause / resume    暂停 / 恢复注入");
    println!("  metis persona <id>      新建或修改一份人设");
}

fn translate(command: Command) -> metis_core::Result<(&'static str, serde_json::Value)> {
    use serde_json::json;
    Ok(match command {
        Command::Status => ("state.status", json!({})),
        Command::Ls => ("persona.list", json!({})),
        Command::Use { persona, r#for } => {
            let mut params = json!({});
            if let Some(persona) = persona {
                params["persona"] = json!(persona);
            }
            if let Some(harness) = r#for {
                params["harness"] = json!(harness);
            }
            ("persona.use", params)
        }
        Command::Mode { mode, r#for } => {
            let mut params = json!({ "mode": mode });
            if let Some(harness) = r#for {
                params["harness"] = json!(harness);
            }
            ("state.mode", params)
        }
        Command::Pause => ("state.power", json!({ "on": false })),
        Command::Resume => ("state.power", json!({ "on": true })),
        Command::Arm { harness, channel } => {
            if channel.is_empty() {
                ("channel.set", json!({ "harness": harness, "channels": ["shim", "slash", "install"] }))
            } else {
                // 旗标拼错时必须报错，不能静默当成"没给通道"（那会悄悄开三条）。
                let parsed: Vec<&str> = channel
                    .iter()
                    .map(|raw| {
                        metis_core::Channel::parse(raw).map(|channel| channel.as_str()).ok_or_else(|| {
                            metis_core::MetisError::new(metis_core::ErrorKind::UnknownChannel, format!("未知通道：{raw}"))
                                .with_remediation("可用：shim, slash, install, project")
                        })
                    })
                    .collect::<metis_core::Result<_>>()?;
                ("channel.set", json!({ "harness": harness, "channels": parsed }))
            }
        }
        Command::Disarm { harness } => ("channel.set", json!({ "harness": harness, "channels": [] })),
        Command::Apply { failed } => ("config.apply", json!({ "onlyFailed": failed })),
        Command::Harnesses { detect } => {
            if detect {
                ("harness.doctor", json!({ "versions": true }))
            } else {
                ("harness.list", json!({}))
            }
        }
        Command::Doctor { no_version } => ("harness.doctor", json!({ "versions": !no_version })),
        Command::Launch { harness, exec, mode, args } => {
            let mut params = json!({ "harness": harness, "dryRun": !exec, "exec": exec, "args": args });
            if let Some(mode) = mode {
                params["mode"] = json!(mode);
            }
            ("harness.launch", params)
        }
        Command::Ask { harness, prompt, exec } => ("harness.ask", json!({ "harness": harness, "prompt": prompt, "exec": exec })),
        Command::Verify { harnesses, timeout_ms } => ("harness.verify", json!({ "harnesses": harnesses, "timeoutMs": timeout_ms })),
        Command::Report => ("harness.verify-report", json!({})),
        Command::Clean { dry_run } => ("channel.clean", json!({ "dryRun": dry_run })),
        Command::Uninstall { harness } => ("channel.uninstall", json!({ "harness": harness })),
        Command::SelfUninstall { yes, dry_run } => ("metis.self-uninstall", json!({ "yes": yes, "dryRun": dry_run })),

        Command::Project { action, harness, dir } => match action.as_str() {
            "list" => ("project.list", json!({})),
            "add" => ("project.add", json!({ "harness": harness.unwrap_or_default(), "dir": dir.unwrap_or_default() })),
            "remove" => ("project.remove", json!({ "harness": harness.unwrap_or_default(), "dir": dir.unwrap_or_default() })),
            other => {
                return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, format!("未知 project 子动作：{other}"))
                    .with_remediation("可用：list / add / remove"));
            }
        },
        Command::Backups { action, yes } => match action.as_str() {
            "usage" => ("metis.backups-usage", json!({})),
            "prune" => ("metis.backups-prune", json!({ "dryRun": !yes })),
            other => {
                return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, format!("未知 backups 子动作：{other}"))
                    .with_remediation("可用：usage / prune"));
            }
        },
        Command::Armor { action, harnesses, harness, layer, hits, rounds } => match action.as_str() {
            "plan" => ("armor.plan", json!({ "harnesses": harnesses })),
            "preflight" => ("armor.preflight", json!({ "harness": harness.or_else(|| harnesses.first().cloned()).unwrap_or_default() })),
            "judge" => ("armor.judge", json!({})),
            "adopt" => ("armor.adopt", json!({ "harness": harness.or_else(|| harnesses.first().cloned()).unwrap_or_default() })),
            "record" => (
                "armor.record",
                json!({
                    "harness": harness.or_else(|| harnesses.first().cloned()).unwrap_or_default(),
                    "layer": layer,
                    "hits": hits.unwrap_or(0),
                    "rounds": rounds.unwrap_or(1),
                }),
            ),
            other => {
                return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, format!("未知 armor 子动作：{other}"))
                    .with_remediation("可用：plan / preflight / run / judge / adopt / record"));
            }
        },
        Command::I18n { .. } => {
            // 正常路径上这个分支走不到（i18n 在 `translate` 之前就处理了）。
            // 但铁律 1 禁止 `unreachable!`：万一将来有人调整顺序，这里要给出**可诊断的错误**，
            // 而不是让程序崩。
            return Err(metis_core::MetisError::new(metis_core::ErrorKind::Internal, "i18n 子命令被错误地路由到了命令表")
                .with_remediation("这是程序缺陷，请把这条输出贴到 issue"));
        }
        Command::Update => ("metis.update-check", json!({})),
        Command::Journal { month } => match month {
            Some(month) => ("metis.journal", json!({ "month": month })),
            None => ("metis.journal", json!({})),
        },
        Command::JournalPrune { keep } => ("metis.journal-prune", json!({ "keep": keep })),
        Command::Import { source, overwrite, dry_run, kind } => {
            let mut params = json!({ "source": source, "overwrite": overwrite, "dryRun": dry_run });
            if let Some(kind) = kind {
                params["kind"] = json!(kind);
            }
            ("metis.import-personas", params)
        }
        Command::ImportAgentPurge { from, overwrite, dry_run } => {
            let mut params = json!({ "overwrite": overwrite, "dryRun": dry_run });
            if let Some(from) = from {
                params["from"] = json!(from);
            }
            ("metis.import-agent-purge", params)
        }
        Command::Hub { action } => return translate_hub(action),
        Command::Persona { id, body, name, description } => {
            let body = match body {
                Some(body) => body,
                None => {
                    let mut buffer = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buffer)
                        .map_err(|error| metis_core::MetisError::from(error).with_remediation("用 --body \"正文\" 直接给小体量正文，或通过管道喂入"))?;
                    buffer
                }
            };
            let mut params = json!({ "persona": id, "body": body });
            if let Some(name) = name {
                params["name"] = json!(name);
            }
            if let Some(description) = description {
                params["description"] = json!(description);
            }
            ("persona.save", params)
        }
        Command::Sync { failed } => ("config.apply", json!({ "onlyFailed": failed })),
        // `install` / `slash` 需要逐家调用 `channel.set`（它一次只处理一家），
        // 所以走 `multi` 分支而不是单条命令。
        Command::Install { .. } | Command::Slash { .. } => {
            return Err(metis_core::MetisError::new(metis_core::ErrorKind::Internal, "这两条走 multi 分支").with_remediation("这是程序缺陷"));
        }
        Command::Rm { id } => ("persona.remove", json!({ "persona": id })),
    })
}

fn translate_hub(action: HubAction) -> metis_core::Result<(&'static str, serde_json::Value)> {
    use serde_json::json;
    Ok(match action {
        HubAction::List { query, tag, provider, cursor, limit, refresh, url } => {
            let mut params = json!({ "limit": limit, "refresh": refresh });
            insert_optional(&mut params, "query", query);
            insert_optional(&mut params, "tag", tag);
            insert_optional(&mut params, "provider", provider);
            insert_optional(&mut params, "cursor", cursor);
            insert_optional(&mut params, "url", url);
            ("metis.hub-list", params)
        }
        HubAction::Show { target, version, refresh, url } => {
            let slug = parse_expert_target(&target)?;
            let mut params = json!({ "slug": slug, "refresh": refresh });
            insert_optional(&mut params, "version", version);
            insert_optional(&mut params, "url", url);
            ("metis.hub-show", params)
        }
        HubAction::Import { target, version, as_id, overwrite, dry_run, no_knowledge, skills, allow_unverified, refresh, url } => {
            let slug = parse_expert_target(&target)?;
            let skills = parse_skills_arg(&skills)?;
            let mut params = json!({
                "slug": slug,
                "overwrite": overwrite,
                "dryRun": dry_run,
                "includeKnowledge": !no_knowledge,
                "skills": skills,
                "allowUnverified": allow_unverified,
                "refresh": refresh,
            });
            insert_optional(&mut params, "version", version);
            insert_optional(&mut params, "as", as_id);
            insert_optional(&mut params, "url", url);
            ("metis.hub-import", params)
        }
        HubAction::Skill { action } => match action {
            HubSkillAction::Install { slug, version, overwrite, dry_run, allow_unverified, refresh, url } => {
                let mut params = json!({ "slug": slug, "overwrite": overwrite, "dryRun": dry_run, "allowUnverified": allow_unverified, "refresh": refresh });
                insert_optional(&mut params, "version", version);
                insert_optional(&mut params, "url", url);
                ("metis.hub-skill-install", params)
            }
            HubSkillAction::Remove { slug } => ("metis.hub-skill-remove", json!({ "slug": slug })),
        },
    })
}

/// `expert/<slug>` 或 `<slug>`；目前只支持 expert。
fn parse_expert_target(target: &str) -> metis_core::Result<String> {
    let target = target.trim();
    let (kind, slug) = match target.split_once('/') {
        Some((kind, slug)) => (Some(kind), slug),
        None => (None, target),
    };
    if let Some(kind) = kind
        && kind != "expert"
    {
        return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, format!("目前只支持 expert 目标：{target}"))
            .with_remediation("用 `expert/<slug>` 或直接写 `<slug>`"));
    }
    if slug.trim().is_empty() {
        return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, format!("缺少 slug：{target}"))
            .with_remediation("例如 `expert/security-reviewer`"));
    }
    Ok(slug.trim().to_owned())
}

fn parse_skills_arg(value: &str) -> metis_core::Result<serde_json::Value> {
    use serde_json::json;
    match value.trim() {
        "all" => Ok(json!("all")),
        "none" => Ok(json!("none")),
        other => {
            let slugs: Vec<&str> = other.split(',').map(str::trim).filter(|slug| !slug.is_empty()).collect();
            if slugs.is_empty() {
                return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, "skills 至少要给一个 slug，或写 all / none")
                    .with_remediation("例如 `--skills wb-apiz,wb-claude-agent-sdk`"));
            }
            Ok(json!(slugs))
        }
    }
}

fn insert_optional(params: &mut serde_json::Value, key: &str, value: Option<String>) {
    if let Some(value) = value {
        params[key] = serde_json::Value::String(value);
    }
}

/// 真正需要 engine 的早退分支（必须在拿到 engine 之后判断）。
fn early_exit(command: &Command, commands: &metis_engine::Commands, engine: &metis_engine::Engine) -> metis_core::Result<bool> {
    use serde_json::json;
    match command {
        // `metis use`（不带 id）：打印当前人设与全部例外，与基线一致。
        Command::Use { persona: None, r#for: None } => {
            let value = commands.run("state.status", engine, &json!({}))?.to_json();
            let current = value["persona"].as_str().unwrap_or("（未选择）");
            println!("当前人设：{current}");
            match value["overrides"].as_object().filter(|o| !o.is_empty()) {
                Some(overrides) => {
                    println!("分家例外：");
                    for (harness, entry) in overrides {
                        let persona = entry["persona"].as_str().unwrap_or("（跟随全局）");
                        let mode = entry["mode"].as_str().unwrap_or("（跟随全局）");
                        println!("  {harness}: 人设={persona} 模式={mode}");
                    }
                }
                None => println!("分家例外：无（全部跟随全局）"),
            }
            Ok(true)
        }
        // `install`：给每家开 install 通道（逐家调用，因为 channel.set 一次只处理一家）。
        Command::Install { harnesses } => {
            let targets = resolve_harnesses(commands, engine, harnesses)?;
            for harness in &targets {
                commands.run(
                    "channel.set",
                    engine,
                    &json!({
                        "harness": harness, "channels": ["install"],
                    }),
                )?;
            }
            println!("已为 {} 家开启 install 通道：{}", targets.len(), targets.join(", "));
            println!("接着跑 `metis apply` 把内容写进去。");
            Ok(true)
        }
        // `slash`：装 / 卸 / 查 `/purge` 扩展。
        Command::Slash { action, harnesses } => {
            match action.as_str() {
                "status" => {
                    let value = commands.run("harness.list", engine, &json!({"detect": true}))?.to_json();
                    println!("{:<16} {:>6}  说明", "执行器", "slash");
                    if let Some(items) = value["harnesses"].as_array() {
                        for item in items {
                            let id = item["id"].as_str().unwrap_or("?");
                            let slash = &item["channels"]["slash"];
                            let ok = slash["ok"].as_bool().unwrap_or(false);
                            let why = slash["why"].as_str().unwrap_or("");
                            let mark = if ok { "可用" } else { "不可用" };
                            if ok {
                                println!("{id:<16} {mark:>6}");
                            } else {
                                println!("{id:<16} {mark:>6}  {why}");
                            }
                        }
                    }
                }
                "on" | "off" => {
                    let on = action == "on";
                    let targets = resolve_harnesses(commands, engine, harnesses)?;
                    let mut done = Vec::new();
                    for harness in &targets {
                        // 只处理支持 slash 的家；不支持的直接跳过并说明。
                        let channels = if on { vec!["slash"] } else { Vec::<&str>::new() };
                        commands.run(
                            "channel.set",
                            engine,
                            &json!({
                                "harness": harness, "channels": channels,
                            }),
                        )?;
                        done.push(harness.clone());
                    }
                    println!("已{} {} 家的 slash 通道：{}", if on { "开启" } else { "关闭" }, done.len(), done.join(", "));
                    if on {
                        println!("接着跑 `metis apply` 写出宿主扩展产物。");
                    }
                }
                other => {
                    return Err(metis_core::MetisError::new(metis_core::ErrorKind::InvalidArgument, format!("未知 slash 子动作：{other}"))
                        .with_remediation("可用：status / on / off"));
                }
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// 解析目标执行器：显式给的，或全部内置的。
fn resolve_harnesses(commands: &metis_engine::Commands, engine: &metis_engine::Engine, explicit: &[String]) -> metis_core::Result<Vec<String>> {
    if !explicit.is_empty() {
        // 拼错要报错，不能静默忽略（`PITFALLS.md` G12）。
        //
        // **必须校验"已注册"而不只是"格式合法"**：`HarnessId::parse` 接受任意
        // 合法字符串（它允许用户自定义 id），所以 `metis install nope` 曾经
        // 一路"成功"，只是什么都没写。实测发现的。
        let registered = metis_harness::registry::all().iter().map(|spec| spec.id).collect::<Vec<_>>();
        for harness in explicit {
            let id = metis_core::HarnessId::parse(harness)?;
            if !registered.contains(&id.as_str()) {
                return Err(metis_core::MetisError::new(metis_core::ErrorKind::UnknownHarness, format!("未注册的执行器：{id}"))
                    .with_remediation(format!("可用的有：{}", registered.join(", "))));
            }
        }
        return Ok(explicit.to_vec());
    }
    let value = commands.run("harness.list", engine, &serde_json::json!({}))?.to_json();
    Ok(value["harnesses"].as_array().map(|items| items.iter().filter_map(|i| i["id"].as_str().map(str::to_owned)).collect()).unwrap_or_default())
}

/// 打印文案表。
fn print_i18n(all: bool, lang: Option<&str>) {
    let lang = lang.and_then(metis_core::i18n::Lang::parse).unwrap_or_default();
    let report = metis_core::i18n::check();
    println!("语言：{}（共 {} 条）", lang.code(), metis_core::i18n::keys(lang).len());
    for (code, count) in &report.counts {
        println!("  {code}: {count}");
    }
    if !report.is_ok() {
        println!("⚠ 两张表不一致：缺 en {} 条、缺 zh {} 条、重复 {} 条", report.missing_in_en.len(), report.missing_in_zh.len(), report.duplicates.len());
    }
    if all {
        for key in metis_core::i18n::keys(lang) {
            println!("{key}\t{}", metis_core::i18n::translate(lang, &key));
        }
    }
}

fn emit(output: &CommandOutput, json: bool) {
    if json {
        println!("{}", serde_json::to_string_pretty(&output.to_json()).unwrap_or_else(|_| "{}".to_owned()));
        return;
    }
    match output {
        CommandOutput::Message(message) => println!("{message}"),
        CommandOutput::Json(value) => print_human(value),
        CommandOutput::Apply(report) => {
            println!("应用完成：{} 条成功，{} 条失败（模式 {}）", report.written_count(), report.failures().len(), report.mode);
            for failure in report.failures() {
                println!("  ✗ {} · {}：{}", failure.harness, failure.channel, failure.error.clone().unwrap_or_default());
            }
            println!("  注意：配置写入不等于当前会话已生效。");
        }
    }
}

fn print_human(value: &serde_json::Value) {
    if let Some(harnesses) = value.get("harnesses").and_then(|v| v.as_array()) {
        println!("{:<14} {:<10} {:<6} {:<6} {:<8} 当前", "执行器", "注入", "常驻", "命令", "会话内");
        for harness in harnesses {
            let id = harness.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let injectable = harness.get("injectable").and_then(|v| v.as_bool()).unwrap_or(false);
            let ok = |key: &str| harness.pointer(&format!("/channels/{key}/ok")).and_then(|v| v.as_bool()).unwrap_or(false);
            let explain = harness.pointer("/activation/explain").and_then(|v| v.as_str()).unwrap_or("-");
            println!(
                "{:<14} {:<10} {:<6} {:<6} {:<8} {}",
                id,
                if injectable { "可以" } else { "不行" },
                mark(ok("install")),
                mark(ok("shim")),
                mark(ok("slash")),
                explain
            );
        }
        return;
    }
    if let Some(personas) = value.get("personas").and_then(|v| v.as_array()) {
        for persona in personas {
            let id = persona.get("id").and_then(|v| v.as_str()).unwrap_or("-");
            let name = persona.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let active = persona.get("active").and_then(|v| v.as_bool()).unwrap_or(false);
            println!("{} {id}  {name}", if active { "*" } else { " " });
        }
        return;
    }
    println!("{}", serde_json::to_string_pretty(value).unwrap_or_default());
}

fn mark(ok: bool) -> &'static str {
    if ok { "✓" } else { "—" }
}
