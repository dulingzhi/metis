//! 错误契约。
//!
//! 规则（AGENTS.md 铁律 1）：任何面向用户的失败都要有一条**补救建议**，让人能照做。
//! `ErrorKind` 是封闭集合，每个都有稳定编号 —— 判定用类型，展示用文案。

use std::fmt;

/// Metis 的错误类别。封闭集合：新增必须同时加文案键与测试。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ErrorKind {
    /// 文件系统读写失败。
    Io,
    /// 权限不足。
    Permission,
    /// 状态文件损坏（原文件已保留）。
    StateCorrupt,
    /// 另一个配置操作正在进行。
    LockHeld,
    /// 未知的执行器。
    UnknownHarness,
    /// 未知的通道。
    UnknownChannel,
    /// 该执行器不支持这条通道。
    UnsupportedChannel,
    /// 人设不存在。
    PersonaMissing,
    /// 人设文件非法（id、front-matter、编码）。
    PersonaInvalid,
    /// 恢复冲突：文件在交换后被修改。
    RestoreConflict,
    /// 检测到外部修改且不属于我们。
    ExternalModification,
    /// 外部程序非零退出。
    ProcessFailed,
    /// 外部程序超时，已被终止。
    ProcessTimeout,
    /// 找不到可执行文件。
    ProgramMissing,
    /// 网络请求失败。
    Network,
    /// 更新失败。
    UpdateFailed,
    /// 参数非法。
    InvalidArgument,
    /// 破坏性操作被安全闸门拒绝。
    Refused,
    /// 内部不变式被破坏（不应发生；发生了必须报告而不是静默）。
    Internal,
}

impl ErrorKind {
    /// 稳定的机器可读编号。写进日志与 `--json` 输出，改动等于破坏兼容。
    pub const fn code(self) -> &'static str {
        match self {
            Self::Io => "io",
            Self::Permission => "permission",
            Self::StateCorrupt => "state-corrupt",
            Self::LockHeld => "lock-held",
            Self::UnknownHarness => "unknown-harness",
            Self::UnknownChannel => "unknown-channel",
            Self::UnsupportedChannel => "unsupported-channel",
            Self::PersonaMissing => "persona-missing",
            Self::PersonaInvalid => "persona-invalid",
            Self::RestoreConflict => "restore-conflict",
            Self::ExternalModification => "external-modification",
            Self::ProcessFailed => "process-failed",
            Self::ProcessTimeout => "process-timeout",
            Self::ProgramMissing => "program-missing",
            Self::Network => "network",
            Self::UpdateFailed => "update-failed",
            Self::InvalidArgument => "invalid-argument",
            Self::Refused => "refused",
            Self::Internal => "internal",
        }
    }

    /// 这个错误值得原样重试一次吗？供 UI 决定是否显示"重试"。
    pub const fn retryable(self) -> bool {
        matches!(self, Self::Io | Self::LockHeld | Self::Network | Self::ProcessTimeout)
    }

    /// 全部类别的稳定编号。`xtask ci` 用它核对"每个 kind 都有文案与测试"。
    pub const ALL_CODES: &'static [&'static str] = &[
        "io",
        "permission",
        "state-corrupt",
        "lock-held",
        "unknown-harness",
        "unknown-channel",
        "unsupported-channel",
        "persona-missing",
        "persona-invalid",
        "restore-conflict",
        "external-modification",
        "process-failed",
        "process-timeout",
        "program-missing",
        "network",
        "update-failed",
        "invalid-argument",
        "refused",
        "internal",
    ];
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Metis 的错误。
///
/// `message` 是面向用户的一句话；`remediation` 是"照做就能修"的具体动作，
/// 不是"请检查配置"这种空话（AGENTS.md 铁律 1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetisError {
    kind: ErrorKind,
    message: String,
    remediation: Option<String>,
    detail: Option<String>,
}

impl MetisError {
    /// 构造一个错误。
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), remediation: None, detail: None }
    }

    /// 附上补救建议。
    #[must_use]
    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = Some(remediation.into());
        self
    }

    /// 附上诊断细节（路径、命令行、退出码）。展示层按需折叠。
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// 错误类别。
    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// 面向用户的一句话。
    pub fn message(&self) -> &str {
        &self.message
    }

    /// 补救建议。
    pub fn remediation(&self) -> Option<&str> {
        self.remediation.as_deref()
    }

    /// 诊断细节。
    pub fn detail(&self) -> Option<&str> {
        self.detail.as_deref()
    }

    /// 是否值得重试。
    pub fn retryable(&self) -> bool {
        self.kind.retryable()
    }

    /// 稳定的机器可读编号。
    pub fn code(&self) -> &'static str {
        self.kind.code()
    }

    /// 序列化为 `--json` 输出用的对象。
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code(),
            "kind": self.kind.code(),
            "message": self.message,
            "remediation": self.remediation,
            "detail": self.detail,
            "retryable": self.retryable(),
        })
    }
}

