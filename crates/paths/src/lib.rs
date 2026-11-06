//! 路径解析。
//!
//! 全部路径都从 [`Paths`] 上取，**根可注入**：测试用临时目录，绝不碰真实 `$HOME`
//! （AGENTS.md 单测契约里的一条）。
//!
//! 这里只**算路径**，不创建文件、不检查存在性（少数 `detect` 用的探测方法除外，它们只读）。

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![deny(missing_docs)]

use std::path::{Path, PathBuf};

use metis_core::{ErrorKind, MetisError, Result, harness::HarnessId};

/// 环境变量：数据目录覆盖。
pub const ENV_HOME: &str = "METIS_HOME";
/// 环境变量：交换 journal 目录覆盖（主要给测试用）。
pub const ENV_JOURNAL_DIR: &str = "METIS_JOURNAL_DIR";

/// 数据目录名（`~/.metis`）。
pub const HOME_DIR_NAME: &str = ".metis";

/// 解析后的路径集合。
///
/// 所有方法都是纯计算：同一个 `Paths` 永远给出同样的路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    home: PathBuf,
    journal_dir_override: Option<PathBuf>,
    /// 把所有宿主主目录重定向到这个根下（`<root>/<harness>`）。
    ///
    /// **只给测试用**：生产的宿主目录必须跟着真实环境变量与用户主目录走。
    /// 没有它，测试一旦碰到 `apply` 就会往真实 `~/.pi`、`~/.codex` 里写东西。
    harness_root_override: Option<PathBuf>,
}

impl Paths {
    /// 用显式的数据目录构造（测试与 `--home` 用）。
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into(), journal_dir_override: None, harness_root_override: None }
    }

    /// 覆盖交换 journal 目录。主要给测试用，避免污染系统临时目录。
    #[must_use]
    pub fn with_journal_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.journal_dir_override = Some(dir.into());
        self
    }

    /// 把宿主主目录全部重定向到 `root/<harness>`。
    ///
    /// **只给测试用**：保证测试不会往真实 `~/.pi`、`~/.codex` 里写任何东西。
    #[must_use]
    pub fn with_harness_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.harness_root_override = Some(root.into());
        self
    }

    /// 按真实环境解析：`METIS_HOME` > 用户主目录下的 `.metis`。
    ///
    /// 相对路径按当前工作目录解析（与基线一致：显式给的环境变量就是个路径，不该被当成魔法字符串）。
    pub fn from_env() -> Result<Self> {
        Self::from_env_with(|key| std::env::var(key).ok(), dirs::home_dir(), std::env::current_dir().ok())
    }

    /// 可注入版本的 [`Paths::from_env`]，便于测试覆盖环境。
    pub fn from_env_with(get: impl Fn(&str) -> Option<String>, home_dir: Option<PathBuf>, current_dir: Option<PathBuf>) -> Result<Self> {
        let journal_dir_override = get(ENV_JOURNAL_DIR).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).map(|v| absolute(&v, current_dir.as_deref()));

        if let Some(raw) = get(ENV_HOME).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            let home = absolute(&raw, current_dir.as_deref());
            let home = normalise(&home)?;
            return Ok(Self { home, journal_dir_override, harness_root_override: None });
        }

        let home = home_dir.ok_or_else(|| {
            MetisError::new(ErrorKind::Io, "找不到用户主目录，无法确定数据目录").with_remediation(format!("设置 {ENV_HOME} 指向一个可写目录"))
        })?;
        Ok(Self { home: home.join(HOME_DIR_NAME), journal_dir_override, harness_root_override: None })
    }

    /// 数据目录本身。
    pub fn home(&self) -> &Path {
        &self.home
    }

    /// `state.json`。
    pub fn state_file(&self) -> PathBuf {
        self.home.join("state.json")
    }

    /// `personas/`。
    pub fn personas_dir(&self) -> PathBuf {
        self.home.join("personas")
    }

    /// `personas/<id>.md`。
    pub fn persona_file(&self, id: &metis_core::PersonaId) -> PathBuf {
        self.personas_dir().join(id.file_name())
    }

    /// `personas/active.md`：当前生效人设的快照。
    pub fn active_persona_file(&self) -> PathBuf {
        self.personas_dir().join("active.md")
    }

    /// `footprints.json`：写入所有权账本。
    pub fn footprints_file(&self) -> PathBuf {
        self.home.join("footprints.json")
    }

    /// `backups/`：整文件回滚用的原件。
    pub fn backups_dir(&self) -> PathBuf {
        self.home.join("backups")
    }

    /// `verification/`。
    pub fn verification_dir(&self) -> PathBuf {
        self.home.join("verification")
    }

    /// `verification/latest.json`。
    pub fn verification_file(&self) -> PathBuf {
        self.verification_dir().join("latest.json")
    }

    /// `armor/`。
    pub fn armor_dir(&self) -> PathBuf {
        self.home.join("armor")
    }

    /// armor 实测数据文件（`armor.json`）。
    pub fn armor_file(&self) -> PathBuf {
        self.home.join("armor.json")
    }

    /// `journal/`。
    pub fn journal_dir(&self) -> PathBuf {
        self.home.join("journal")
    }

    /// `shim/`：裸命令接管的包装脚本目录。
    pub fn shim_dir(&self) -> PathBuf {
        self.home.join("shim")
    }

    /// `logs/`。
    pub fn logs_dir(&self) -> PathBuf {
        self.home.join("logs")
    }

    /// 控制锁文件。
    pub fn lock_file(&self) -> PathBuf {
        self.home.join(".apply.lock")
    }

    /// 交换 journal 所在目录：覆盖值 > 系统临时目录下的 `metis-restore`。
    pub fn restore_journal_dir(&self) -> PathBuf {
        if let Some(dir) = &self.journal_dir_override {
            return dir.clone();
        }
        std::env::temp_dir().join("metis-restore")
    }

    /// `runtime/`：启动租约等短生命周期文件。
    pub fn runtime_dir(&self) -> PathBuf {
        self.home.join("runtime")
    }

    /// 某家执行器的主目录（按 `docs/HARNESS-MATRIX.md` 的探测规则）。
    ///
    /// 环境变量优先；没设置时按平台默认。未知 id 返回错误而不是猜。
    pub fn harness_home(&self, harness: &HarnessId, env: impl Fn(&str) -> Option<String>, home_dir: &Path) -> Result<PathBuf> {
        let layout = HarnessLayout::of(harness).ok_or_else(|| {
            MetisError::new(ErrorKind::UnknownHarness, format!("不知道 {harness} 的主目录在哪"))
                .with_remediation("在 docs/HARNESS-MATRIX.md 登记这家执行器的探测规则")
        })?;
        if let Some(root) = &self.harness_root_override {
            return Ok(root.join(harness.as_str()));
        }
        Ok(layout.resolve(env, home_dir))
    }
}

