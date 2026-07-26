//! Codex Usage API 的离线响应解析与标准化。
//!
//! 本模块只处理已取得的 JSON 字符串：不发现凭据、不读取文件、不发起网络请求，也不
//! 持有 token。输入按 `docs/额度领域模型.md` 第 3.1 节映射为现有脱敏 contract，
//! 真实 Provider 接入后仍由同一个解析入口产出 [`QuotaSnapshot`]。

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;

use crate::contracts::{ProviderIdentity, QuotaSnapshot, QuotaWindow, QuotaWindowKind};

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
