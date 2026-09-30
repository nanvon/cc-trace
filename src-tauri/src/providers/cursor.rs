//! Cursor 额度来源：只读登录态 → Dashboard `usage-summary` → 标准化额度窗口。
//!
//! Cursor 没有面向个人套餐的公开稳定额度 API，这里集中处理 Dashboard 的非公开响应；
//! 端点与字段映射对齐 cc-bar v1.1.1 的 `CursorQuotaClient`。凭据只读采用，不写回。
//!
//! 三档口径：`Total` 是套餐总量，`Auto` 与 `API` 是两类模型的用量桶。
//! 服务端可以只给其中一部分——缺哪一档就不显示哪一档，不拿别的档位顶替。

use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde_json::Value;
use tokio::sync::Mutex;

use super::credentials::cursor::{self, CursorCredentials};
use super::credentials::{self as credentials_mod, Discovery};
use super::{BoxFuture, ProviderFetchOutcome, QuotaProvider, http};
use crate::contracts::{
    ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot, QuotaWindow, QuotaWindowKind,
};

const ENDPOINT: &str = "https://cursor.com/api/usage-summary";

#[derive(Clone)]
pub struct CursorProvider {
    refresh_lock: Arc<Mutex<()>>,
}

impl std::fmt::Debug for CursorProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CursorProvider").finish()
    }
}

impl CursorProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }
}

impl QuotaProvider for CursorProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Cursor
    }

    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome> {
        Box::pin(async move {
            let _guard = self.refresh_lock.lock().await;
            self.fetch_once().await
        })
    }
}

/// 一次取数的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCursorUsage {
    pub identity: Option<ProviderIdentity>,
    pub identity_key: Option<String>,
    pub snapshot: QuotaSnapshot,
}

impl CursorProvider {
    async fn fetch_once(&self) -> ProviderFetchOutcome {
        let credentials = match cursor::discover() {
            Discovery::Found(credentials) => credentials,
            Discovery::Missing => return ProviderFetchOutcome::NoCredentials,
            Discovery::Unsupported => return ProviderFetchOutcome::Unsupported,
            // 令牌过期与读不出来都要用户去 Cursor 重新登录或检查权限，
            // 不能报成「没有登录过」。
            Discovery::Unreadable | Discovery::Expired => {
                return ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Credentials,
                };
            }
        };

        match fetch_usage(&credentials, Utc::now()).await {
            Ok(parsed) => ProviderFetchOutcome::Success {
                identity: parsed.identity,
                identity_key: parsed.identity_key,
                snapshot: parsed.snapshot,
            },
            Err(outcome) => outcome,
        }
    }
}

pub async fn fetch_usage(
    credentials: &CursorCredentials,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedCursorUsage, ProviderFetchOutcome> {
    let response = http::client()
        .get(ENDPOINT)
        .header(reqwest::header::ACCEPT, "application/json")
        .header(
            reqwest::header::COOKIE,
            credentials.cookie_header().expose(),
        )
        .send()
        .await
        .map_err(|error| http::classify_transport(&error))?;
    if let Some(failure) = http::classify_response(&response) {
        return Err(failure);
    }
    let body = response
        .text()
        .await
        .map_err(|error| http::classify_transport(&error))?;
    let root: Value = serde_json::from_str(&body).map_err(|_| ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    })?;

    let mut parsed = parse_usage(&root, fetched_at)?;
    parsed.identity = Some(ProviderIdentity {
        account: credentials.email.clone(),
        plan: parsed.identity.and_then(|identity| identity.plan),
        credential_source: None,
    });
    parsed.identity_key =
        credentials_mod::identity_fingerprint("cursor", &[credentials.account_key().as_ref()]);
    Ok(parsed)
}

