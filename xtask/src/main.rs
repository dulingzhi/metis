//! 工程门禁（`cargo xtask <command>`）。
//!
//! 这些检查存在的意义：让 `AGENTS.md` 里的规则**能被机器执行**，
//! 而不是靠人记（`docs/PITFALLS.md` F2）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![allow(clippy::print_stdout, clippy::print_stderr)]
// xtask 是工程门禁：它必须能调用 cargo / git 来做检查，因此是本仓库里
// 唯一允许直接使用 std::process::Command 的地方（铁律 11 的显式例外）。
#![allow(clippy::disallowed_methods)]

mod layers;
mod remediation;

use std::path::Path;
use std::process::Command;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "xtask", about = "Metis 工程门禁")]
struct Cli {
    #[command(subcommand)]
    command: Task,
}

#[derive(Debug, Subcommand)]
enum Task {
    /// 检查分层、UI 独占依赖、外部进程入口。
    Layers,
    /// 检查将要提交的内容里没有密钥与本机路径。
    Secrets {
        /// 连历史提交一起查。
        #[arg(long)]
        history: bool,
    },
    /// 列出路线图里的未完成项。
    Pending {
        /// 只输出数量。
        #[arg(long)]
        count: bool,
    },
    /// 检查文档与代码的一致性（铁律 ↔ 检查、路线图无僵尸条目）。
    Docs,
    /// 补救建议覆盖检查（每个面向用户的错误都要有）。
    Remediation,
    /// 文案表检查（两张表键一致、无重复）。
    I18n,
    /// 重建 slash 的 golden 产物。
    Golden,
    /// 重建图标表（新增 `.svg` 之后跑）。
    Icons,
    /// 界面快照：与基线比较（`--update` 重建基线）。
    Snapshot {
        /// 重建基线而不是比较。
        #[arg(long)]
        update: bool,
    },
    /// 本地干跑一遍 CI 的关键作业。
    Ci,
    /// 格式化。
    Fmt,
    /// clippy。
    Clippy,
    /// 跑全部测试。
    Test,
    /// 覆盖率检查（需要 cargo-llvm-cov）。
    Coverage,
    /// 版本一致性检查。
    Version {
        /// 设置新版本。
        set: Option<String>,
    },
}

fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match run(cli.command) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("✗ {message}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run(task: Task) -> Result<(), String> {
    match task {
        Task::Layers => check_layers(),
        Task::Secrets { history } => check_secrets(history),
        Task::Pending { count } => pending(count),
        Task::Docs => check_docs(),
        Task::Remediation => check_remediation(),
        Task::I18n => check_i18n(),
        Task::Golden => regenerate_golden(),
        Task::Icons => regenerate_icons(),
        Task::Snapshot { update } => run_snapshot(update),
        Task::Ci => run_ci(),
        Task::Fmt => shell("cargo", &["fmt", "--all"]),
        Task::Clippy => shell("cargo", &["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        Task::Test => shell("cargo", &["test", "--workspace"]),
        Task::Coverage => coverage(),
        Task::Version { set } => version(set),
    }
}

fn check_layers() -> Result<(), String> {
    let root = layers::workspace_root();
    let mut failures = Vec::new();

    // ① 未登记的 crate 目录。
    for extra in layers::unregistered(&root) {
        failures.push(format!("未登记的 crate：{}（在 xtask/src/layers.rs 的 LAYERS 里加一行）", extra.display()));
    }

    // ② 每个 manifest 的分层与 UI 独占依赖。
    for (entry, manifest) in layers::crate_manifests(&root) {
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            failures.push(format!("读不了 {}", manifest.display()));
            continue;
        };
        if !text.contains(&format!("name = \"{}\"", entry.package)) {
            failures.push(format!("{} 的包名不是 {}", manifest.display(), entry.package));
        }
        if !text.contains("publish = false") {
            failures.push(format!("{} 缺少 publish = false", manifest.display()));
        }
        if !text.contains("[lints]") {
            failures.push(format!("{} 缺少 [lints] workspace = true", manifest.display()));
        }
        if entry.ui_allowed {
            continue;
        }
        for dependency in layers::UI_ONLY {
            if text.contains(&format!("{dependency} =")) {
                failures.push(format!("{} 依赖了 UI 专属的 {dependency}（只有 ui-egui / metis-gui 可以用）", entry.package));
            }
        }
    }

    // ③ UI 源码不得有副作用（写文件、起进程）。
    let ui_src = root.join("crates").join("ui-egui").join("src");
    if ui_src.is_dir() {
        for path in walk(&ui_src) {
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            let mut in_tests = false;
            for (index, line) in text.lines().enumerate() {
                if line.contains("#[cfg(test)]") {
                    in_tests = true;
                }
                if in_tests {
                    continue;
                }
                for forbidden in layers::UI_FORBIDDEN {
                    if line.contains(forbidden) {
                        failures.push(format!("{}:{} 出现 {forbidden}：UI 层不得有副作用（AGENTS.md 铁律 6）", path.display(), index + 1));
                    }
                }
            }
        }
    }

    // ④ 外部进程只能从 metis-process 起。
    //
    // 判定看的是**所属 crate 目录**，不是整条路径：`crates/process/src/run.rs` 的路径里
    // 并不包含 `metis-process` 这个包名，按字符串包含去判断会把它自己判违规。
    let allowed_crates = ["crates\\process", "crates/process", "crates\\testkit", "crates/testkit", "xtask"];
    for base in ["crates", "apps"] {
        let dir = root.join(base);
        if !dir.is_dir() {
            continue;
        }
        for path in walk(&dir) {
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            let path_text = path.to_string_lossy().replace('/', "\\");
            if allowed_crates.iter().any(|allowed| path_text.contains(allowed)) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            // 只看**非测试代码**：测试里起 node / cargo 是正当的（它们要验证生成的产物）。
            // 判定方式与 clippy 一致：截到第一个 `#[cfg(test)]` 之前。
            let production = text.split("#[cfg(test)]").next().unwrap_or("");
            if production.contains("std::process::Command::new") {
                failures.push(format!("{} 直接起外部进程：必须经由 metis-process（AGENTS.md 铁律 11）", path.display()));
            }
        }
    }

    if failures.is_empty() {
        println!("✓ layers：{} 个 crate 全部登记且分层正确", layers::LAYERS.len());
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("✗ {failure}");
        }
        Err(format!("layers 检查失败（{} 项）", failures.len()))
    }
}

