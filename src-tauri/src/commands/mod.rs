//! Tauri command 边界。
//!
//! 每个 command 校验输入、调用 `crate::app` 的用例并返回明确 contract。
//! 错误映射成稳定的 `code`，由前端查 i18n 文案：载荷里不出现 Rust 枚举名、
//! 文件路径、系统错误原文或凭据内容。

pub mod codex_accounts;
pub mod command_code;
pub mod diagnostics;
pub mod quota;
pub mod service_status;
pub mod settings;
pub mod status;
pub mod usage;
pub mod window;

#[cfg(debug_assertions)]
pub mod dev;

use serde::Serialize;

/// 可展示的命令失败。`code` 是稳定标识，不是给用户看的文本。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
}

impl CommandError {
    /// 请求的窗口不存在或无法显示。
    pub const WINDOW_UNAVAILABLE: Self = Self {
        code: "windowUnavailable",
    };

    /// 设置写入失败，界面必须保留原值并明确提示。
    pub const SETTINGS_WRITE_FAILED: Self = Self {
        code: "settingsWriteFailed",
    };

    pub const INVALID_USAGE_QUERY: Self = Self {
        code: "invalidUsageQuery",
    };

    pub const USAGE_UNAVAILABLE: Self = Self {
        code: "usageUnavailable",
    };

    pub const USAGE_SCAN_BUSY: Self = Self {
        code: "usageScanBusy",
    };

    /// 输入不合法：格式不对、字段缺失、列表与当前状态不匹配。
    pub const INVALID_ARGUMENT: Self = Self {
        code: "invalidArgument",
    };

    /// 目标不存在：账号已被删除，或 id 不属于当前列表。
    pub const NOT_FOUND: Self = Self { code: "notFound" };

    /// 粘贴的内容不是可用的 Codex 凭据。
    pub const CODEX_ACCOUNT_INVALID: Self = Self {
        code: "codexAccountInvalid",
    };

    /// 凭据存在但读不出来（系统凭据存储拒绝访问）。
    pub const CODEX_ACCOUNT_UNREADABLE: Self = Self {
        code: "codexAccountUnreadable",
    };

    /// 服务端拒绝了这个凭据：需要重新登录或换一个令牌。
    pub const CODEX_ACCOUNT_REJECTED: Self = Self {
        code: "codexAccountRejected",
    };

    /// 取数时网络不可用，无法确认凭据有效性。
    pub const CODEX_ACCOUNT_OFFLINE: Self = Self {
        code: "codexAccountOffline",
    };

    /// 取数被限流，稍后重试即可。
    pub const CODEX_ACCOUNT_RATE_LIMITED: Self = Self {
        code: "codexAccountRateLimited",
    };

    /// 取数成功但响应里没有身份，无法给账号一个稳定键。
    pub const CODEX_ACCOUNT_UNIDENTIFIED: Self = Self {
        code: "codexAccountUnidentified",
    };

    /// 凭据写入系统秘密存储失败。
    pub const CODEX_ACCOUNT_STORE_FAILED: Self = Self {
        code: "codexAccountStoreFailed",
    };

    /// 凭据写入系统秘密存储失败（手动 API Key）。
    pub const CREDENTIAL_STORE_FAILED: Self = Self {
        code: "credentialStoreFailed",
    };

    /// 把一次取数结果映射成导入流程可展示的错误码。
    /// `Success` 走到这里说明调用方漏了成功分支，按输入不合法处理。
    pub fn from_fetch_outcome(outcome: crate::providers::ProviderFetchOutcome) -> Self {
        use crate::contracts::ErrorKind;
        use crate::providers::ProviderFetchOutcome;

        match outcome {
            ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            } => Self::CODEX_ACCOUNT_REJECTED,
            ProviderFetchOutcome::Offline => Self::CODEX_ACCOUNT_OFFLINE,
            ProviderFetchOutcome::RateLimited { .. } => Self::CODEX_ACCOUNT_RATE_LIMITED,
            ProviderFetchOutcome::NoCredentials
            | ProviderFetchOutcome::Unsupported
            | ProviderFetchOutcome::Failed { .. }
            | ProviderFetchOutcome::Success { .. } => Self::CODEX_ACCOUNT_INVALID,
        }
    }
}
