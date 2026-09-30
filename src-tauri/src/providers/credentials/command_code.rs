//! Command Code 凭据发现。
//!
//! 与 Codex／Claude 的「一个官方 CLI，一个文件」不同，Command Code 的令牌可能散落在
//! 四个地方，优先顺序与 cc-bar v1.1.1 一致（先 CLI，再 Pi，再 OpenCode，再环境变量），
//! 最后才回落到用户手动填写的 API Key。
//!
//! 手动 API Key 存在 CC Trace 自己的系统秘密存储里（macOS 钥匙串 / Windows 凭据管理器），
//! 见 [`crate::platform::secret_store`] 与 [ADR-0035]。
//!
//! 路径全部按家目录拼接，Windows 用 `%USERPROFILE%`：这三条路径都是各 CLI 自己定义的
//! 家目录相对位置，不随平台变化；OpenCode 的库路径与本地扫描共用同一份约定
//! （见 `crate::usage::ScanRoots`），不在这里再写一遍。
//!
//! [ADR-0035]: ../../../../docs/决策/ADR-0035-服务矩阵与Windows凭据存储.md

use serde_json::Value;

use crate::contracts::CommandCodeCredentialPreference;
use crate::platform::secret_store::{self, COMMAND_CODE_API_KEY_SLOT, SecretRead};

use super::{Discovery, Secret, home_dir, non_empty};

/// 令牌从哪来。界面用它说明「当前用的是哪个来源」，也是排查登录问题的第一手信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandCodeSource {
    /// `~/.commandcode/auth.json`（Command Code CLI 自己的登录态）。
    Cli,
    /// `~/.pi/agent/auth.json` 的 `commandcode` 段。
    Pi,
    /// `~/.local/share/opencode/auth.json` 的 `command-code` 段。
    OpenCode,
    /// 环境变量 `COMMAND_CODE_API_KEY` / `COMMANDCODE_API_KEY`。
    Environment,
    /// 用户在设置里手动填写、存在系统秘密存储里的 API Key。
    Manual,
}

impl CommandCodeSource {
    pub fn key(self) -> &'static str {
        match self {
            Self::Cli => "commandcode",
            Self::Pi => "pi",
            Self::OpenCode => "opencode",
            Self::Environment => "env",
            Self::Manual => "keychain",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct CommandCodeCredentials {
    pub access_token: Secret,
    pub source: CommandCodeSource,
}

/// 手动实现：只暴露来源，不暴露令牌，见 `docs/日志与诊断.md` 第 5 节。
impl std::fmt::Debug for CommandCodeCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CommandCodeCredentials")
            .field("source", &self.source.key())
            .finish()
    }
}

/// 令牌清洗：去掉首尾空白、拒绝明显不是令牌的输入。
///
/// 规则与 cc-bar 一致：长度至少 10、全部是可打印 ASCII。粘贴时混进换行或中文标点是
/// 最常见的输入错误，与其带着脏令牌发请求吃一个 401，不如在这里判成「没有凭据」。
pub(crate) fn sanitize_token(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.len() < 10 {
        return None;
    }
    if !trimmed
        .chars()
        .all(|character| character.is_ascii() && !character.is_ascii_control())
    {
        return None;
    }
    Some(trimmed.to_owned())
}

