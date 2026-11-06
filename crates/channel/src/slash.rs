//! `slash` 通道：生成宿主侧的 JS/TS 产物。
//!
//! **Metis 自己从不运行 JS**，但它必须把这些文件写出来 —— 宿主加载的就是它们
//! （`docs/ARCHITECTURE.md` §8）。
//!
//! 三条硬规则：
//! 1. 模板是本仓库自己的资产（`assets/host/*.mjs`），用 `include_str!` 内嵌，**不复制上游代码**；
//! 2. 占位符只有两个：数据目录、执行器 id。别的一律编译期确定；
//! 3. 产物有 golden 测试：给定固定输入，字节必须与仓库里的 golden 一致。

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, HarnessId, MetisError, Result};
use metis_fs::footprint::{self, Footprint, Undo};

/// 宿主运行时（被其它模板 import）。
pub const RUNTIME: &str = include_str!("../assets/host/runtime.mjs");
/// pi / omp / pi-desktop 的扩展。
pub const SLASH_EXTENSION: &str = include_str!("../assets/host/slash-extension.mjs");
/// opencode v1 插件。
pub const OPENCODE_V1: &str = include_str!("../assets/host/opencode-v1.mjs");
/// opencode v2 插件。
pub const OPENCODE_V2: &str = include_str!("../assets/host/opencode-v2.mjs");
/// DSH cordis 插件。
pub const DSH_PLUGIN: &str = include_str!("../assets/host/dsh-plugin.mjs");
/// 运行时行为测试脚本（由 `cargo test` 通过 node 执行）。
pub const RUNTIME_BEHAVIOUR: &str = include_str!("../assets/host/runtime.behaviour.mjs");

/// 运行时文件名。
pub const RUNTIME_FILE: &str = "runtime.mjs";

/// 一次 slash 安装的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashOutcome {
    /// 写出来的主产物。
    pub file: PathBuf,
    /// 同目录下的运行时文件。
    pub runtime: PathBuf,
    /// 是否新写（`false` 表示内容已一致）。
    pub written: bool,
}

/// 这家执行器的 slash 产物规格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlashSpec {
    /// 相对宿主主目录的文件路径。
    pub relative: &'static str,
    /// 用哪个模板。
    pub template: Template,
}

/// 模板种类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    /// pi / omp / pi-desktop 的扩展。
    Extension,
    /// opencode v1。
    OpencodeV1,
    /// opencode v2。
    OpencodeV2,
    /// DSH cordis 插件。
    Dsh,
}

impl Template {
    /// 模板正文。
    pub fn source(self) -> &'static str {
        match self {
            Self::Extension => SLASH_EXTENSION,
            Self::OpencodeV1 => OPENCODE_V1,
            Self::OpencodeV2 => OPENCODE_V2,
            Self::Dsh => DSH_PLUGIN,
        }
    }
}

/// 查这家的 slash 规格。`None` 表示这家没有可用的 slash 通道。
pub fn spec_for(harness: &HarnessId) -> Option<SlashSpec> {
    match harness.as_str() {
        "pi" => Some(SlashSpec { relative: "extensions/metis.mjs", template: Template::Extension }),
        "omp" => Some(SlashSpec { relative: "agent/extensions/metis.mjs", template: Template::Extension }),
        "pi-desktop" => Some(SlashSpec { relative: "plugins/imported/metis/main.mjs", template: Template::Extension }),
        "opencode" => Some(SlashSpec { relative: "plugins/metis.mjs", template: Template::OpencodeV1 }),
        "opencode2" => Some(SlashSpec { relative: "plugins/metis-v2.mjs", template: Template::OpencodeV2 }),
        // DSH 的插件装在每个 profile 的 node_modules 下，由 engine 逐 profile 展开。
        // DSH 的插件必须放在 profile 的 `node_modules/<name>` 下：宿主按包名解析，
        // 放别处它会找不到（这条曾经写错，实测把文件写进了 profile 根目录）。
        "dsh" => Some(SlashSpec { relative: "node_modules/metis/index.mjs", template: Template::Dsh }),
        _ => None,
    }
}

