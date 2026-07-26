//! Claude Code OAuth Usage API 的离线响应解析与标准化。
//!
//! 本模块只处理已取得的 JSON 字符串：不发现凭据、不读取文件、不发起网络请求，也不
//! 持有 token。输入按 `docs/额度领域模型.md` 第 3.2 节映射为现有脱敏 contract，
//! 真实 Provider 接入后仍由同一个解析入口产出 [`QuotaSnapshot`]。

use chrono::{DateTime, Duration, Utc};
use serde::Deserialize;
use serde_json::Value;

use crate::contracts::{QuotaSnapshot, QuotaWindow, QuotaWindowKind};

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

    // 非生效窗口仍保留供完整展示，但不能进入主要额度判断。
    if let Some(primary_index) = windows
        .iter()
        .position(|window| window.is_active && window.kind == QuotaWindowKind::FiveHour)
        .or_else(|| {
            windows
                .iter()
                .position(|window| window.is_active && window.kind == QuotaWindowKind::Weekly)
        })
    {
        windows[primary_index].is_primary = true;
    }

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
    fn inactive_session_is_retained_but_active_weekly_becomes_primary() {
        let input = r#"{
            "limits": [
                {"kind":"session","percent":15,"is_active":false},
                {"kind":"weekly_all","percent":25,"is_active":true}
            ]
        }"#;
        let parsed = parse_usage_response(input, captured_at()).expect("response parses");

        assert!(!parsed.snapshot.windows[0].is_active);
        assert!(!parsed.snapshot.windows[0].is_primary);
        assert!(parsed.snapshot.windows[1].is_primary);
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