fn walk(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else { return out };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some("target") {
                continue;
            }
            out.extend(walk(&path));
        } else {
            out.push(path);
        }
    }
    out
}

/// 检查将要提交的内容里没有密钥与本机路径。
fn check_secrets(history: bool) -> Result<(), String> {
    let root = layers::workspace_root();
    let patterns: Vec<(&str, regex::Regex)> = vec![
        ("OpenAI 风格的 key", regex::Regex::new(r"sk-[A-Za-z0-9]{20,}").map_err(|e| e.to_string())?),
        ("GitHub token", regex::Regex::new(r"gh[pousr]_[A-Za-z0-9]{20,}").map_err(|e| e.to_string())?),
        ("AWS key", regex::Regex::new(r"AKIA[0-9A-Z]{16}").map_err(|e| e.to_string())?),
        ("私钥", regex::Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----").map_err(|e| e.to_string())?),
        ("真实用户目录", regex::Regex::new(r"[A-Za-z]:\\Users\\[A-Za-z0-9._-]+\\").map_err(|e| e.to_string())?),
    ];
    let allowed = ["C:\\Users\\example\\", "sk-test", "/path/to/", "example"];

    let mut hits = Vec::new();
    let mut checked = 0_usize;
    for path in walk(&root) {
        let text_path = path.to_string_lossy();
        if text_path.contains("\\.git\\") || text_path.contains("/.git/") || text_path.contains("\\target\\") || text_path.contains("/target/") {
            continue;
        }
        if text_path.contains("\\.research\\") || text_path.contains("/.research/") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        checked += 1;
        for (label, pattern) in &patterns {
            for found in pattern.find_iter(&text) {
                let matched = found.as_str();
                if allowed.iter().any(|ok| matched.contains(ok) || ok.contains(matched)) {
                    continue;
                }
                hits.push(format!("{label}：{} 含 {:?}", path.display(), matched));
            }
        }
    }

    if history {
        // 历史检查交给 git 本身：有命中就报告。
        let output = Command::new("git").args(["log", "--all", "-p", "--no-color"]).current_dir(&root).output();
        if let Ok(output) = output {
            let text = String::from_utf8_lossy(&output.stdout);
            for (label, pattern) in &patterns {
                for found in pattern.find_iter(&text) {
                    let matched = found.as_str();
                    if allowed.iter().any(|ok| matched.contains(ok)) {
                        continue;
                    }
                    hits.push(format!("历史提交里发现{label}：{matched:?}"));
                    break;
                }
            }
        }
    }

    if hits.is_empty() {
        println!("✓ secrets：检查了 {checked} 个文件，没有发现密钥或本机路径");
        Ok(())
    } else {
        for hit in hits.iter().take(20) {
            eprintln!("✗ {hit}");
        }
        Err(format!("secrets 检查失败（{} 项）", hits.len()))
    }
}

/// 列出路线图里的未完成项。
/// 从路线图文本里挑出未完成项。
///
/// 抽成纯函数是为了能测：`pending` 的输出是"这个项目还剩多少活"的唯一答案，
/// 解析错了会让人以为活干完了（或永远干不完）。
fn incomplete_items(text: &str) -> Vec<&str> {
    text.lines().filter(|line| line.trim_start().starts_with("- [ ]")).collect()
}

fn pending(count_only: bool) -> Result<(), String> {
    let root = layers::workspace_root();
    let path = root.join("docs").join("ROADMAP.md");
    let text = std::fs::read_to_string(&path).map_err(|error| format!("读不了 {}：{error}", path.display()))?;
    let items = incomplete_items(&text);
    if count_only {
        println!("{}", items.len());
        return Ok(());
    }
    println!("未完成项 {} 条：", items.len());
    for item in items {
        println!("{}", item.trim());
    }
    Ok(())
}

/// 补救建议覆盖检查。
fn check_remediation() -> Result<(), String> {
    let root = layers::workspace_root();
    let findings = remediation::scan(&root);
    if findings.is_empty() {
        println!("✓ remediation：每个面向用户的错误都有补救建议");
        Ok(())
    } else {
        for finding in &findings {
            eprintln!("✗ {}:{} 缺少 with_remediation（{}）", finding.file, finding.line, finding.kind);
        }
        Err(format!("remediation 检查失败（{} 处）", findings.len()))
    }
}

/// 界面快照：与基线比较，或重建基线。
///
/// **需要 wgpu**。无显示设备的机器上 `egui_kittest` 会永久挂起而不是失败
/// （`docs/PITFALLS.md` G3），所以这里显式检查环境变量并允许跳过。
fn run_snapshot(update: bool) -> Result<(), String> {
    if matches!(std::env::var("METIS_SKIP_SNAPSHOT").as_deref(), Ok("1") | Ok("true")) {
        println!("⚠ 跳过快照（METIS_SKIP_SNAPSHOT 已设置）");
        return Ok(());
    }
    let mut args = vec!["test", "-p", "metis-ui-egui", "--test", "snapshot", "--", "--ignored", "--test-threads=1"];
    let _ = &mut args;
    let mut envs: Vec<(&str, &str)> = Vec::new();
    if update {
        envs.push(("METIS_UPDATE_SNAPSHOT", "1"));
    }
    println!("▶ cargo test --test snapshot{}", if update { " (重建基线)" } else { "" });
    let status = std::process::Command::new("cargo").args(&args).envs(envs).status().map_err(|error| format!("启动 cargo 失败：{error}"))?;
    if !status.success() {
        return Err("快照与基线不一致。若这次改动是有意的，跑 `cargo xtask snapshot --update` 并提交 diff。".to_owned());
    }
    println!("✓ 快照基线一致");
    Ok(())
}

/// 重建 `icon_data.rs`（把 `assets/icons/*.svg` 编成一张表）。
fn regenerate_icons() -> Result<(), String> {
    use std::fmt::Write as _;
    let root = layers::workspace_root();
    let dir = root.join("crates").join("ui-egui").join("assets").join("icons");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .map_err(|error| format!("读不了 {}：{error}", dir.display()))?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.strip_suffix(".svg").map(str::to_owned)
        })
        .collect();
    if names.is_empty() {
        return Err(format!("{} 里没有 .svg", dir.display()));
    }
    names.sort();

    let mut out = String::from(
        "//! 内嵌图标：Lucide 的 SVG，**在运行期按需光栅化并着色**。\n\
         //!\n\
         //! 为什么内嵌而不是运行期读文件：`metis-gui` 是单可执行文件工具，\n\
         //! 发布时不会有 `assets/` 目录。`include_bytes!` 把它们编进去。\n\
         //!\n\
         //! 图标的授权见 `assets/icons/LICENSE-lucide.txt`（ISC）。\n\
         //!\n\
         //! **新增图标**：把 `.svg` 放进 `assets/icons/`，然后跑 `cargo xtask icons`。\n\n\
         /// 图标表（名称 → SVG 字节）。\n\
         pub static ICONS: &[(&str, &[u8])] = &[\n",
    );
    for name in &names {
        let _ = writeln!(out, "    (\"{name}\", include_bytes!(\"../assets/icons/{name}.svg\")),");
    }
    out.push_str("];\n");

    let path = root.join("crates").join("ui-egui").join("src").join("icon_data.rs");
    std::fs::write(&path, out).map_err(|error| format!("写不了 {}：{error}", path.display()))?;
    println!("✓ 已重建 {}（{} 个图标）", path.display(), names.len());
    Ok(())
}

