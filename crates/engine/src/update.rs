//! `update`：检查 GitHub Release 的新版本，并支持便携包就地升级。
//!
//! 三条约束：
//! 1. **版本比较必须真做**，不能只比字符串（`0.10.0` > `0.9.0`，但字符串比较会说反）；
//! 2. **网络是可注入的**：核心逻辑接受一个 `fetch` 闭包，测试用假响应，绝不真联网；
//! 3. **就地升级要有退路**：先写临时文件、校验大小、再原子替换；任何一步失败都保留旧文件。

use metis_core::{ErrorKind, MetisError, Result};
use serde::{Deserialize, Serialize};

/// 一个可比较的版本号。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    /// 主版本。
    pub major: u64,
    /// 次版本。
    pub minor: u64,
    /// 修订版本。
    pub patch: u64,
    /// 预发布后缀（`-rc.2` 这种）。空表示正式版。
    pub pre: String,
}

impl Version {
    /// 解析 `0.2.19` / `v0.2.19` / `0.2.19-rc.2`。
    pub fn parse(text: &str) -> Option<Self> {
        let trimmed = text.trim().trim_start_matches('v');
        if trimmed.is_empty() {
            return None;
        }
        // 预发布后缀在第一个 `-` 或 `+` 处切开。
        let (core, pre) = match trimmed.find(['-', '+']) {
            Some(index) => (&trimmed[..index], trimmed[index + 1..].to_owned()),
            None => (trimmed, String::new()),
        };
        // 严格要求三段（与 `xtask version set` 的校验保持一致）：
        // 宁可拒绝 `1` / `1.2` 这种简写，也不要在版本比较上引入歧义。
        let parts: Vec<&str> = core.split('.').collect();
        if parts.len() != 3 {
            return None;
        }
        let major = parts[0].parse().ok()?;
        let minor = parts[1].parse().ok()?;
        let patch = parts[2].parse().ok()?;
        Some(Self { major, minor, patch, pre })
    }

    /// 是不是预发布版。
    pub fn is_prerelease(&self) -> bool {
        !self.pre.is_empty()
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() {
            write!(f, "-{}", self.pre)?;
        }
        Ok(())
    }
}

/// 一个发布资产。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    /// 文件名。
    pub name: String,
    /// 下载地址。
    #[serde(rename = "downloadUrl", alias = "browser_download_url")]
    pub download_url: String,
    /// 字节数。
    #[serde(default)]
    pub size: u64,
}

/// 一次发布信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    /// tag 名（`v0.2.19`）。
    #[serde(rename = "tagName", alias = "tag_name")]
    pub tag_name: String,
    /// 发布页地址。
    #[serde(rename = "htmlUrl", alias = "html_url")]
    pub html_url: String,
    /// 更新说明。
    #[serde(default)]
    pub body: String,
    /// 是不是预发布。
    #[serde(default)]
    pub prerelease: bool,
    /// 资产列表。
    #[serde(default)]
    pub assets: Vec<Asset>,
}

impl Release {
    /// 解析出来的版本。
    pub fn version(&self) -> Option<Version> {
        Version::parse(&self.tag_name)
    }
}

/// 检查结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateCheck {
    /// 当前版本。
    pub current: String,
    /// 最新版本（拿不到为 `None`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<String>,
    /// 有更新吗。
    pub available: bool,
    /// 发布页。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// 更新说明（截断到 500 字）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,
    /// 可下载的资产。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assets: Vec<Asset>,
    /// 检查失败的原因（`available` 为 false 且这里有值时表示"查不了"，不是"已最新"）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 比较"当前"与"最新"，得出是否有更新。