pub fn discover(preference: CommandCodeCredentialPreference) -> Discovery<CommandCodeCredentials> {
    if preference == CommandCodeCredentialPreference::Manual {
        return match manual_key() {
            ManualKey::Found(token) => Discovery::Found(CommandCodeCredentials {
                access_token: token,
                source: CommandCodeSource::Manual,
            }),
            ManualKey::Missing => Discovery::Missing,
            ManualKey::Unreadable => Discovery::Unreadable,
        };
    }

    let home = home_dir();
    if let Some(home) = home.as_ref() {
        if let Some(token) = read_flat_token(&home.join(".commandcode/auth.json")) {
            return Discovery::Found(CommandCodeCredentials {
                access_token: token,
                source: CommandCodeSource::Cli,
            });
        }
        if let Some(token) = read_provider_token(
            &home.join(".pi/agent/auth.json"),
            &["commandcode", "command-code"],
        ) {
            return Discovery::Found(CommandCodeCredentials {
                access_token: token,
                source: CommandCodeSource::Pi,
            });
        }
        if let Some(token) = read_provider_token(
            &home.join(".local/share/opencode/auth.json"),
            &["command-code", "commandcode"],
        ) {
            return Discovery::Found(CommandCodeCredentials {
                access_token: token,
                source: CommandCodeSource::OpenCode,
            });
        }
    }

    if let Some(token) = environment_token() {
        return Discovery::Found(CommandCodeCredentials {
            access_token: token,
            source: CommandCodeSource::Environment,
        });
    }

    // 自动模式下的最后一档：用户手动填过 API Key 就直接用。
    match manual_key() {
        ManualKey::Found(token) => Discovery::Found(CommandCodeCredentials {
            access_token: token,
            source: CommandCodeSource::Manual,
        }),
        // 前面几档都不存在、手动 Key 也没有：这是「没有凭据」，
        // 不是「读不出来」，所以在这里把 Unreadable 降级成 Missing 之外的判断要保留。
        ManualKey::Missing => Discovery::Missing,
        ManualKey::Unreadable => Discovery::Unreadable,
    }
}

enum ManualKey {
    Found(Secret),
    Missing,
    Unreadable,
}

fn manual_key() -> ManualKey {
    match secret_store::read(COMMAND_CODE_API_KEY_SLOT) {
        SecretRead::Found(secret) => match sanitize_token(Some(secret.expose())) {
            Some(token) => ManualKey::Found(Secret::new(token)),
            None => ManualKey::Missing,
        },
        SecretRead::Missing => ManualKey::Missing,
        SecretRead::Denied | SecretRead::Failed => ManualKey::Unreadable,
    }
}

/// 保存手动填写的 API Key。空串等于删除。
pub fn save_manual_key(api_key: &str) -> SecretWriteOutcome {
    match sanitize_token(Some(api_key)) {
        Some(_) => {
            match secret_store::write(COMMAND_CODE_API_KEY_SLOT, &Secret::new(api_key.trim())) {
                secret_store::SecretWrite::Ok => SecretWriteOutcome::Ok,
                secret_store::SecretWrite::Denied => SecretWriteOutcome::Denied,
                secret_store::SecretWrite::Failed => SecretWriteOutcome::Failed,
            }
        }
        None => SecretWriteOutcome::Invalid,
    }
}