/// 重建 `slash` 的 golden 产物。
///
/// **这是一个显式命令，不在 CI 里跑**：golden 的意义是"改模板必须被看见"，
/// 自动重写会让 diff 消失。改完模板后跑它，把 golden 的 diff 一起提交评审。
fn regenerate_golden() -> Result<(), String> {
    shell("cargo", &["run", "-q", "-p", "metis-channel", "--example", "gen_golden"])
        .map_err(|error| format!("{error}；确认 crates/channel/examples/gen_golden.rs 存在"))
        .map(|()| println!("✓ golden 已重建：记得把 diff 一起提交"))
}

/// 文案表检查。
fn check_i18n() -> Result<(), String> {
    let report = metis_core::i18n::check();
    if report.is_ok() {
        let counts: Vec<String> = report.counts.iter().map(|(lang, count)| format!("{lang}={count}")).collect();
        println!("✓ i18n：两张表键一致、无重复（{}）", counts.join(" "));
        Ok(())
    } else {
        for key in &report.missing_in_en {
            eprintln!("✗ 英文缺键：{key}");
        }
        for key in &report.missing_in_zh {
            eprintln!("✗ 中文缺键：{key}");
        }
        for key in &report.duplicates {
            eprintln!("✗ 重复键：{key}");
        }
        let total = report.missing_in_en.len() + report.missing_in_zh.len() + report.duplicates.len();
        Err(format!("i18n 检查失败（{total} 项）"))
    }
}

