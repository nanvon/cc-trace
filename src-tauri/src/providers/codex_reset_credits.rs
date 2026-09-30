//! Codex 额外「Full reset」credit 的查询。
//!
//! 与 `get_usage`（`wham/usage`）是独立接口，**不参与常规额度快照**，也不接入调度：
//! 只在设置页按需查询，用户点开才发请求。
//!
//! 端点与字段映射对齐 cc-bar v1.1.1 的 `CodexResetCreditsClient`。

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::Value;

use super::credentials::codex::CodexCredentials;
use super::{ProviderFetchOutcome, http};
use crate::contracts::ErrorKind;

const ENDPOINT: &str = "https://chatgpt.com/backend-api/wham/rate-limit-reset-credits";

/// 一个 credit。`id` 由业务字段拼出，服务端不提供稳定 id。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetCredit {
    pub id: String,
    pub status: String,
    pub title: String,
    pub granted_at: Option<String>,
    pub expires_at: Option<String>,
}

/// 查询结果。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexResetCredits {
    /// 可用数量由服务端给出，不由列表推算：列表里还包含已用完与已过期的条目。
    pub available_count: i64,
    pub credits: Vec<CodexResetCredit>,
}

/// 查一次额外重置 credit。
pub async fn fetch(
    credentials: &CodexCredentials,
) -> Result<CodexResetCredits, ProviderFetchOutcome> {
    let mut request = http::client()
        .get(ENDPOINT)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", credentials.access_token.expose()),
        )
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, "codex-cli");
    if let Some(account_id) = credentials.account_id.as_ref() {
        request = request.header("ChatGPT-Account-Id", account_id.expose());
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
    let root: Value = serde_json::from_str(&body).map_err(|_| ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    })?;

    parse(&root).ok_or(ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    })
}

/// 解析响应。`available_count` 缺失时按 0 处理（界面显示「没有可用次数」比报错更有用），
/// 但整个载荷不是对象时判成协议错误。
pub fn parse(root: &Value) -> Option<CodexResetCredits> {
    root.as_object()?;

    let available_count = root
        .get("available_count")
        .and_then(as_integer)
        .unwrap_or(0);
    let credits = root
        .get("credits")
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(parse_credit).collect())
        .unwrap_or_default();

    Some(CodexResetCredits {
        available_count,
        credits,
    })
}

fn parse_credit(value: &Value) -> Option<CodexResetCredit> {
    let status = value.get("status").and_then(Value::as_str)?.trim();
    let title = value.get("title").and_then(Value::as_str)?.trim();
    let granted_at = value.get("granted_at").and_then(as_time);
    let expires_at = value.get("expires_at").and_then(as_time);

    // 服务端没有稳定 id，用业务字段拼一个：同一份可用列表两次查询之间保持一致，
    // 避免前端列表在刷新后重建。
    let id = format!(
        "{title}-{status}-{}",
        granted_at
            .as_deref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|time| time.timestamp().to_string())
            .unwrap_or_else(|| "0".to_owned())
    );

    Some(CodexResetCredit {
        id,
        status: status.to_owned(),
        title: title.to_owned(),
        granted_at,
        expires_at,
    })
}

fn as_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|v| v as i64)),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// 时间字段可能是 epoch 秒／毫秒，也可能是 ISO 8601 字符串（含小数秒）。
fn as_time(value: &Value) -> Option<String> {
    let time = match value {
        Value::Number(number) => {
            let raw = number.as_f64()?;
            if !raw.is_finite() || raw <= 0.0 {
                return None;
            }
            let millis = if raw > 10_000_000_000.0 {
                raw.round()
            } else {
                (raw * 1000.0).round()
            };
            return Utc.timestamp_millis_opt(millis as i64).single().map(iso);
        }
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            DateTime::parse_from_rfc3339(trimmed)
                .ok()
                .map(|time| time.with_timezone(&Utc))?
        }
        _ => return None,
    };
    Some(iso(time))
}

fn iso(time: DateTime<Utc>) -> String {
    time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_counts_and_credits() {
        let root = serde_json::json!({
            "available_count": 2,
            "credits": [
                {
                    "status": "available",
                    "title": "Full reset",
                    "granted_at": 1_790_000_000_u64,
                    "expires_at": "2026-11-01T00:00:00.500Z"
                },
                { "status": "used", "title": "Full reset" }
            ]
        });

        let parsed = parse(&root).expect("parses");
        assert_eq!(parsed.available_count, 2);
        assert_eq!(parsed.credits.len(), 2);
        assert_eq!(
            parsed.credits[0].granted_at.as_deref(),
            Some("2026-09-21T14:13:20Z")
        );
        assert_eq!(
            parsed.credits[0].expires_at.as_deref(),
            Some("2026-11-01T00:00:00Z")
        );
        assert!(parsed.credits[1].granted_at.is_none());
        assert_ne!(parsed.credits[0].id, parsed.credits[1].id);
        assert_eq!(parsed.credits[0].id, "Full reset-available-1790000000");
        assert_eq!(parsed.credits[1].id, "Full reset-used-0");
    }

    #[test]
    fn millisecond_timestamps_are_recognized() {
        let root = serde_json::json!({
            "credits": [{ "status": "available", "title": "t", "granted_at": 1_790_000_000_000_u64 }]
        });
        let parsed = parse(&root).expect("parses");
        assert_eq!(
            parsed.credits[0].granted_at.as_deref(),
            Some("2026-09-21T14:13:20Z")
        );
    }

    #[test]
    fn a_missing_count_is_zero_not_an_error() {
        let parsed = parse(&serde_json::json!({ "credits": [] })).expect("parses");
        assert_eq!(parsed.available_count, 0);
        assert!(parsed.credits.is_empty());
    }

    #[test]
    fn a_non_object_payload_is_a_protocol_error() {
        assert!(parse(&serde_json::json!([1, 2, 3])).is_none());
        assert!(parse(&serde_json::json!("text")).is_none());
    }

    #[test]
    fn credits_without_a_status_or_title_are_skipped() {
        let root = serde_json::json!({
            "credits": [{ "title": "t" }, { "status": "available" }]
        });
        let parsed = parse(&root).expect("parses");
        assert!(parsed.credits.is_empty());
    }
}
