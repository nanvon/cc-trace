//! Codex 额度来源：凭据发现、token 续期、Usage 请求与响应标准化。
//!
//! 协议见 `docs/额度领域模型.md` 第 3.1 节，凭据来源见第 5 节。响应解析
//! ([`parse_usage_response`]) 与网络分开，因此 Fixture 测试不需要出网。
//!
//! 秘密只在本模块与 [`credentials`](super::credentials) 之间流动：请求头由这里拼装，
//! 返回值只有脱敏 contract。

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use tokio::sync::Mutex;

use super::credentials::codex::{CodexCredentials, RefreshedTokens};
use super::credentials::{self, Discovery, Secret};
use super::{BoxFuture, ProviderFetchOutcome, QuotaProvider, http};
use crate::contracts::{
    ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot, QuotaWindow, QuotaWindowKind,
};
use crate::scheduler::params::CODEX_TOKEN_REFRESH_SKEW_SECS;

const USAGE_ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/usage";
const TOKEN_ENDPOINT: &str = "https://auth.openai.com/oauth/token";
/// Codex CLI 的公开 OAuth client id。它不是秘密：任何 Codex CLI 安装包里都有同一份。
const OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";

const FIVE_HOUR_SECONDS: u64 = 5 * 60 * 60;
const WEEKLY_SECONDS: u64 = 7 * 24 * 60 * 60;

/// 解析成功后的脱敏结果。这里只保留计划名与标准化额度，不携带响应身份原文。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCodexUsage {
    pub identity: Option<ProviderIdentity>,
    pub snapshot: QuotaSnapshot,
}

/// 不包含 serde 原始错误、响应片段或身份字段，避免未来被直接写进日志时泄露响应内容。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodexUsageParseError {
    InvalidResponse,
    MissingRateLimit,
    MissingPrimaryWindow,
    MissingUsedPercent,
    InvalidWindowSeconds,
    InvalidResetTime,
}

#[derive(Deserialize)]
struct UsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<RateLimit>,
}

#[derive(Deserialize)]
struct RateLimit {
    primary_window: Option<RawWindow>,
    #[serde(default)]
    secondary_window: Option<RawWindow>,
}

#[derive(Deserialize)]
struct RawWindow {
    used_percent: Option<f64>,
    reset_at: Option<f64>,
    reset_after_seconds: Option<f64>,
    limit_window_seconds: Option<f64>,
}

/// 把 Codex `/wham/usage` JSON 标准化为 CC Trace 的脱敏额度契约。
///
/// `captured_at` 由调用方提供，让相对 reset 的计算可测试，也保证快照采集时刻与解析
/// 使用同一个时间基准。
pub fn parse_usage_response(
    input: &str,
    captured_at: DateTime<Utc>,
) -> Result<ParsedCodexUsage, CodexUsageParseError> {
    let response: UsageResponse =
        serde_json::from_str(input).map_err(|_| CodexUsageParseError::InvalidResponse)?;
    let rate_limit = response
        .rate_limit
        .ok_or(CodexUsageParseError::MissingRateLimit)?;
    let primary = rate_limit
        .primary_window
        .ok_or(CodexUsageParseError::MissingPrimaryWindow)?;

    let mut windows = vec![normalize_window(primary, captured_at)?];
    if let Some(secondary) = rate_limit.secondary_window {
        windows.push(normalize_window(secondary, captured_at)?);
    }

    // 主要额度按领域语义选择：优先 fiveHour，缺失时退到 weekly。
    // unknown 仍可展示，但不能冒充主要额度。
    if let Some(primary_index) = windows
        .iter()
        .position(|window| window.kind == QuotaWindowKind::FiveHour)
        .or_else(|| {
            windows
                .iter()
                .position(|window| window.kind == QuotaWindowKind::Weekly)
        })
    {
        windows[primary_index].is_primary = true;
    }

    let plan = response
        .plan_type
        .and_then(|value| non_empty(value.as_str()).map(str::to_owned));
    let identity = plan.map(|plan| ProviderIdentity {
        account_hint: None,
        plan: Some(plan),
    });

    Ok(ParsedCodexUsage {
        identity,
        snapshot: QuotaSnapshot {
            windows,
            captured_at: captured_at.to_rfc3339(),
        },
    })
}

