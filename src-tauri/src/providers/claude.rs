//! Claude Code 额度来源：凭据发现、token 续期、OAuth Usage 请求与响应标准化。
//!
//! 协议见 `docs/额度领域模型.md` 第 3.2 节，凭据来源与平台差异见第 5 节。响应解析
//! ([`parse_usage_response`]) 与网络分开，因此 Fixture 测试不需要出网。
//!
//! 刷新回写落回**读到凭据的那个来源**（文件或 macOS 钥匙串）：Claude Code 的
//! refresh token 会轮换，不回写会作废 CLI 手里的那份，见
//! [ADR-0014](../../../docs/决策/ADR-0014-token刷新结果回写外部凭据.md)。

use std::sync::Arc;
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::Mutex;

use super::credentials::claude::{ClaudeCredentials, RefreshedTokens};
use super::credentials::{self, Discovery, Secret};
use super::{BoxFuture, ProviderFetchOutcome, QuotaProvider, http};
use crate::contracts::{
    ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot, QuotaWindow, QuotaWindowKind,
};
use crate::scheduler::params::CLAUDE_TOKEN_REFRESH_SKEW_SECS;

const USAGE_ENDPOINT: &str = "https://api.anthropic.com/api/oauth/usage";
const TOKEN_ENDPOINT: &str = "https://platform.claude.com/v1/oauth/token";
const OAUTH_BETA_HEADER: &str = "oauth-2025-04-20";
/// Claude Code 的公开 OAuth client id，与 CLI 使用同一份，不是秘密。
const OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// 刷新被拒后等待多久再重读来源：给抢先成功的其他客户端一点写入时间。
const SOFT_RECOVERY_DELAY: StdDuration = StdDuration::from_secs(1);
/// 刷新响应没给 `expires_in` 时的保守默认寿命（秒）。
const DEFAULT_TOKEN_LIFETIME_SECS: i64 = 3_600;

const FIVE_HOUR_SECONDS: u64 = 5 * 60 * 60;
const WEEKLY_SECONDS: u64 = 7 * 24 * 60 * 60;

/// 解析成功后的脱敏结果。Claude Usage 响应不提供可安全映射的身份字段。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedClaudeUsage {
    pub snapshot: QuotaSnapshot,
}

/// 不包含 serde 原始错误、响应片段或 scope 原文，避免未来被直接写进日志时泄露响应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeUsageParseError {
    InvalidResponse,
    MissingQuotaWindows,
    MissingLimitKind,
    MissingUsedPercent,
    InvalidUsedPercent,
    InvalidResetTime,
    MissingScopedIdentity,
}

#[derive(Deserialize)]
struct UsageResponse {
    #[serde(default)]
    five_hour: Option<LegacyWindow>,
    #[serde(default)]
    seven_day: Option<LegacyWindow>,
    #[serde(default)]
    seven_day_opus: Option<LegacyWindow>,
    #[serde(default)]
    seven_day_sonnet: Option<LegacyWindow>,
    #[serde(default)]
    limits: Vec<RawLimit>,
}

#[derive(Deserialize)]
struct LegacyWindow {
    utilization: Option<f64>,
    resets_at: Option<Value>,
}

#[derive(Deserialize)]
struct RawLimit {
    kind: Option<String>,
    percent: Option<f64>,
    resets_at: Option<Value>,
    is_active: Option<bool>,
    scope: Option<RawScope>,
}

