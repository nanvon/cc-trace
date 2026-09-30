//! 导入的 Codex 副账号：列表、导入、改名、显示开关、排序、删除。
//!
//! 凭据在系统秘密存储里，本层只搬运「用户粘贴的内容」与脱敏元数据：
//! 返回载荷里永远没有 token 或 `auth.json` 原文。
//!
//! 导入流程见 `docs/决策/ADR-0032-额度主体与多账号.md`：
//! OAuth 形态（含 `tokens`）在本地就能解出账号与计划；personal access token
//! 不透明，必须先联网取一次额度确认有效并回填身份，取数失败就不落库。

use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tauri::State;

use crate::app::AppCore;
use crate::providers::codex;
use crate::providers::credentials::codex as codex_credentials;
use crate::providers::credentials::{Discovery, Secret};
use crate::storage::{ImportedCodexAccount, identity_hash};

use super::CommandError;

/// 导入账号的脱敏视图。不含槽位名、token 或 `auth.json` 原文。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedCodexAccountView {
    /// 复合身份 `{account_id}:{user_id}`，是增删改的操作键。
    pub id: String,
    pub display_name: String,
    pub email: Option<String>,
    pub plan: Option<String>,
    pub visible: bool,
    pub personal_access_token: bool,
    pub added_at: String,
}

impl ImportedCodexAccountView {
    fn from_account(account: &ImportedCodexAccount, index: usize) -> Self {
        Self {
            id: account.id.clone(),
            display_name: account.display_name(&format!("Codex 账号 {}", index + 1)),
            email: account.email.clone(),
            plan: account.plan.clone(),
            visible: account.visible,
            personal_access_token: account.personal_access_token,
            added_at: account.added_at.clone(),
        }
    }
}

fn views(accounts: &[ImportedCodexAccount]) -> Vec<ImportedCodexAccountView> {
    accounts
        .iter()
        .enumerate()
        .map(|(index, account)| ImportedCodexAccountView::from_account(account, index))
        .collect()
}

/// 导入内容的形态。
enum ImportInput {
    /// 用户粘贴了 `auth.json` 内容。
    AuthJson(String),
    /// 用户粘贴了不透明的 personal access token。
    PersonalAccessToken(String),
}

/// 判断粘贴内容是什么。以 `{` 开头的按 JSON 处理，否则按下透明令牌处理：
/// 两者在界面里是同一个输入框，用户不该先选格式再粘贴。
fn classify(payload: &str) -> Option<ImportInput> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with('{') {
        return Some(ImportInput::AuthJson(trimmed.to_owned()));
    }
    if trimmed.contains(char::is_whitespace) || trimmed.len() > 4096 {
        return None;
    }
    Some(ImportInput::PersonalAccessToken(trimmed.to_owned()))
}

/// 查某个 Codex 主体的额外重置 credit。
///
/// 省略 `subject_id` 时查主账号。这条命令**不接入调度**：只有用户点开设置里的
/// 「其他 Codex 账号」区域时才发请求，并且结果不写进任何缓存。
#[tauri::command]
pub async fn codex_reset_credits(
    core: State<'_, Arc<AppCore>>,
    subject_id: Option<String>,
) -> Result<crate::providers::codex_reset_credits::CodexResetCredits, CommandError> {
    let subject_id = subject_id.unwrap_or_else(|| "codex".to_owned());
    let imported = Arc::clone(core.imported_codex_store());
    let credentials = match codex::discover_credentials_for_subject(&subject_id, &imported) {
        Discovery::Found(credentials) => credentials,
        Discovery::Missing => return Err(CommandError::CODEX_ACCOUNT_INVALID),
        Discovery::Expired | Discovery::Unreadable => {
            return Err(CommandError::CODEX_ACCOUNT_UNREADABLE);
        }
        Discovery::Unsupported => return Err(CommandError::CODEX_ACCOUNT_INVALID),
    };

    crate::providers::codex_reset_credits::fetch(&credentials)
        .await
        .map_err(CommandError::from_fetch_outcome)
}

