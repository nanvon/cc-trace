//! Claude Code 凭据发现与 token 回写。
//!
//! 发现顺序（见 `docs/额度领域模型.md` 第 5 节）：
//!
//! 1. `~/.claude/.credentials.json`；
//! 2. macOS 上文件缺失、为空或无法解析时，读取系统钥匙串，见
//!    [ADR-0013](../../../../docs/决策/ADR-0013-macOS读取ClaudeCode钥匙串凭据.md)；
//! 3. 显示用邮箱可由 `~/.claude.json` 的 `oauthAccount.emailAddress` 兜底。
//!
//! Windows 只有第 1 步，两个平台的来源因此不一致，这是 ADR-0013 明确接受的代价。
//!
//! 刷新结果回写**读到它的那个来源**：refresh token 会轮换，不回写会作废 Claude Code
//! CLI 手里的那份，见 [ADR-0014](../../../../docs/决策/ADR-0014-token刷新结果回写外部凭据.md)。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};

use super::{Discovery, Secret, home_dir, non_empty, replace_json_atomically};
use crate::platform::keychain::{self, KeychainRead};

/// 凭据实际来自哪里。回写必须落回同一个来源，不能换地方。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeSource {
    File,
    Keychain,
}

/// 一份可用的 Claude Code OAuth 凭据。
#[derive(Clone, PartialEq, Eq)]
pub struct ClaudeCredentials {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub email: Option<Secret>,
    /// 订阅类型不是秘密，可直接进入脱敏身份。
    pub subscription: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub source: ClaudeSource,
    /// 只在凭据来自 macOS 钥匙串时存在，用于把刷新结果写回刚才读到的精确条目。
    pub keychain_account: Option<Secret>,
}

/// 手动实现：只暴露「有没有」与来源，不暴露任何取值。
impl fmt::Debug for ClaudeCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ClaudeCredentials")
            .field("source", &self.source)
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("has_email", &self.email.is_some())
            .field("subscription", &self.subscription)
            .field("expires_at", &self.expires_at)
            .field("has_keychain_account", &self.keychain_account.is_some())
            .finish()
    }
}

/// 刷新成功后要回写的内容。
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshedTokens {
    pub access_token: Secret,
    pub refresh_token: Secret,
    pub expires_at: DateTime<Utc>,
}

impl fmt::Debug for RefreshedTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RefreshedTokens(<redacted>)")
    }
}

pub fn credentials_path() -> Option<PathBuf> {
    Some(home_dir()?.join(".claude").join(".credentials.json"))
}

fn config_path() -> Option<PathBuf> {
    Some(home_dir()?.join(".claude.json"))
}

/// 只读发现本机 Claude Code 凭据。
pub fn discover() -> Discovery<ClaudeCredentials> {
    let file = read_file();
    let discovery = match file {
        // 文件里有可用凭据就用它，不去打扰钥匙串（避免多余的授权弹窗）。
        Discovery::Found(credentials) => Discovery::Found(credentials),
        // 文件缺失、为空或无法解析时才回退钥匙串：这正是 ADR-0013 要覆盖的情形。
        Discovery::Missing | Discovery::Unsupported => read_keychain(),
        // 文件存在却读不出来（权限）不能悄悄换来源，否则用户永远不知道文件有问题。
        Discovery::Unreadable => Discovery::Unreadable,
    };

    match discovery {
        Discovery::Found(credentials) if credentials.email.is_none() => {
            Discovery::Found(ClaudeCredentials {
                email: read_email_fallback().map(Secret::new),
                ..credentials
            })
        }
        other => other,
    }
}

fn read_file() -> Discovery<ClaudeCredentials> {
    let Some(path) = credentials_path() else {
        return Discovery::Missing;
    };

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Discovery::Missing,
        Err(_) => return Discovery::Unreadable,
    };

    parse(&raw, ClaudeSource::File)
}

fn read_keychain() -> Discovery<ClaudeCredentials> {
    match keychain::read_claude_credentials() {
        KeychainRead::Found { payload, account } => {
            match parse(payload.expose(), ClaudeSource::Keychain) {
                Discovery::Found(credentials) => Discovery::Found(ClaudeCredentials {
                    keychain_account: Some(account),
                    ..credentials
                }),
                other => other,
            }
        }
        KeychainRead::Missing => Discovery::Missing,
        KeychainRead::Unreadable => Discovery::Unreadable,
    }
}

/// `~/.claude.json` 只用来补一个显示用邮箱，读不到就算了，不影响凭据可用性。
fn read_email_fallback() -> Option<String> {
    let raw = fs::read_to_string(config_path()?).ok()?;
    let root: serde_json::Value = serde_json::from_str(&raw).ok()?;
    non_empty(
        root.get("oauthAccount")?
            .get("emailAddress")
            .and_then(serde_json::Value::as_str),
    )
}