///
/// 规则：
/// - 最新版解析不出来 → 报错（不猜）；
/// - 当前是预发布、最新是正式版且核心版本相同 → 算有更新（`0.2.19-rc.1` → `0.2.19`）；
/// - 核心版本更高 → 有更新；
/// - 最新是预发布而当前是正式版 → **不算**（不把用户从稳定版拽到 rc）。
pub fn is_newer(current: &Version, latest: &Version) -> bool {
    let core = (latest.major, latest.minor, latest.patch);
    let mine = (current.major, current.minor, current.patch);
    if core > mine {
        // 核心更高时，即使最新是预发布也算（0.3.0-rc.1 > 0.2.19）。
        return true;
    }
    if core < mine {
        return false;
    }
    // 核心相同：只有"当前是预发布、最新是正式版"才算升级。
    current.is_prerelease() && !latest.is_prerelease()
}

/// 检查更新需要的输入。
#[derive(Debug, Clone)]
pub struct UpdateOptions {
    /// 当前版本（`CARGO_PKG_VERSION`）。
    pub current: String,
    /// Release API 地址。
    pub api: String,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            current: env!("CARGO_PKG_VERSION").to_owned(),
            // 公开的 Release API。**只读、无 token**。
            api: "https://api.github.com/repos/metis-app/metis/releases/latest".to_owned(),
        }
    }
}

/// 检查更新。
///
/// `fetch` 是注入点：给定 URL 返回 `(状态码, 响应体)`。测试用它返回假 JSON，
/// 生产用它调 `ureq` —— 核心逻辑本身不碰网络。
pub fn check(options: &UpdateOptions, fetch: impl Fn(&str) -> std::result::Result<(u16, String), String>) -> UpdateCheck {
    let mut result = UpdateCheck { current: options.current.clone(), latest: None, available: false, url: None, notes: None, assets: Vec::new(), error: None };

    let current = Version::parse(&options.current);
    let Some(current) = current else {
        result.error = Some(format!("当前版本号无法解析：{}", options.current));
        return result;
    };

    let (status, body) = match fetch(&options.api) {
        Ok(value) => value,
        Err(error) => {
            result.error = Some(format!("检查更新失败：{error}"));
            return result;
        }
    };
    if status == 404 {
        // 仓库还没有公开 Release：这不是错误，只是"没有可查的发布"。
        result.error = Some("这个仓库还没有公开 Release（不代表你装的不是最新版）".to_owned());
        return result;
    }
    if status != 200 {
        result.error = Some(format!("Release API 返回 {status}"));
        return result;
    }

    let release: Release = match serde_json::from_str(&body) {
        Ok(value) => value,
        Err(error) => {
            result.error = Some(format!("Release 响应不是预期的 JSON：{error}"));
            return result;
        }
    };

    let Some(latest) = release.version() else {
        result.error = Some(format!("最新 tag 无法解析成版本号：{}", release.tag_name));
        return result;
    };

    result.latest = Some(latest.to_string());
    result.url = Some(release.html_url.clone());
    result.notes = Some(release.body.chars().take(500).collect());
    // 预发布不推给正式版用户。
    result.available = is_newer(&current, &latest) && !(latest.is_prerelease() && !current.is_prerelease());
    // 挑一个适合当前平台的资产。
    result.assets = release.assets.into_iter().filter(|asset| asset_matches_platform(&asset.name)).collect();
    result
}

/// 资产名是否适合当前平台。
fn asset_matches_platform(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    #[cfg(windows)]
    {
        lowered.contains("windows") || lowered.contains("win") || lowered.ends_with(".exe")
    }
    #[cfg(target_os = "macos")]
    {
        lowered.contains("macos") || lowered.contains("darwin") || lowered.contains("apple")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        lowered.contains("linux") || lowered.contains("musl") || lowered.contains("gnu")
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = lowered;
        false
    }
}