#[derive(Deserialize)]
struct RawScope {
    model: Option<ScopeValue>,
    surface: Option<ScopeValue>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ScopeValue {
    Object(ScopeObject),
    Name(String),
}

#[derive(Deserialize)]
struct ScopeObject {
    id: Option<String>,
    display_name: Option<String>,
    name: Option<String>,
}

#[derive(Default)]
struct GenericLimits {
    session: Option<QuotaWindow>,
    weekly: Option<QuotaWindow>,
    models: Vec<QuotaWindow>,
    unknown: Vec<QuotaWindow>,
}

/// 把 Claude Code `/api/oauth/usage` JSON 标准化为 CC Trace 的脱敏额度契约。
///
/// 动态 `limits` 优先于同语义 legacy 字段；动态窗口缺少 reset 时，才从本次响应中的
/// legacy 窗口补齐。跨快照的未来 reset 保留仍属于调度／快照层，不在解析器中实现。
pub fn parse_usage_response(
    input: &str,
    captured_at: DateTime<Utc>,
) -> Result<ParsedClaudeUsage, ClaudeUsageParseError> {
    let response: UsageResponse =
        serde_json::from_str(input).map_err(|_| ClaudeUsageParseError::InvalidResponse)?;

    let legacy_session = response
        .five_hour
        .map(|raw| {
            normalize_legacy_window(raw, QuotaWindowKind::FiveHour, "claude.five-hour", None)
        })
        .transpose()?;
    let legacy_weekly = response
        .seven_day
        .map(|raw| normalize_legacy_window(raw, QuotaWindowKind::Weekly, "claude.weekly", None))
        .transpose()?;
    let legacy_opus = response
        .seven_day_opus
        .map(|raw| {
            normalize_legacy_window(
                raw,
                QuotaWindowKind::ModelWeekly,
                "claude.model.opus",
                Some("Opus"),
            )
        })
        .transpose()?;
    let legacy_sonnet = response
        .seven_day_sonnet
        .map(|raw| {
            normalize_legacy_window(
                raw,
                QuotaWindowKind::ModelWeekly,
                "claude.model.sonnet",
                Some("Sonnet"),
            )
        })
        .transpose()?;

    let mut generic = parse_generic_limits(response.limits)?;
    let session = merge_preferred(generic.session.take(), legacy_session);
    let weekly = merge_preferred(generic.weekly.take(), legacy_weekly);
    append_legacy_model("Opus", legacy_opus, &mut generic.models);
    append_legacy_model("Sonnet", legacy_sonnet, &mut generic.models);

    let mut windows = Vec::new();
    windows.extend(session);
    windows.extend(weekly);
    windows.extend(generic.models);
    windows.extend(generic.unknown);

    if windows.is_empty() {
        return Err(ClaudeUsageParseError::MissingQuotaWindows);
    }

    // Provider 标准化后的返回顺序拥有主次语义，不再按窗口类型或 active 状态重选。
    windows[0].is_primary = true;

    Ok(ParsedClaudeUsage {
        snapshot: QuotaSnapshot {
            windows,
            captured_at: captured_at.to_rfc3339(),
        },
    })
}

fn parse_generic_limits(rows: Vec<RawLimit>) -> Result<GenericLimits, ClaudeUsageParseError> {
    let mut result = GenericLimits::default();

    for row in rows {
        let kind = row
            .kind
            .as_deref()
            .and_then(non_empty)
            .ok_or(ClaudeUsageParseError::MissingLimitKind)?;
        let used_percent = required_percent(row.percent)?;
        let resets_at = parse_reset_time(row.resets_at)?;
        let is_active = row.is_active.unwrap_or(true);

        match kind {
            "session" => {
                result.session = Some(window(
                    "claude.five-hour",
                    QuotaWindowKind::FiveHour,
                    None,
                    used_percent,
                    resets_at,
                    Some(FIVE_HOUR_SECONDS),
                    is_active,
                ));
            }
            "weekly_all" => {
                result.weekly = Some(window(
                    "claude.weekly",
                    QuotaWindowKind::Weekly,
                    None,
                    used_percent,
                    resets_at,
                    Some(WEEKLY_SECONDS),
                    is_active,
                ));
            }
            "weekly_scoped" => {
                let (raw_id, display_name) = scoped_identity(row.scope)
                    .ok_or(ClaudeUsageParseError::MissingScopedIdentity)?;
                let id = scoped_window_id(raw_id.as_deref(), display_name.as_str());
                upsert_by_id(
                    &mut result.models,
                    window(
                        id.as_str(),
                        QuotaWindowKind::ModelWeekly,
                        Some(display_name.as_str()),
                        used_percent,
                        resets_at,
                        Some(WEEKLY_SECONDS),
                        is_active,
                    ),
                );
            }
            unknown_kind => {
                let id = format!("claude.unknown.{}", slug(unknown_kind));
                upsert_by_id(
                    &mut result.unknown,
                    window(
                        id.as_str(),
                        QuotaWindowKind::Unknown,
                        None,
                        used_percent,
                        resets_at,
                        None,
                        is_active,
                    ),
                );
            }
        }
    }

    Ok(result)
}

fn normalize_legacy_window(
    raw: LegacyWindow,
    kind: QuotaWindowKind,
    id: &str,
    display_name: Option<&str>,
) -> Result<QuotaWindow, ClaudeUsageParseError> {
    let used_percent = required_percent(raw.utilization)?;
    let resets_at = parse_reset_time(raw.resets_at)?;
    let window_seconds = match kind {
        QuotaWindowKind::FiveHour => Some(FIVE_HOUR_SECONDS),
        QuotaWindowKind::Weekly | QuotaWindowKind::ModelWeekly => Some(WEEKLY_SECONDS),
        QuotaWindowKind::Unknown => None,
    };

    Ok(window(
        id,
        kind,
        display_name,
        used_percent,
        resets_at,
        window_seconds,
        true,
    ))
}

fn window(
    id: &str,
    kind: QuotaWindowKind,
    display_name: Option<&str>,
    used_percent: f64,
    resets_at: Option<String>,
    window_seconds: Option<u64>,
    is_active: bool,
) -> QuotaWindow {
    QuotaWindow {
        id: id.to_owned(),
        kind,
        display_name: display_name.map(str::to_owned),
        used_percent,
        remaining_percent: QuotaWindow::normalized_remaining(used_percent),
        resets_at,
        window_seconds,
        is_active,
        is_primary: false,
    }
}

fn merge_preferred(
    preferred: Option<QuotaWindow>,
    fallback: Option<QuotaWindow>,
) -> Option<QuotaWindow> {
    let Some(mut preferred) = preferred else {
        return fallback;
    };

    if preferred.resets_at.is_none() {
        preferred.resets_at = fallback.and_then(|window| window.resets_at);
    }
    Some(preferred)
}

fn append_legacy_model(name: &str, legacy: Option<QuotaWindow>, models: &mut Vec<QuotaWindow>) {
    let Some(legacy) = legacy else {
        return;
    };
    let name = name.to_lowercase();

    if let Some(existing) = models.iter_mut().find(|window| {
        window.id.to_lowercase().contains(name.as_str())
            || window
                .display_name
                .as_deref()
                .is_some_and(|display_name| display_name.to_lowercase().contains(name.as_str()))
    }) {
        if existing.resets_at.is_none() {
            existing.resets_at = legacy.resets_at;
        }
        return;
    }

    models.push(legacy);
}

fn scoped_identity(scope: Option<RawScope>) -> Option<(Option<String>, String)> {
    let scope = scope?;
    let model_name = scope.model.as_ref().and_then(scope_name);
    let surface_name = scope.surface.as_ref().and_then(scope_name);
    let display_name = model_name.or(surface_name)?;
    let raw_id = scope
        .model
        .as_ref()
        .and_then(scope_id)
        .or_else(|| scope.surface.as_ref().and_then(scope_id));

    Some((raw_id, display_name))
}

fn scope_name(value: &ScopeValue) -> Option<String> {
    match value {
        ScopeValue::Object(value) => value
            .display_name
            .as_deref()
            .and_then(non_empty)
            .or_else(|| value.name.as_deref().and_then(non_empty))
            .map(str::to_owned),
        ScopeValue::Name(value) => non_empty(value).map(str::to_owned),
    }
}

fn scope_id(value: &ScopeValue) -> Option<String> {
    match value {
        ScopeValue::Object(value) => value.id.as_deref().and_then(non_empty).map(str::to_owned),
        ScopeValue::Name(_) => None,
    }
}

fn scoped_window_id(raw_id: Option<&str>, display_name: &str) -> String {
    format!("claude.model.{}", slug(raw_id.unwrap_or(display_name)))
}

fn slug(value: &str) -> String {
    let mut result = String::new();
    let mut needs_separator = false;

    for character in value.trim().chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            if needs_separator && !result.is_empty() {
                result.push('-');
            }
            result.push(character);
            needs_separator = false;
        } else {
            needs_separator = true;
        }
    }

    if result.is_empty() {
        "unspecified".to_owned()
    } else {
        result
    }
}

