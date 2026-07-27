//! Codex 凭据发现与 token 回写。
//!
//! 来源：`CODEX_HOME/auth.json`，未设置时 `~/.codex/auth.json`。
//! 首版只支持 OAuth 凭据；只含 Personal Access Token 的文件判为
//! [`Discovery::Unsupported`]，见
//! [ADR-0006](../../../../docs/决策/ADR-0006-首版只支持Codex-OAuth凭据.md)。

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use super::{Discovery, Secret, home_dir, jwt, non_empty, replace_json_atomically};

/// OpenAI 在 access／id token 里放身份 claim 的命名空间。
const AUTH_CLAIM_NAMESPACE: &str = "https://api.openai.com/auth";

/// 一份可用的 Codex OAuth 凭据。
#[derive(Clone, PartialEq, Eq)]
pub struct CodexCredentials {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    /// 请求头 `ChatGPT-Account-Id`，同时参与身份变化判断。
    pub account_id: Option<Secret>,
    pub email: Option<Secret>,
    /// 计划名不是秘密，可直接进入脱敏身份。
    pub plan: Option<String>,
    /// 取自 access token 的 JWT `exp`；无法解出时为 `None`，此时不主动刷新。
    pub access_expires_at: Option<DateTime<Utc>>,
}

/// 手动实现：只暴露「有没有」，不暴露任何取值，见 `docs/日志与诊断.md` 第 5 节。
impl fmt::Debug for CodexCredentials {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodexCredentials")
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("has_account_id", &self.account_id.is_some())
            .field("has_email", &self.email.is_some())
            .field("plan", &self.plan)
            .field("access_expires_at", &self.access_expires_at)
            .finish()
    }
}

/// 刷新成功后要回写的三件套。`id_token` 缺失时保留文件中的原值。
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshedTokens {
    pub access_token: Secret,
    pub refresh_token: Secret,
    pub id_token: Option<Secret>,
}

impl fmt::Debug for RefreshedTokens {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("RefreshedTokens(<redacted>)")
    }
}

/// `auth.json` 的位置。`CODEX_HOME` 优先，与 Codex CLI 的解析顺序一致。
pub fn auth_path() -> Option<PathBuf> {
    if let Some(codex_home) = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
    {
        return Some(codex_home.join("auth.json"));
    }

    Some(home_dir()?.join(".codex").join("auth.json"))
}

/// 只读发现本机 Codex 凭据。
pub fn discover() -> Discovery<CodexCredentials> {
    let Some(path) = auth_path() else {
        return Discovery::Missing;
    };

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Discovery::Missing,
        Err(_) => return Discovery::Unreadable,
    };

    parse(&raw)
}

/// 把已读到的 `auth.json` 内容解析成凭据。与文件 IO 分开，便于用 Fixture 测试。
pub fn parse(raw: &str) -> Discovery<CodexCredentials> {
    let Ok(root) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Discovery::Unsupported;
    };

    let tokens = root.get("tokens");
    let access_token = tokens
        .and_then(|tokens| tokens.get("access_token"))
        .and_then(serde_json::Value::as_str);

    let Some(access_token) = non_empty(access_token) else {
        // 没有 OAuth access token 时，只含 PAT 的文件是「存在但不支持」，
        // 与「完全没有登录过」必须区分开。
        let has_pat = non_empty(
            root.get("personal_access_token")
                .and_then(serde_json::Value::as_str),
        )
        .is_some();
        return if has_pat {
            Discovery::Unsupported
        } else {
            Discovery::Missing
        };
    };

    let refresh_token = non_empty(
        tokens
            .and_then(|tokens| tokens.get("refresh_token"))
            .and_then(serde_json::Value::as_str),
    )
    .map(Secret::new);

    let id_claims = non_empty(
        tokens
            .and_then(|tokens| tokens.get("id_token"))
            .and_then(serde_json::Value::as_str),
    )
    .and_then(|token| jwt::decode_payload(&token));
    let access_claims = jwt::decode_payload(&access_token);

    let account_id = non_empty(
        tokens
            .and_then(|tokens| tokens.get("account_id"))
            .and_then(serde_json::Value::as_str),
    )
    .or_else(|| auth_claim("chatgpt_account_id", access_claims.as_ref()))
    .or_else(|| auth_claim("chatgpt_account_id", id_claims.as_ref()))
    .map(Secret::new);

    let email = id_claims
        .as_ref()
        .and_then(|claims| non_empty(claims.get("email").and_then(serde_json::Value::as_str)))
        .map(Secret::new);

    let plan = auth_claim("chatgpt_plan_type", id_claims.as_ref())
        .or_else(|| {
            id_claims.as_ref().and_then(|claims| {
                non_empty(
                    claims
                        .get("chatgpt_plan_type")
                        .and_then(serde_json::Value::as_str),
                )
            })
        })
        .or_else(|| auth_claim("chatgpt_plan_type", access_claims.as_ref()));

    Discovery::Found(CodexCredentials {
        access_expires_at: jwt::expires_at(&access_token),
        access_token: Secret::new(access_token),
        refresh_token,
        account_id,
        email,
        plan,
    })
}