/// 一家执行器的主目录布局。数据表，不是 `if` 链（AGENTS.md「数据优先于代码」）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarnessLayout {
    /// 覆盖用的环境变量名。
    pub env_var: &'static str,
    /// 默认相对路径（相对用户主目录）；Windows 上另有可能的 `%LOCALAPPDATA%` 的相对路径。
    pub default_relative: &'static str,
    /// Windows 上改用 `%LOCALAPPDATA%` 的相对路径（若适用）。
    pub windows_local_appdata: Option<&'static str>,
    /// 环境变量是"配置根"，其下还要拼一级子目录（XDG 风格）。
    ///
    /// 例如 `XDG_CONFIG_HOME=~/.config` 时，opencode 的目录是 `~/.config/opencode` ——
    /// 环境变量本身**不是**宿主目录。这个语义搞错会把文件写到 `~/.config/plugins/` 下。
    pub env_is_parent: bool,
}

impl HarnessLayout {
    /// 查表。
    pub fn of(harness: &HarnessId) -> Option<Self> {
        Self::TABLE.iter().find(|(id, _)| *id == harness.as_str()).map(|(_, layout)| *layout)
    }

    /// 解析成绝对路径。
    pub fn resolve(&self, env: impl Fn(&str) -> Option<String>, home_dir: &Path) -> PathBuf {
        if let Some(value) = env(self.env_var).map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()) {
            let base = PathBuf::from(value);
            if self.env_is_parent {
                // XDG 风格：环境变量是配置根，宿主目录是它下面那一级。
                if let Some(leaf) = Path::new(self.default_relative).file_name() {
                    return base.join(leaf);
                }
            }
            return base;
        }
        if cfg!(windows)
            && let Some(relative) = self.windows_local_appdata
            && let Some(local) = env("LOCALAPPDATA")
        {
            return PathBuf::from(local).join(relative);
        }
        home_dir.join(self.default_relative)
    }

    /// 内置的 15 家布局表。与 `docs/HARNESS-MATRIX.md` §2 一一对应。
    pub const TABLE: &'static [(&'static str, HarnessLayout)] = &[
        ("pi", HarnessLayout { env_var: "PI_HOME", default_relative: ".pi/agent", windows_local_appdata: None, env_is_parent: false }),
        ("pi-desktop", HarnessLayout { env_var: "PI_DESKTOP_DATA_DIR", default_relative: ".pi-desktop", windows_local_appdata: None, env_is_parent: false }),
        ("omp", HarnessLayout { env_var: "OMP_HOME", default_relative: ".omp", windows_local_appdata: None, env_is_parent: false }),
        ("opencode", HarnessLayout { env_var: "XDG_CONFIG_HOME", default_relative: ".config/opencode", windows_local_appdata: None, env_is_parent: true }),
        ("opencode2", HarnessLayout { env_var: "XDG_CONFIG_HOME", default_relative: ".config/opencode", windows_local_appdata: None, env_is_parent: true }),
        ("dsh", HarnessLayout { env_var: "DSH_HOME", default_relative: ".dsh", windows_local_appdata: None, env_is_parent: false }),
        ("codex", HarnessLayout { env_var: "CODEX_HOME", default_relative: ".codex", windows_local_appdata: None, env_is_parent: false }),
        ("claude", HarnessLayout { env_var: "CLAUDE_CONFIG_DIR", default_relative: ".claude", windows_local_appdata: None, env_is_parent: false }),
        ("grok", HarnessLayout { env_var: "GROK_HOME", default_relative: ".grok", windows_local_appdata: None, env_is_parent: false }),
        ("codebuddy", HarnessLayout { env_var: "CODEBUDDY_HOME", default_relative: ".codebuddy", windows_local_appdata: None, env_is_parent: false }),
        ("hermes", HarnessLayout { env_var: "HERMES_HOME", default_relative: ".hermes", windows_local_appdata: Some("hermes"), env_is_parent: false }),
        ("kilo", HarnessLayout { env_var: "KILO_CONFIG_DIR", default_relative: ".config/kilo", windows_local_appdata: None, env_is_parent: false }),
        ("workbuddy", HarnessLayout { env_var: "WORKBUDDY_CONFIG_DIR", default_relative: ".workbuddy", windows_local_appdata: None, env_is_parent: false }),
        ("zcode", HarnessLayout { env_var: "ZCODE_HOME", default_relative: ".zcode", windows_local_appdata: None, env_is_parent: false }),
        (
            "antigravity",
            HarnessLayout { env_var: "ANTIGRAVITY_HOME", default_relative: "Programs/antigravity", windows_local_appdata: None, env_is_parent: false },
        ),
    ];
}

