//! 探测：本机装了哪些执行器、版本、主目录、有哪些标记文件。
//!
//! 只读：不改任何文件、不起宿主（版本探测例外，见下）。
//!
//! 版本探测走 `metis-process`（铁律 11）：命令缺失、超时、非零退出都降级成"版本未知"，
//! 不让探测本身失败。

use std::path::{Path, PathBuf};

use metis_core::{HarnessId, Result};
use metis_process::Exec;

use crate::registry;

/// 一家的探测结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Detected {
    /// 哪家。
    pub harness: HarnessId,
    /// PATH 里找到的可执行文件（桌面版可能没有）。
    pub binary: Option<PathBuf>,
    /// 主目录（按环境变量与平台默认解析；目录可能不存在）。
    pub home: PathBuf,
    /// 主目录是否存在。
    pub home_exists: bool,
    /// 主目录里找到的标记文件。
    pub markers: Vec<String>,
    /// 版本字符串（`--version` 第一行；拿不到就是 `None`）。
    pub version: Option<String>,
    /// 只有桌面版、没有命令行入口。
    pub desktop_only: bool,
}

impl Detected {
    /// 本机装了吗：有可执行文件，或主目录存在（桌面版通常只有后者）。
    pub fn installed(&self) -> bool {
        self.binary.is_some() || self.home_exists
    }

    /// 一行状态摘要。
    pub fn summary(&self) -> String {
        if !self.installed() {
            return "未安装".to_owned();
        }
        match (&self.binary, &self.version) {
            (Some(binary), Some(version)) => format!("{}（{version}）", binary.display()),
            (Some(binary), None) => binary.display().to_string(),
            // 关键：**配置目录存在 ≠ 只有桌面版**。
            // 目录在、命令不在 PATH 上，是"装了但命令不在 PATH"，不是"没有命令行入口"。
            // 后者只由 static 事实决定（`desktop_only`），不能从文件系统推断。
            (None, _) if self.desktop_only => "只有桌面版（无命令行入口）".to_owned(),
            (None, _) => "配置目录在，但 PATH 里找不到命令".to_owned(),
        }
    }
}

/// 探测选项。
#[derive(Debug, Clone, Copy)]
pub struct DetectOptions {
    /// 是否探测版本（会起一次进程；调用方可以关掉以加快速度）。
    pub probe_versions: bool,
}

impl Default for DetectOptions {
    fn default() -> Self {
        Self { probe_versions: true }
    }
}

/// 探测一家（PATH 用真实环境）。
pub fn detect(harness: &HarnessId, env: impl Fn(&str) -> Option<String>, user_home: &Path, options: &DetectOptions) -> Result<Detected> {
    detect_with(harness, env, metis_process::find_program, user_home, options)
}

/// 探测一家，**查找函数可注入**。
///
/// 为什么必须可注入：开发机上真的装着 `codex`、`pi`，如果探测直接读真实 PATH，
/// 测试就永远不可能稳定（"未安装"那条断言会在有 codex 的机器上失败）。
pub fn detect_with(
    harness: &HarnessId,
    env: impl Fn(&str) -> Option<String>,
    find: impl Fn(&str) -> Option<PathBuf>,
    user_home: &Path,
    options: &DetectOptions,
) -> Result<Detected> {
    let Some(spec) = registry::find(harness.as_str()) else {
        return Ok(Detected {
            harness: harness.clone(),
            binary: None,
            home: user_home.to_path_buf(),
            home_exists: false,
            markers: Vec::new(),
            version: None,
            desktop_only: false,
        });
    };

    let binary = spec.probe.bins.iter().find_map(|bin| find(bin));
    let home = metis_paths::HarnessLayout::of(harness).map(|layout| layout.resolve(&env, user_home)).unwrap_or_else(|| user_home.to_path_buf());
    let home_exists = home.is_dir();
    let markers = if home_exists {
        spec.probe.markers.iter().filter(|marker| home.join(marker).exists()).map(|marker| (*marker).to_owned()).collect()
    } else {
        Vec::new()
    };

    let version = if options.probe_versions { binary.as_ref().and_then(|path| version_of(path, spec.probe.version_args.as_deref())) } else { None };

    Ok(Detected { harness: harness.clone(), binary, home, home_exists, markers, version, desktop_only: spec.probe.desktop_only })
}

/// 读版本：跑 `--version`，取第一行非空输出。
///
/// 失败一律返回 `None`：探测不该因为某一家版本命令怪而整份失败。
fn version_of(binary: &Path, args: Option<&[&str]>) -> Option<String> {
    let args = args?;
    let outcome = Exec::new(binary.display().to_string())
        .args(args.iter().copied())
        // 版本命令通常很快；给短一点的截止时间，避免探测整体变慢。
        .wall_clock(std::time::Duration::from_secs(5))
        .allow_output_overflow()
        .run()
        .ok()?;
    let text = if outcome.stdout.trim().is_empty() { outcome.stderr } else { outcome.stdout };
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    // 只保留第一行且在合理长度内：有的程序会打印整段 banner。
    Some(line.chars().take(120).collect())
}

