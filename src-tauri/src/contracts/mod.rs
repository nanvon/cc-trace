//! 跨 command 边界使用的脱敏 DTO。
//!
//! 所有类型都可序列化且不含秘密、端点原文、文件路径或凭据内容。
//! 状态语义见 `docs/状态与错误模型.md`，字段语义见 `docs/额度领域模型.md`。

mod app_status;
mod error;
mod quota;
mod settings;
mod usage;

pub use app_status::AppStatus;
pub use error::{AppError, ErrorKind};
pub use quota::{
    ProviderAvailability, ProviderId, ProviderIdentity, ProviderSnapshot, QuotaSnapshot,
    QuotaState, QuotaWindow, QuotaWindowKind, RefreshState, RefreshStatePayload, SnapshotFreshness,
};
pub use settings::{
    AppearancePreference, LanguagePreference, OnboardingState, RefreshInterval,
    SETTINGS_SCHEMA_VERSION, Settings, SettingsUpdate,
};
pub use usage::{
    UsageConversation, UsageConversationPage, UsageConversationQuery, UsageCostTotals, UsageFilter,
    UsageGroupBy, UsageRepriceResult, UsageScanState, UsageScanStatus, UsageSource, UsageSpeed,
    UsageSummary, UsageSummaryQuery, UsageSummaryRow, UsageTokenTotals,
};