pub fn clear_manual_key() -> SecretWriteOutcome {
    match secret_store::delete(COMMAND_CODE_API_KEY_SLOT) {
        secret_store::SecretWrite::Ok => SecretWriteOutcome::Ok,
        secret_store::SecretWrite::Denied => SecretWriteOutcome::Denied,
        secret_store::SecretWrite::Failed => SecretWriteOutcome::Failed,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretWriteOutcome {
    Ok,
    Invalid,
    Denied,
    Failed,
}

fn environment_token() -> Option<Secret> {
    for name in ["COMMAND_CODE_API_KEY", "COMMANDCODE_API_KEY"] {
        if let Ok(value) = std::env::var(name)
            && let Some(token) = sanitize_token(Some(&value))
        {
            return Some(Secret::new(token));
        }
    }
    None
}

/// `~/.commandcode/auth.json`：令牌在顶层，字段名有几种历史写法。
fn read_flat_token(path: &std::path::Path) -> Option<Secret> {
    let root = read_json(path)?;
    flat_token(&root)
}

/// Pi／OpenCode 的 `auth.json`：令牌嵌在某个 provider 段里，也可能是裸字符串。
fn read_provider_token(path: &std::path::Path, providers: &[&str]) -> Option<Secret> {
    let root = read_json(path)?;
    for provider in providers {
        if let Some(section) = root.get(*provider) {
            if let Some(token) = flat_token(section) {
                return Some(token);
            }
            if let Some(token) = sanitize_token(section.as_str()).map(Secret::new) {
                return Some(token);
            }
        }
    }
    None
}

fn flat_token(value: &Value) -> Option<Secret> {
    for field in [
        "access",
        "access_token",
        "apiKey",
        "api_key",
        "key",
        "token",
    ] {
        if let Some(token) = sanitize_token(value.get(field).and_then(Value::as_str)) {
            return Some(Secret::new(token));
        }
    }
    None
}

fn read_json(path: &std::path::Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// 账号身份键：用于额度历史序列与缓存的身份变化判断。
///
/// 与 cc-bar 的 `accountKey` 同构：org 优先，其次登录名，其次邮箱；三者都没有时用
/// 令牌前后缀——那种情况下它只是「大致能区分」的键，不承诺稳定。
pub fn account_key(
    org_id: Option<&str>,
    login: Option<&str>,
    email: Option<&str>,
    access_token: &Secret,
) -> String {
    if let Some(org_id) = non_empty(org_id) {
        let owner = non_empty(login)
            .or_else(|| non_empty(email))
            .unwrap_or_else(|| "user".to_owned());
        return format!("org:{org_id}:{owner}");
    }
    if let Some(login) = non_empty(login) {
        return format!("user:{login}");
    }
    if let Some(email) = non_empty(email) {
        return format!("email:{email}");
    }
    let token = access_token.expose();
    let prefix: String = token.chars().take(8).collect();
    let suffix: String = token
        .chars()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("token:{prefix}...{suffix}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_rejects_obviously_broken_input() {
        assert_eq!(
            sanitize_token(Some("  abcdefghij  ")).as_deref(),
            Some("abcdefghij")
        );
        assert!(sanitize_token(Some("short")).is_none());
        // 规则与 cc-bar 一致：只接受 0x20～0x7E 的可打印 ASCII。
        // 制表符、换行、中文都在拒绝范围内；中间的空格不在拒绝范围内（服务端会拒）。
        assert!(sanitize_token(Some("abcdefghij\tk")).is_none());
        assert!(sanitize_token(Some("abcdefghij\nk")).is_none());
        assert!(sanitize_token(Some("包含中文的令牌九个数")).is_none());
        assert!(sanitize_token(None).is_none());
    }

    #[test]
    fn tokens_are_read_from_every_supported_field_name() {
        for field in [
            "access",
            "access_token",
            "apiKey",
            "api_key",
            "key",
            "token",
        ] {
            let value = serde_json::json!({ field: "abcdefghijklmnop" });
            assert!(flat_token(&value).is_some(), "{field} must be recognized");
        }
        let value = serde_json::json!({ "access": "short" });
        assert!(flat_token(&value).is_none());
    }

    #[test]
    fn source_keys_match_the_wire_values() {
        assert_eq!(CommandCodeSource::Cli.key(), "commandcode");
        assert_eq!(CommandCodeSource::Pi.key(), "pi");
        assert_eq!(CommandCodeSource::OpenCode.key(), "opencode");
        assert_eq!(CommandCodeSource::Environment.key(), "env");
        assert_eq!(CommandCodeSource::Manual.key(), "keychain");
    }

    #[test]
    fn the_account_key_prefers_org_then_login_then_email() {
        let token = Secret::new("abcdefghijklmnop");
        assert_eq!(
            account_key(Some("org-1"), Some("alice"), Some("a@example.com"), &token),
            "org:org-1:alice"
        );
        assert_eq!(
            account_key(None, Some("alice"), Some("a@example.com"), &token),
            "user:alice"
        );
        assert_eq!(
            account_key(None, None, Some("a@example.com"), &token),
            "email:a@example.com"
        );
        assert_eq!(
            account_key(None, None, None, &token),
            "token:abcdefgh...klmnop"
        );
    }

    #[test]
    fn a_credential_never_prints_its_token() {
        let credentials = CommandCodeCredentials {
            access_token: Secret::new("abcdefghijklmnop"),
            source: CommandCodeSource::Cli,
        };
        assert!(!format!("{credentials:?}").contains("abcdefghijklmnop"));
    }
}