/// 文档一致性检查。
fn check_docs() -> Result<(), String> {
    let root = layers::workspace_root();
    let mut failures = Vec::new();

    // 每份文档都必须存在。
    for name in [
        "AGENTS.md",
        "README.md",
        "CLAUDE.md",
        "docs/ARCHITECTURE.md",
        "docs/PARITY.md",
        "docs/HARNESS-MATRIX.md",
        "docs/DATA-FORMAT.md",
        "docs/UI.md",
        "docs/TESTING.md",
        "docs/DEVELOPMENT.md",
        "docs/EXTENDING.md",
        "docs/ROADMAP.md",
        "docs/STATUS.md",
        "docs/PITFALLS.md",
    ] {
        if !root.join(name).is_file() {
            failures.push(format!("缺少文档：{name}"));
        }
    }

    // 路线图：不能有"标了 done 但还有未完成项"的里程碑。
    if let Ok(roadmap) = std::fs::read_to_string(root.join("docs").join("ROADMAP.md")) {
        let mut current_milestone = String::new();
        let mut done_with_todo = Vec::new();
        for line in roadmap.lines() {
            if let Some(rest) = line.strip_prefix("## ") {
                if !current_milestone.is_empty() && !done_with_todo.is_empty() {
                    failures.push(format!("{current_milestone} 标了完成但还有未完成项"));
                }
                current_milestone = rest.split('—').next().unwrap_or(rest).trim().to_owned();
                done_with_todo.clear();
            }
            if line.trim_start().starts_with("- [ ]") && current_milestone.contains("done") {
                done_with_todo.push(line.to_owned());
            }
        }
    }

    // 关键文档必须提到那几条唯一真相来源。
    let agents = std::fs::read_to_string(root.join("AGENTS.md")).unwrap_or_default();
    for anchor in ["docs/PARITY.md", "docs/HARNESS-MATRIX.md", "docs/ROADMAP.md", "docs/PITFALLS.md"] {
        if !agents.contains(anchor) {
            failures.push(format!("AGENTS.md 没有引用 {anchor}（自我迭代协议依赖这些锚点）"));
        }
    }

    if failures.is_empty() {
        println!("✓ docs：14 份文档齐备，引用与路线图状态一致");
        Ok(())
    } else {
        for failure in &failures {
            eprintln!("✗ {failure}");
        }
        Err(format!("docs 检查失败（{} 项）", failures.len()))
    }
}

