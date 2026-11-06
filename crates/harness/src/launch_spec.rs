//! 启动参数：每家的参数形状。
//!
//! **纯函数**：只描述"怎么启动"，不 spawn、不写盘（基线的好设计，保留）。
//! 事实来源：`docs/HARNESS-MATRIX.md` 的"启动参数一览"。
//!
//! 三条反复出现的约束：
//! 1. **命令行有长度上限**（Windows 约 8191 / 进程级 32767）：长人设必须走文件或扩展，
//!    不能塞进参数（`PITFALLS.md` C4）。
//! 2. **TOML 值要正确转义**：`codex -c` 的值按 TOML 解析。
//! 3. **`replace` 在五家上实际按 `strict` 执行**（没有宿主文本读取口），必须提示用户。

use metis_core::{HarnessId, Mode};

use super::launch::{forward_slashes, inline, toml_string};
use super::registry;

/// 一次交互式启动的计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// 可执行名。
    pub program: String,
    /// 参数。
    pub args: Vec<String>,
    /// 给用户看的备注（例如"注入靠扩展，不占命令行"）。
    pub note: Option<String>,
    /// 残留说明（例如"不补丁时残留 62 字身份句"）。
    pub residue: Option<String>,
    /// 这家**没有命令行入口**（只有桌面版）。
    pub no_cli: bool,
    /// 需要人工介入的说明（不代劳，只给配方）。
    pub manual: Vec<String>,
}

impl LaunchPlan {
    /// 直接透传。
    pub fn passthrough(program: impl Into<String>, args: Vec<String>) -> Self {
        Self { program: program.into(), args, note: None, residue: None, no_cli: false, manual: Vec::new() }
    }

    /// 附备注。
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// 附残留说明。
    #[must_use]
    pub fn with_residue(mut self, residue: impl Into<String>) -> Self {
        self.residue = Some(residue.into());
        self
    }

    /// 标记"这家没有命令行入口"。
    #[must_use]
    pub fn desktop_only(mut self) -> Self {
        self.no_cli = true;
        self
    }

    /// 附人工配方。
    #[must_use]
    pub fn with_manual(mut self, lines: Vec<String>) -> Self {
        self.manual = lines;
        self
    }

    /// 一行预览（**不含**人设正文：会把人设泄漏进终端与 CI 日志）。
    pub fn preview(&self) -> String {
        let mut out = self.program.clone();
        for arg in &self.args {
            out.push(' ');
            let redacted = redact(arg);
            if redacted.contains(' ') || redacted.contains('"') {
                out.push('"');
                out.push_str(&redacted.replace('"', "\\\""));
                out.push('"');
            } else {
                out.push_str(&redacted);
            }
        }
        out
    }
}

/// 把人设正文换成占位符，避免泄漏。
///
/// 基线在这一点上做得很对：终端输出、CI 日志、issue 里贴的命令行都不该带完整人设。
///
/// **注意**：不只是"预览字符串"要脱敏，**结构化输出里的 `args` 数组同样要脱敏** ——
/// 客户端拿 `--json` 一打印就泄漏了，而这正是脚本最喜欢做的事。
pub fn redact(arg: &str) -> String {
    // `-c developer_instructions="…"` 这种形式：值在 `=` 之后。
    if let Some((prefix, value)) = arg.split_once('=')
        && prefix.ends_with("developer_instructions")
        && value.chars().count() > 40
    {
        return format!("{prefix}=<persona {} 字>", value.chars().count());
    }
    if arg.chars().count() > 200 {
        return format!("<{} 字>", arg.chars().count());
    }
    arg.to_owned()
}

/// 一次一次性问答（`--ask`）的计划。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneshootPlan {
    /// 可执行名。
    pub program: String,
    /// 参数。
    pub args: Vec<String>,
    /// 给用户看的备注。
    pub note: Option<String>,
}

impl OneshootPlan {
    /// 一行预览（同样做脱敏）。
    pub fn preview(&self) -> String {
        let mut out = self.program.clone();
        for arg in &self.args {
            out.push(' ');
            let redacted = redact(arg);
            if redacted.contains(' ') {
                out.push('"');
                out.push_str(&redacted);
                out.push('"');
            } else {
                out.push_str(&redacted);
            }
        }
        out
    }
}