/// 把已读到的 payload 解析成凭据。文件与钥匙串共用同一套结构。
pub fn parse(raw: &str, source: ClaudeSource) -> Discovery<ClaudeCredentials> {
    if raw.trim().is_empty() {
        return Discovery::Missing;
    }

    let Ok(root) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Discovery::Unsupported;
    };
    let Some(oauth) = root.get("claudeAiOauth") else {
        return Discovery::Unsupported;
    };

    // 两种命名都出现过，取到哪个用哪个，不猜结构。
    let access_token = non_empty(field(oauth, "accessToken", "access_token"));
    let Some(access_token) = access_token else {
        return Discovery::Missing;
    };

    Discovery::Found(ClaudeCredentials {
        access_token: Secret::new(access_token),
        refresh_token: non_empty(field(oauth, "refreshToken", "refresh_token")).map(Secret::new),
        email: non_empty(field(oauth, "emailAddress", "email")).map(Secret::new),
        subscription: non_empty(
            oauth
                .get("subscriptionType")
                .and_then(serde_json::Value::as_str),
        ),
        expires_at: parse_expires_at(oauth.get("expiresAt")),
        source,
        keychain_account: None,
    })
}

/// 把刷新结果原子回写读到它的那个来源。
pub fn write_back(expected: &ClaudeCredentials, tokens: &RefreshedTokens) -> io::Result<()> {
    match expected.source {
        ClaudeSource::File => {
            let path = credentials_path().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "claude credentials path is not resolvable",
                )
            })?;
            write_back_to_file(&path, expected, tokens)
        }
        ClaudeSource::Keychain => {
            let account = expected.keychain_account.as_ref().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "claude keychain account is unavailable for write back",
                )
            })?;
            let existing = match keychain::read_claude_credentials_for_account(account) {
                KeychainRead::Found { payload, .. } => payload,
                KeychainRead::Missing => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "claude keychain source disappeared before write back",
                    ));
                }
                KeychainRead::Unreadable => {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "claude keychain source is not readable before write back",
                    ));
                }
            };
            let updated =
                merged_payload(existing.expose(), expected, tokens, ClaudeSource::Keychain)?;
            let serialized = serde_json::to_string(&updated)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            keychain::write_claude_credentials(account, &existing, &Secret::new(serialized))
        }
    }
}

/// 回写到指定文件。与 [`write_back`] 分开，让测试不必碰真实的凭据文件。
pub fn write_back_to_file(
    path: &Path,
    expected: &ClaudeCredentials,
    tokens: &RefreshedTokens,
) -> io::Result<()> {
    let existing = fs::read_to_string(path)?;
    let updated = merged_payload(&existing, expected, tokens, ClaudeSource::File)?;
    replace_json_atomically(path, &updated)
}

/// 只替换 token 三件套，保留 payload 中其余字段（`subscriptionType`、`scopes` 等）。
fn merged_payload(
    existing: &str,
    expected: &ClaudeCredentials,
    tokens: &RefreshedTokens,
    source: ClaudeSource,
) -> io::Result<serde_json::Value> {
    let Discovery::Found(current) = parse(existing, source) else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "claude credential source no longer contains supported OAuth credentials",
        ));
    };
    if !same_token_version(&current, expected) {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "claude credential source changed during token refresh",
        ));
    }
    let mut root = serde_json::from_str::<serde_json::Value>(existing)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    let oauth = root
        .as_object_mut()
        .and_then(|object| object.get_mut("claudeAiOauth"))
        .and_then(serde_json::Value::as_object_mut)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "claude OAuth payload is no longer writable",
            )
        })?;

    let access_key = if oauth.contains_key("accessToken") {
        "accessToken"
    } else {
        "access_token"
    };
    let refresh_key = if oauth.contains_key("refreshToken") {
        "refreshToken"
    } else {
        "refresh_token"
    };
    oauth.insert(
        access_key.to_owned(),
        serde_json::Value::String(tokens.access_token.expose().to_owned()),
    );
    oauth.insert(
        refresh_key.to_owned(),
        serde_json::Value::String(tokens.refresh_token.expose().to_owned()),
    );
    // Claude Code 用毫秒时间戳，写回时必须保持同一单位，否则 CLI 会认为凭据早已过期。
    oauth.insert(
        "expiresAt".to_owned(),
        serde_json::Value::from(tokens.expires_at.timestamp_millis()),
    );

    Ok(root)
}

fn same_token_version(current: &ClaudeCredentials, expected: &ClaudeCredentials) -> bool {
    current.source == expected.source
        && current.access_token == expected.access_token
        && current.refresh_token == expected.refresh_token
}