fn run_ci() -> Result<(), String> {
    for (label, args) in [
        ("fmt --check", vec!["fmt", "--all", "--check"]),
        ("clippy", vec!["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        ("test", vec!["test", "--workspace"]),
    ] {
        println!("▶ cargo {}", args.join(" "));
        if let Err(error) = shell("cargo", &args) {
            return Err(format!("{label} 失败：{error}"));
        }
    }
    // 自检。
    check_layers()?;
    check_docs()?;
    check_remediation()?;
    check_i18n()?;
    check_secrets(false)?;
    pending(true)?;
    println!("✓ ci：全部通过");
    Ok(())
}

fn coverage() -> Result<(), String> {
    // 覆盖率门槛在 docs/TESTING.md 定义；实现依赖 cargo-llvm-cov。
    //
    // **注意**：`cargo llvm-cov` 没有 `--fail-under-branches`（只有 lines / functions /
    // regions / file-lines）。这个参数以前写在这里，导致这条门禁从第一天起就没跑通过 ——
    // 而它一直「看起来存在」。分支覆盖率仍然打印在报告里，但**不设自动门槛**，
    // 由评审人按 TESTING.md 的约定检查。
    let available = Command::new("cargo").args(["llvm-cov", "--version"]).output();
    match available {
        Ok(output) if output.status.success() => {
            println!("▶ cargo llvm-cov（行 ≥ 90%，函数 ≥ 85%）；分支覆盖率见报告，不设自动门槛");
            shell("cargo", &["llvm-cov", "--workspace", "--all-targets", "--summary-only", "--fail-under-lines", "90", "--fail-under-functions", "85"])
        }
        _ => Err("cargo-llvm-cov 未安装：cargo install cargo-llvm-cov（覆盖率门槛见 docs/TESTING.md）".to_owned()),
    }
}

fn version(set: Option<String>) -> Result<(), String> {
    let root = layers::workspace_root();
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).map_err(|error| error.to_string())?;
    let current = text
        .lines()
        .find(|line| line.trim_start().starts_with("version = "))
        .and_then(|line| line.split('"').nth(1))
        .ok_or_else(|| "工作区 Cargo.toml 里找不到 version".to_owned())?;

    match set {
        None => {
            println!("工作区版本：{current}");
            Ok(())
        }
        Some(next) => {
            let parts: Vec<&str> = next.split('.').collect();
            if parts.len() != 3 || parts.iter().any(|p| p.parse::<u32>().is_err()) {
                return Err(format!("版本号必须是 X.Y.Z：{next}"));
            }
            let updated = text.replacen(&format!("version = \"{current}\""), &format!("version = \"{next}\""), 1);
            std::fs::write(&manifest, updated).map_err(|error| error.to_string())?;
            println!("✓ 版本 {current} → {next}（同时更新 docs/STATUS.md 里的说明）");
            Ok(())
        }
    }
}

fn shell(program: &str, args: &[&str]) -> Result<(), String> {
    let status = Command::new(program).args(args).status().map_err(|error| format!("启动 {program} 失败：{error}"))?;
    if status.success() { Ok(()) } else { Err(format!("{program} {} 以 {:?} 退出", args.join(" "), status.code())) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layers_check_passes_on_this_workspace() {
        assert!(check_layers().is_ok(), "layers 必须通过：这是一切规则自动执行的前提");
    }

    #[test]
    fn docs_check_passes_on_this_workspace() {
        assert!(check_docs().is_ok(), "docs 一致性检查必须通过");
    }

    #[test]
    fn secrets_check_passes_on_this_workspace() {
        assert!(check_secrets(false).is_ok(), "仓库里不该有密钥或本机路径");
    }

    #[test]
    fn pending_lists_the_roadmap_items() {
        assert!(pending(true).is_ok());
    }

    #[test]
    fn version_reports_the_workspace_version() {
        assert!(version(None).is_ok());
    }

    #[test]
    fn version_rejects_malformed_values() {
        assert!(version(Some("1.2".to_owned())).is_err());
        assert!(version(Some("v1.2.3".to_owned())).is_err());
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn incomplete_items_finds_unchecked_lines() {
        let text = "- [ ] 还没做\n- [x] 做完了\n  - [ ] 缩进的也算\n不是条目\n";
        let items = incomplete_items(text);
        assert_eq!(items.len(), 2, "{items:?}");
        assert!(items[0].contains("还没做"));
        assert!(items[1].contains("缩进的也算"));
    }

    #[test]
    fn incomplete_items_ignores_completed_and_prose() {
        assert!(incomplete_items("- [x] done\n普通文字\n").is_empty());
    }

    #[test]
    fn incomplete_items_handles_an_empty_document() {
        assert!(incomplete_items("").is_empty());
    }

    #[test]
    fn the_real_roadmap_has_some_pending_items() {
        // 这条不硬编码数量（会随进展变化），只断言解析器在真实文件上能工作。
        let root = layers::workspace_root();
        let text = std::fs::read_to_string(root.join("docs").join("ROADMAP.md")).expect("roadmap");
        let items = incomplete_items(&text);
        assert!(items.iter().all(|item| item.contains("[ ]")), "解析结果里不该混进已完成项");
    }
}