fn normalize_window(
    raw: RawWindow,
    captured_at: DateTime<Utc>,
) -> Result<QuotaWindow, CodexUsageParseError> {
    let used_percent = raw
        .used_percent
        .filter(|value| value.is_finite())
        .ok_or(CodexUsageParseError::MissingUsedPercent)?;
    let window_seconds = raw
        .limit_window_seconds
        .map(parse_window_seconds)
        .transpose()?;
    let resets_at = parse_reset_time(raw.reset_at, raw.reset_after_seconds, captured_at)?;
    let kind = classify_window(window_seconds);

    Ok(QuotaWindow {
        id: stable_window_id(kind, window_seconds),
        kind,
        display_name: None,
        used_percent,
        remaining_percent: QuotaWindow::normalized_remaining(used_percent),
        resets_at,
        window_seconds,
        is_active: true,
        is_primary: false,
    })
}

fn classify_window(window_seconds: Option<u64>) -> QuotaWindowKind {
    match window_seconds {
        Some(FIVE_HOUR_SECONDS) => QuotaWindowKind::FiveHour,
        Some(WEEKLY_SECONDS) => QuotaWindowKind::Weekly,
        _ => QuotaWindowKind::Unknown,
    }
}

fn stable_window_id(kind: QuotaWindowKind, window_seconds: Option<u64>) -> String {
    match kind {
        QuotaWindowKind::FiveHour => "codex.five-hour".to_owned(),
        QuotaWindowKind::Weekly => "codex.weekly".to_owned(),
        QuotaWindowKind::Unknown => format!(
            "codex.unknown-{}",
            window_seconds
                .map(|seconds| seconds.to_string())
                .unwrap_or_else(|| "unspecified".to_owned())
        ),
        QuotaWindowKind::ModelWeekly => {
            unreachable!("Codex Usage API does not produce model-weekly windows")
        }
    }
}

fn parse_window_seconds(value: f64) -> Result<u64, CodexUsageParseError> {
    let supported_range = 0.0..u64::MAX as f64;
    if !value.is_finite() || !supported_range.contains(&value) || value.fract() != 0.0 {
        return Err(CodexUsageParseError::InvalidWindowSeconds);
    }

    Ok(value as u64)
}

fn parse_reset_time(
    reset_at: Option<f64>,
    reset_after_seconds: Option<f64>,
    captured_at: DateTime<Utc>,
) -> Result<Option<String>, CodexUsageParseError> {
    if let Some(reset_at) = reset_at {
        return unix_seconds(reset_at)
            .map(|value| Some(value.to_rfc3339()))
            .ok_or(CodexUsageParseError::InvalidResetTime);
    }

    let Some(reset_after_seconds) = reset_after_seconds else {
        return Ok(None);
    };
    let duration =
        duration_from_seconds(reset_after_seconds).ok_or(CodexUsageParseError::InvalidResetTime)?;
    captured_at
        .checked_add_signed(duration)
        .map(|value| Some(value.to_rfc3339()))
        .ok_or(CodexUsageParseError::InvalidResetTime)
}

fn unix_seconds(value: f64) -> Option<DateTime<Utc>> {
    duration_from_seconds(value)
        .and_then(|duration| DateTime::from_timestamp(0, 0)?.checked_add_signed(duration))
}