fn upsert_by_id(windows: &mut Vec<QuotaWindow>, candidate: QuotaWindow) {
    if let Some(existing) = windows.iter_mut().find(|window| window.id == candidate.id) {
        *existing = candidate;
    } else {
        windows.push(candidate);
    }
}

fn required_percent(value: Option<f64>) -> Result<f64, ClaudeUsageParseError> {
    let value = value.ok_or(ClaudeUsageParseError::MissingUsedPercent)?;
    if !value.is_finite() {
        return Err(ClaudeUsageParseError::InvalidUsedPercent);
    }
    Ok(value)
}

fn parse_reset_time(value: Option<Value>) -> Result<Option<String>, ClaudeUsageParseError> {
    let Some(value) = value else {
        return Ok(None);
    };

    let parsed = match value {
        Value::String(value) => DateTime::parse_from_rfc3339(value.as_str())
            .ok()
            .map(|value| value.with_timezone(&Utc)),
        Value::Number(value) => value.as_f64().and_then(unix_seconds),
        _ => None,
    };

    parsed
        .map(|value| Some(value.to_rfc3339()))
        .ok_or(ClaudeUsageParseError::InvalidResetTime)
}

fn unix_seconds(value: f64) -> Option<DateTime<Utc>> {
    if !value.is_finite() {
        return None;
    }
    let milliseconds = value * 1_000.0;
    let supported_range = i64::MIN as f64..i64::MAX as f64;
    if !milliseconds.is_finite() || !supported_range.contains(&milliseconds) {
        return None;
    }

    DateTime::from_timestamp(0, 0)?
        .checked_add_signed(Duration::milliseconds(milliseconds.round() as i64))
}

