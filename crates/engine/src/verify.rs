//! `verify`：用本地假 provider 抓宿主的**真实请求**，判定人设落在哪一层。
//!
//! 这是"写入成功 ≠ 运行时生效"唯一的反证手段（`docs/ARCHITECTURE.md` §11）。
//!
//! 判定口径：
//! - `ok`：请求里命中了本次的标记，且落在 system / developer 层；
//! - `user-only`：命中了，但只在 user 层（压不住身份）；
//! - `no-hit`：抓到了请求，但标记不在；
//! - `no-request`：宿主没发请求（未安装 / 参数不对）；
//! - `blocked`：这家没有可用的隔离通道（我们知道做不到，如实写）。

use std::path::Path;
use std::time::{Duration, Instant};

use metis_core::{ErrorKind, HarnessId, MetisError, Result};
use serde::{Deserialize, Serialize};

/// 报告格式版本。
pub const REPORT_VERSION: u32 = 1;

/// 一家验证的结论。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VerifyStatus {
    /// 命中，且落在特权层（system / developer）。
    Ok,
    /// 只在 user 层命中。
    UserOnly,
    /// 抓到了请求但没有标记。
    NoHit,
    /// 没有抓到请求。
    NoRequest,
    /// 这家没有可用的隔离通道。
    Blocked,
    /// 本机没装。
    Missing,
}

impl VerifyStatus {
    /// 序列化值。
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::UserOnly => "user-only",
            Self::NoHit => "no-hit",
            Self::NoRequest => "no-request",
            Self::Blocked => "blocked",
            Self::Missing => "missing",
        }
    }

    /// 算不算"通过"。
    pub const fn passed(self) -> bool {
        matches!(self, Self::Ok)
    }
}

/// 一家的验证结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HarnessResult {
    /// 哪家。
    pub harness: String,
    /// 结论。
    pub status: VerifyStatus,
    /// 标记落在哪个角色（`None` = 没命中）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub where_: Option<String>,
    /// 抓到的请求数。
    #[serde(default)]
    pub requests: usize,
    /// 用的模式。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// 说明（失败原因 / 为什么 blocked）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 各角色的字符数（**不保存正文**）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub roles: Vec<RoleSummary>,
}

/// 一个角色的摘要。只记长度与命中次数 —— 正文永不落盘。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleSummary {
    /// 角色名。
    pub role: String,
    /// 字符数。
    pub chars: usize,
    /// 命中次数。
    #[serde(rename = "markerCount")]
    pub marker_count: usize,
}

/// 一次验证的报告。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyReport {
    /// 格式版本。
    pub v: u32,
    /// 时间（RFC 3339）。
    pub at: String,
    /// 逐家结果。
    pub results: Vec<HarnessResult>,
}

impl VerifyReport {
    /// 有几家通过。
    pub fn passed(&self) -> usize {
        self.results.iter().filter(|r| r.status.passed()).count()
    }

    /// 有失败吗（`no-hit` / `user-only` 才算失败；`missing` / `blocked` 是"测不了"）。
    pub fn failed(&self) -> usize {
        self.results.iter().filter(|r| matches!(r.status, VerifyStatus::NoHit | VerifyStatus::UserOnly)).count()
    }

    /// 写入 `verification/latest.json`。
    pub fn save(&self, path: &Path) -> Result<()> {
        let mut bytes =
            serde_json::to_vec_pretty(self).map_err(|error| MetisError::new(ErrorKind::Internal, "验证报告无法序列化").with_detail(error.to_string()))?;
        bytes.push(b'\n');
        metis_fs::write_atomic(path, &bytes)
    }

    /// 读 `verification/latest.json`。
    pub fn load(path: &Path) -> Result<Option<Self>> {
        let Some(text) = metis_fs::read_to_string_optional(path)? else { return Ok(None) };
        serde_json::from_str(&text).map(Some).map_err(|error| {
            MetisError::new(ErrorKind::StateCorrupt, format!("验证报告损坏：{}", path.display()))
                .with_detail(error.to_string())
                .with_remediation("删掉这个报告文件，然后重新跑一次 verify")
        })
    }
}