/// 生成一次交互式启动的计划。
///
/// `persona_text` 与 `persona_path` 都可能是 `None`（`off` 模式或没有人设）。
pub fn plan(harness: &HarnessId, mode: Mode, persona_text: Option<&str>, persona_path: Option<&str>, extra_args: &[String]) -> LaunchPlan {
    let spec = registry::find(harness.as_str());
    let desktop_only = spec.is_some_and(|s| s.probe.desktop_only && s.probe.bins.is_empty());
    if desktop_only {
        return LaunchPlan::passthrough(harness.as_str(), extra_args.to_vec()).desktop_only().with_manual(vec![
            format!("{harness} 只有桌面客户端，没有命令行入口：Metis 不代你启动它"),
            format!("装它的常驻通道：metis arm {harness} --channel install"),
            "改完设置后重开那个客户端，新会话才会读到人设".to_owned(),
        ]);
    }

    let text = persona_text.map(inline);
    let path = persona_path.map(forward_slashes);
    let inject = mode.injects();

    match harness.as_str() {
        // pi：命令行不加人设参数。原因是 `pi.cmd` 走 cmd.exe，10K 人设会撞 8191 上限。
        "pi" => LaunchPlan::passthrough("pi", extra_args.to_vec()).with_note("注入靠扩展（sys 层、无长度限制），不占命令行"),
        "omp" => {
            if !inject {
                return LaunchPlan::passthrough("omp", extra_args.to_vec());
            }
            let flag = if mode == Mode::Append { "--append-system-prompt" } else { "--system-prompt" };
            let mut args = vec![flag.to_owned(), text.clone().unwrap_or_default()];
            args.extend_from_slice(extra_args);
            LaunchPlan {
                program: "omp".to_owned(),
                args,
                note: None,
                residue: Some("MCP tool routes 仍会追加（10K 量级），关掉 MCP server 可缩".to_owned()),
                no_cli: false,
                manual: Vec::new(),
            }
        }
        "codex" => {
            if !inject {
                return LaunchPlan::passthrough("codex", extra_args.to_vec());
            }
            // `-c` 的值按 TOML 解析：整段人设必须包成 basic string，否则空格/引号/换行会失败。
            let setting = if mode == Mode::Append {
                format!("developer_instructions={}", toml_string(&text.clone().unwrap_or_default()))
            } else {
                format!("model_instructions_file={}", toml_string(&path.clone().unwrap_or_default()))
            };
            let mut args = vec!["-c".to_owned(), setting];
            args.extend_from_slice(extra_args);
            LaunchPlan {
                program: "codex".to_owned(),
                args,
                note: Some("实际请求角色是 developer，不是 system".to_owned()),
                residue: None,
                no_cli: false,
                manual: Vec::new(),
            }
        }
        "claude" => {
            if !inject {
                return LaunchPlan::passthrough("claude", extra_args.to_vec());
            }
            let flag = if mode == Mode::Append { "--append-system-prompt-file" } else { "--system-prompt-file" };
            let mut args = vec![flag.to_owned(), path.clone().unwrap_or_default()];
            args.extend_from_slice(extra_args);
            LaunchPlan {
                program: "claude".to_owned(),
                args,
                note: None,
                residue: Some("不补丁时残留 62 字身份句".to_owned()),
                no_cli: false,
                manual: Vec::new(),
            }
        }
        "grok" => {
            if !inject {
                return LaunchPlan::passthrough("grok", extra_args.to_vec());
            }
            let flag = if mode == Mode::Append { "--rules" } else { "--system-prompt-override" };
            let mut args = vec![flag.to_owned(), text.clone().unwrap_or_default()];
            args.extend_from_slice(extra_args);
            LaunchPlan::passthrough("grok", args)
        }
        "codebuddy" => {
            if !inject {
                return LaunchPlan::passthrough("codebuddy", extra_args.to_vec());
            }
            // replace 走文件：内联 9.6K 会把 .cmd 包装器撞上 8191 上限。
            let mut args = if mode == Mode::Append {
                vec!["--append-system-prompt".to_owned(), text.clone().unwrap_or_default()]
            } else {
                vec!["--system-prompt-file".to_owned(), path.clone().unwrap_or_default()]
            };
            args.extend_from_slice(extra_args);
            LaunchPlan::passthrough("codebuddy", args)
        }
        "hermes" => {
            // 非 off 时追加 --ignore-rules；SOUL.md 的换出由 channel 层做。
            let mut args = Vec::new();
            if inject {
                args.push("--ignore-rules".to_owned());
            }
            args.extend_from_slice(extra_args);
            LaunchPlan::passthrough("hermes", args).with_note("SOUL.md 仅供新会话读取；退出后还原启动前的原字节")
        }
        "kilo" => {
            let mut args = vec!["run".to_owned()];
            args.extend_from_slice(extra_args);
            LaunchPlan::passthrough("kilo", args).with_note("注入靠临时安装 agent/code.md，退出时按原字节还原")
        }
        "opencode" => {
            let mut args = extra_args.to_vec();
            if args.iter().any(|a| a == "--attach" || a.starts_with("--attach=")) {
                // 一次性启用使用独立宿主；--attach 指向已有服务，那条服务不归我们管。
                return LaunchPlan::passthrough("opencode", args)
                    .with_manual(vec!["--attach 指向已有的 opencode 服务，那条服务不归 Metis 管；请给它装常驻通道".to_owned()]);
            }
            args.extend(extra_args.iter().cloned());
            LaunchPlan::passthrough("opencode", args).with_note("注入靠 .opencode/plugin 下的临时插件")
        }
        "opencode2" => {
            let mut args = extra_args.to_vec();
            args.extend(extra_args.iter().cloned());
            LaunchPlan::passthrough("opencode2", args).with_note("v2 插件 API 与 v1 不同，不能混用")
        }
        "dsh" => {
            let mut args = vec!["--profile".to_owned(), "headless".to_owned()];
            args.extend_from_slice(extra_args);
            LaunchPlan::passthrough("dsh", args).with_note("命令行只能起 headless；桌面/网页入口请装常驻插件")
        }
        "zcode" => LaunchPlan::passthrough("zcode", extra_args.to_vec()).with_manual(vec![
            "zcode 的 CLI 拿不到 customSystemPrompt（只有 app-server / SDK 能传）".to_owned(),
            "注入配方：~/.zcode/cli/config.json 里 hooks.enabled=true + UserPromptSubmit hook 返回 additionalContext".to_owned(),
            "这条路线 Metis 不代劳；身份要靠常驻 AGENTS.md 那条通道".to_owned(),
        ]),
        "antigravity" => LaunchPlan::passthrough("antigravity", extra_args.to_vec()).with_manual(vec![
            "antigravity 的系统提示词在二进制里（Go embed.FS），只能等长替换".to_owned(),
            "等长替换会让签名失效、更新被覆盖 —— Metis 不提供这条路线".to_owned(),
        ]),
        "pi-desktop" | "workbuddy" => LaunchPlan::passthrough(harness.as_str(), extra_args.to_vec())
            .desktop_only()
            .with_manual(vec![format!("{harness} 没有命令行入口"), format!("装它的常驻通道：metis arm {harness} --channel install")]),
        _ => LaunchPlan::passthrough(harness.as_str(), extra_args.to_vec()),
    }
}