fn non_empty(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

// --- 真实额度来源 ---

/// 真实 Claude Code 额度来源：凭据发现 → 按需续期 → Usage 请求 → 标准化。
pub struct ClaudeProvider {
    /// 同一 Provider 进程内只允许一个刷新任务。Claude 的 refresh token 是一次性的，
    /// 并发刷新会让其中一个必然拿到 `invalid_grant`。
    refresh_lock: Mutex<()>,
}

impl ClaudeProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            refresh_lock: Mutex::new(()),
        })
    }

    async fn fetch_once(&self) -> ProviderFetchOutcome {
        let credentials = match credentials::claude::discover() {
            Discovery::Found(credentials) => credentials,
            Discovery::Missing => return ProviderFetchOutcome::NoCredentials,
            Discovery::Unsupported => return ProviderFetchOutcome::Unsupported,
            // 钥匙串授权被拒也落这里：有登录态，只是我们拿不到。
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

        let response = match http::client()
            .get(USAGE_ENDPOINT)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", access_token.expose()),
            )
            .header(reqwest::header::ACCEPT, "application/json")
            .header("anthropic-beta", OAUTH_BETA_HEADER)
            .send()
            .await
        {
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
            identity: Some(ProviderIdentity {
                account_hint: credentials.email.as_ref().map(Secret::hint),
                plan: credentials.subscription.clone(),
            }),
            identity_key: credentials::identity_fingerprint(
                "claude",
                &[credentials.email.as_ref()],
            ),
            snapshot: parsed.snapshot,
        }
    }

    async fn ensure_fresh_access_token(
        &self,
        credentials: &ClaudeCredentials,
    ) -> Result<Secret, ProviderFetchOutcome> {
        // 没有过期时刻就不主动刷新：让 Provider 用 401 告诉我们，而不是抢 refresh token。
        let Some(expires_at) = credentials.expires_at else {
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

        // 排队期间 Claude Code CLI 或另一次刷新可能已经写入新 token，先重读再决定。
        let refresh_token = match reread() {
            Some(latest) => {
                if latest
                    .expires_at
                    .is_some_and(|expires_at| !is_expiring(expires_at, Utc::now()))
                {
                    return Ok(latest.access_token);
                }
                latest.refresh_token.unwrap_or(refresh_token)
            }
            None => refresh_token,
        };

        let refreshed = match refresh_tokens(&refresh_token).await {
            Ok(refreshed) => refreshed,
            // 服务端说这份 refresh token 已经作废：多半是别的客户端抢先刷成功了。
            // 等一拍重读，发现新值就静默采用；否则报凭据类错误。首版不做 delegated
            // refresh，见 ADR-0007。
            Err(RefreshFailure::Revoked) => {
                tokio::time::sleep(SOFT_RECOVERY_DELAY).await;
                return recovered_token(&refresh_token).ok_or(ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Credentials,
                });
            }
            Err(RefreshFailure::Outcome(outcome)) => return Err(outcome),
        };

        credentials::claude::write_back(credentials.source, &refreshed).map_err(|_| {
            ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            }
        })?;

        Ok(refreshed.access_token)
    }
}