/// 把 `usage-summary` 响应标准化成额度快照。
///
/// Total 的取值优先级与 cc-bar 一致：团队套餐看 `teamUsage.pooled`，
/// 个人套餐先看 `individualUsage.plan.totalPercentUsed`，再看各类 `used/limit` 比率。
pub fn parse_usage(
    root: &Value,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedCursorUsage, ProviderFetchOutcome> {
    let individual = root.get("individualUsage");
    let plan = individual.and_then(|value| value.get("plan"));
    let overall = individual.and_then(|value| value.get("overall"));
    let pooled = root.get("teamUsage").and_then(|value| value.get("pooled"));

    let cycle_start = date(root.get("billingCycleStart"));
    let cycle_end = date(root.get("billingCycleEnd"));
    let window_seconds = cycle_duration(cycle_start, cycle_end);
    let resets_at = cycle_end.map(iso);
    let is_unlimited = root
        .get("isUnlimited")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let limit_type = string(root.get("limitType")).map(|value| value.to_lowercase());
    let total_used_percent = if limit_type.as_deref() == Some("team") {
        ratio_percent(pooled)
    } else {
        number(plan.and_then(|value| value.get("totalPercentUsed")))
            .or_else(|| ratio_percent(plan))
            .or_else(|| ratio_percent(overall))
            .or_else(|| ratio_percent(pooled))
    };

    let mut windows = Vec::new();
    if let Some(used_percent) = total_used_percent {
        windows.push(window(
            "cursor-total",
            "Total",
            QuotaWindowKind::Total,
            used_percent,
            resets_at.clone(),
            window_seconds,
            is_unlimited,
        ));
    }
    if let Some(used_percent) = number(plan.and_then(|value| value.get("autoPercentUsed"))) {
        windows.push(window(
            "cursor-auto",
            "Auto",
            QuotaWindowKind::Auto,
            used_percent,
            resets_at.clone(),
            window_seconds,
            false,
        ));
    }
    if let Some(used_percent) = number(plan.and_then(|value| value.get("apiPercentUsed"))) {
        windows.push(window(
            "cursor-api",
            "API",
            QuotaWindowKind::Api,
            used_percent,
            resets_at,
            window_seconds,
            false,
        ));
    }

    if windows.is_empty() {
        // 响应里一档用量都没有：结构不符预期，不是「用量为零」。
        return Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Protocol,
        });
    }
    for (index, window) in windows.iter_mut().enumerate() {
        window.is_primary = index == 0;
    }

    Ok(ParsedCursorUsage {
        identity: Some(ProviderIdentity {
            account: None,
            plan: formatted_plan_type(string(root.get("membershipType"))),
            credential_source: None,
        }),
        identity_key: None,
        snapshot: QuotaSnapshot {
            windows,
            captured_at: iso(fetched_at),
        },
    })
}

#[allow(clippy::too_many_arguments)]
fn window(
    id: &str,
    display_name: &str,
    kind: QuotaWindowKind,
    used_percent: f64,
    resets_at: Option<String>,
    window_seconds: Option<u64>,
    unlimited: bool,
) -> QuotaWindow {
    let used_percent = used_percent.clamp(0.0, 100.0);
    QuotaWindow {
        id: id.to_owned(),
        kind,
        display_name: Some(display_name.to_owned()),
        used_percent,
        remaining_percent: QuotaWindow::normalized_remaining(used_percent),
        resets_at,
        window_seconds,
        is_active: true,
        is_primary: false,
        unlimited,
    }
}

/// `{ used, limit }` 桶换算成百分比。`enabled` 为 false 或 limit 非正时返回 `None`：
/// 那是「这一档不适用」，不是「用量为零」。
fn ratio_percent(bucket: Option<&Value>) -> Option<f64> {
    let bucket = bucket?;
    if bucket.get("enabled").and_then(Value::as_bool) == Some(false) {
        return None;
    }
    let used = number(bucket.get("used"))?;
    let limit = number(bucket.get("limit"))?;
    if limit <= 0.0 {
        return None;
    }
    let percent = used / limit * 100.0;
    percent.is_finite().then_some(percent)
}

/// 数字字段可能是 number 也可能是字符串。布尔值不参与数值转换：
/// `true` 被当成 1 会让 Total 显示成 1%，比缺失更难发现。
fn number(value: Option<&Value>) -> Option<f64> {
    let parsed = match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    parsed.is_finite().then_some(parsed)
}