/// 探测全部 15 家。
pub fn detect_all(env: impl Fn(&str) -> Option<String> + Sync, user_home: &Path, options: &DetectOptions) -> Vec<Detected> {
    // **并行**探测：15 家每家都要扫一遍 PATH 并可能起一次 `--version`，
    // 串行时首屏会明显卡顿（真机上 ~1.2s → ~0.2s）。
    //
    // `options.probe_versions` 为 false 时没有外部进程，纯文件系统检查，
    // 也能并行 —— 而且此时不会有进程风暴的顾虑。
    let ids: Vec<HarnessId> = registry::all().iter().filter_map(|spec| HarnessId::parse(spec.id).ok()).collect();
    let env = &env;
    let options = *options;

    // 结果必须**保持注册表顺序**：界面按这个顺序显示，乱序会让用户每次看到的列表都不一样。
    let results: Vec<Option<Detected>> = std::thread::scope(|scope| {
        let handles: Vec<_> = ids.iter().map(|harness| scope.spawn(move || detect(harness, env, user_home, &options).ok())).collect();
        handles.into_iter().map(|handle| handle.join().unwrap_or(None)).collect()
    });

    ids.into_iter()
        .zip(results)
        .map(|(harness, detected)| {
            detected.unwrap_or(Detected {
                harness,
                binary: None,
                home: user_home.to_path_buf(),
                home_exists: false,
                markers: Vec::new(),
                version: None,
                desktop_only: false,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("id")
    }

    /// 造一个假的宿主主目录：`<tmp>/homes/<harness>`。
    fn fake_home(tmp: &Path, harness: &str, markers: &[&str]) -> PathBuf {
        let home = tmp.join("homes").join(harness);
        std::fs::create_dir_all(&home).expect("mkdir");
        for marker in markers {
            let path = home.join(marker);
            if marker.ends_with(".exe") || marker.contains('.') {
                std::fs::write(&path, b"x").expect("write marker");
            } else {
                std::fs::create_dir_all(&path).expect("mkdir marker");
            }
        }
        home
    }

    #[test]
    fn not_installed_when_neither_binary_nor_home_exists() {
        let tmp = tempfile::tempdir().expect("tmp");
        let detected = detect_with(&id("codex"), |_| None, |_| None, tmp.path(), &DetectOptions { probe_versions: false }).expect("detect");
        assert!(!detected.installed());
        assert_eq!(detected.summary(), "未安装");
    }

    #[test]
    fn desktop_only_when_home_exists_without_binary() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = fake_home(tmp.path(), "pi-desktop", &["plugins", "pi.sqlite"]);
        let detected = detect_with(
            &id("pi-desktop"),
            |key| (key == "PI_DESKTOP_DATA_DIR").then(|| home.display().to_string()),
            |_| None,
            tmp.path(),
            &DetectOptions { probe_versions: false },
        )
        .expect("detect");
        assert!(detected.installed(), "主目录存在就算装了");
        assert!(detected.desktop_only);
        assert_eq!(detected.markers, vec!["plugins".to_owned(), "pi.sqlite".to_owned()]);
        assert!(detected.summary().contains("只有桌面版"));
    }

    #[test]
    fn config_dir_without_binary_is_not_reported_as_desktop_only() {
        // 回归：配置目录存在但命令不在 PATH 上，跟"只有桌面版"是两件事。
        let tmp = tempfile::tempdir().expect("tmp");
        let home = fake_home(tmp.path(), "pi", &["extensions"]);
        let detected = detect_with(
            &id("pi"),
            |key| (key == "PI_HOME").then(|| home.display().to_string()),
            |_| None, // PATH 里没有 pi
            tmp.path(),
            &DetectOptions { probe_versions: false },
        )
        .expect("detect");
        assert!(detected.installed(), "目录在就算装了");
        assert!(!detected.desktop_only, "pi 是有命令行的");
        assert!(detected.summary().contains("PATH"), "应说清是 PATH 问题：{}", detected.summary());
        assert!(!detected.summary().contains("桌面版"), "不该误判成桌面版：{}", detected.summary());
    }

    #[test]
    fn desktop_only_is_decided_by_static_facts_not_by_the_filesystem() {
        let tmp = tempfile::tempdir().expect("tmp");
        let home = fake_home(tmp.path(), "workbuddy", &["WorkBuddy.exe"]);
        let detected = detect_with(
            &id("workbuddy"),
            |key| (key == "WORKBUDDY_CONFIG_DIR").then(|| home.display().to_string()),
            |_| None,
            tmp.path(),
            &DetectOptions { probe_versions: false },
        )
        .expect("detect");
        assert!(detected.desktop_only);
        assert!(detected.summary().contains("桌面版"));
    }

    #[test]
    fn markers_are_listed_in_registry_order() {
        let tmp = tempfile::tempdir().expect("tmp");
        let _ = fake_home(tmp.path(), "opencode", &["opencode.json", "plugins"]);
        // opencode 的布局是 `<XDG_CONFIG_HOME>/opencode`，所以环境变量要给父目录。
        let detected = detect_with(
            &id("opencode"),
            |key| (key == "XDG_CONFIG_HOME").then(|| tmp.path().join("homes").display().to_string()),
            |_| None,
            tmp.path(),
            &DetectOptions { probe_versions: false },
        )
        .expect("detect");
        assert_eq!(detected.home, tmp.path().join("homes").join("opencode"), "主目录应拼在 XDG 之下");
        assert_eq!(detected.markers, vec!["opencode.json".to_owned(), "plugins".to_owned()]);
    }

    #[test]
    fn missing_markers_are_not_listed() {
        let tmp = tempfile::tempdir().expect("tmp");
        let _ = fake_home(tmp.path(), "codex", &[]);
        let detected = detect_with(
            &id("codex"),
            |key| (key == "CODEX_HOME").then(|| tmp.path().join("homes").join("codex").display().to_string()),
            |_| None,
            tmp.path(),
            &DetectOptions { probe_versions: false },
        )
        .expect("detect");
        assert!(detected.markers.is_empty(), "没有标记文件时不该编造");
    }

    #[test]
    fn version_probe_is_optional() {
        let tmp = tempfile::tempdir().expect("tmp");
        let detected = detect_with(&id("codex"), |_| None, |_| None, tmp.path(), &DetectOptions { probe_versions: false }).expect("detect");
        assert!(detected.version.is_none(), "关掉版本探测就不该有版本");
    }

    #[test]
    fn unknown_harness_yields_an_empty_result_instead_of_an_error() {
        let tmp = tempfile::tempdir().expect("tmp");
        let detected = detect_with(&id("cursor"), |_| None, |_| None, tmp.path(), &DetectOptions::default()).expect("不应报错");
        assert!(!detected.installed());
    }

    #[test]
    fn parallel_detection_keeps_the_registry_order() {
        // 并行之后结果必须**仍按注册表顺序**：否则界面每次刷新看到的顺序都会变。
        let home = std::path::Path::new("C:/Users/example");
        let detected = detect_all(|_| None, home, &DetectOptions { probe_versions: false });
        let expected: Vec<String> = registry::all().iter().map(|spec| spec.id.to_owned()).collect();
        let actual: Vec<String> = detected.iter().map(|d| d.harness.as_str().to_owned()).collect();
        assert_eq!(actual, expected, "并行探测改变了顺序");
    }

    #[test]
    fn parallel_detection_matches_the_serial_result() {
        // **不测耗时**（`AGENTS.md` 测试纪律：不写依赖墙钟的测试）。
        //
        // 这条曾经写成"并行必须比串行快 2 倍"，结果在负载高的机器上偶发失败，
        // 让 `cargo xtask coverage` 无端变红（`docs/PITFALLS.md` G55）。
        // 并行化要保证的是**结果一致 + 顺序不变**，速度只是副作用。
        let home_owned = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("C:/Users/example"));
        let home = home_owned.as_path();
        let options = DetectOptions { probe_versions: false };

        let serial: Vec<Detected> = registry::all()
            .iter()
            .filter_map(|spec| HarnessId::parse(spec.id).ok())
            .filter_map(|harness| detect(&harness, |_| None, home, &options).ok())
            .collect();
        let parallel = detect_all(|_| None, home, &options);

        assert_eq!(parallel.len(), serial.len(), "并行与串行应给出同样多的结果");
        for (a, b) in parallel.iter().zip(serial.iter()) {
            assert_eq!(a.harness, b.harness, "顺序必须一致");
            assert_eq!(a.home_exists, b.home_exists);
            assert_eq!(a.binary, b.binary);
        }
    }

    #[test]
    fn detect_all_covers_fifteen() {
        let tmp = tempfile::tempdir().expect("tmp");
        let all = detect_all(|_| None, tmp.path(), &DetectOptions { probe_versions: false });
        assert_eq!(all.len(), 15);
        let ids: Vec<&str> = all.iter().map(|d| d.harness.as_str()).collect();
        assert!(ids.contains(&"codex"));
        assert!(ids.contains(&"antigravity"));
    }

    #[test]
    fn home_comes_from_the_layout_table() {
        let tmp = tempfile::tempdir().expect("tmp");
        let detected = detect_with(&id("codex"), |_| None, |_| None, tmp.path(), &DetectOptions { probe_versions: false }).expect("detect");
        assert!(detected.home.ends_with(".codex"), "主目录应来自布局表：{}", detected.home.display());
        assert!(!detected.home_exists);
    }

    #[test]
    fn version_of_tolerates_a_missing_program() {
        assert!(version_of(Path::new("/definitely/not/here"), Some(&["--version"])).is_none());
    }

    #[test]
    fn version_of_tolerates_no_version_args() {
        assert!(version_of(Path::new("cmd"), None).is_none());
    }
}