impl fmt::Display for MetisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(remediation) = &self.remediation {
            write!(f, "（{remediation}）")?;
        }
        Ok(())
    }
}

impl std::error::Error for MetisError {}

impl From<std::io::Error> for MetisError {
    fn from(error: std::io::Error) -> Self {
        let kind = if error.kind() == std::io::ErrorKind::PermissionDenied { ErrorKind::Permission } else { ErrorKind::Io };
        Self::new(kind, format!("文件操作失败：{error}"))
    }
}

/// Metis 的结果类型。
pub type Result<T> = std::result::Result<T, MetisError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_has_a_unique_code() {
        let mut codes: Vec<&str> = ErrorKind::ALL_CODES.to_vec();
        codes.sort_unstable();
        let before = codes.len();
        codes.dedup();
        assert_eq!(before, codes.len(), "错误编号重复：{codes:?}");
        assert_eq!(before, 19, "新增了 ErrorKind 却没更新 ALL_CODES");
    }

    #[test]
    fn all_codes_match_the_enum_variants() {
        // 每个 variant 的 code() 都必须出现在 ALL_CODES 里，反之亦然。
        let variants = [
            ErrorKind::Io,
            ErrorKind::Permission,
            ErrorKind::StateCorrupt,
            ErrorKind::LockHeld,
            ErrorKind::UnknownHarness,
            ErrorKind::UnknownChannel,
            ErrorKind::UnsupportedChannel,
            ErrorKind::PersonaMissing,
            ErrorKind::PersonaInvalid,
            ErrorKind::RestoreConflict,
            ErrorKind::ExternalModification,
            ErrorKind::ProcessFailed,
            ErrorKind::ProcessTimeout,
            ErrorKind::ProgramMissing,
            ErrorKind::Network,
            ErrorKind::UpdateFailed,
            ErrorKind::InvalidArgument,
            ErrorKind::Refused,
            ErrorKind::Internal,
        ];
        for variant in variants {
            assert!(ErrorKind::ALL_CODES.contains(&variant.code()), "{} 不在 ALL_CODES 里", variant.code());
        }
        assert_eq!(variants.len(), ErrorKind::ALL_CODES.len());
    }

    #[test]
    fn retryable_is_narrow_and_explicit() {
        assert!(ErrorKind::Io.retryable());
        assert!(ErrorKind::LockHeld.retryable());
        assert!(ErrorKind::Network.retryable());
        assert!(ErrorKind::ProcessTimeout.retryable());
        assert!(!ErrorKind::Refused.retryable(), "被闸门拒绝的错误重试也没用");
        assert!(!ErrorKind::StateCorrupt.retryable(), "损坏的文件重试还是损坏");
        assert!(!ErrorKind::ProgramMissing.retryable());
    }

    #[test]
    fn remediation_is_carried_through_display() {
        let error = MetisError::new(ErrorKind::ProgramMissing, "找不到 codex").with_remediation("先安装 codex，或从 PATH 里移除 shim");
        assert_eq!(error.kind(), ErrorKind::ProgramMissing);
        assert_eq!(error.code(), "program-missing");
        assert!(error.remediation().is_some());
        let shown = error.to_string();
        assert!(shown.contains("找不到 codex"));
        assert!(shown.contains("先安装 codex"), "Display 必须带出补救建议");
    }

    #[test]
    fn json_shape_is_stable() {
        let error =
            MetisError::new(ErrorKind::LockHeld, "另一个配置操作尚未结束").with_detail("PID 4242").with_remediation("等它结束，或确认没有进程在跑后删掉锁文件");
        let json = error.to_json();
        assert_eq!(json["code"], "lock-held");
        assert_eq!(json["retryable"], true);
        assert_eq!(json["detail"], "PID 4242");
        assert_eq!(json["remediation"], "等它结束，或确认没有进程在跑后删掉锁文件");

        // 没有补救建议时该字段是 `null`（不是缺字段，也不是空串）。
        let bare = MetisError::new(ErrorKind::Internal, "内部错误");
        assert!(bare.to_json()["remediation"].is_null());
    }

    #[test]
    fn io_errors_map_to_permission_or_io() {
        let denied = MetisError::from(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access"));
        assert_eq!(denied.kind(), ErrorKind::Permission);
        let missing = MetisError::from(std::io::Error::new(std::io::ErrorKind::NotFound, "gone"));
        assert_eq!(missing.kind(), ErrorKind::Io);
    }
}