fn field<'a>(oauth: &'a serde_json::Value, camel: &str, snake: &str) -> Option<&'a str> {
    oauth
        .get(camel)
        .and_then(serde_json::Value::as_str)
        .or_else(|| oauth.get(snake).and_then(serde_json::Value::as_str))
}

/// `expiresAt` 在实测中同时出现过数字与字符串、秒与毫秒，四种组合都要能读。
fn parse_expires_at(value: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    const MILLISECOND_THRESHOLD: f64 = 10_000_000_000.0;

    let raw = match value? {
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !raw.is_finite() {
        return None;
    }

    let milliseconds = if raw > MILLISECOND_THRESHOLD {
        raw
    } else {
        raw * 1_000.0
    };
    Utc.timestamp_millis_opt(milliseconds as i64).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file_fixture() -> &'static str {
        r#"{
            "claudeAiOauth": {
                "accessToken": "at_fixture",
                "refreshToken": "rt_fixture",
                "expiresAt": 1700000000000,
                "subscriptionType": "max",
                "scopes": ["user:inference"]
            }
        }"#
    }

    fn expected_credentials(raw: &str) -> ClaudeCredentials {
        let Discovery::Found(credentials) = parse(raw, ClaudeSource::File) else {
            panic!("fixture parses");
        };
        credentials
    }

    #[test]
    fn parses_tokens_subscription_and_a_millisecond_expiry() {
        let Discovery::Found(credentials) = parse(file_fixture(), ClaudeSource::File) else {
            panic!("the file fixture must be supported");
        };

        assert_eq!(credentials.access_token.expose(), "at_fixture");
        assert_eq!(
            credentials.refresh_token.as_ref().map(Secret::expose),
            Some("rt_fixture")
        );
        assert_eq!(credentials.subscription.as_deref(), Some("max"));
        assert_eq!(
            credentials.expires_at,
            DateTime::from_timestamp(1_700_000_000, 0)
        );
        assert_eq!(credentials.source, ClaudeSource::File);
    }

    #[test]
    fn expiry_accepts_seconds_milliseconds_and_strings() {
        let expected = DateTime::from_timestamp(1_700_000_000, 0);
        for raw in [
            r#"{"claudeAiOauth":{"accessToken":"a","expiresAt":1700000000}}"#,
            r#"{"claudeAiOauth":{"accessToken":"a","expiresAt":1700000000000}}"#,
            r#"{"claudeAiOauth":{"accessToken":"a","expiresAt":"1700000000000"}}"#,
        ] {
            let Discovery::Found(credentials) = parse(raw, ClaudeSource::File) else {
                panic!("{raw:?} must parse");
            };
            assert_eq!(credentials.expires_at, expected, "{raw:?}");
        }
    }

    #[test]
    fn snake_case_token_fields_are_accepted_too() {
        let raw = r#"{"claudeAiOauth":{"access_token":"a","refresh_token":"r","email":"u@example.test"}}"#;
        let Discovery::Found(credentials) = parse(raw, ClaudeSource::Keychain) else {
            panic!("snake_case payloads must parse");
        };
        assert_eq!(credentials.access_token.expose(), "a");
        assert_eq!(
            credentials.refresh_token.as_ref().map(Secret::expose),
            Some("r")
        );
        assert_eq!(
            credentials.email.as_ref().map(Secret::expose),
            Some("u@example.test")
        );
    }

    #[test]
    fn an_empty_payload_is_missing_and_an_unknown_shape_is_unsupported() {
        assert_eq!(parse("", ClaudeSource::File), Discovery::Missing);
        assert_eq!(parse("   ", ClaudeSource::File), Discovery::Missing);
        assert_eq!(
            parse(r#"{"claudeAiOauth":{}}"#, ClaudeSource::File),
            Discovery::Missing
        );
        assert_eq!(
            parse("{ not json", ClaudeSource::File),
            Discovery::Unsupported
        );
        assert_eq!(
            parse(r#"{"somethingElse":{}}"#, ClaudeSource::File),
            Discovery::Unsupported,
            "an unrecognised shape must not read as a missing login"
        );
    }

    #[test]
    fn write_back_keeps_unrelated_fields_and_writes_milliseconds() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".credentials.json");
        fs::write(&path, file_fixture()).expect("seed credentials");

        let expires_at = DateTime::from_timestamp(1_800_000_000, 0).expect("valid timestamp");
        let expected = expected_credentials(file_fixture());
        write_back_to_file(
            &path,
            &expected,
            &RefreshedTokens {
                access_token: Secret::new("new-access"),
                refresh_token: Secret::new("new-refresh"),
                expires_at,
            },
        )
        .expect("write back succeeds");

        let root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read back")).expect("json");
        let oauth = &root["claudeAiOauth"];

        assert_eq!(oauth["accessToken"], "new-access");
        assert_eq!(oauth["refreshToken"], "new-refresh");
        assert_eq!(oauth["expiresAt"], 1_800_000_000_000_i64);
        assert_eq!(
            oauth["subscriptionType"], "max",
            "the write back must not drop fields Claude Code owns"
        );
        assert!(oauth["scopes"].is_array());
    }

    #[test]
    fn a_written_back_payload_reads_back_as_the_same_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".credentials.json");
        fs::write(&path, file_fixture()).expect("seed credentials");

        let expires_at = DateTime::from_timestamp(1_800_000_000, 0).expect("valid timestamp");
        let expected = expected_credentials(file_fixture());
        write_back_to_file(
            &path,
            &expected,
            &RefreshedTokens {
                access_token: Secret::new("new-access"),
                refresh_token: Secret::new("new-refresh"),
                expires_at,
            },
        )
        .expect("write back succeeds");

        let raw = fs::read_to_string(&path).expect("read back");
        let Discovery::Found(credentials) = parse(&raw, ClaudeSource::File) else {
            panic!("our own write back must remain parseable");
        };
        assert_eq!(credentials.access_token.expose(), "new-access");
        assert_eq!(credentials.expires_at, Some(expires_at));
    }

    #[test]
    fn write_back_preserves_snake_case_token_field_names() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".credentials.json");
        let fixture = r#"{"claudeAiOauth":{"access_token":"old-a","refresh_token":"old-r"}}"#;
        fs::write(&path, fixture).expect("seed credentials");

        write_back_to_file(
            &path,
            &expected_credentials(fixture),
            &RefreshedTokens {
                access_token: Secret::new("new-a"),
                refresh_token: Secret::new("new-r"),
                expires_at: DateTime::from_timestamp(1_800_000_000, 0).expect("timestamp"),
            },
        )
        .expect("write back succeeds");

        let root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read back")).expect("json");
        let oauth = &root["claudeAiOauth"];
        assert_eq!(oauth["access_token"], "new-a");
        assert_eq!(oauth["refresh_token"], "new-r");
        assert!(oauth.get("accessToken").is_none());
        assert!(oauth.get("refreshToken").is_none());
    }

    #[test]
    fn write_back_refuses_to_replace_a_corrupt_or_missing_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".credentials.json");
        let tokens = RefreshedTokens {
            access_token: Secret::new("new-a"),
            refresh_token: Secret::new("new-r"),
            expires_at: DateTime::from_timestamp(1_800_000_000, 0).expect("timestamp"),
        };
        let expected = expected_credentials(file_fixture());

        fs::write(&path, "{ not json").expect("seed corrupt credentials");
        let corrupt = write_back_to_file(&path, &expected, &tokens)
            .expect_err("corrupt external credentials must fail closed");
        assert_eq!(corrupt.kind(), io::ErrorKind::InvalidData);
        assert_eq!(fs::read_to_string(&path).expect("read back"), "{ not json");

        fs::remove_file(&path).expect("remove fixture");
        let missing = write_back_to_file(&path, &expected, &tokens)
            .expect_err("a missing source must stay missing");
        assert_eq!(missing.kind(), io::ErrorKind::NotFound);
        assert!(!path.exists());
    }

    #[test]
    fn write_back_refuses_to_overwrite_a_newer_token_version() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".credentials.json");
        let expected = expected_credentials(file_fixture());
        let changed = r#"{"claudeAiOauth":{"accessToken":"newer-a","refreshToken":"newer-r"}}"#;
        fs::write(&path, changed).expect("seed changed credentials");

        let result = write_back_to_file(
            &path,
            &expected,
            &RefreshedTokens {
                access_token: Secret::new("ours-a"),
                refresh_token: Secret::new("ours-r"),
                expires_at: DateTime::from_timestamp(1_800_000_000, 0).expect("timestamp"),
            },
        );

        assert_eq!(
            result
                .expect_err("a newer CLI token version must win")
                .kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(fs::read_to_string(&path).expect("read back"), changed);
    }

    #[test]
    fn debug_output_reveals_presence_but_never_values() {
        let Discovery::Found(credentials) = parse(file_fixture(), ClaudeSource::Keychain) else {
            panic!("fixture parses");
        };

        let rendered = format!("{credentials:?}");
        assert!(rendered.contains("source: Keychain"));
        assert!(rendered.contains("has_refresh_token: true"));
        assert!(!rendered.contains("at_fixture"));
        assert!(!rendered.contains("rt_fixture"));
    }
}
