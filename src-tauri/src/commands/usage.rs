//! 本地用量 command。
//!
//! command 不接受路径、SQL 或排序表达式；所有输入先由 Rust 用例校验。

use std::sync::Arc;

use tauri::State;

use crate::app::AppCore;
use crate::contracts::{
    UsageConversation, UsageConversationPage, UsageConversationQuery, UsageRepriceResult,
    UsageScanStatus, UsageSummary, UsageSummaryQuery,
};
use crate::usage::UsageError;

use super::CommandError;

#[tauri::command]
pub fn usage_scan_start(core: State<'_, Arc<AppCore>>) -> Result<UsageScanStatus, CommandError> {
    core.usage().start_default_scan().map_err(map_usage_error)
}

#[tauri::command]
pub fn usage_scan_cancel(core: State<'_, Arc<AppCore>>) -> UsageScanStatus {
    core.usage().cancel_scan()
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
pub fn usage_get_conversation(
    core: State<'_, Arc<AppCore>>,
    conversation_key: String,
) -> Result<Option<UsageConversation>, CommandError> {
    core.usage()
        .conversation(conversation_key)
        .map_err(map_usage_error)
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

fn map_usage_error(error: UsageError) -> CommandError {
    match error {
        UsageError::InvalidQuery => CommandError::INVALID_USAGE_QUERY,
        UsageError::Unavailable => CommandError::USAGE_UNAVAILABLE,
        UsageError::ScanBusy => CommandError::USAGE_SCAN_BUSY,
    }
}