fn string(value: Option<&Value>) -> Option<String> {
    let text = value?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn date(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let text = string(value)?;
    DateTime::parse_from_rfc3339(&text)
        .ok()
        .map(|time| time.with_timezone(&Utc))
}

fn iso(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn cycle_duration(start: Option<DateTime<Utc>>, end: Option<DateTime<Utc>>) -> Option<u64> {
    let (start, end) = (start?, end?);
    let seconds = (end - start).num_seconds();
    (seconds > 0).then(|| u64::try_from(seconds).unwrap_or_default())
}

/// `pro_plus` / `business-annual` 这类取值转成可展示的套餐名。
fn formatted_plan_type(raw: Option<String>) -> Option<String> {
    let raw = raw?;
    let words: Vec<String> = raw
        .split(['_', '-', ' '])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            match characters.next() {
                Some(first) => {
                    first.to_uppercase().collect::<String>() + &characters.as_str().to_lowercase()
                }
                None => String::new(),
            }
        })
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .expect("valid")
            .with_timezone(&Utc)
    }

    fn response() -> Value {
        serde_json::json!({
            "individualUsage": {
                "plan": {
                    "totalPercentUsed": 33.5,
                    "autoPercentUsed": 12,
                    "apiPercentUsed": 0
                }
            },
            "billingCycleStart": "2026-09-15T00:00:00Z",
            "billingCycleEnd": "2026-10-15T00:00:00Z",
            "membershipType": "pro_plus"
        })
    }

    #[test]
    fn parses_three_buckets_in_product_order() {
        let parsed = parse_usage(&response(), fetched_at()).expect("parses");

        let ids: Vec<&str> = parsed
            .snapshot
            .windows
            .iter()
            .map(|window| window.id.as_str())
            .collect();
        assert_eq!(ids, ["cursor-total", "cursor-auto", "cursor-api"]);

        let total = &parsed.snapshot.windows[0];
        assert_eq!(total.kind, QuotaWindowKind::Total);
        assert!((total.used_percent - 33.5).abs() < f64::EPSILON);
        assert!(total.is_primary);
        assert_eq!(total.resets_at.as_deref(), Some("2026-10-15T00:00:00Z"));
        assert_eq!(total.window_seconds, Some(2_592_000));

        // 0% 也要显示：它是真实的读数，不是缺失。
        let api = &parsed.snapshot.windows[2];
        assert_eq!(api.kind, QuotaWindowKind::Api);
        assert_eq!(api.used_percent, 0.0);

        assert_eq!(
            parsed.identity.and_then(|value| value.plan).as_deref(),
            Some("Pro Plus")
        );
    }

    #[test]
    fn team_plans_read_the_pooled_bucket() {
        let mut payload = response();
        payload["limitType"] = serde_json::json!("team");
        payload["teamUsage"] = serde_json::json!({ "pooled": { "used": 250, "limit": 1000 } });

        let parsed = parse_usage(&payload, fetched_at()).expect("parses");
        assert!((parsed.snapshot.windows[0].used_percent - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn percentage_fields_fall_back_to_used_over_limit_ratios() {
        let payload = serde_json::json!({
            "individualUsage": { "plan": { "used": 30, "limit": 120 } }
        });

        let parsed = parse_usage(&payload, fetched_at()).expect("parses");
        assert_eq!(parsed.snapshot.windows.len(), 1);
        assert!((parsed.snapshot.windows[0].used_percent - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_disabled_bucket_is_not_a_zero_percent_window() {
        let payload = serde_json::json!({
            "individualUsage": {
                "plan": { "totalPercentUsed": 10 },
                "overall": { "enabled": false, "used": 0, "limit": 100 }
            }
        });

        let parsed = parse_usage(&payload, fetched_at()).expect("parses");
        assert_eq!(parsed.snapshot.windows.len(), 1);
        assert_eq!(parsed.snapshot.windows[0].id, "cursor-total");
    }

    #[test]
    fn an_unlimited_plan_marks_the_total_window() {
        let payload = serde_json::json!({
            "individualUsage": { "plan": { "totalPercentUsed": 0 } },
            "isUnlimited": true
        });

        let parsed = parse_usage(&payload, fetched_at()).expect("parses");
        assert!(parsed.snapshot.windows[0].unlimited);
    }

    #[test]
    fn booleans_are_never_coerced_into_percentages() {
        let payload = serde_json::json!({
            "individualUsage": {
                "plan": { "totalPercentUsed": true, "autoPercentUsed": 5 }
            }
        });

        let parsed = parse_usage(&payload, fetched_at()).expect("parses");
        let ids: Vec<&str> = parsed
            .snapshot
            .windows
            .iter()
            .map(|window| window.id.as_str())
            .collect();
        assert_eq!(ids, ["cursor-auto"]);
    }

    #[test]
    fn an_unexpected_shape_is_a_protocol_error_not_an_empty_quota() {
        let payload = serde_json::json!({ "individualUsage": {} });
        assert!(matches!(
            parse_usage(&payload, fetched_at()),
            Err(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol
            })
        ));
    }

    #[test]
    fn percentages_outside_the_range_are_clamped() {
        let payload = serde_json::json!({
            "individualUsage": { "plan": { "totalPercentUsed": 240, "apiPercentUsed": -5 } }
        });

        let parsed = parse_usage(&payload, fetched_at()).expect("parses");
        assert_eq!(parsed.snapshot.windows[0].used_percent, 100.0);
        assert_eq!(parsed.snapshot.windows[1].used_percent, 0.0);
    }

    #[test]
    fn plan_types_are_formatted_for_display() {
        assert_eq!(
            formatted_plan_type(Some("pro_plus".to_owned())).as_deref(),
            Some("Pro Plus")
        );
        assert_eq!(
            formatted_plan_type(Some("business-annual".to_owned())).as_deref(),
            Some("Business Annual")
        );
        assert_eq!(formatted_plan_type(Some("   ".to_owned())).as_deref(), None);
    }
}
