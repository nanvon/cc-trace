//! 诊断导出、日志目录与检查更新。
//!
//! 导出走 Rust 侧的系统保存对话框（`tauri-plugin-dialog`，只在 Rust 调用，
//! 前端没有 dialog／fs capability）。命令返回值不含任何路径。

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::app::AppCore;
use crate::diagnostics::update::{self, UpdateStatus};
use crate::diagnostics::{bundle, log};
use crate::platform::open;
use crate::providers::credentials::Discovery;
use crate::providers::credentials::claude::{self as claude_credentials, ClaudeSource};
use crate::providers::credentials::claude_desktop;
use crate::providers::credentials::command_code as command_code_credentials;

use super::CommandError;

pub const EVENT_UPDATE_STATUS: &str = "update://status";

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportOutcome {
    Saved,
    Cancelled,
    Failed,
}

/// 生成脱敏诊断包并让用户选择保存位置。
#[tauri::command]
pub async fn diagnostics_export(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
) -> Result<ExportOutcome, CommandError> {
    let version = env!("CARGO_PKG_VERSION");
    let text = bundle::build(
        version,
        &core.settings(),
        &core.quota_state(),
        core.imported_codex_accounts().len(),
    );
    let file_name = format!(
        "cc-trace-diagnostics-{}.txt",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );

    let dialog_app = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_file_name(&file_name)
            .add_filter("Text", &["txt"])
            .blocking_save_file()
    })
    .await;

    let Ok(Some(path)) = picked else {
        return Ok(ExportOutcome::Cancelled);
    };
    let Some(path) = path.as_path() else {
        return Ok(ExportOutcome::Failed);
    };
    match std::fs::write(path, text) {
        Ok(()) => {
            log::info("commands", "diagnostics_export", &[("result", "saved")]);
            Ok(ExportOutcome::Saved)
        }
        Err(_) => {
            log::error(
                "commands",
                "diagnostics_export",
                &[("result", "write_failed")],
            );
            Ok(ExportOutcome::Failed)
        }
    }
}

/// 在系统文件管理器里打开日志目录。目录不存在时返回 `false`。
#[tauri::command]
pub fn diagnostics_reveal_logs() -> bool {
    match log::log_dir() {
        Some(dir) if dir.exists() => open::open_folder(&dir),
        _ => false,
    }
}

/// 手动检查更新。结果同时记入内存，设置页重开后仍可读到。
#[tauri::command]
pub async fn update_check(app: AppHandle) -> UpdateStatus {
    let status = update::check(env!("CARGO_PKG_VERSION")).await;
    let _ = app.emit(EVENT_UPDATE_STATUS, &status);
    status
}

/// 最近一次检查结果（启动检查或手动检查）；没检查过是 `idle`。
#[tauri::command]
pub fn update_status() -> UpdateStatus {
    update::last_status()
}

/// 打开本仓库的最新 Release 页面。
#[tauri::command]
pub fn update_open_release() -> bool {
    open::open_url(update::RELEASES_PAGE)
}

/// 各服务当前凭据来源的语义名，不含路径、账号或任何秘密。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialSources {
    /// `file`、`keychain`、`none`、`unreadable`、`expired`。Windows 只会出现 `file`／`none`／`unreadable`。
    pub claude_code: &'static str,
    /// Claude Desktop 的本地缓存是否存在（存在不代表账号匹配，只借 access token）。
    pub claude_desktop: bool,
    /// `commandcode`、`pi`、`opencode`、`env`、`keychain`（手动）、`none`、`unreadable`。
    pub command_code: &'static str,
}

fn discovery_label<T>(
    discovery: &Discovery<T>,
    found: impl Fn(&T) -> &'static str,
) -> &'static str {
    match discovery {
        Discovery::Found(value) => found(value),
        Discovery::Unreadable => "unreadable",
        Discovery::Expired => "expired",
        Discovery::Missing | Discovery::Unsupported => "none",
    }
}

/// 只读查询凭据来源。钥匙串读取可能阻塞，放到阻塞线程池。
#[tauri::command]
pub async fn credential_sources_get(
    core: State<'_, Arc<AppCore>>,
) -> Result<CredentialSources, CommandError> {
    let preference = core.settings().command_code_credential;
    tauri::async_runtime::spawn_blocking(move || {
        let claude = claude_credentials::discover();
        let command_code = command_code_credentials::discover(preference);
        CredentialSources {
            claude_code: discovery_label(&claude, |credentials| match credentials.source {
                ClaudeSource::File => "file",
                ClaudeSource::Keychain => "keychain",
                ClaudeSource::Desktop => "desktop",
            }),
            claude_desktop: claude_desktop::has_credential_material(),
            command_code: discovery_label(&command_code, |credentials| credentials.source.key()),
        }
    })
    .await
    .map_err(|_| CommandError::NOT_FOUND)
}