#[tauri::command]
pub fn codex_accounts_get(core: State<'_, Arc<AppCore>>) -> Vec<ImportedCodexAccountView> {
    views(&core.imported_codex_accounts())
}

/// 导入一个副账号。返回更新后的完整列表，前端不需要自己拼增量。
#[tauri::command]
pub async fn codex_accounts_import(
    core: State<'_, Arc<AppCore>>,
    payload: String,
    alias: Option<String>,
) -> Result<Vec<ImportedCodexAccountView>, CommandError> {
    let Some(input) = classify(&payload) else {
        return Err(CommandError::INVALID_ARGUMENT);
    };

    let (account, credentials_payload) = match input {
        ImportInput::AuthJson(raw) => match codex_credentials::parse_imported(&raw, None) {
            Discovery::Found(credentials) => {
                let account_id = credentials
                    .account_id
                    .as_ref()
                    .map(Secret::expose)
                    .map(str::to_owned);
                let Some(account_id) = account_id else {
                    // 本地解不出 account id 就没法给这个账号一个稳定身份，
                    // 之后的缓存与历史都会挂在一个假的键上。
                    return Err(CommandError::CODEX_ACCOUNT_UNIDENTIFIED);
                };
                let id = match credentials.user_id.as_ref().map(Secret::expose) {
                    Some(user_id) => format!("{account_id}:{user_id}"),
                    None => account_id,
                };
                let account = ImportedCodexAccount {
                    id,
                    alias: alias.unwrap_or_default().trim().to_owned(),
                    email: credentials
                        .email
                        .as_ref()
                        .map(Secret::expose)
                        .map(str::to_owned),
                    plan: credentials.plan.clone(),
                    visible: true,
                    personal_access_token: false,
                    added_at: Utc::now().to_rfc3339(),
                };
                // 归一化后再存：只保留我们能续期的那几个字段，
                // 不把粘贴内容里的其他字段（例如别的工具的键）一起存进秘密存储。
                let normalized = serde_json::to_string(&serde_json::json!({
                    "tokens": {
                        "access_token": credentials.access_token.expose(),
                        "refresh_token": credentials
                            .refresh_token
                            .as_ref()
                            .map(Secret::expose),
                        "account_id": account.chatgpt_account_id(),
                    }
                }))
                .map_err(|_| CommandError::INVALID_ARGUMENT)?;
                (account, normalized)
            }
            Discovery::Unreadable => return Err(CommandError::CODEX_ACCOUNT_UNREADABLE),
            _ => return Err(CommandError::CODEX_ACCOUNT_INVALID),
        },
        ImportInput::PersonalAccessToken(token) => {
            let secret = Secret::new(token);
            let parsed = codex::fetch_usage_with_token(&secret, None)
                .await
                .map_err(CommandError::from_fetch_outcome)?;
            let Some(account_id) = parsed.account_id else {
                return Err(CommandError::CODEX_ACCOUNT_UNIDENTIFIED);
            };
            let id = match parsed.user_id {
                Some(user_id) => format!("{account_id}:{user_id}"),
                None => account_id,
            };
            let account = ImportedCodexAccount {
                id,
                alias: alias.unwrap_or_default().trim().to_owned(),
                email: parsed.email,
                plan: parsed
                    .identity
                    .and_then(|identity| identity.plan)
                    .or_else(|| Some("Personal access token".to_owned())),
                visible: true,
                personal_access_token: true,
                added_at: Utc::now().to_rfc3339(),
            };
            let normalized = serde_json::to_string(&serde_json::json!({
                "personal_access_token": secret.expose(),
            }))
            .map_err(|_| CommandError::INVALID_ARGUMENT)?;
            (account, normalized)
        }
    };

    let mut accounts = core.imported_codex_accounts();
    // 同一账号重复导入按更新处理：凭据换成新的，别名保留用户已有取值。
    let identity = identity_hash(&account.id);
    let position = accounts
        .iter()
        .position(|existing| existing.identity_hash() == identity);
    match position {
        Some(index) => {
            let existing_alias = accounts[index].alias.clone();
            let mut updated = account;
            if updated.alias.is_empty() {
                updated.alias = existing_alias;
            }
            accounts[index] = updated;
        }
        None => accounts.push(account),
    }

    let slot_owner = identity_hash(
        accounts
            .get(position.unwrap_or(accounts.len() - 1))
            .map(|value| value.id.as_str())
            .unwrap_or_default(),
    );
    if !core
        .imported_codex_store()
        .save_credentials(&slot_owner, &credentials_payload)
        .is_ok()
    {
        return Err(CommandError::CODEX_ACCOUNT_STORE_FAILED);
    }
    core.replace_imported_codex_accounts(&accounts)
        .map_err(|_| CommandError::CODEX_ACCOUNT_STORE_FAILED)?;
    Ok(views(&core.imported_codex_accounts()))
}

