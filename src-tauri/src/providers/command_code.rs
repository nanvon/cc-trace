//! Command Code 额度来源：whoami → credits + subscriptions → 标准化额度窗口。
//!
//! 端点与字段映射对齐 cc-bar v1.1.1 的 `CommandCodeQuotaClient`：
//! `/alpha/whoami` 取账号与组织，`/alpha/billing/credits` 取窗口与月度 credits，
//! `/alpha/billing/subscriptions` 取套餐名。credits 与 subscriptions 并发发起，
//! subscriptions 失败不影响窗口展示（套餐名缺失比额度缺失轻得多）。
//!
//! 凭据来源见 [`super::credentials::command_code`]：五个来源依次尝试。

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use tokio::sync::Mutex;

use super::credentials::command_code::{self, CommandCodeCredentials};
use super::credentials::{self, Discovery, Secret};
use super::{BoxFuture, ProviderFetchOutcome, QuotaProvider, http};
use crate::contracts::{
    CommandCodeCredentialPreference, ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot,
    QuotaWindow, QuotaWindowKind, Settings,
};
use std::sync::Mutex as StdMutex;

const BASE_URL: &str = "https://api.commandcode.ai";
/// GOAT 套餐的月度 credits 上限。服务端只回「还剩多少」，容量是产品侧常量。
const GOAT_MONTHLY_CAP: f64 = 70.0;
const FIVE_HOUR_SECONDS: u64 = 18_000;
const WEEKLY_SECONDS: u64 = 604_800;
const MONTHLY_SECONDS: u64 = 30 * 86_400;

/// Command Code 额度来源。
#[derive(Clone)]
pub struct CommandCodeProvider {
    refresh_lock: Arc<Mutex<()>>,
    /// 凭据偏好的唯一真值在设置里。这里只读，不缓存：用户改成「手动」后
    /// 下一次刷新就该按新偏好走，不需要重启。
    settings: Arc<StdMutex<Settings>>,
}

impl std::fmt::Debug for CommandCodeProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("CommandCodeProvider").finish()
    }
}

impl CommandCodeProvider {
    pub fn new(settings: Arc<StdMutex<Settings>>) -> Arc<Self> {
        Arc::new(Self {
            refresh_lock: Arc::new(Mutex::new(())),
            settings,
        })
    }
}

impl QuotaProvider for CommandCodeProvider {
    fn id(&self) -> ProviderId {
        ProviderId::CommandCode
    }

    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome> {
        Box::pin(async move {
            let _guard = self.refresh_lock.lock().await;
            self.fetch_once().await
        })
    }
}

/// 一次取数的结果：标准化快照 + 身份。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedCommandCodeUsage {
    pub identity: Option<ProviderIdentity>,
    pub identity_key: Option<String>,
    pub snapshot: QuotaSnapshot,
}

impl CommandCodeProvider {
    async fn fetch_once(&self) -> ProviderFetchOutcome {
        let preference = self.credential_preference();
        let credentials = match command_code::discover(preference) {
            Discovery::Found(credentials) => credentials,
            Discovery::Missing => return ProviderFetchOutcome::NoCredentials,
            Discovery::Unsupported => return ProviderFetchOutcome::Unsupported,
            Discovery::Unreadable => {
                return ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Credentials,
                };
            }
        };

        match fetch_usage(&credentials).await {
            Ok(parsed) => ProviderFetchOutcome::Success {
                identity: parsed.identity,
                identity_key: parsed.identity_key,
                snapshot: parsed.snapshot,
            },
            Err(outcome) => outcome,
        }
    }

    /// 凭据偏好来自设置。锁被毒化（另一个线程 panic）时按「自动」处理：
    /// 手动模式是用户显式选择，读不到设置时按自动更接近默认预期，也比整块失败好。
    fn credential_preference(&self) -> CommandCodeCredentialPreference {
        match self.settings.lock() {
            Ok(settings) => settings.command_code_credential,
            Err(poisoned) => poisoned.into_inner().command_code_credential,
        }
    }
}