impl QuotaProvider for ClaudeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Claude
    }

    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome> {
        Box::pin(self.fetch_once())
    }
}

/// 重新读取凭据来源，只关心 token 与过期时刻。
fn reread() -> Option<ClaudeCredentials> {
    match credentials::claude::discover() {
        Discovery::Found(credentials) => Some(credentials),
        _ => None,
    }
}

/// 软恢复：别的客户端刷新成功时，来源里会出现一个与我们手里不同且仍新鲜的 token。
fn recovered_token(attempted_refresh: &Secret) -> Option<Secret> {
    let latest = reread()?;
    let rotated = latest
        .refresh_token
        .as_ref()
        .is_some_and(|token| token != attempted_refresh);
    let fresh = latest
        .expires_at
        .is_some_and(|expires_at| expires_at > Utc::now());

    (rotated && fresh).then_some(latest.access_token)
}

fn is_expiring(expires_at: DateTime<Utc>, now: DateTime<Utc>) -> bool {
    expires_at - now < Duration::seconds(CLAUDE_TOKEN_REFRESH_SKEW_SECS)
}

/// 刷新失败的两种形态。`Revoked` 需要走软恢复，其余直接映射成展示状态。
enum RefreshFailure {
    Revoked,
    Outcome(ProviderFetchOutcome),
}

async fn refresh_tokens(refresh_token: &Secret) -> Result<RefreshedTokens, RefreshFailure> {
    let response = http::client()
        .post(TOKEN_ENDPOINT)
        .header(reqwest::header::ACCEPT, "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.expose()),
            ("client_id", OAUTH_CLIENT_ID),
        ])
        .send()
        .await
        .map_err(|error| RefreshFailure::Outcome(http::classify_transport(&error)))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| RefreshFailure::Outcome(http::classify_transport(&error)))?;

    if !status.is_success() {
        return Err(if is_invalid_grant(status, &body) {
            RefreshFailure::Revoked
        } else {
            RefreshFailure::Outcome(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            })
        });
    }

    parse_refresh_response(&body, refresh_token, Utc::now()).ok_or(RefreshFailure::Outcome(
        ProviderFetchOutcome::Failed {
            kind: ErrorKind::Credentials,
        },
    ))
}

/// `400 invalid_grant` 是「这份 refresh token 已被轮换掉」，不是普通的凭据失效。
fn is_invalid_grant(status: reqwest::StatusCode, body: &str) -> bool {
    status == reqwest::StatusCode::BAD_REQUEST && body.contains("invalid_grant")
}

