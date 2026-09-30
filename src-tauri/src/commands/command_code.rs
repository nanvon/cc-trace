//! Command Code 的凭据管理：手动 API Key 的写入、清除与状态查询。
//!
//! 自动模式下的四个凭据来源由 [`crate::providers::credentials::command_code`] 依次尝试，
//! 界面只负责显示「当前用的是哪一个」；手动模式只看这里写的 Key。

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::app::AppCore;
use crate::contracts::CommandCodeCredentialPreference;
use crate::providers::credentials::command_code::{self, SecretWriteOutcome};

use super::CommandError;

/// 凭据状态：偏好 + 是否已填写手动 Key。
/// 不回传 Key 本身，也不回传它的长度或前后缀。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandCodeCredentialState {
    pub preference: CommandCodeCredentialPreference,
    pub has_manual_key: bool,
}

#[tauri::command]
pub fn command_code_credential_state(core: State<'_, Arc<AppCore>>) -> CommandCodeCredentialState {
    CommandCodeCredentialState {
        preference: core.settings().command_code_credential,
        has_manual_key: matches!(
            command_code::discover(CommandCodeCredentialPreference::Manual),
            crate::providers::credentials::Discovery::Found(_)
        ),
    }
}

/// 写入手动 API Key。写成功后按新凭据刷新一次，让界面立刻反映结果。
#[tauri::command]
pub fn command_code_set_api_key(
    app: tauri::AppHandle,
    core: State<'_, Arc<AppCore>>,
    api_key: String,
) -> Result<CommandCodeCredentialState, CommandError> {
    match command_code::save_manual_key(&api_key) {
        SecretWriteOutcome::Ok => {}
        SecretWriteOutcome::Invalid => return Err(CommandError::INVALID_ARGUMENT),
        SecretWriteOutcome::Denied | SecretWriteOutcome::Failed => {
            return Err(CommandError::CREDENTIAL_STORE_FAILED);
        }
    }

    core.set_command_code_credential_preference(CommandCodeCredentialPreference::Manual)
        .map_err(|_| CommandError::SETTINGS_WRITE_FAILED)?;
    core.refresh_subject(
        &app,
        "commandCode",
        crate::scheduler::RefreshTrigger::Manual,
    );

    Ok(CommandCodeCredentialState {
        preference: CommandCodeCredentialPreference::Manual,
        has_manual_key: true,
    })
}

/// 清除手动 API Key，并把偏好退回自动模式。
#[tauri::command]
pub fn command_code_clear_api_key(
    app: tauri::AppHandle,
    core: State<'_, Arc<AppCore>>,
) -> Result<CommandCodeCredentialState, CommandError> {
    match command_code::clear_manual_key() {
        SecretWriteOutcome::Ok => {}
        SecretWriteOutcome::Invalid => return Err(CommandError::INVALID_ARGUMENT),
        SecretWriteOutcome::Denied | SecretWriteOutcome::Failed => {
            return Err(CommandError::CREDENTIAL_STORE_FAILED);
        }
    }

    core.set_command_code_credential_preference(CommandCodeCredentialPreference::Automatic)
        .map_err(|_| CommandError::SETTINGS_WRITE_FAILED)?;
    core.refresh_subject(
        &app,
        "commandCode",
        crate::scheduler::RefreshTrigger::Manual,
    );

    Ok(CommandCodeCredentialState {
        preference: CommandCodeCredentialPreference::Automatic,
        has_manual_key: false,
    })
}