fn duration_from_seconds(value: f64) -> Option<Duration> {
    if !value.is_finite() {
        return None;
    }

    let milliseconds = value * 1_000.0;
    let supported_range = i64::MIN as f64..i64::MAX as f64;
    if !milliseconds.is_finite() || !supported_range.contains(&milliseconds) {
        return None;
    }

    Some(Duration::milliseconds(milliseconds.round() as i64))
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

// --- 真实额度来源 ---

/// 真实 Codex 额度来源：凭据发现 → 按需续期 → Usage 请求 → 标准化。
pub struct CodexProvider {
    /// 同一 Provider 进程内只允许一个刷新任务，其余调用等待同一结果，
    /// 见 `docs/额度领域模型.md` 第 5.2 节。用异步锁而不是 `std::sync::Mutex`：
    /// 临界区里有 `await`。
    refresh_lock: Mutex<()>,
}

impl CodexProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            refresh_lock: Mutex::new(()),
        })
    }

    async fn fetch_once(&self) -> ProviderFetchOutcome {
        let credentials = match credentials::codex::discover() {
            Discovery::Found(credentials) => credentials,
            Discovery::Missing => return ProviderFetchOutcome::NoCredentials,
            Discovery::Unsupported => return ProviderFetchOutcome::Unsupported,
            // 有登录态但读不出来。说成「没有凭据」会把权限问题伪装成未登录。
            Discovery::Unreadable => {
                return ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Credentials,
                };
            }
        };

        let access_token = match self.ensure_fresh_access_token(&credentials).await {
            Ok(token) => token,
            Err(outcome) => return outcome,
        };

        let mut request = http::client()
            .get(USAGE_ENDPOINT)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", access_token.expose()),
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::USER_AGENT, "codex-cli");
        if let Some(account_id) = credentials.account_id.as_ref() {
            request = request.header("ChatGPT-Account-Id", account_id.expose());
        }

        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => return http::classify_transport(&error),
        };
        if let Some(failure) = http::classify_response(&response) {
            return failure;
        }

        let body = match response.text().await {
            Ok(body) => body,
            Err(error) => return http::classify_transport(&error),
        };

        let parsed = match parse_usage_response(&body, Utc::now()) {
            Ok(parsed) => parsed,
            Err(_) => {
                return ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Protocol,
                };
            }
        };

        ProviderFetchOutcome::Success {
            identity: Some(identity_of(&credentials, parsed.identity)),
            identity_key: credentials::identity_fingerprint(
                "codex",
                &[credentials.account_id.as_ref(), credentials.email.as_ref()],
            ),
            snapshot: parsed.snapshot,
        }
    }

    /// 返回可用于本次请求的 access token，必要时先续期并回写 `auth.json`。
    async fn ensure_fresh_access_token(
        &self,
        credentials: &CodexCredentials,
    ) -> Result<Secret, ProviderFetchOutcome> {
        // 无法解出过期时刻（不透明 token）时不主动刷新：让 Provider 用 401 告诉我们。
        let Some(expires_at) = credentials.access_expires_at else {
            return Ok(credentials.access_token.clone());
        };
        if !is_expiring(expires_at, Utc::now()) {
            return Ok(credentials.access_token.clone());
        }

        let Some(refresh_token) = credentials.refresh_token.clone() else {
            return Err(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            });
        };

        let _guard = self.refresh_lock.lock().await;

        // 拿到锁后重读：排队期间 Codex CLI 或另一次刷新可能已经写入了新 token。
        let refresh_token = match credentials::codex::discover() {
            Discovery::Found(latest) => {
                if latest
                    .access_expires_at
                    .is_some_and(|expires_at| !is_expiring(expires_at, Utc::now()))
                {
                    return Ok(latest.access_token);
                }
                latest.refresh_token.unwrap_or(refresh_token)
            }
            _ => refresh_token,
        };

        let refreshed = refresh_tokens(&refresh_token).await?;
        credentials::codex::write_back(&refreshed, Utc::now()).map_err(|_| {
            // 服务端已经轮换了 token，但我们没能存下来。报凭据类错误而不是静默继续：
            // 下次启动会拿着作废的 refresh token，用户需要知道。
            ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            }
        })?;

        Ok(refreshed.access_token)
    }
}

impl QuotaProvider for CodexProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Codex
    }

    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome> {
        Box::pin(self.fetch_once())
    }
}