/// 生成一次性问答（`--ask`）的计划。
///
/// 只收录**参数形状已知**的家；其余返回 `None`，调用方报错而不是猜。
pub fn oneshot(harness: &HarnessId, prompt: &str, mode: Mode, persona_text: Option<&str>, persona_path: Option<&str>) -> Option<OneshootPlan> {
    let text = persona_text.map(inline);
    let path = persona_path.map(forward_slashes);
    let inject = mode.injects();
    let plan = match harness.as_str() {
        "pi" => OneshootPlan {
            program: "pi".to_owned(),
            args: vec!["-p".to_owned(), prompt.to_owned()],
            note: Some("注入靠扩展（sys 层）；人设走 state.json，不占命令行".to_owned()),
        },
        "claude" => {
            let mut args = vec!["-p".to_owned(), prompt.to_owned()];
            if inject {
                args.push(if mode == Mode::Append { "--append-system-prompt-file" } else { "--system-prompt-file" }.to_owned());
                args.push(path.clone().unwrap_or_default());
            }
            OneshootPlan { program: "claude".to_owned(), args, note: None }
        }
        "codex" => {
            let mut args = Vec::new();
            if inject {
                let setting = if mode == Mode::Append {
                    format!("developer_instructions={}", toml_string(&text.clone().unwrap_or_default()))
                } else {
                    format!("model_instructions_file={}", toml_string(&path.clone().unwrap_or_default()))
                };
                args.push("-c".to_owned());
                args.push(setting);
            }
            args.push("exec".to_owned());
            args.push(prompt.to_owned());
            OneshootPlan { program: "codex".to_owned(), args, note: None }
        }
        "grok" => {
            let mut args = vec!["-p".to_owned(), prompt.to_owned(), "--always-approve".to_owned()];
            if inject {
                args.push(if mode == Mode::Append { "--rules" } else { "--system-prompt-override" }.to_owned());
                args.push(text.clone().unwrap_or_default());
            }
            OneshootPlan { program: "grok".to_owned(), args, note: None }
        }
        "omp" => {
            let mut args = vec!["-p".to_owned()];
            if inject {
                args.push(if mode == Mode::Append { "--append-system-prompt" } else { "--system-prompt" }.to_owned());
                args.push(text.clone().unwrap_or_default());
            }
            args.push(prompt.to_owned());
            OneshootPlan { program: "omp".to_owned(), args, note: None }
        }
        "opencode" => OneshootPlan { program: "opencode".to_owned(), args: vec!["run".to_owned(), prompt.to_owned()], note: None },
        "opencode2" => OneshootPlan { program: "opencode2".to_owned(), args: vec!["run".to_owned(), prompt.to_owned()], note: None },
        "dsh" => OneshootPlan { program: "dsh".to_owned(), args: vec!["--profile".to_owned(), "headless".to_owned(), prompt.to_owned()], note: None },
        "kilo" => {
            OneshootPlan { program: "kilo".to_owned(), args: vec!["run".to_owned(), prompt.to_owned(), "--format".to_owned(), "json".to_owned()], note: None }
        }
        "codebuddy" => {
            let mut args = vec!["-p".to_owned(), prompt.to_owned(), "--max-turns".to_owned(), "5".to_owned()];
            if inject {
                if mode == Mode::Append {
                    args.push("--append-system-prompt".to_owned());
                    args.push(text.clone().unwrap_or_default());
                } else {
                    args.push("--system-prompt-file".to_owned());
                    args.push(path.clone().unwrap_or_default());
                }
            }
            OneshootPlan { program: "codebuddy".to_owned(), args, note: None }
        }
        "hermes" => OneshootPlan {
            program: "hermes".to_owned(),
            args: vec!["-z".to_owned(), prompt.to_owned()],
            note: Some("SOUL.md 的换出由 channel 层负责".to_owned()),
        },
        "zcode" => OneshootPlan {
            program: "zcode".to_owned(),
            args: vec![
                "--prompt".to_owned(),
                prompt.to_owned(),
                "--mode".to_owned(),
                "yolo".to_owned(),
                "--output-format".to_owned(),
                "stream-json".to_owned(),
                "--no-color".to_owned(),
            ],
            note: Some("人设对 zcode CLI 不生效：customSystemPrompt 只有 app-server/SDK 能传".to_owned()),
        },
        // pi-desktop / workbuddy / antigravity：没有 CLI，也就没有一次性模式。
        _ => return None,
    };
    Some(plan)
}