fn absolute(value: &str, current_dir: Option<&Path>) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match current_dir {
        Some(dir) => dir.join(path),
        None => path.to_path_buf(),
    }
}

/// 规范化：去掉 `.`、处理 `..`（不解析符号链接，不要求路径存在）。
///
/// 破坏性操作的安全闸门依赖它（见 `ARCHITECTURE.md` §7.3）。
pub fn normalise(path: &Path) -> Result<PathBuf> {
    use std::path::Component;
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return Err(MetisError::new(ErrorKind::InvalidArgument, format!("路径越过了根：{}", path.display()))
                        .with_remediation("给一个绝对路径，或者一个在数据目录内的相对路径"));
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    Ok(out)
}

/// 破坏性操作的安全闸门。
///
/// 命中任一条即拒绝，并说明命中了哪一条。见 `ARCHITECTURE.md` §7.3。
pub fn destructive_gate(home: &Path, home_dir: Option<&Path>, temp_dir: Option<&Path>, cwd: Option<&Path>) -> Result<()> {
    let refuse = |why: &str, remediation: &str| Err(MetisError::new(ErrorKind::Refused, why.to_owned()).with_remediation(remediation.to_owned()));

    let normalised = normalise(home)?;
    let file_name = normalised.file_name().and_then(|n| n.to_str());
    if file_name.is_none_or(|n| n.is_empty()) {
        return refuse(&format!("拒绝操作：数据目录看起来是盘符根或分隔符根（{}）", normalised.display()), "用一个真正的目录，例如 ~/.metis");
    }
    // 数据目录不能是主目录 / 临时目录，也不能是它们的上级。
    for (label, ancestor) in [("用户主目录", home_dir), ("临时目录", temp_dir)] {
        let Some(ancestor) = ancestor else { continue };
        let ancestor = normalise(ancestor)?;
        if normalised == ancestor {
            return refuse(&format!("拒绝操作：数据目录就是{label}本身（{}）", normalised.display()), "数据目录必须是一个独立目录，例如 ~/.metis");
        }
        if ancestor.starts_with(&normalised) {
            return refuse(&format!("拒绝操作：数据目录是{label}的上级（{}）", normalised.display()), "数据目录必须是一个独立目录，例如 ~/.metis");
        }
    }
    // 当前工作目录不能落在数据目录里。
    if let Some(cwd) = cwd {
        let cwd = normalise(cwd)?;
        if cwd.starts_with(&normalised) {
            return refuse(&format!("拒绝操作：当前目录在数据目录内（{}）", cwd.display()), "先切换到一个普通项目目录，再重试");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 一个绝对路径的"假主目录"。
    ///
    /// **不能用 Windows 字面量**：在 Linux/macOS 上反斜杠只是普通字符，
    /// `Path::new("C:\\Users\\example")` 是一个**相对路径**（单个文件名），
    /// 于是所有依赖 `parent()`/`components()`/`starts_with()` 的断言全部失真。
    /// CI 的 Linux 测试就是这么红的。
    fn fake_home() -> PathBuf {
        if cfg!(windows) { PathBuf::from("C:\\Users\\example") } else { PathBuf::from("/home/example") }
    }

    /// 平台无关的绝对路径构造器：`abs("data", "metis")`
    /// → Windows `D:\data\metis`，Unix `/data/metis`。
    fn abs(parts: &[&str]) -> PathBuf {
        let mut out = if cfg!(windows) { PathBuf::from("D:\\") } else { PathBuf::from("/") };
        for part in parts {
            out.push(part);
        }
        // 自检：这必须是绝对路径。**这就是这类测试在 Unix 上翻车的原因** ——
        // `Path::new("D:\\x")` 在 Unix 上是相对路径，后续 `parent()`/`starts_with()`
        // 全部失真。把它做成断言，写错会当场炸，而不是在 CI 上才炸。
        assert!(out.is_absolute(), "abs() 必须产出绝对路径：{out:?}");
        out
    }

    #[test]
    fn explicit_home_wins_over_default() {
        // **不能用 Windows 字面量路径**：反斜杠在 Linux/macOS 上只是普通字符，
        // 整串会变成一个文件名，`Path::new("D:\\data\\metis")` 与解析出来的
        // 绝对路径永远不相等（CI 的 Linux 测试因此全红）。
        // 用绝对路径构造，让两个平台都想同。
        let explicit = if cfg!(windows) { PathBuf::from("D:\\data\\metis") } else { PathBuf::from("/data/metis") };
        let cwd = if cfg!(windows) { PathBuf::from("D:\\work") } else { PathBuf::from("/work") };
        let paths = Paths::from_env_with(
            |key| match key {
                ENV_HOME => Some(explicit.display().to_string()),
                _ => None,
            },
            Some(fake_home()),
            Some(cwd),
        )
        .expect("paths");
        assert_eq!(paths.home(), explicit.as_path());
    }

    #[test]
    fn default_home_is_dot_metis_under_user_home() {
        let paths = Paths::from_env_with(|_| None, Some(fake_home()), None).expect("paths");
        assert!(paths.home().ends_with(HOME_DIR_NAME));
        assert!(paths.home().starts_with(fake_home()));
    }

    #[test]
    fn relative_env_home_resolves_against_cwd() {
        // `cwd` 必须是**绝对的**，否则在 Unix 上 `D:\work` 是相对路径，
        // join 之后仍然不是绝对路径，这条断言必失败。
        let paths = Paths::from_env_with(|key| (key == ENV_HOME).then(|| "data/metis".to_owned()), Some(fake_home()), Some(abs(&["work"]))).expect("paths");
        assert!(paths.home().is_absolute(), "相对路径必须被解析成绝对路径");
        assert!(paths.home().ends_with("metis"));
    }

    #[test]
    fn blank_env_home_is_ignored() {
        let paths = Paths::from_env_with(|key| (key == ENV_HOME).then(|| "   ".to_owned()), Some(fake_home()), None).expect("paths");
        assert!(paths.home().ends_with(HOME_DIR_NAME));
    }

    #[test]
    fn missing_user_home_is_an_error_with_remediation() {
        let result = Paths::from_env_with(|_| None, None, None);
        let error = result.expect_err("应该有错误");
        assert_eq!(error.kind(), ErrorKind::Io);
        assert!(error.remediation().is_some(), "必须告诉用户设 METIS_HOME");
    }

    #[test]
    fn all_children_live_under_home() {
        let paths = Paths::with_home(abs(&["ap"]));
        for child in [
            paths.state_file(),
            paths.personas_dir(),
            paths.active_persona_file(),
            paths.footprints_file(),
            paths.backups_dir(),
            paths.verification_file(),
            paths.armor_dir(),
            paths.journal_dir(),
            paths.shim_dir(),
            paths.logs_dir(),
            paths.lock_file(),
            paths.runtime_dir(),
        ] {
            assert!(child.starts_with(paths.home()), "{} 不在数据目录内", child.display());
        }
    }

    #[test]
    fn persona_file_joins_id_into_personas_dir() {
        let paths = Paths::with_home(abs(&["ap"]));
        let id = metis_core::PersonaId::parse("concise").expect("id");
        assert_eq!(paths.persona_file(&id), paths.personas_dir().join("concise.md"));
    }

    #[test]
    fn journal_dir_can_be_overridden() {
        let journal = abs(&["tmp", "journals"]);
        let paths = Paths::with_home(abs(&["ap"])).with_journal_dir(&journal);
        assert_eq!(paths.restore_journal_dir(), journal);
    }

    #[test]
    fn default_journal_dir_mentions_metis() {
        let paths = Paths::with_home(abs(&["ap"]));
        assert!(paths.restore_journal_dir().to_string_lossy().contains("metis-restore"));
    }

    #[test]
    fn every_builtin_harness_has_a_layout() {
        for id in HarnessId::BUILTIN {
            let harness = HarnessId::parse(id).expect("id");
            assert!(HarnessLayout::of(&harness).is_some(), "{id} 缺少主目录布局");
        }
        assert_eq!(HarnessLayout::TABLE.len(), 15);
    }

    #[test]
    fn harness_home_honours_env_override() {
        let harness = HarnessId::parse("codex").expect("id");
        let paths = Paths::with_home(abs(&["ap"]));
        // 覆盖值要与默认根**不同**，才能证明"覆盖生效了"而不是"恰好相等"。
        let explicit = if cfg!(windows) { PathBuf::from("E:\\codex") } else { PathBuf::from("/opt/codex") };
        let home = paths.harness_home(&harness, |key| (key == "CODEX_HOME").then(|| explicit.display().to_string()), &fake_home()).expect("home");
        assert_eq!(home, explicit);
    }

    #[test]
    fn harness_home_defaults_to_user_home() {
        let harness = HarnessId::parse("codex").expect("id");
        let paths = Paths::with_home(abs(&["ap"]));
        let user_home = fake_home();
        let home = paths.harness_home(&harness, |_| None, &user_home).expect("home");
        assert!(home.ends_with(".codex"));
        // **`starts_with` 必须拿 `&Path`**：`Path::new("C:\\Users\\example")` 在 Unix 上
        // 是相对路径，`starts_with` 永远不成立（CI 的 Linux 测试踩过）。
        assert!(home.starts_with(&user_home), "{home:?} 应挂在 {user_home:?} 下");
    }

    #[test]
    fn unknown_harness_home_is_an_error() {
        let harness = HarnessId::parse("cursor").expect("id");
        let paths = Paths::with_home(abs(&["ap"]));
        let result = paths.harness_home(&harness, |_| None, &fake_home());
        assert_eq!(result.err().map(|e| e.kind()), Some(ErrorKind::UnknownHarness));
    }

    #[test]
    fn xdg_style_env_is_a_parent_not_the_harness_home() {
        // 回归：XDG_CONFIG_HOME 是"配置根"，opencode 的目录是它下面的 `opencode/`。
        // 把环境变量当成宿主目录，会把文件写到 `~/.config/plugins/` 这种错地方。
        let paths = Paths::with_home(abs(&["ap"]));
        let xdg = abs(&["xdg"]);
        let home = paths
            .harness_home(&HarnessId::parse("opencode").expect("id"), |key| (key == "XDG_CONFIG_HOME").then(|| xdg.display().to_string()), &fake_home())
            .expect("home");
        assert_eq!(home, xdg.join("opencode"), "XDG 之下还要拼一级");
    }

    #[test]
    fn home_style_env_is_the_harness_home_itself() {
        // 对照：CODEX_HOME 就是宿主目录，不能再拼一级。
        let paths = Paths::with_home(abs(&["ap"]));
        // **输入与断言必须用同一个来源**：只改断言不改输入，两边在 Unix 上对不上
        // （`D:\codex` 在 Unix 上是相对路径，而 `abs()` 产出绝对路径）。
        let codex = abs(&["codex"]);
        let home = paths
            .harness_home(&HarnessId::parse("codex").expect("id"), |key| (key == "CODEX_HOME").then(|| codex.display().to_string()), &fake_home())
            .expect("home");
        assert_eq!(home, codex);
    }

    #[test]
    fn harness_root_override_redirects_every_harness() {
        // 这一条保护的是"测试不碰真实用户目录"：没有它，apply 的测试会往 ~/.pi 里写东西。
        let root = abs(&["fake-homes"]);
        let paths = Paths::with_home(abs(&["ap"])).with_harness_root(&root);
        let home = paths.harness_home(&HarnessId::parse("codex").expect("id"), |_| None, &fake_home()).expect("home");
        assert_eq!(home, root.join("codex"), "harness root 覆盖之后应挂在新根下");
        assert!(!home.starts_with(fake_home()), "绝不能落到真实主目录");
    }

    #[test]
    fn normalise_removes_dot_and_parent_segments() {
        assert_eq!(normalise(&abs(&["a", "b", "..", "c"])).map(|p| p.to_string_lossy().into_owned()), Ok(abs(&["a", "c"]).to_string_lossy().into_owned()));
        assert_eq!(normalise(&abs(&["a", ".", "b"])).map(|p| p.to_string_lossy().into_owned()), Ok(abs(&["a", "b"]).to_string_lossy().into_owned()));
    }

    #[test]
    fn gate_refuses_root_and_home_and_temp() {
        let temp = &abs(&["tmp"]);
        let home = &fake_home();

        // 平台无关的"根"：Windows 是 `D:\`，Unix 是 `/`。
        let root = if cfg!(windows) { Path::new("D:\\") } else { Path::new("/") };
        assert_eq!(destructive_gate(root, Some(home), Some(temp), None).err().map(|e| e.kind()), Some(ErrorKind::Refused), "根必须拒绝");
        assert_eq!(destructive_gate(home, Some(home), Some(temp), None).err().map(|e| e.kind()), Some(ErrorKind::Refused), "主目录本身必须拒绝");
        assert_eq!(destructive_gate(temp, Some(home), Some(temp), None).err().map(|e| e.kind()), Some(ErrorKind::Refused), "临时目录本身必须拒绝");
        assert_eq!(
            destructive_gate(home.parent().unwrap_or(home), Some(home), Some(temp), None).err().map(|e| e.kind()),
            Some(ErrorKind::Refused),
            "主目录的上级必须拒绝"
        );
    }

    #[test]
    fn gate_refuses_cwd_inside_home() {
        let home = &fake_home().join("ap");
        let inside = &home.join("sub");
        let result = destructive_gate(home, None, None, Some(inside));
        assert_eq!(result.err().map(|e| e.kind()), Some(ErrorKind::Refused));
    }

    #[test]
    fn gate_allows_a_sane_home() {
        let base = fake_home();
        let result = destructive_gate(&base.join(".metis"), Some(&base), Some(&std::env::temp_dir()), Some(&base.join("work").join("project")));
        assert!(result.is_ok(), "正常的数据目录必须放行");
    }

    #[test]
    fn gate_error_explains_which_rule_was_hit() {
        let root = if cfg!(windows) { Path::new("C:\\") } else { Path::new("/") };
        let error = destructive_gate(root, None, None, None).expect_err("should refuse");
        assert!(error.remediation().is_some());
        assert!(error.message().contains("根"), "应说清命中了哪一条：{}", error.message());
    }
}