/// 把刷新结果原子回写 `auth.json`，只改 token 三件套与 `last_refresh`。
///
/// 回写是 refresh token 轮换语义的必然要求，边界见 ADR-0014。写入失败时原文件不变，
/// 调用方必须把本次刷新按凭据类 `error` 处理。
pub fn write_back(tokens: &RefreshedTokens, now: DateTime<Utc>) -> io::Result<()> {
    let path = auth_path().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "codex auth path is not resolvable")
    })?;

    write_back_to(&path, tokens, now)
}

/// 回写到指定路径。与 [`write_back`] 分开，让测试不必碰真实的 `~/.codex/auth.json`。
pub fn write_back_to(path: &Path, tokens: &RefreshedTokens, now: DateTime<Utc>) -> io::Result<()> {
    let mut root = fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));

    let object = root.as_object_mut().expect("root is an object");
    let entry = object
        .entry("tokens")
        .or_insert_with(|| serde_json::json!({}));
    if !entry.is_object() {
        *entry = serde_json::json!({});
    }
    let token_object = entry.as_object_mut().expect("tokens is an object");

    token_object.insert(
        "access_token".to_owned(),
        serde_json::Value::String(tokens.access_token.expose().to_owned()),
    );
    token_object.insert(
        "refresh_token".to_owned(),
        serde_json::Value::String(tokens.refresh_token.expose().to_owned()),
    );
    if let Some(id_token) = tokens.id_token.as_ref() {
        token_object.insert(
            "id_token".to_owned(),
            serde_json::Value::String(id_token.expose().to_owned()),
        );
    }

    object.insert(
        "last_refresh".to_owned(),
        serde_json::Value::String(now.to_rfc3339()),
    );

    replace_json_atomically(path, &root)
}