/// 把模板实例化：替换占位符。
///
/// 只有两个占位符，且都做了转义 —— 数据目录里可能有反斜杠与引号。
pub fn render(template: Template, home: &Path, harness: &HarnessId) -> String {
    let home_literal = js_string(&home.display().to_string());
    let harness_literal = js_string(harness.as_str());
    template.source().replace("__METIS_HOME__", &home_literal).replace("__METIS_HARNESS__", &harness_literal)
}

/// 生成一个 JS 字符串字面量（含引号）。
fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            // 防止把 `</script>` 之类的东西塞进模板上下文。
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

/// 安装 slash 通道：写主产物 + 运行时。
///
/// 幂等：内容一致时不重写，返回 `written: false`。
pub fn install(home: &Path, harness: &HarnessId, harness_home: &Path) -> Result<SlashOutcome> {
    let spec = spec_for(harness).ok_or_else(|| {
        MetisError::new(ErrorKind::UnsupportedChannel, format!("{harness} 没有能改系统提示词的 /purge"))
            .with_remediation("这家只能用常驻配置或启动参数（见 docs/HARNESS-MATRIX.md）")
    })?;

    // relative 用正斜杠书写（跨平台可读），join 之前换成平台分隔符，
    // 否则 Windows 上会出现 `node_modules/metis/index.mjs` 这种混合路径。
    let file = harness_home.join(spec.relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    let runtime = file.parent().map(|dir| dir.join(RUNTIME_FILE)).unwrap_or_else(|| harness_home.join(RUNTIME_FILE));

    let content = render(spec.template, home, harness);
    let mut written = false;

    // 内容一致也要**核对账本**：文件可能是别的 profile 写的、或账本被清过。
    // 只按"内容相同就跳过"会让账本漏记，撤销时留一堆孤儿文件（实测踩过）。
    for (target, want) in [(&runtime, RUNTIME.to_owned()), (&file, content)] {
        let current = metis_fs::read_to_string_optional(target)?;
        if current.as_deref() != Some(want.as_str()) {
            metis_fs::write_atomic(target, want.as_bytes())?;
            written = true;
        }
        record_owned(home, harness, target)?;
    }

    Ok(SlashOutcome { file, runtime, written })
}

/// 枚举"每个 profile 一份"的落点目录。
///
/// 只返回**已经存在**的 profile（`<root>/<profiles>/<name>`）：凭空创建 profile
/// 会让宿主的包管理器认不出来，反而更难收拾（`HARNESS-MATRIX.md` dsh 一节）。
pub fn profile_dirs(_home: &Path, _harness: &HarnessId, harness_home: &Path, relative: &str) -> Vec<PathBuf> {
    let root = harness_home.join(relative);
    let Ok(entries) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut out: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        // 只认真正的 profile：必须有 `package.json`。
        // 这一条很关键：`profiles/node_modules` 也是个目录，把它当 profile 会写错地方。
        .filter(|path| path.join("package.json").is_file())
        .collect();
    out.sort();
    out
}
/// 卸载 slash 通道：删文件、销账、并清掉我们建的空目录。
///
/// 为什么必须按账本而不是"按 spec 重算路径"：落点可能是按 profile 展开的，
/// 而 profile 列表在卸载时可能已经变了（用户删了 profile）。账本记的是**实际写过什么**。
pub fn revert(home: &Path, harness: &HarnessId) -> Result<usize> {
    let mut removed = 0_usize;
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in footprint::load(home)? {
        if entry.harness != harness.as_str() || entry.channel != "slash" {
            continue;
        }
        metis_fs::remove_if_exists(&entry.path)?;
        if let Some(parent) = entry.path.parent() {
            dirs.push(parent.to_path_buf());
        }
        footprint::cancel(home, footprint::CancelKey::Id(&entry.id))?;
        removed += 1;
    }
    // 清空目录：只删**空**的，用户往里放过别的东西就留着。
    dirs.sort();
    dirs.dedup();
    for dir in dirs {
        let _ = std::fs::remove_dir(&dir);
    }
    Ok(removed)
}

fn record_owned(home: &Path, harness: &HarnessId, file: &Path) -> Result<()> {
    footprint::record(
        home,
        Footprint {
            id: String::new(),
            path: file.to_path_buf(),
            kind: "file".to_owned(),
            harness: harness.as_str().to_owned(),
            channel: "slash".to_owned(),
            undo: Undo::Generated,
            sha256: metis_fs::hash_file(file)?,
            created_at: String::new(),
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod golden_tests {
    //! **golden 测试**：给定固定输入，产物字节必须与仓库里的 golden 一致。
    //!
    //! 为什么必须有：`slash` 的产物是**宿主真正加载的代码**。模板一改，
    //! 所有装了 Metis 的宿主行为都会变——而"看起来没变"是很容易骗过眼睛的。
    //! golden 把"当前产物长什么样"变成可 diff 的事实。
    //!
    //! 重建方式（有意写成显式命令，而不是自动重写）：
    //! `cargo xtask golden` —— 改模板后跑它，然后把 diff 一起提交评审。
    use super::{Template, render};
    use metis_core::HarnessId;

    /// golden 用的固定输入。**改这个就等于改所有 golden**，要单独说明理由。
    const FIXTURE_HOME: &str = "C:/Users/example/.metis";
    const FIXTURE_HARNESS: &str = "codex";

    fn expected(name: &str) -> &'static str {
        match name {
            "extension.mjs" => include_str!("../assets/golden/extension.mjs"),
            "opencode-v1.mjs" => include_str!("../assets/golden/opencode-v1.mjs"),
            "opencode-v2.mjs" => include_str!("../assets/golden/opencode-v2.mjs"),
            "dsh-plugin.mjs" => include_str!("../assets/golden/dsh-plugin.mjs"),
            "runtime.mjs" => include_str!("../assets/golden/runtime.mjs"),
            other => panic!("没有这个 golden：{other}"),
        }
    }

    fn check(name: &str, template: Template) {
        let harness = HarnessId::parse(FIXTURE_HARNESS).expect("id");
        let actual = render(template, std::path::Path::new(FIXTURE_HOME), &harness);
        let want = expected(name);
        assert_eq!(
            actual, want,
            "\n{name} 与 golden 不一致。\n\
             如果这次改动**是有意**的：跑 `cargo xtask golden` 重建，并把 diff 一起提交。\n\
             如果是无意的：说明模板被改坏了。\n"
        );
    }

    #[test]
    fn extension_matches_golden() {
        check("extension.mjs", Template::Extension);
    }

    #[test]
    fn opencode_v1_matches_golden() {
        check("opencode-v1.mjs", Template::OpencodeV1);
    }

    #[test]
    fn opencode_v2_matches_golden() {
        check("opencode-v2.mjs", Template::OpencodeV2);
    }

    #[test]
    fn dsh_plugin_matches_golden() {
        check("dsh-plugin.mjs", Template::Dsh);
    }

    #[test]
    fn runtime_matches_golden() {
        // `runtime.mjs` 是**原样写出**的（不经过占位符替换），所以它不复用 `check()`。
        // 但它是所有其它产物 import 的核心 —— 改坏了全盘皆坏，必须有 golden。
        assert_eq!(super::RUNTIME, expected("runtime.mjs"), "runtime.mjs 与 golden 不一致；有意改动请跑 `cargo xtask golden`");
    }

    #[test]
    fn every_template_has_a_golden() {
        // 新增模板却忘了加 golden，会被这条抓到。
        for template in [Template::Extension, Template::OpencodeV1, Template::OpencodeV2, Template::Dsh] {
            let name = match template {
                Template::Extension => "extension.mjs",
                Template::OpencodeV1 => "opencode-v1.mjs",
                Template::OpencodeV2 => "opencode-v2.mjs",
                Template::Dsh => "dsh-plugin.mjs",
            };
            assert!(!expected(name).is_empty(), "{name} 的 golden 是空的");
        }
    }

    #[test]
    fn golden_contains_the_fixture_values() {
        // golden 必须真的带上了固定的 home 与 harness —— 否则它测的是"模板原文"，
        // 而不是"替换之后产物"。
        for name in ["extension.mjs", "opencode-v1.mjs", "opencode-v2.mjs", "dsh-plugin.mjs"] {
            let text = expected(name);
            assert!(text.contains(FIXTURE_HOME), "{name} 里没有固定的 home");
            assert!(!text.contains("__METIS_HOME__"), "{name} 还有未替换的占位符");
            assert!(!text.contains("__METIS_HARNESS__"), "{name} 还有未替换的占位符");
        }
    }

    #[test]
    fn golden_never_contains_a_real_persona_body() {
        // 隐私：产物里只应该有路径与 id，不该出现任何人设正文。
        for name in ["extension.mjs", "opencode-v1.mjs", "opencode-v2.mjs", "dsh-plugin.mjs"] {
            let text = expected(name);
            assert!(!text.contains("用简体中文"), "{name} 里出现了人设正文");
        }
    }
}

#[cfg(test)]
// 测试要调 node 校验生成的 JS：这是唯一合理的例外（测试代码，不起宿主进程）。
#[allow(clippy::disallowed_methods)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("harness id")
    }

    #[test]
    fn profile_dirs_require_a_package_json() {
        // 回归：profiles/node_modules 也是个目录，把它当 profile 会把插件写到错的地方。
        let dir = tempfile::tempdir().expect("tempdir");
        let profiles = dir.path().join("profiles");
        std::fs::create_dir_all(profiles.join("desktop")).expect("mkdir");
        std::fs::create_dir_all(profiles.join("node_modules").join("something")).expect("mkdir");
        std::fs::write(profiles.join("desktop").join("package.json"), "{}").expect("write");

        let found = profile_dirs(dir.path(), &id("dsh"), dir.path(), "profiles");
        assert_eq!(found.len(), 1, "只有带 package.json 的才算 profile：{found:?}");
        assert!(found[0].ends_with("desktop"));
    }

    #[test]
    fn profile_dirs_returns_empty_when_nothing_exists() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(profile_dirs(dir.path(), &id("dsh"), dir.path(), "profiles").is_empty());
    }

    #[test]
    fn installed_path_uses_platform_separators() {
        // 回归：`relative` 用正斜杠书写，落盘路径必须是平台分隔符，不能出现混用。
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let harness_home = dir.path().join("dsh");
        std::fs::create_dir_all(harness_home.join("profiles").join("desktop")).expect("mkdir");
        std::fs::write(harness_home.join("profiles").join("desktop").join("package.json"), "{}").expect("write");

        let profile = harness_home.join("profiles").join("desktop");
        let outcome = install(&home, &id("dsh"), &profile).expect("install");
        let rendered = outcome.file.display().to_string();
        assert!(!rendered.contains("node_modules/metis"), "路径里不该有正斜杠混用：{rendered}");
        assert!(outcome.file.is_file());
    }

    #[test]
    fn every_slash_harness_has_a_spec() {
        for value in ["pi", "omp", "pi-desktop", "opencode", "opencode2", "dsh"] {
            assert!(spec_for(&id(value)).is_some(), "{value} 应该有 slash 规格");
        }
        // 只有这六家有：其余家写 /purge 也不会进 system 层。
        assert!(spec_for(&id("codex")).is_none());
        assert!(spec_for(&id("claude")).is_none());
        assert!(spec_for(&id("antigravity")).is_none());
    }

    #[test]
    fn render_substitutes_both_placeholders() {
        let home = Path::new("C:\\Users\\example\\.metis");
        let rendered = render(Template::Extension, home, &id("pi"));
        assert!(!rendered.contains("__METIS_HOME__"), "占位符必须被替换");
        assert!(!rendered.contains("__METIS_HARNESS__"));
        assert!(rendered.contains("C:\\\\Users\\\\example\\\\.metis"), "反斜杠必须转义");
        assert!(rendered.contains("\"pi\""));
    }

    #[test]
    fn js_string_escapes_dangerous_characters() {
        assert_eq!(js_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(js_string("a\nb"), "\"a\\nb\"");
        assert_eq!(js_string("</script>"), "\"\\u003c/script\\u003e\"");
    }

    #[test]
    fn rendered_output_is_syntactically_valid_javascript() {
        // 用 node 的解析器验证，而不是我们自己断言"看起来对"。
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("metis home"); // 故意带空格
        for (value, template) in [
            ("pi", Template::Extension),
            ("omp", Template::Extension),
            ("pi-desktop", Template::Extension),
            ("opencode", Template::OpencodeV1),
            ("opencode2", Template::OpencodeV2),
            ("dsh", Template::Dsh),
        ] {
            let rendered = render(template, &home, &id(value));
            let file = dir.path().join(format!("{value}.mjs"));
            std::fs::write(&file, &rendered).expect("write");
            let output = std::process::Command::new("node").args(["--check", &file.display().to_string()]).output().expect("node 必须可用");
            assert!(output.status.success(), "{value} 的产物不是合法 JS：\n{}\n---\n{rendered}", String::from_utf8_lossy(&output.stderr));
        }
    }

    #[test]
    fn runtime_is_syntactically_valid_javascript() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("runtime.mjs");
        std::fs::write(&file, RUNTIME).expect("write");
        let output = std::process::Command::new("node").args(["--check", &file.display().to_string()]).output().expect("node 必须可用");
        assert!(output.status.success(), "运行时不是合法 JS：\n{}", String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn install_then_revert_cleans_up() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let harness_home = dir.path().join("pi-home");
        std::fs::create_dir_all(&harness_home).expect("mkdir");

        let outcome = install(&home, &id("pi"), &harness_home).expect("install");
        assert!(outcome.written);
        assert!(outcome.file.is_file(), "主产物必须落盘");
        assert!(outcome.runtime.is_file(), "运行时必须落盘");
        assert_eq!(footprint::load(&home).expect("load").len(), 2, "两个文件各记一笔");

        let removed = revert(&home, &id("pi")).expect("revert");
        assert_eq!(removed, 2);
        assert!(!outcome.file.exists());
        assert!(!outcome.runtime.exists());
        assert!(footprint::load(&home).expect("load").is_empty(), "撤销后账本应清空");
    }

    #[test]
    fn install_is_idempotent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let harness_home = dir.path().join("pi-home");
        std::fs::create_dir_all(&harness_home).expect("mkdir");

        let first = install(&home, &id("pi"), &harness_home).expect("first");
        assert!(first.written);
        let second = install(&home, &id("pi"), &harness_home).expect("second");
        assert!(!second.written, "内容一致时不该重写");
    }

    #[test]
    fn unsupported_harness_is_refused_with_remediation() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = install(dir.path(), &id("codex"), dir.path()).expect_err("must refuse");
        assert_eq!(error.kind(), ErrorKind::UnsupportedChannel);
        assert!(error.remediation().is_some());
    }

    #[test]
    fn generated_file_never_embeds_the_persona_body() {
        // 产物只认路径，人设正文在运行时才读 —— 否则换人设就得重装。
        let rendered = render(Template::Extension, Path::new("/home/u/.metis"), &id("pi"));
        assert!(rendered.contains("readPersona"), "应在运行时读人设，而不是嵌进产物");
    }

    #[test]
    fn runtime_behaviour_tests_pass() {
        // 语法正确不等于语义正确：直接跑一遍运行时的真实函数，
        // 断言注入、四种模式、幂等、失败降级。这是 slash 通道最核心的契约。
        let dir = tempfile::tempdir().expect("tempdir");
        let runtime = dir.path().join(RUNTIME_FILE);
        std::fs::write(&runtime, RUNTIME).expect("write runtime");
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).expect("mkdir");

        let script = dir.path().join("behaviour.mjs");
        std::fs::write(&script, RUNTIME_BEHAVIOUR).expect("write behaviour script");

        let output =
            std::process::Command::new("node").arg(&script).env("METIS_RUNTIME", &runtime).env("METIS_TEST_HOME", &home).output().expect("node 必须可用");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(output.status.success(), "运行时行为测试失败：\n{stderr}");
        assert!(stdout.contains("PASS"), "应报告通过数量：{stdout}");
        // 至少覆盖了：stripIdentity / rewriteTexts / resolveActivation / 人设 / epoch / 幂等 / 降级。
        let count: usize = stdout.trim().trim_start_matches("PASS ").parse().unwrap_or(0);
        assert!(count >= 30, "行为断言至少要 30 条，实际 {count}");
    }
}