/// 展示身份：账号提示只用脱敏 hint，计划名优先本地凭据，缺失时用响应补位。
fn identity_of(
    credentials: &CodexCredentials,
    from_response: Option<ProviderIdentity>,
) -> ProviderIdentity {
    ProviderIdentity {
        account_hint: credentials
            .email
            .as_ref()
            .or(credentials.account_id.as_ref())
            .map(Secret::hint),
        plan: credentials
            .plan
            .clone()
            .or_else(|| from_response.and_then(|identity| identity.plan)),
    }
}

fn is_expiring(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    expires_at - now < Duration::seconds(CODEX_TOKEN_REFRESH_SKEW_SECS)
}

async fn refresh_tokens(refresh_token: &Secret) -> Result<RefreshedTokens, ProviderFetchOutcome> {
    let response = http::client()
        .post(TOKEN_ENDPOINT)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose()),
            ("client_id", OAUTH_CLIENT_ID),
            ("scope", "openid profile email"),
        ])
        .send()
        .await
        .map_err(|error| http::classify_transport(&error))?;

    if !response.status().is_success() {
        // 刷新失败一律是凭据类：下一步是「在 Codex CLI 重新登录」，
        // 不能伪装成离线，见 docs/状态与错误模型.md 第 4 节。
        return Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Credentials,
        });
    }

    let body = response
        .text()
        .await
        .map_err(|error| http::classify_transport(&error))?;

    parse_refresh_response(&body, refresh_token).ok_or(ProviderFetchOutcome::Failed {
        kind: ErrorKind::Credentials,
    })
}