fn auth_claim(key: &str, claims: Option<&serde_json::Value>) -> Option<String> {
    non_empty(
        claims?
            .get(AUTH_CLAIM_NAMESPACE)?
            .get(key)
            .and_then(serde_json::Value::as_str),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// payload：`{"exp":1700000000,"email":"user@example.test",
    /// "https://api.openai.com/auth":{"chatgpt_account_id":"acct_fixture_0001",
    /// "chatgpt_plan_type":"plus"}}`。内容全部虚构。
    const FAKE_ID_TOKEN: &str = concat!(
        "eyJhbGciOiJSUzI1NiJ9",
        ".",
        "eyJleHAiOjE3MDAwMDAwMDAsImVtYWlsIjoidXNlckBleGFtcGxlLnRlc3QiLCJodHRwczovL2FwaS",
        "5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF9maXh0dXJlXzAwMDEi",
        "LCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0",
        ".",
        "c2ln"
    );
    /// payload：`{"exp":1700000000}`。
    const FAKE_ACCESS_TOKEN: &str = "eyJhbGciOiJSUzI1NiJ9.eyJleHAiOjE3MDAwMDAwMDB9.c2ln";

    fn oauth_fixture() -> String {
        format!(
            r#"{{"OPENAI_API_KEY":null,"tokens":{{"id_token":"{FAKE_ID_TOKEN}","access_token":"{FAKE_ACCESS_TOKEN}","refresh_token":"rt_fixture"}},"last_refresh":"2026-07-01T00:00:00Z"}}"#
        )
    }

    #[test]
    fn oauth_credentials_carry_identity_plan_and_expiry() {
        let Discovery::Found(credentials) = parse(&oauth_fixture()) else {
            panic!("the OAuth fixture must be supported");
        };

        assert_eq!(credentials.access_token.expose(), FAKE_ACCESS_TOKEN);
        assert_eq!(
            credentials.refresh_token.as_ref().map(Secret::expose),
            Some("rt_fixture")
        );
        assert_eq!(
            credentials.account_id.as_ref().map(Secret::expose),
            Some("acct_fixture_0001")
        );
        assert_eq!(
            credentials.email.as_ref().map(Secret::expose),
            Some("user@example.test")
        );
        assert_eq!(credentials.plan.as_deref(), Some("plus"));
        assert_eq!(
            credentials.access_expires_at,
            DateTime::from_timestamp(1_700_000_000, 0)
        );
    }

    #[test]
    fn a_personal_access_token_is_unsupported_not_missing() {
        let raw = r#"{"OPENAI_API_KEY":null,"personal_access_token":"at-fixture"}"#;
        assert_eq!(parse(raw), Discovery::Unsupported);
    }

    #[test]
    fn an_empty_or_tokenless_file_reads_as_missing_credentials() {
        for raw in [
            "{}",
            r#"{"tokens":{}}"#,
            r#"{"tokens":{"access_token":"   "}}"#,
            r#"{"tokens":{"access_token":""},"personal_access_token":""}"#,
        ] {
            assert_eq!(parse(raw), Discovery::Missing, "{raw:?}");
        }
    }

    #[test]
    fn an_unparseable_file_is_unsupported_rather_than_a_silent_missing() {
        assert_eq!(parse("{ not json"), Discovery::Unsupported);
    }

    #[test]
    fn account_id_falls_back_to_the_token_claim_when_the_field_is_absent() {
        let Discovery::Found(credentials) = parse(&oauth_fixture()) else {
            panic!("fixture parses");
        };
        assert_eq!(
            credentials.account_id.as_ref().map(Secret::expose),
            Some("acct_fixture_0001"),
            "the claim must fill in for a missing tokens.account_id"
        );
    }

    #[test]
    fn an_opaque_access_token_yields_no_expiry_so_we_do_not_refresh_blindly() {
        let raw = r#"{"tokens":{"access_token":"opaque-not-a-jwt","refresh_token":"rt"}}"#;
        let Discovery::Found(credentials) = parse(raw) else {
            panic!("an opaque OAuth token is still usable");
        };
        assert_eq!(credentials.access_expires_at, None);
    }

    #[test]
    fn write_back_only_replaces_tokens_and_keeps_the_other_fields() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("auth.json");
        fs::write(&path, oauth_fixture()).expect("seed auth.json");

        let now = DateTime::from_timestamp(1_800_000_000, 0).expect("valid timestamp");
        write_back_to(
            &path,
            &RefreshedTokens {
                access_token: Secret::new("new-access"),
                refresh_token: Secret::new("new-refresh"),
                id_token: None,
            },
            now,
        )
        .expect("write back succeeds");

        let root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read back")).expect("json");

        assert_eq!(root["tokens"]["access_token"], "new-access");
        assert_eq!(root["tokens"]["refresh_token"], "new-refresh");
        assert_eq!(
            root["tokens"]["id_token"], FAKE_ID_TOKEN,
            "an absent id_token must not erase the stored one"
        );
        assert_eq!(root["last_refresh"], now.to_rfc3339());
        assert!(
            root.get("OPENAI_API_KEY").is_some(),
            "unrelated fields must survive the write back"
        );
    }

    #[test]
    fn write_back_recovers_from_a_corrupt_auth_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("auth.json");
        fs::write(&path, "{ not json").expect("seed a broken file");

        write_back_to(
            &path,
            &RefreshedTokens {
                access_token: Secret::new("new-access"),
                refresh_token: Secret::new("new-refresh"),
                id_token: Some(Secret::new("new-id")),
            },
            Utc::now(),
        )
        .expect("write back succeeds");

        let root: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).expect("read back")).expect("json");
        assert_eq!(root["tokens"]["id_token"], "new-id");
    }

    #[test]
    fn debug_output_reveals_presence_but_never_values() {
        let Discovery::Found(credentials) = parse(&oauth_fixture()) else {
            panic!("fixture parses");
        };

        let rendered = format!("{credentials:?}");
        assert!(rendered.contains("has_refresh_token: true"));
        assert!(!rendered.contains("rt_fixture"));
        assert!(!rendered.contains("acct_fixture_0001"));
        assert!(!rendered.contains("user@example.test"));
        assert!(!rendered.contains(FAKE_ACCESS_TOKEN));
    }
}