/// 取一次额度。whoami 失败即整体失败；subscriptions 失败只丢套餐名。
pub async fn fetch_usage(
    credentials: &CommandCodeCredentials,
) -> Result<ParsedCommandCodeUsage, ProviderFetchOutcome> {
    let whoami = request("/alpha/whoami", &[], credentials).await?;
    let whoami_root = unwrap_data(&whoami);
    let user = whoami_root.get("user");
    let org = whoami_root.get("org");
    let login = string_at(user, "userName");
    let email = string_at(user, "email");
    let name = string_at(user, "name");
    let org_id = string_at(org, "id");

    let query: Vec<(&str, String)> = match org_id.as_deref() {
        Some(org_id) if !org_id.trim().is_empty() => vec![("orgId", org_id.to_owned())],
        _ => Vec::new(),
    };

    let (credits, subscriptions) = tokio::join!(
        request("/alpha/billing/credits", &query, credentials),
        request("/alpha/billing/subscriptions", &[], credentials),
    );
    let credits = credits?;
    let subscriptions = subscriptions.ok();

    let mut parsed = parse_usage(
        unwrap_data(&credits),
        subscriptions.as_ref().map(unwrap_data),
        Utc::now(),
    )?;
    parsed.identity = Some(ProviderIdentity {
        account: email.clone().or_else(|| login.clone()).or(name),
        plan: parsed.identity.and_then(|identity| identity.plan),
        credential_source: None,
    });
    parsed.identity_key = Some(
        credentials::identity_fingerprint(
            "command-code",
            &[
                org_id.as_deref().map(Secret::new).as_ref(),
                login.as_deref().map(Secret::new).as_ref(),
                email.as_deref().map(Secret::new).as_ref(),
            ],
        )
        .unwrap_or_else(|| {
            // 三个身份字段都缺失时退回到令牌派生的账号键：它与 cc-bar 的 accountKey 同构，
            // 仍能区分不同令牌，只是不承诺跨登录稳定。
            let key = command_code::account_key(None, None, None, &credentials.access_token);
            credentials::identity_fingerprint("command-code", &[Some(&Secret::new(key))])
                .unwrap_or_default()
        }),
    );

    Ok(parsed)
}

/// 服务端可能把载荷包在 `data` 里，也可能直接平铺。
fn unwrap_data(value: &Value) -> &Value {
    value.get("data").unwrap_or(value)
}

fn string_at(value: Option<&Value>, key: &str) -> Option<String> {
    let text = value?.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

async fn request(
    path: &str,
    query: &[(&str, String)],
    credentials: &CommandCodeCredentials,
) -> Result<Value, ProviderFetchOutcome> {
    let mut request = http::client()
        .get(format!("{BASE_URL}{path}"))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", credentials.access_token.expose()),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "cc-trace");
    if !query.is_empty() {
        request = request.query(query);
    }

    let response = request
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
    serde_json::from_str(&body).map_err(|_| ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    })
}

/// 把 credits 与 subscriptions 响应标准化成额度快照。
///
/// 与 cc-bar 一致的两处细节：5h／weekly 的容量为 0 或缺失时整条窗口不出现（宁缺不猜）；
/// used 为 0 且重置时间缺失或已过时按窗口长度预估下一次重置（否则界面会显示一个
/// 已经过去的重置时间）。
pub fn parse_usage(
    credits_root: &Value,
    subscriptions_root: Option<&Value>,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedCommandCodeUsage, ProviderFetchOutcome> {
    let plan = format_plan_id(string_at(subscriptions_root, "planId").as_deref());

    let window_limits = credits_root.get("windowLimits");
    let five_hour = window_limits
        .and_then(|value| value.get("fiveHour"))
        .or_else(|| credits_root.get("fiveHour"));
    let weekly = window_limits
        .and_then(|value| value.get("weekly"))
        .or_else(|| credits_root.get("weekly"));

    let mut windows = Vec::new();
    if let Some(window) = parse_window(
        five_hour,
        "command-code-five-hour",
        "5HOUR",
        QuotaWindowKind::FiveHour,
        FIVE_HOUR_SECONDS,
        fetched_at,
    ) {
        windows.push(window);
    }
    if let Some(window) = parse_window(
        weekly,
        "command-code-weekly",
        "WEEKLY",
        QuotaWindowKind::Weekly,
        WEEKLY_SECONDS,
        fetched_at,
    ) {
        windows.push(window);
    }

    let monthly_raw = credits_root
        .get("credits")
        .and_then(|value| value.get("monthlyCredits"))
        .or_else(|| credits_root.get("monthlyCredits"));
    if let Some(remaining) = monthly_raw.and_then(as_number)
        && plan.as_deref() == Some("GOAT")
    {
        let used = (GOAT_MONTHLY_CAP - remaining).max(0.0);
        let used_percent = (used / GOAT_MONTHLY_CAP * 100.0).clamp(0.0, 100.0);
        windows.push(QuotaWindow {
            id: "command-code-monthly".to_owned(),
            kind: QuotaWindowKind::Monthly,
            display_name: Some("MONTHLY".to_owned()),
            used_percent,
            remaining_percent: QuotaWindow::normalized_remaining(used_percent),
            resets_at: string_at(subscriptions_root, "currentPeriodEnd"),
            window_seconds: Some(MONTHLY_SECONDS),
            is_active: false,
            is_primary: false,
            unlimited: false,
        });
    }

    for (index, window) in windows.iter_mut().enumerate() {
        window.is_primary = index == 0;
    }

    if windows.is_empty() {
        // 一个窗口都没有说明响应结构与预期不符，而不是「额度用完了」。
        return Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Protocol,
        });
    }

    Ok(ParsedCommandCodeUsage {
        identity: Some(ProviderIdentity {
            account: None,
            plan,
            credential_source: None,
        }),
        identity_key: None,
        snapshot: QuotaSnapshot {
            windows,
            captured_at: fetched_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
    })
}

