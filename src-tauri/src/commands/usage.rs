//! 本地用量 command。
//!
//! command 不接受路径、SQL 或排序表达式；所有输入先由 Rust 用例校验。

use std::sync::Arc;

use tauri::State;

use crate::app::AppCore;
use crate::contracts::{
    PricingCatalogRefreshStatus, QuotaCyclePage, QuotaCycleQuery, QuotaHistory, QuotaHistoryQuery,
    UsageConversation, UsageConversationBreakdown, UsageConversationPage,
    UsageConversationProjectOption, UsageConversationQuery, UsageProjectBreakdown,
    UsageProjectBreakdownQuery, UsageProjectPage, UsageProjectQuery, UsageRepriceResult,
    UsageScanStatus, UsageSource, UsageSummary, UsageSummaryQuery,
};
use crate::usage::CursorRemoteOutcome;
use crate::usage::UsageError;

use super::CommandError;

/// 一次远端刷新的结局（脱敏 DTO）：只带分类与计数，不带响应原文。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum CursorRemoteOutcomeView {
    /// 没有可用的 Cursor 登录态：远端计量整体跳过，不是错误。
    NoCredential,
    /// 距上次尝试太近或仍在退避期内。
    Throttled,
    /// 拉取成功（可能只是补齐了部分日子）。
    Updated { ranges: usize, buckets: usize },
    /// 拉取失败；上一次的远端数据保留。
    Failed { error: String, retry_after: String },
}

impl From<CursorRemoteOutcome> for CursorRemoteOutcomeView {
    fn from(outcome: CursorRemoteOutcome) -> Self {
        match outcome {
            CursorRemoteOutcome::NoCredential => Self::NoCredential,
            CursorRemoteOutcome::Throttled => Self::Throttled,
            CursorRemoteOutcome::Updated { ranges, buckets } => Self::Updated { ranges, buckets },
            CursorRemoteOutcome::Failed { error, retry_after } => {
                Self::Failed { error, retry_after }
            }
        }
    }
}

#[tauri::command]
pub fn usage_scan_start(core: State<'_, Arc<AppCore>>) -> Result<UsageScanStatus, CommandError> {
    core.usage().start_default_scan().map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_scan_cancel(core: State<'_, Arc<AppCore>>) -> UsageScanStatus {
    core.usage().cancel_scan()
}

/// 手动拉取 Cursor 远端计量。返回一次刷新的结局，界面据此说明状态。
///
/// `force` 在这里恒为真：用户点了刷新就该真发请求；被限流后的退避仍然拦住它。
#[tauri::command]
pub async fn usage_cursor_remote_refresh(
    core: State<'_, Arc<AppCore>>,
) -> Result<CursorRemoteOutcomeView, CommandError> {
    let outcome = core.usage().refresh_cursor_remote(true).await;
    Ok(CursorRemoteOutcomeView::from(outcome))
}

/// 远端计量的当前状态：上次成功、上次错误、退避截止、已覆盖天数。
#[tauri::command]
pub fn usage_cursor_remote_status(
    core: State<'_, Arc<AppCore>>,
) -> crate::usage::CursorRemoteStatus {
    core.usage().cursor_remote_status()
}

#[tauri::command]
pub fn usage_scan_status(core: State<'_, Arc<AppCore>>) -> UsageScanStatus {
    core.usage().scan_status()
}

#[tauri::command]
pub fn usage_get_summary(
    core: State<'_, Arc<AppCore>>,
    query: UsageSummaryQuery,
) -> Result<UsageSummary, CommandError> {
    core.usage().summary(query).map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_list_conversations(
    core: State<'_, Arc<AppCore>>,
    query: UsageConversationQuery,
) -> Result<UsageConversationPage, CommandError> {
    core.usage().conversations(query).map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_list_projects(
    core: State<'_, Arc<AppCore>>,
    query: UsageProjectQuery,
) -> Result<UsageProjectPage, CommandError> {
    core.usage().projects(query).map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_get_project_breakdown(
    core: State<'_, Arc<AppCore>>,
    query: UsageProjectBreakdownQuery,
) -> Result<UsageProjectBreakdown, CommandError> {
    core.usage()
        .project_breakdown(query)
        .map_err(map_usage_error)
}

/// 在系统文件管理器中显示项目目录。只放行库里记录过的项目根与 worktree 目录，
/// 目录已不存在时返回 `false`。
#[tauri::command]
pub fn usage_reveal_project(
    core: State<'_, Arc<AppCore>>,
    path: String,
) -> Result<bool, CommandError> {
    let target = core
        .usage()
        .revealable_project_path(&path)
        .map_err(map_usage_error)?;
    Ok(target.is_some_and(|dir| crate::platform::open::open_folder(&dir)))
}

#[tauri::command]
pub fn usage_list_conversation_projects(
    core: State<'_, Arc<AppCore>>,
    query: UsageConversationQuery,
) -> Result<Vec<UsageConversationProjectOption>, CommandError> {
    core.usage()
        .conversation_projects(query)
        .map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_get_conversation(
    core: State<'_, Arc<AppCore>>,
    conversation_key: String,
) -> Result<Option<UsageConversation>, CommandError> {
    core.usage()
        .conversation(conversation_key)
        .map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_get_conversation_breakdown(
    core: State<'_, Arc<AppCore>>,
    conversation_key: String,
) -> Result<Option<UsageConversationBreakdown>, CommandError> {
    core.usage()
        .conversation_breakdown(conversation_key)
        .map_err(map_usage_error)
}

/// 额度周期与用满预估。周期是从额度事件现算的派生结果，不落盘。
#[tauri::command]
pub fn usage_get_quota_cycles(
    core: State<'_, Arc<AppCore>>,
    query: QuotaCycleQuery,
) -> Result<QuotaCyclePage, CommandError> {
    let labels = core.subject_labels();
    core.usage()
        .quota_cycles(query, &labels)
        .map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_get_quota_history(
    core: State<'_, Arc<AppCore>>,
    query: QuotaHistoryQuery,
) -> Result<QuotaHistory, CommandError> {
    core.usage().quota_history(query).map_err(map_usage_error)
}

#[tauri::command]
pub async fn usage_reprice(
    core: State<'_, Arc<AppCore>>,
) -> Result<UsageRepriceResult, CommandError> {
    let usage = core.usage();
    tauri::async_runtime::spawn_blocking(move || usage.reprice())
        .await
        .map_err(|_| CommandError::USAGE_UNAVAILABLE)?
        .map_err(map_usage_error)
}

#[tauri::command]
pub async fn usage_refresh_pricing_catalog(
    core: State<'_, Arc<AppCore>>,
) -> Result<PricingCatalogRefreshStatus, CommandError> {
    let usage = core.usage();
    usage
        .refresh_pricing_catalog()
        .await
        .map_err(map_usage_error)
}

#[tauri::command]
pub async fn usage_rebuild_data(
    core: State<'_, Arc<AppCore>>,
    sources: Option<Vec<UsageSource>>,
) -> Result<UsageScanStatus, CommandError> {
    let usage = core.usage();
    tauri::async_runtime::spawn_blocking(move || usage.rebuild(sources))
        .await
        .map_err(|_| CommandError::USAGE_UNAVAILABLE)?
        .map_err(map_usage_error)
}

fn map_usage_error(error: UsageError) -> CommandError {
    match error {
        UsageError::InvalidQuery => CommandError::INVALID_USAGE_QUERY,
        UsageError::Unavailable => CommandError::USAGE_UNAVAILABLE,
        UsageError::ScanBusy => CommandError::USAGE_SCAN_BUSY,
    }
}