fn parse_refresh_response(
    body: &str,
    previous_refresh: &Secret,
    now: DateTime<Utc>,
) -> Option<RefreshedTokens> {
    let root: Value = serde_json::from_str(body).ok()?;

    let access_token = non_empty(root.get("access_token")?.as_str()?)?;
    let refresh_token = root
        .get("refresh_token")
        .and_then(Value::as_str)
        .and_then(non_empty)
        .map(Secret::new)
        .unwrap_or_else(|| previous_refresh.clone());
    let lifetime = root
        .get("expires_in")
        .and_then(Value::as_f64)
        .filter(|seconds| seconds.is_finite() && *seconds > 0.0)
        .map(|seconds| seconds as i64)
        .unwrap_or(DEFAULT_TOKEN_LIFETIME_SECS);

    Some(RefreshedTokens {
        access_token: Secret::new(access_token),
        refresh_token,
        expires_at: now + Duration::seconds(lifetime),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIXED: &str = include_str!("../../../fixtures/providers/claude/usage-mixed.json");
    const LEGACY_ONLY: &str =
        include_str!("../../../fixtures/providers/claude/usage-legacy-only.json");
    const SCOPED_AND_UNKNOWN: &str =
        include_str!("../../../fixtures/providers/claude/usage-scoped-and-unknown.json");

    fn captured_at() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid fixture timestamp")
    }

    // --- token 续期 ---

    #[test]
    fn a_refresh_response_turns_expires_in_into_an_absolute_instant() {
        let previous = Secret::new("rt_old");
        let now = captured_at();
        let parsed = parse_refresh_response(
            r#"{"access_token":"at_new","refresh_token":"rt_new","expires_in":28800}"#,
            &previous,
            now,
        )
        .expect("fixture parses");

        assert_eq!(parsed.access_token.expose(), "at_new");
        assert_eq!(parsed.refresh_token.expose(), "rt_new");
        assert_eq!(parsed.expires_at, now + Duration::seconds(28_800));
    }

    #[test]
    fn a_missing_or_nonsensical_lifetime_falls_back_to_one_hour() {
        let previous = Secret::new("rt_old");
        let now = captured_at();
        for body in [
            r#"{"access_token":"a"}"#,
            r#"{"access_token":"a","expires_in":0}"#,
            r#"{"access_token":"a","expires_in":-5}"#,
            r#"{"access_token":"a","expires_in":"soon"}"#,
        ] {
            let parsed =
                parse_refresh_response(body, &previous, now).expect("an access token is enough");
            assert_eq!(
                parsed.expires_at,
                now + Duration::seconds(DEFAULT_TOKEN_LIFETIME_SECS),
                "{body:?}"
            );
            assert_eq!(
                parsed.refresh_token.expose(),
                "rt_old",
                "an absent refresh_token means it was not rotated"
            );
        }
    }

    #[test]
    fn a_refresh_response_without_an_access_token_is_not_usable() {
        let previous = Secret::new("rt_old");
        for body in ["not json", "{}", r#"{"access_token":""}"#] {
            assert!(
                parse_refresh_response(body, &previous, captured_at()).is_none(),
                "{body:?} must not produce tokens"
            );
        }
    }

    #[test]
    fn only_a_400_invalid_grant_triggers_the_soft_recovery_path() {
        assert!(is_invalid_grant(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_grant"}"#
        ));
        assert!(!is_invalid_grant(
            reqwest::StatusCode::BAD_REQUEST,
            r#"{"error":"invalid_request"}"#
        ));
        assert!(
            !is_invalid_grant(
                reqwest::StatusCode::UNAUTHORIZED,
                r#"{"error":"invalid_grant"}"#
            ),
            "a 401 is a plain credential failure, not a rotated token"
        );
    }

    #[test]
    fn refresh_starts_only_thirty_seconds_before_expiry() {
        let now = captured_at();
        assert!(
            !is_expiring(now + Duration::seconds(31), now),
            "CC Trace must leave the refresh token to Claude Code for as long as possible"
        );
        assert!(is_expiring(now + Duration::seconds(29), now));
        assert!(is_expiring(now - Duration::hours(1), now));
    }

    #[test]
    fn mixed_fixture_merges_dynamic_and_legacy_windows_without_duplicates() {
        let parsed = parse_usage_response(MIXED, captured_at()).expect("fixture parses");

        assert_eq!(parsed.snapshot.captured_at, captured_at().to_rfc3339());
        assert_eq!(parsed.snapshot.windows.len(), 4);

        let session = &parsed.snapshot.windows[0];
        assert_eq!(session.id, "claude.five-hour");
        assert_eq!(session.kind, QuotaWindowKind::FiveHour);
        assert_eq!(session.used_percent, 12.0);
        assert_eq!(session.remaining_percent, 88.0);
        assert_eq!(session.window_seconds, Some(FIVE_HOUR_SECONDS));
        assert_eq!(
            session.resets_at,
            Some(
                DateTime::parse_from_rfc3339("2026-07-13T02:30:00.424333+00:00")
                    .expect("fixture reset")
                    .with_timezone(&Utc)
                    .to_rfc3339()
            )
        );
        assert!(session.is_active);
        assert!(session.is_primary);

        let weekly = &parsed.snapshot.windows[1];
        assert_eq!(weekly.id, "claude.weekly");
        assert_eq!(weekly.kind, QuotaWindowKind::Weekly);
        assert_eq!(weekly.used_percent, 22.0);
        assert_eq!(
            weekly.resets_at,
            Some(
                DateTime::parse_from_rfc3339("2026-07-20T00:00:00Z")
                    .expect("fixture reset")
                    .with_timezone(&Utc)
                    .to_rfc3339()
            )
        );
        assert!(!weekly.is_primary);

        let opus = &parsed.snapshot.windows[2];
        assert_eq!(opus.id, "claude.model.claude-opus-4-1");
        assert_eq!(opus.kind, QuotaWindowKind::ModelWeekly);
        assert_eq!(opus.display_name.as_deref(), Some("Opus 4.1"));
        assert_eq!(opus.used_percent, 30.0);
        assert!(opus.resets_at.is_some(), "legacy Opus reset fills the gap");

        let fable = &parsed.snapshot.windows[3];
        assert_eq!(fable.id, "claude.model.fable");
        assert_eq!(fable.display_name.as_deref(), Some("Fable"));
        assert!(!fable.is_active);
    }

    #[test]
    fn legacy_only_fixture_maps_all_supported_windows() {
        let parsed = parse_usage_response(LEGACY_ONLY, captured_at()).expect("fixture parses");

        assert_eq!(parsed.snapshot.windows.len(), 4);
        assert_eq!(
            parsed
                .snapshot
                .windows
                .iter()
                .map(|window| window.kind)
                .collect::<Vec<_>>(),
            vec![
                QuotaWindowKind::FiveHour,
                QuotaWindowKind::Weekly,
                QuotaWindowKind::ModelWeekly,
                QuotaWindowKind::ModelWeekly,
            ]
        );
        assert!(parsed.snapshot.windows[0].is_primary);
        assert_eq!(
            parsed.snapshot.windows[1].resets_at,
            Some(
                DateTime::from_timestamp(1_784_505_600, 0)
                    .expect("fixture reset")
                    .to_rfc3339()
            )
        );
        assert_eq!(parsed.snapshot.windows[2].id, "claude.model.opus");
        assert_eq!(parsed.snapshot.windows[3].id, "claude.model.sonnet");
        assert_eq!(parsed.snapshot.windows[3].remaining_percent, 0.0);
    }

    #[test]
    fn scoped_ids_are_stable_duplicate_ids_replace_and_unknown_kind_is_kept() {
        let parsed =
            parse_usage_response(SCOPED_AND_UNKNOWN, captured_at()).expect("fixture parses");

        assert_eq!(parsed.snapshot.windows.len(), 5);
        let weekly = &parsed.snapshot.windows[0];
        assert_eq!(weekly.kind, QuotaWindowKind::Weekly);
        assert!(
            weekly.is_primary,
            "weekly is primary when session is absent"
        );

        let opus = &parsed.snapshot.windows[1];
        assert_eq!(opus.id, "claude.model.model-opus-4-1");
        assert_eq!(opus.display_name.as_deref(), Some("Opus Latest"));
        assert_eq!(opus.used_percent, 55.0);

        let surface = &parsed.snapshot.windows[2];
        assert_eq!(surface.id, "claude.model.surface-review");
        assert_eq!(surface.display_name.as_deref(), Some("Code Review"));

        let named_surface = &parsed.snapshot.windows[3];
        assert_eq!(named_surface.id, "claude.model.terminal");
        assert_eq!(named_surface.display_name.as_deref(), Some("terminal"));

        let unknown = &parsed.snapshot.windows[4];
        assert_eq!(unknown.id, "claude.unknown.monthly-beta");
        assert_eq!(unknown.kind, QuotaWindowKind::Unknown);
        assert_eq!(unknown.window_seconds, None);
        assert_eq!(
            unknown.resets_at,
            Some(
                DateTime::from_timestamp(1_800_000_000, 0)
                    .expect("fixture reset")
                    .to_rfc3339()
            )
        );
        assert!(!unknown.is_primary);
    }

    #[test]
    fn the_first_returned_window_stays_primary_even_when_inactive() {
        let input = r#"{
            "limits": [
                {"kind":"session","percent":15,"is_active":false},
                {"kind":"weekly_all","percent":25,"is_active":true}
            ]
        }"#;
        let parsed = parse_usage_response(input, captured_at()).expect("response parses");

        assert!(!parsed.snapshot.windows[0].is_active);
        assert!(parsed.snapshot.windows[0].is_primary);
        assert!(!parsed.snapshot.windows[1].is_primary);
    }

    #[test]
    fn percentages_are_clamped_without_rewriting_provider_values() {
        let input = r#"{
            "limits": [
                {"kind":"session","percent":-10},
                {"kind":"weekly_all","percent":140}
            ]
        }"#;
        let parsed = parse_usage_response(input, captured_at()).expect("response parses");

        assert_eq!(parsed.snapshot.windows[0].used_percent, -10.0);
        assert_eq!(parsed.snapshot.windows[0].remaining_percent, 100.0);
        assert_eq!(parsed.snapshot.windows[1].used_percent, 140.0);
        assert_eq!(parsed.snapshot.windows[1].remaining_percent, 0.0);
    }

    #[test]
    fn response_shape_errors_are_classified_without_returning_raw_content() {
        for (input, expected) in [
            ("not json", ClaudeUsageParseError::InvalidResponse),
            ("{}", ClaudeUsageParseError::MissingQuotaWindows),
            (
                r#"{"limits":[{"percent":10}]}"#,
                ClaudeUsageParseError::MissingLimitKind,
            ),
            (
                r#"{"limits":[{"kind":"session"}]}"#,
                ClaudeUsageParseError::MissingUsedPercent,
            ),
            (
                r#"{"limits":[{"kind":"weekly_scoped","percent":10,"scope":null}]}"#,
                ClaudeUsageParseError::MissingScopedIdentity,
            ),
        ] {
            assert_eq!(parse_usage_response(input, captured_at()), Err(expected));
        }
    }

    #[test]
    fn invalid_reset_is_a_protocol_error_instead_of_becoming_a_fake_time() {
        let input = r#"{
            "limits": [
                {"kind":"session","percent":10,"resets_at":"not-a-time"}
            ]
        }"#;

        assert_eq!(
            parse_usage_response(input, captured_at()),
            Err(ClaudeUsageParseError::InvalidResetTime)
        );
    }
}