#[tauri::command]
pub fn codex_accounts_update(
    core: State<'_, Arc<AppCore>>,
    id: String,
    alias: Option<String>,
    visible: Option<bool>,
) -> Result<Vec<ImportedCodexAccountView>, CommandError> {
    let mut accounts = core.imported_codex_accounts();
    let Some(account) = accounts.iter_mut().find(|account| account.id == id) else {
        return Err(CommandError::NOT_FOUND);
    };
    if let Some(alias) = alias {
        account.alias = alias.trim().to_owned();
    }
    if let Some(visible) = visible {
        account.visible = visible;
    }
    core.replace_imported_codex_accounts(&accounts)
        .map_err(|_| CommandError::CODEX_ACCOUNT_STORE_FAILED)?;
    Ok(views(&core.imported_codex_accounts()))
}

/// 删除账号：先删凭据，再删元数据。
///
/// 顺序是有意的：元数据删了但凭据还在，会留下一个用户看不见、系统凭据界面里躺着
/// 的 token；反过来只是留下一条没有凭据的记录，界面上会显示成未登录。
#[tauri::command]
pub fn codex_accounts_remove(
    core: State<'_, Arc<AppCore>>,
    id: String,
) -> Result<Vec<ImportedCodexAccountView>, CommandError> {
    let mut accounts = core.imported_codex_accounts();
    let Some(position) = accounts.iter().position(|account| account.id == id) else {
        return Err(CommandError::NOT_FOUND);
    };
    let removed = accounts.remove(position);
    core.imported_codex_store()
        .remove_credentials(&removed.identity_hash());
    core.replace_imported_codex_accounts(&accounts)
        .map_err(|_| CommandError::CODEX_ACCOUNT_STORE_FAILED)?;
    Ok(views(&core.imported_codex_accounts()))
}

/// 重排账号：输入是完整的 id 顺序，缺失或多余的 id 一律拒绝，
/// 避免前端传半份列表时把账号顺序改成一个谁也不认识的样子。
#[tauri::command]
pub fn codex_accounts_reorder(
    core: State<'_, Arc<AppCore>>,
    ids: Vec<String>,
) -> Result<Vec<ImportedCodexAccountView>, CommandError> {
    let accounts = core.imported_codex_accounts();
    if ids.len() != accounts.len() {
        return Err(CommandError::INVALID_ARGUMENT);
    }
    let mut reordered = Vec::with_capacity(accounts.len());
    for id in &ids {
        let Some(account) = accounts.iter().find(|account| &account.id == id) else {
            return Err(CommandError::INVALID_ARGUMENT);
        };
        reordered.push(account.clone());
    }
    core.replace_imported_codex_accounts(&reordered)
        .map_err(|_| CommandError::CODEX_ACCOUNT_STORE_FAILED)?;
    Ok(views(&core.imported_codex_accounts()))
}