/// 验证一家需要的输入。
#[derive(Debug, Clone)]
pub struct VerifyInput {
    /// 哪家。
    pub harness: HarnessId,
    /// 要执行的命令（例如 `codex exec`），由 `launch_spec` 生成。
    pub program: String,
    /// 参数（已经带上了 base-url 之类的覆盖参数）。
    pub args: Vec<String>,
    /// 环境变量覆盖。
    pub env: Vec<(String, String)>,
    /// 本次的模式。
    pub mode: String,
    /// 这家是否可隔离验证；`None` = 可以，`Some(原因)` = 不行。
    pub blocked: Option<String>,
}

/// 跑一次验证：起 provider → 执行宿主 → 收请求 → 判定。
///
/// `deadline` 是整体上限：宿主不一定会退出，抓不到就如实报 `no-request`。
pub fn verify_one(input: &VerifyInput, marker: &str, deadline: Duration) -> Result<HarnessResult> {
    if let Some(reason) = &input.blocked {
        return Ok(HarnessResult {
            harness: input.harness.to_string(),
            status: VerifyStatus::Blocked,
            where_: None,
            requests: 0,
            mode: Some(input.mode.clone()),
            reason: Some(reason.clone()),
            roles: Vec::new(),
        });
    }
    let provider = metis_process::FakeProvider::start(marker).map_err(|error| {
        MetisError::new(ErrorKind::Io, format!("假 provider 启动失败：{error}")).with_remediation("本机可能不允许监听回环端口；检查防火墙或安全软件")
    })?;
    verify_with(input, &provider, marker, deadline)
}

/// 用一个**已经启动**的 provider 验证。
///
/// 调用方需要先拿到端口才能把 base-url 写进宿主的配方，所以把 provider 的
/// 生命周期交给调用方 —— `verify_one` 只是"自己起一个再调它"的便捷入口。
pub fn verify_with(input: &VerifyInput, provider: &metis_process::FakeProvider, _marker: &str, deadline: Duration) -> Result<HarnessResult> {
    if let Some(reason) = &input.blocked {
        return Ok(HarnessResult {
            harness: input.harness.to_string(),
            status: VerifyStatus::Blocked,
            where_: None,
            requests: 0,
            mode: Some(input.mode.clone()),
            reason: Some(reason.clone()),
            roles: Vec::new(),
        });
    }

    let mut exec = metis_process::Exec::new(input.program.clone())
        .args(input.args.iter().cloned())
        // 宿主可能挂着等交互：给一个短上限，抓不到就如实报。
        .wall_clock(deadline)
        .allow_output_overflow();
    for (key, value) in &input.env {
        exec = exec.env(key.clone(), value.clone());
    }
    // 宿主的退出码不重要：我们要的是它**发出去的请求**。
    let outcome = exec.run();
    // 宿主自己的输出是排查"为什么没抓到请求"的唯一线索：
    // 把它带进失败原因，比让用户猜"可能没装"有用得多。
    let host_stderr = match &outcome {
        Ok(o) => o.stderr.clone(),
        Err(e) => e.message().to_owned(),
    };

    // 宿主进程已退出，但请求可能还在路上（TCP 缓冲、客户端延迟 flush）。
    //
    // 这里**不能**用 `deadline` 推导：`deadline` 是"宿主可以跑多久"，
    // 不是"我们等请求多久"。宿主用满 deadline 时，推导出来的收尾窗口会接近 0，
    // 于是必然误判成 `no-request`（实测并发 + 插桩构建下稳定复现）。
    // 收尾窗口是一个**独立常量**，与宿主超时无关。
    let settle = Duration::from_secs(5);
    let start = Instant::now();
    while provider.count() == 0 && start.elapsed() < settle {
        std::thread::sleep(Duration::from_millis(20));
    }

    let requests = provider.requests();
    if requests.is_empty() {
        return Ok(HarnessResult {
            harness: input.harness.to_string(),
            status: VerifyStatus::NoRequest,
            where_: None,
            requests: 0,
            mode: Some(input.mode.clone()),
            reason: Some(if host_stderr.trim().is_empty() {
                "宿主没有向本地假 provider 发出请求（可能没装、参数不被接受，或它忽略 base-url）".to_owned()
            } else {
                format!("宿主没有向本地假 provider 发出请求；宿主输出：{}", host_stderr.trim())
            }),
            roles: Vec::new(),
        });
    }

    let hit_role = requests.iter().find_map(|request| request.hit_role().map(str::to_owned));
    let roles: Vec<RoleSummary> = requests
        .iter()
        .flat_map(|request| request.roles.iter())
        .map(|role| RoleSummary { role: role.role.clone(), chars: role.chars, marker_count: role.marker_count })
        .collect();

    let status = match hit_role.as_deref() {
        Some("system") | Some("developer") => VerifyStatus::Ok,
        Some(_) => VerifyStatus::UserOnly,
        None => VerifyStatus::NoHit,
    };

    Ok(HarnessResult {
        harness: input.harness.to_string(),
        status,
        where_: hit_role,
        requests: requests.len(),
        mode: Some(input.mode.clone()),
        reason: match status {
            VerifyStatus::UserOnly => Some("只落在 user 层：压不住身份，这不是有效的注入".to_owned()),
            VerifyStatus::NoHit => Some("请求里没有本次的标记：人设没有进入请求".to_owned()),
            _ => None,
        },
        roles,
    })
}