fn parse_window(
    value: Option<&Value>,
    id: &str,
    display_name: &str,
    kind: QuotaWindowKind,
    window_seconds: u64,
    fetched_at: DateTime<Utc>,
) -> Option<QuotaWindow> {
    let value = value?;
    let cap = value.get("cap").and_then(as_number)?;
    if cap <= 0.0 {
        return None;
    }
    let used = value.get("used").and_then(as_number).unwrap_or(0.0);
    let used_percent = (used / cap * 100.0).clamp(0.0, 100.0);

    let mut resets_at = value.get("resetAt").and_then(as_reset_time);
    let cycle_not_started = used_percent == 0.0
        && resets_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .is_none_or(|reset| reset <= fetched_at);
    if cycle_not_started {
        resets_at = Some(
            (fetched_at + chrono::Duration::seconds(window_seconds as i64))
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        );
    }

    Some(QuotaWindow {
        id: id.to_owned(),
        kind,
        display_name: Some(display_name.to_owned()),
        used_percent,
        remaining_percent: QuotaWindow::normalized_remaining(used_percent),
        resets_at,
        window_seconds: Some(window_seconds),
        is_active: true,
        is_primary: false,
        unlimited: false,
    })
}

/// 数字字段在服务端可能是 number 也可能是字符串。返回 `None` 表示「不是数字」，
/// 与 0 区分开——`cap` 缺失与 `cap = 0` 的处理相同，但 `used` 缺失要当 0。
fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// 重置时间可能是秒、毫秒或 ISO 8601 字符串。
fn as_reset_time(value: &Value) -> Option<String> {
    if let Some(number) = as_number(value) {
        if number <= 0.0 {
            return None;
        }
        let seconds = if number > 1_000_000_000_000.0 {
            number / 1000.0
        } else {
            number
        };
        let millis = (seconds * 1000.0).round();
        let millis = i64::try_from(millis as i128).ok()?;
        return Utc
            .timestamp_millis_opt(millis)
            .single()
            .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
    }
    let text = value.as_str()?.trim();
    let parsed = DateTime::parse_from_rfc3339(text).ok()?;
    (parsed.timestamp() > 0).then(|| {
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    })
}