/// 解析刷新响应。响应不返回 `refresh_token` 时沿用旧值——这是它未轮换的信号。
fn parse_refresh_response(body: &str, previous_refresh: &Secret) -> Option<RefreshedTokens> {
    let root: serde_json::Value = serde_json::from_str(body).ok()?;

    let access_token = non_empty(root.get("access_token")?.as_str()?)?;
    let refresh_token = root
        .get("refresh_token")
        .and_then(serde_json::Value::as_str)
        .and_then(non_empty)
        .map(Secret::new)
        .unwrap_or_else(|| previous_refresh.clone());
    let id_token = root
        .get("id_token")
        .and_then(serde_json::Value::as_str)
        .and_then(non_empty)
        .map(Secret::new);

    Some(RefreshedTokens {
        access_token: Secret::new(access_token),
        refresh_token,
        id_token,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const NORMAL: &str = include_str!("../../../fixtures/providers/codex/usage-normal.json");
    const WEEKLY_ONLY: &str =
        include_str!("../../../fixtures/providers/codex/usage-weekly-only.json");
    const UNKNOWN_WINDOW: &str =
        include_str!("../../../fixtures/providers/codex/usage-unknown-window.json");

    fn captured_at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid fixture timestamp")
    }

    #[test]
    fn normal_fixture_maps_primary_secondary_and_plan() {
        let parsed = parse_usage_response(NORMAL, captured_at()).expect("fixture parses");

        assert_eq!(
            parsed.identity,
            Some(ProviderIdentity {
                account_hint: None,
                plan: Some("plus".to_owned()),
            })
        );
        assert_eq!(parsed.snapshot.captured_at, captured_at().to_rfc3339());
        assert_eq!(parsed.snapshot.windows.len(), 2);

        let primary = &parsed.snapshot.windows[0];
        assert_eq!(primary.id, "codex.five-hour");
        assert_eq!(primary.kind, QuotaWindowKind::FiveHour);
        assert_eq!(primary.used_percent, 32.0);
        assert_eq!(primary.remaining_percent, 68.0);
        assert_eq!(primary.window_seconds, Some(FIVE_HOUR_SECONDS));
        assert_eq!(
            primary.resets_at,
            Some(
                DateTime::from_timestamp(1_800_000_000, 0)
                    .expect("valid fixture timestamp")
                    .to_rfc3339()
            )
        );
        assert!(primary.is_primary);

        let secondary = &parsed.snapshot.windows[1];
        assert_eq!(secondary.id, "codex.weekly");
        assert_eq!(secondary.kind, QuotaWindowKind::Weekly);
        assert_eq!(secondary.remaining_percent, 87.5);
        assert_eq!(secondary.window_seconds, Some(WEEKLY_SECONDS));
        assert_eq!(
            secondary.resets_at,
            Some((captured_at() + Duration::hours(2)).to_rfc3339())
        );
        assert!(!secondary.is_primary);
    }

    #[test]
    fn weekly_only_fixture_does_not_invent_a_five_hour_window() {
        let parsed = parse_usage_response(WEEKLY_ONLY, captured_at()).expect("fixture parses");

        assert_eq!(parsed.snapshot.windows.len(), 1);
        let weekly = &parsed.snapshot.windows[0];
        assert_eq!(weekly.kind, QuotaWindowKind::Weekly);
        assert_eq!(weekly.id, "codex.weekly");
        assert!(weekly.is_primary);
        assert!(
            parsed
                .snapshot
                .windows
                .iter()
                .all(|window| window.kind != QuotaWindowKind::FiveHour)
        );
    }

    #[test]
    fn unknown_window_stays_unknown_clamps_remaining_and_keeps_reset_missing() {
        let parsed = parse_usage_response(UNKNOWN_WINDOW, captured_at()).expect("fixture parses");

        assert!(parsed.identity.is_none());
        assert_eq!(parsed.snapshot.windows.len(), 1);
        let window = &parsed.snapshot.windows[0];
        assert_eq!(window.kind, QuotaWindowKind::Unknown);
        assert_eq!(window.id, "codex.unknown-86400");
        assert_eq!(window.window_seconds, Some(86_400));
        assert_eq!(window.used_percent, 140.0);
        assert_eq!(window.remaining_percent, 0.0);
        assert!(window.resets_at.is_none());
        assert!(!window.is_primary);
    }

    #[test]
    fn negative_used_percent_clamps_remaining_to_one_hundred() {
        let parsed = parse_usage_response(
            r#"{
                "rate_limit": {
                    "primary_window": {
                        "used_percent": -10,
                        "limit_window_seconds": 18000
                    }
                }
            }"#,
            captured_at(),
        )
        .expect("negative provider percentage is normalized");

        let window = &parsed.snapshot.windows[0];
        assert_eq!(window.used_percent, -10.0);
        assert_eq!(window.remaining_percent, 100.0);
    }

    #[test]
    fn missing_window_duration_stays_unknown() {
        let parsed = parse_usage_response(
            r#"{"rate_limit":{"primary_window":{"used_percent":25}}}"#,
            captured_at(),
        )
        .expect("window duration is optional");

        assert_eq!(parsed.snapshot.windows[0].kind, QuotaWindowKind::Unknown);
        assert_eq!(parsed.snapshot.windows[0].id, "codex.unknown-unspecified");
        assert_eq!(parsed.snapshot.windows[0].window_seconds, None);
    }

    #[test]
    fn response_shape_errors_are_classified_without_returning_raw_content() {
        for (input, expected) in [
            ("not json", CodexUsageParseError::InvalidResponse),
            (
                r#"{"plan_type":"plus"}"#,
                CodexUsageParseError::MissingRateLimit,
            ),
            (
                r#"{"rate_limit":{}}"#,
                CodexUsageParseError::MissingPrimaryWindow,
            ),
            (
                r#"{"rate_limit":{"primary_window":{}}}"#,
                CodexUsageParseError::MissingUsedPercent,
            ),
            (
                r#"{"rate_limit":{"primary_window":{"used_percent":10,"limit_window_seconds":1.5}}}"#,
                CodexUsageParseError::InvalidWindowSeconds,
            ),
        ] {
            assert_eq!(parse_usage_response(input, captured_at()), Err(expected));
        }
    }

    // --- token 续期 ---

    #[test]
    fn a_refresh_response_without_a_new_refresh_token_keeps_the_old_one() {
        let previous = Secret::new("rt_old");
        let parsed = parse_refresh_response(r#"{"access_token":"at_new"}"#, &previous)
            .expect("a response with an access token is usable");

        assert_eq!(parsed.access_token.expose(), "at_new");
        assert_eq!(
            parsed.refresh_token.expose(),
            "rt_old",
            "an absent refresh_token means it was not rotated"
        );
        assert!(parsed.id_token.is_none());
    }

    #[test]
    fn a_rotated_refresh_token_replaces_the_old_one() {
        let previous = Secret::new("rt_old");
        let parsed = parse_refresh_response(
            r#"{"access_token":"at_new","refresh_token":"rt_new","id_token":"id_new"}"#,
            &previous,
        )
        .expect("fixture parses");

        assert_eq!(parsed.refresh_token.expose(), "rt_new");
        assert_eq!(parsed.id_token.as_ref().map(Secret::expose), Some("id_new"));
    }

    #[test]
    fn a_refresh_response_without_an_access_token_is_not_usable() {
        let previous = Secret::new("rt_old");
        for body in [
            "not json",
            "{}",
            r#"{"access_token":""}"#,
            r#"{"access_token":null}"#,
            r#"{"error":"invalid_grant"}"#,
        ] {
            assert!(
                parse_refresh_response(body, &previous).is_none(),
                "{body:?} must not produce tokens"
            );
        }
    }

    #[test]
    fn refresh_starts_five_minutes_before_the_token_expires() {
        let now = captured_at();
        assert!(!is_expiring(now + Duration::seconds(301), now));
        assert!(is_expiring(now + Duration::seconds(299), now));
        assert!(
            is_expiring(now - Duration::hours(1), now),
            "an already expired token must refresh"
        );
    }

    #[test]
    fn the_display_identity_only_carries_a_hint_and_a_plan() {
        let Discovery::Found(credentials) = credentials::codex::parse(&oauth_credentials()) else {
            panic!("fixture parses");
        };

        let identity = identity_of(&credentials, None);
        assert_eq!(identity.account_hint.as_deref(), Some("u***@example.test"));
        assert_eq!(identity.plan.as_deref(), Some("plus"));
    }

    #[test]
    fn the_response_plan_fills_in_when_the_local_credentials_have_none() {
        let raw = r#"{"tokens":{"access_token":"opaque"}}"#;
        let Discovery::Found(credentials) = credentials::codex::parse(raw) else {
            panic!("an opaque token is still usable");
        };

        let identity = identity_of(
            &credentials,
            Some(ProviderIdentity {
                account_hint: None,
                plan: Some("pro".to_owned()),
            }),
        );
        assert_eq!(identity.plan.as_deref(), Some("pro"));
        assert_eq!(
            identity.account_hint, None,
            "the response identity must never supply an account hint"
        );
    }

    /// 与 `credentials::codex` 的 Fixture 同构，内容全部虚构。
    fn oauth_credentials() -> String {
        const ID_TOKEN: &str = concat!(
            "eyJhbGciOiJSUzI1NiJ9",
            ".",
            "eyJleHAiOjE3MDAwMDAwMDAsImVtYWlsIjoidXNlckBleGFtcGxlLnRlc3QiLCJodHRwczovL2FwaS",
            "5vcGVuYWkuY29tL2F1dGgiOnsiY2hhdGdwdF9hY2NvdW50X2lkIjoiYWNjdF9maXh0dXJlXzAwMDEi",
            "LCJjaGF0Z3B0X3BsYW5fdHlwZSI6InBsdXMifX0",
            ".",
            "c2ln"
        );
        format!(
            r#"{{"tokens":{{"id_token":"{ID_TOKEN}","access_token":"opaque","refresh_token":"rt"}}}}"#
        )
    }

    #[test]
    fn invalid_reset_is_a_protocol_error_instead_of_becoming_a_fake_time() {
        let input = r#"{
            "rate_limit": {
                "primary_window": {
                    "used_percent": 10,
                    "reset_at": 1e30,
                    "limit_window_seconds": 18000
                }
            }
        }"#;

        assert_eq!(
            parse_usage_response(input, captured_at()),
            Err(CodexUsageParseError::InvalidResetTime)
        );
    }
}