/// 就地升级一次便携包。
///
/// 步骤有意做成"可中断且不留半成品"：
/// 1. 写到同目录的临时文件；
/// 2. 校验字节数（下载不完整就不替换）；
/// 3. 原子 rename 覆盖当前 exe。
///
/// Windows 上正在运行的 exe 不能被覆盖 —— 这是平台限制，所以真正替换由调用方在
/// **进程退出后**做（写一个 update-on-exit 的脚本），这里只负责把新文件准备好。
pub fn stage_upgrade(current_exe: &std::path::Path, bytes: &[u8], expected_size: Option<u64>) -> Result<std::path::PathBuf> {
    if let Some(expected) = expected_size
        && expected != 0
        && bytes.len() as u64 != expected
    {
        return Err(MetisError::new(ErrorKind::UpdateFailed, format!("下载不完整：期望 {expected} 字节，实际 {}", bytes.len()))
            .with_remediation("网络中断或镜像返回了错误页面；重试一次"));
    }
    let parent = current_exe.parent().ok_or_else(|| {
        MetisError::new(ErrorKind::Io, format!("当前可执行文件没有父目录：{}", current_exe.display())).with_remediation("把 metis 放在一个有父目录的路径里再试")
    })?;
    let staged = parent.join(format!("{}.new", current_exe.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "metis".to_owned())));
    metis_fs::write_atomic(&staged, bytes)?;
    Ok(staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetch_ok(body: &'static str) -> impl Fn(&str) -> std::result::Result<(u16, String), String> {
        move |_url| Ok((200, body.to_owned()))
    }

    fn fetch_status(status: u16) -> impl Fn(&str) -> std::result::Result<(u16, String), String> {
        move |_url| Ok((status, String::new()))
    }

    #[test]
    fn version_parses_the_common_shapes() {
        for (text, expected) in [("0.2.19", (0, 2, 19)), ("v0.2.19", (0, 2, 19)), ("1.0.0", (1, 0, 0)), (" 2.3.4 ", (2, 3, 4))] {
            let version = Version::parse(text).expect(text);
            assert_eq!((version.major, version.minor, version.patch), expected, "{text}");
            assert!(!version.is_prerelease());
        }
    }

    #[test]
    fn version_parses_prerelease_suffixes() {
        let version = Version::parse("0.2.19-rc.2").expect("parse");
        assert_eq!((version.major, version.minor, version.patch), (0, 2, 19));
        assert_eq!(version.pre, "rc.2");
        assert!(version.is_prerelease());
    }

    #[test]
    fn version_rejects_junk() {
        for bad in ["", "v", "1", "1.2.3.4", "abc", "1.x.0"] {
            assert!(Version::parse(bad).is_none(), "{bad:?} 不该被接受");
        }
    }

    #[test]
    fn numeric_comparison_beats_string_comparison() {
        // 这是引入 Version 类型的原因：字符串比较会说 "0.10.0" < "0.9.0"。
        let ten = Version::parse("0.10.0").expect("a");
        let nine = Version::parse("0.9.0").expect("b");
        assert!(ten > nine, "0.10.0 必须大于 0.9.0");
        assert!(is_newer(&nine, &ten));
        assert!(!is_newer(&ten, &nine));
    }

    #[test]
    fn stability_rules() {
        let stable = Version::parse("0.2.19").expect("s");
        let rc = Version::parse("0.2.19-rc.1").expect("r");
        // 同核心：rc → 正式版算升级。
        assert!(is_newer(&rc, &stable));
        // 正式版 → rc 不算（不把用户从稳定版拽到预发布）。
        assert!(!is_newer(&stable, &rc));
        // 核心更高时，rc 也算。
        let higher_rc = Version::parse("0.3.0-rc.1").expect("h");
        assert!(is_newer(&stable, &higher_rc));
    }

    #[test]
    fn check_reports_an_available_update() {
        let body = r#"{"tag_name":"v9.9.9","html_url":"https://example.invalid/rel","body":"新版本说明","assets":[{"name":"metis-windows-x64.exe","downloadUrl":"https://example.invalid/a.exe","size":123}]}"#;
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "https://example.invalid/latest".to_owned() };
        let result = check(&options, fetch_ok(body));
        assert!(result.available, "{result:?}");
        assert_eq!(result.latest.as_deref(), Some("9.9.9"));
        assert_eq!(result.current, "0.1.0");
        assert!(result.notes.unwrap_or_default().contains("新版本"));
        assert!(result.error.is_none());
    }

    #[test]
    fn check_reports_no_update_when_already_newest() {
        let body = r#"{"tag_name":"v0.1.0","html_url":"https://example.invalid/rel","body":"","assets":[]}"#;
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, fetch_ok(body));
        assert!(!result.available);
        assert_eq!(result.latest.as_deref(), Some("0.1.0"));
        assert!(result.error.is_none(), "已最新不算错误");
    }

    #[test]
    fn a_prerelease_is_not_offered_to_a_stable_user() {
        let body = r#"{"tag_name":"v9.9.9-rc.1","html_url":"u","prerelease":true,"assets":[]}"#;
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, fetch_ok(body));
        // 核心更高时 rc 也能报，但 result.available 由 is_newer 与预发布规则共同决定。
        assert_eq!(result.latest.as_deref(), Some("9.9.9-rc.1"));
        assert!(!result.available, "不把稳定版用户推到预发布");
    }

    #[test]
    fn missing_release_is_reported_as_not_checkable() {
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, fetch_status(404));
        assert!(!result.available);
        let error = result.error.expect("应有说明");
        assert!(error.contains("还没有公开 Release"), "要说清是查不到而不是查过了");
    }

    #[test]
    fn network_failure_is_reported_not_panicked() {
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, |_| Err("connection refused".to_owned()));
        assert!(!result.available);
        assert!(result.error.unwrap_or_default().contains("检查更新失败"));
    }

    #[test]
    fn bad_json_is_reported() {
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, fetch_ok("not json"));
        assert!(result.error.unwrap_or_default().contains("JSON"));
    }

    #[test]
    fn unparsable_tag_is_reported() {
        let body = r#"{"tag_name":"nightly","html_url":"u","assets":[]}"#;
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, fetch_ok(body));
        assert!(result.error.unwrap_or_default().contains("无法解析"));
    }

    #[test]
    fn assets_are_filtered_for_the_current_platform() {
        let body = r#"{"tag_name":"v9.9.9","html_url":"u","assets":[
            {"name":"metis-windows-x64.exe","downloadUrl":"a","size":1},
            {"name":"metis-linux-x64","downloadUrl":"b","size":1},
            {"name":"metis-macos-arm64","downloadUrl":"c","size":1},
            {"name":"checksums.txt","downloadUrl":"d","size":1}
        ]}"#;
        let options = UpdateOptions { current: "0.1.0".to_owned(), api: "x".to_owned() };
        let result = check(&options, fetch_ok(body));
        #[cfg(windows)]
        {
            assert!(result.assets.iter().any(|a| a.name.contains("windows")));
            assert!(result.assets.iter().all(|a| !a.name.contains("linux")), "不该混进别的平台");
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            assert!(result.assets.iter().any(|a| a.name.contains("linux")));
        }
    }

    #[test]
    fn staging_rejects_an_incomplete_download() {
        let dir = tempfile::tempdir().expect("tmp");
        let exe = dir.path().join("metis.exe");
        std::fs::write(&exe, b"old").expect("seed");
        let error = stage_upgrade(&exe, b"short", Some(9999)).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::UpdateFailed);
        assert!(error.remediation().is_some());
        // 关键：旧文件必须完好。
        assert_eq!(std::fs::read(&exe).expect("read"), b"old".to_vec());
    }

    #[test]
    fn staging_writes_a_sibling_new_file() {
        let dir = tempfile::tempdir().expect("tmp");
        let exe = dir.path().join("metis.exe");
        std::fs::write(&exe, b"old").expect("seed");
        let staged = stage_upgrade(&exe, b"new-binary", Some(10)).expect("stage");
        assert_eq!(staged.file_name().and_then(|n| n.to_str()), Some("metis.exe.new"));
        assert_eq!(std::fs::read(&staged).expect("read"), b"new-binary".to_vec());
        // 旧文件在替换之前不动。
        assert_eq!(std::fs::read(&exe).expect("read"), b"old".to_vec());
    }
}