/// `replace` 在这五家上实际按 `strict` 执行（启动入口没有宿主文本读取口）。
pub const STRICT_REPLACE: [&str; 5] = ["codex", "claude", "grok", "codebuddy", "hermes"];

/// 模式降级提示：`replace` 在这五家上要么提示、要么按 strict 执行。
pub fn replace_downgrade_note(harness: &HarnessId, mode: Mode) -> Option<String> {
    if mode != Mode::Replace || !STRICT_REPLACE.contains(&harness.as_str()) {
        return None;
    }
    Some(format!("{harness} 没有宿主提示词读取口：replace 实际按 strict 执行（宿主原有说明也会被拿掉）；想保留请用 --mode append"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("harness id")
    }

    const BODY: &str = "用简体中文协作。";

    fn plan_for(harness: &str, mode: Mode) -> LaunchPlan {
        plan(&id(harness), mode, Some(BODY), Some("C:\\Users\\example\\.metis\\personas\\p.md"), &[])
    }

    #[test]
    fn off_mode_adds_no_injection_arguments() {
        for harness in ["pi", "omp", "codex", "claude", "grok", "codebuddy", "hermes", "kilo", "opencode"] {
            let plan = plan_for(harness, Mode::Off);
            let joined = plan.args.join(" ");
            assert!(!joined.contains(BODY), "{harness} 在 off 下不该注入人设：{joined}");
            assert!(!joined.contains("--system-prompt"), "{harness} 在 off 下不该带注入参数：{joined}");
        }
    }

    #[test]
    fn pi_passes_through_because_of_the_windows_command_line_limit() {
        let plan = plan_for("pi", Mode::Replace);
        assert_eq!(plan.args, Vec::<String>::new(), "pi 不能在命令行带人设（8191 上限）");
        assert!(plan.note.as_deref().unwrap_or_default().contains("扩展"));
    }

    #[test]
    fn codex_wraps_the_persona_path_as_a_toml_string() {
        let plan = plan_for("codex", Mode::Replace);
        assert_eq!(plan.args[0], "-c");
        assert!(plan.args[1].starts_with("model_instructions_file=\""));
        assert!(plan.args[1].contains("C:/Users/example"), "路径必须转正斜杠：{}", plan.args[1]);
        assert!(plan.note.as_deref().unwrap_or_default().contains("developer"), "必须说明实际是 developer 角色");
    }

    #[test]
    fn codex_append_escapes_the_persona_as_toml() {
        let plan = plan(harness_of("codex"), Mode::Append, Some("带 \"引号\" 与\n换行"), Some("/p.md"), &[]);
        let setting = &plan.args[1];
        assert!(setting.starts_with("developer_instructions=\""));
        assert!(setting.contains("\\\""), "引号必须转义：{setting}");
        assert!(setting.contains("\\n"), "换行必须转义：{setting}");
    }

    fn harness_of(value: &str) -> &'static HarnessId {
        // 让测试里能直接传引用；用 Box::leak 只为测试方便。
        Box::leak(Box::new(id(value)))
    }

    #[test]
    fn claude_uses_the_file_flags() {
        let replace = plan_for("claude", Mode::Replace);
        assert_eq!(replace.args[0], "--system-prompt-file");
        let append = plan_for("claude", Mode::Append);
        assert_eq!(append.args[0], "--append-system-prompt-file");
        assert!(replace.residue.as_deref().unwrap_or_default().contains("62"));
    }

    #[test]
    fn grok_uses_override_and_rules() {
        assert_eq!(plan_for("grok", Mode::Replace).args[0], "--system-prompt-override");
        assert_eq!(plan_for("grok", Mode::Append).args[0], "--rules");
    }

    #[test]
    fn omp_uses_system_prompt_flags() {
        assert_eq!(plan_for("omp", Mode::Replace).args[0], "--system-prompt");
        assert_eq!(plan_for("omp", Mode::Append).args[0], "--append-system-prompt");
        assert!(plan_for("omp", Mode::Replace).residue.is_some(), "omp 的 MCP 残留要说明");
    }

    #[test]
    fn codebuddy_replace_uses_a_file_because_of_the_cmd_wrapper_limit() {
        let plan = plan_for("codebuddy", Mode::Replace);
        assert_eq!(plan.args[0], "--system-prompt-file", "replace 必须走文件，避免 8191 上限");
        let append = plan_for("codebuddy", Mode::Append);
        assert_eq!(append.args[0], "--append-system-prompt");
    }

    #[test]
    fn hermes_adds_ignore_rules_only_when_injecting() {
        assert!(plan_for("hermes", Mode::Replace).args.contains(&"--ignore-rules".to_owned()));
        assert!(!plan_for("hermes", Mode::Off).args.contains(&"--ignore-rules".to_owned()));
    }

    #[test]
    fn dsh_forces_the_headless_profile() {
        let plan = plan_for("dsh", Mode::Replace);
        assert_eq!(&plan.args[..2], &["--profile".to_owned(), "headless".to_owned()]);
    }

    #[test]
    fn desktop_only_harnesses_report_no_cli_and_give_a_next_step() {
        for harness in ["pi-desktop", "workbuddy"] {
            let plan = plan_for(harness, Mode::Replace);
            assert!(plan.no_cli, "{harness} 应标记为没有命令行入口");
            assert!(!plan.manual.is_empty(), "{harness} 应给出下一步");
            assert!(plan.manual.iter().any(|line| line.contains("arm")), "{harness} 应提示装常驻通道");
        }
    }

    #[test]
    fn zcode_and_antigravity_give_manual_recipes() {
        for harness in ["zcode", "antigravity"] {
            let plan = plan_for(harness, Mode::Replace);
            assert!(!plan.manual.is_empty(), "{harness} 应给人工配方");
        }
        assert!(plan_for("antigravity", Mode::Replace).manual.iter().any(|l| l.contains("Metis 不提供")));
    }

    #[test]
    fn preview_redacts_the_persona_body() {
        let long = "x".repeat(500);
        let plan = plan(harness_of("omp"), Mode::Replace, Some(&long), Some("/p.md"), &[]);
        let preview = plan.preview();
        assert!(!preview.contains(&long), "预览不能泄漏完整人设：{preview}");
        assert!(preview.contains("500 字"), "应替换成字数占位：{preview}");
    }

    #[test]
    fn preview_redacts_developer_instructions() {
        let long = "y".repeat(300);
        let plan = plan(harness_of("codex"), Mode::Append, Some(&long), Some("/p.md"), &[]);
        let preview = plan.preview();
        assert!(!preview.contains(&long), "预览不能泄漏人设：{preview}");
        assert!(preview.contains("<persona"), "应替换成占位：{preview}");
    }

    #[test]
    fn replace_downgrade_note_only_for_the_five() {
        for harness in STRICT_REPLACE {
            assert!(replace_downgrade_note(&id(harness), Mode::Replace).is_some(), "{harness} 应提示降级");
        }
        assert!(replace_downgrade_note(&id("pi"), Mode::Replace).is_none(), "pi 有扩展，不需要降级提示");
        assert!(replace_downgrade_note(&id("codex"), Mode::Append).is_none(), "append 不降级");
    }

    #[test]
    fn replace_downgrade_note_tells_the_user_what_to_do() {
        let note = replace_downgrade_note(&id("claude"), Mode::Replace).expect("note");
        assert!(note.contains("append"), "必须告诉用户改用 append：{note}");
    }

    #[test]
    fn oneshot_covers_the_known_harnesses_and_refuses_the_rest() {
        for harness in ["pi", "claude", "codex", "grok", "omp", "opencode", "opencode2", "dsh", "kilo", "codebuddy", "hermes", "zcode"] {
            assert!(oneshot(&id(harness), "解释这段代码", Mode::Replace, Some(BODY), Some("/p.md")).is_some(), "{harness} 应有一性次模式");
        }
        for harness in ["pi-desktop", "workbuddy", "antigravity"] {
            assert!(oneshot(&id(harness), "x", Mode::Replace, Some(BODY), Some("/p.md")).is_none(), "{harness} 没有 CLI，不该猜");
        }
    }

    #[test]
    fn oneshot_off_mode_has_no_injection() {
        let plan = oneshot(&id("claude"), "问", Mode::Off, Some(BODY), Some("/p.md")).expect("plan");
        assert_eq!(plan.args, vec!["-p".to_owned(), "问".to_owned()]);
    }

    #[test]
    fn oneshot_codex_puts_the_prompt_after_exec() {
        let plan = oneshot(&id("codex"), "问", Mode::Replace, Some(BODY), Some("/p.md")).expect("plan");
        let exec = plan.args.iter().position(|a| a == "exec").expect("exec");
        assert_eq!(plan.args.get(exec + 1).map(String::as_str), Some("问"));
    }

    #[test]
    fn oneshot_zcode_says_the_persona_does_not_apply() {
        let plan = oneshot(&id("zcode"), "问", Mode::Replace, Some(BODY), Some("/p.md")).expect("plan");
        assert!(plan.note.as_deref().unwrap_or_default().contains("不生效"));
    }

    #[test]
    fn every_builtin_harness_produces_some_plan() {
        // 不允许"这家的 plan 是空的"：要么有参数、要么有 manual、要么标记 desktop_only。
        for harness in HarnessId::BUILTIN {
            let plan = plan(&id(harness), Mode::Replace, Some(BODY), Some("/p.md"), &[]);
            let informative = !plan.args.is_empty() || !plan.manual.is_empty() || plan.no_cli || plan.note.is_some();
            assert!(informative, "{harness} 的 plan 什么都没说");
        }
    }

    #[test]
    fn extra_args_are_passed_through() {
        let plan = plan(&id("codex"), Mode::Replace, Some(BODY), Some("/p.md"), &["--help".to_owned()]);
        assert_eq!(plan.args.last().map(String::as_str), Some("--help"));
    }
}