/// 套餐名归一：服务端给的是 `goat`、`pro-monthly` 这类 id。
fn format_plan_id(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    let lower = raw.to_lowercase();
    if lower.contains("goat") {
        return Some("GOAT".to_owned());
    }
    if lower.contains("pro") {
        return Some("Pro".to_owned());
    }
    if lower.contains("team") {
        return Some("Team".to_owned());
    }
    if lower.contains("enterprise") {
        return Some("Enterprise".to_owned());
    }
    let mut characters = raw.chars();
    characters
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + characters.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0)
            .single()
            .expect("valid time")
    }

    /// 服务端把载荷包在 `data` 里，并带一个正数的 5h 重置时间。
    fn credits_fixture() -> Value {
        serde_json::json!({
            "data": {
                "windowLimits": {
                    "fiveHour": { "cap": 20, "used": 5, "resetAt": 1_790_000_000_000_u64 },
                    "weekly": { "cap": 200, "used": 50 }
                },
                "credits": { "monthlyCredits": 40 }
            }
        })
    }

    #[test]
    fn parses_three_windows_with_the_expected_percentages() {
        let subscriptions = serde_json::json!({ "data": { "planId": "goat-monthly" } });
        let parsed = parse_usage(
            unwrap_data(&credits_fixture()),
            Some(unwrap_data(&subscriptions)),
            fetched_at(),
        )
        .expect("parses");

        let ids: Vec<&str> = parsed
            .snapshot
            .windows
            .iter()
            .map(|window| window.id.as_str())
            .collect();
        assert_eq!(
            ids,
            [
                "command-code-five-hour",
                "command-code-weekly",
                "command-code-monthly"
            ]
        );

        let five_hour = &parsed.snapshot.windows[0];
        assert_eq!(five_hour.kind, QuotaWindowKind::FiveHour);
        assert!((five_hour.used_percent - 25.0).abs() < f64::EPSILON);
        assert!(five_hour.is_primary);
        // 重置时间是毫秒时间戳，换算成秒。
        assert_eq!(five_hour.resets_at.as_deref(), Some("2026-09-21T14:13:20Z"));

        let weekly = &parsed.snapshot.windows[1];
        assert!((weekly.used_percent - 25.0).abs() < f64::EPSILON);
        assert!(!weekly.is_primary);
        // 已经用了一部分却没有重置时间：不猜，界面显示「重置时间未知」。
        assert_eq!(weekly.resets_at, None);

        let monthly = &parsed.snapshot.windows[2];
        assert_eq!(monthly.kind, QuotaWindowKind::Monthly);
        // 70 容量、还剩 40 → 已用 30/70。
        assert!((monthly.used_percent - (30.0 / 70.0 * 100.0)).abs() < 1e-9);
        assert_eq!(
            parsed.identity.and_then(|value| value.plan).as_deref(),
            Some("GOAT")
        );
    }

    #[test]
    fn a_zero_capacity_window_is_omitted_instead_of_guessed() {
        let credits = serde_json::json!({
            "fiveHour": { "cap": 0, "used": 0 },
            "weekly": { "cap": 100, "used": 10 }
        });

        let parsed = parse_usage(&credits, None, fetched_at()).expect("parses");
        assert_eq!(parsed.snapshot.windows.len(), 1);
        assert_eq!(parsed.snapshot.windows[0].id, "command-code-weekly");
    }

    #[test]
    fn flat_and_nested_payloads_are_both_accepted() {
        let nested = serde_json::json!({
            "windowLimits": { "weekly": { "cap": 10, "used": 1 } }
        });
        let flat = serde_json::json!({ "weekly": { "cap": 10, "used": 1 } });

        for payload in [nested, flat] {
            let parsed = parse_usage(&payload, None, fetched_at()).expect("parses");
            assert_eq!(parsed.snapshot.windows.len(), 1);
            assert_eq!(parsed.snapshot.windows[0].id, "command-code-weekly");
        }
    }

    #[test]
    fn an_empty_response_is_a_protocol_error_not_an_empty_quota() {
        let outcome = parse_usage(&serde_json::json!({}), None, fetched_at());
        assert!(matches!(
            outcome,
            Err(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol
            })
        ));
    }

    #[test]
    fn a_fresh_cycle_without_a_reset_time_is_estimated_from_the_window() {
        let credits = serde_json::json!({
            "windowLimits": { "fiveHour": { "cap": 20, "used": 0 } }
        });

        let parsed = parse_usage(&credits, None, fetched_at()).expect("parses");
        assert_eq!(
            parsed.snapshot.windows[0].resets_at.as_deref(),
            Some("2026-10-01T17:00:00Z")
        );
    }

    #[test]
    fn a_stale_reset_time_on_a_fresh_cycle_is_replaced() {
        let credits = serde_json::json!({
            "windowLimits": {
                "weekly": { "cap": 20, "used": 0, "resetAt": "2026-09-01T00:00:00Z" }
            }
        });

        let parsed = parse_usage(&credits, None, fetched_at()).expect("parses");
        assert_eq!(
            parsed.snapshot.windows[0].resets_at.as_deref(),
            Some("2026-10-08T12:00:00Z")
        );
    }

    #[test]
    fn numeric_fields_accept_strings() {
        let credits = serde_json::json!({
            "windowLimits": { "fiveHour": { "cap": "20", "used": "5" } }
        });
        let parsed = parse_usage(&credits, None, fetched_at()).expect("parses");
        assert!((parsed.snapshot.windows[0].used_percent - 25.0).abs() < f64::EPSILON);
    }

    #[test]
    fn plan_ids_are_normalized() {
        assert_eq!(format_plan_id(Some("goat")).as_deref(), Some("GOAT"));
        assert_eq!(format_plan_id(Some("pro-monthly")).as_deref(), Some("Pro"));
        assert_eq!(format_plan_id(Some("team")).as_deref(), Some("Team"));
        assert_eq!(format_plan_id(Some("weird")).as_deref(), Some("Weird"));
        assert_eq!(format_plan_id(Some("  ")).as_deref(), None);
        assert_eq!(format_plan_id(None).as_deref(), None);
    }

    #[test]
    fn a_second_precision_timestamp_is_not_treated_as_milliseconds() {
        let credits = serde_json::json!({
            "windowLimits": {
                "fiveHour": { "cap": 10, "used": 5, "resetAt": 1_790_000_000_u64 }
            }
        });
        let parsed = parse_usage(&credits, None, fetched_at()).expect("parses");
        assert_eq!(
            parsed.snapshot.windows[0].resets_at.as_deref(),
            Some("2026-09-21T14:13:20Z")
        );
    }
}