/// 生成一个不会与历史记录撞车的标记。
///
/// 每次验证换新标记：旧文件、旧会话里的残留不能被误判成本次命中。
pub fn make_marker(harness: &HarnessId) -> String {
    let tag = uuid::Uuid::new_v4().simple().to_string();
    format!("METIS_VERIFY_{}_{}", harness.as_str().to_uppercase().replace('-', "_"), &tag[..12])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: &str) -> HarnessId {
        HarnessId::parse(value).expect("id")
    }

    #[test]
    fn marker_is_unique_per_call_and_mentions_the_harness() {
        let first = make_marker(&id("codex"));
        let second = make_marker(&id("codex"));
        assert_ne!(first, second, "每次必须换新标记");
        assert!(first.contains("CODEX"), "标记里应能看出是哪家：{first}");
        assert!(first.starts_with("METIS_VERIFY_"));
    }

    #[test]
    fn marker_safe_for_dash_names() {
        let marker = make_marker(&id("pi-desktop"));
        assert!(marker.contains("PI_DESKTOP"), "连字符要换成下划线：{marker}");
        assert!(!marker.contains('-'));
    }

    #[test]
    fn blocked_harness_reports_why_without_starting_anything() {
        let input = VerifyInput {
            harness: id("claude"),
            program: "definitely-not-a-real-program".to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            mode: "replace".to_owned(),
            blocked: Some("这家没有可用的隔离 provider 通道".to_owned()),
        };
        let result = verify_one(&input, "MARK", Duration::from_millis(100)).expect("verify");
        assert_eq!(result.status, VerifyStatus::Blocked);
        assert!(result.reason.unwrap_or_default().contains("隔离"));
        assert_eq!(result.requests, 0);
    }

    #[test]
    fn a_program_that_never_calls_the_provider_is_no_request() {
        // 用一个真的不存在请求行为的程序：应该报 no-request 而不是崩。
        #[cfg(windows)]
        let (program, args) = ("cmd", vec!["/C".to_owned(), "exit 0".to_owned()]);
        #[cfg(not(windows))]
        let (program, args) = ("sh", vec!["-c".to_owned(), "exit 0".to_owned()]);

        let input = VerifyInput { harness: id("codex"), program: program.to_owned(), args, env: Vec::new(), mode: "replace".to_owned(), blocked: None };
        let result = verify_one(&input, "MARK", Duration::from_secs(3)).expect("verify");
        assert_eq!(result.status, VerifyStatus::NoRequest);
        assert!(result.reason.is_some(), "必须说明为什么没抓到");
    }

    #[test]
    fn missing_program_is_no_request_with_a_reason() {
        let input = VerifyInput {
            harness: id("codex"),
            program: "metis-no-such-program-xyz".to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            mode: "replace".to_owned(),
            blocked: None,
        };
        let result = verify_one(&input, "MARK", Duration::from_millis(200)).expect("verify");
        assert_eq!(result.status, VerifyStatus::NoRequest);
    }

    #[test]
    fn statuses_have_stable_strings_and_pass_semantics() {
        assert_eq!(VerifyStatus::Ok.as_str(), "ok");
        assert_eq!(VerifyStatus::UserOnly.as_str(), "user-only");
        assert_eq!(VerifyStatus::NoHit.as_str(), "no-hit");
        assert_eq!(VerifyStatus::NoRequest.as_str(), "no-request");
        assert_eq!(VerifyStatus::Blocked.as_str(), "blocked");
        assert_eq!(VerifyStatus::Missing.as_str(), "missing");
        assert!(VerifyStatus::Ok.passed());
        for status in [VerifyStatus::UserOnly, VerifyStatus::NoHit, VerifyStatus::NoRequest, VerifyStatus::Blocked, VerifyStatus::Missing] {
            assert!(!status.passed(), "{} 不该算通过", status.as_str());
        }
    }

    #[test]
    fn report_counts_and_round_trips() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("verification").join("latest.json");
        let report = VerifyReport {
            v: REPORT_VERSION,
            at: "2026-11-06T00:00:00Z".to_owned(),
            results: vec![
                HarnessResult {
                    harness: "codex".to_owned(),
                    status: VerifyStatus::Ok,
                    where_: Some("developer".to_owned()),
                    requests: 1,
                    mode: Some("replace".to_owned()),
                    reason: None,
                    roles: vec![RoleSummary { role: "developer".to_owned(), chars: 120, marker_count: 1 }],
                },
                HarnessResult {
                    harness: "claude".to_owned(),
                    status: VerifyStatus::Blocked,
                    where_: None,
                    requests: 0,
                    mode: Some("replace".to_owned()),
                    reason: Some("没有隔离通道".to_owned()),
                    roles: Vec::new(),
                },
                HarnessResult {
                    harness: "grok".to_owned(),
                    status: VerifyStatus::NoHit,
                    where_: None,
                    requests: 1,
                    mode: Some("replace".to_owned()),
                    reason: Some("没有标记".to_owned()),
                    roles: Vec::new(),
                },
            ],
        };
        assert_eq!(report.passed(), 1);
        assert_eq!(report.failed(), 1, "blocked 不算失败（那是测不了）");

        report.save(&path).expect("save");
        let loaded = VerifyReport::load(&path).expect("load").expect("present");
        assert_eq!(loaded, report);
    }

    #[test]
    fn missing_report_is_none_not_an_error() {
        let dir = tempfile::tempdir().expect("tmp");
        assert!(VerifyReport::load(&dir.path().join("nope.json")).expect("load").is_none());
    }

    #[test]
    fn corrupt_report_is_an_error() {
        let dir = tempfile::tempdir().expect("tmp");
        let path = dir.path().join("bad.json");
        std::fs::write(&path, b"{ broken").expect("seed");
        let error = VerifyReport::load(&path).expect_err("must fail");
        assert_eq!(error.kind(), ErrorKind::StateCorrupt);
    }

    #[test]
    fn report_never_contains_persona_text() {
        // 隐私约束：报告只记角色、长度、命中次数。这条测试守着"不写正文"这个承诺。
        let report = VerifyReport {
            v: REPORT_VERSION,
            at: "now".to_owned(),
            results: vec![HarnessResult {
                harness: "codex".to_owned(),
                status: VerifyStatus::Ok,
                where_: Some("developer".to_owned()),
                requests: 1,
                mode: Some("replace".to_owned()),
                reason: None,
                roles: vec![RoleSummary { role: "developer".to_owned(), chars: 1200, marker_count: 1 }],
            }],
        };
        let json = serde_json::to_string(&report).expect("json");
        assert!(!json.contains("用简体中文"), "不该有正文");
        assert!(json.contains("\"chars\":1200"), "应记录长度");
        assert!(json.contains("\"markerCount\":1"), "应记录命中次数");
    }
}
