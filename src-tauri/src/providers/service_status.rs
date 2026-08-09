//! OpenAI／Anthropic 公开 Statuspage 状态链客户端。
//!
//! 数据源与行为见 [ADR-0026]：与额度请求共用 `super::http` 的共享客户端
//! （含系统代理探测与参数基线超时），失败由调用方保留旧值，不参与额度退避。
//! [ADR-0026]: ../../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md

use chrono::Utc;

use super::http;
use crate::contracts::{ServiceStatus, ServiceStatusIndicator};

/// Statuspage 的 `indicator` 枚举名；未知值归为 `Unknown`。
const INDICATOR_NONE: &str = "none";
const INDICATOR_MINOR: &str = "minor";
const INDICATOR_MAJOR: &str = "major";
const INDICATOR_CRITICAL: &str = "critical";
const INDICATOR_MAINTENANCE: &str = "maintenance";

/// OpenAI 官方状态页（Codex 的服务状态来源）。
pub const OPENAI_STATUS_URL: &str = "https://status.openai.com/api/v2/status.json";
/// Anthropic 官方状态页（Claude Code 的服务状态来源）。
pub const ANTHROPIC_STATUS_URL: &str = "https://status.claude.com/api/v2/status.json";

/// 一次状态拉取的失败。不携带响应体、端点原文或路径，只够区分「不可用」。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ServiceStatusError;

/// 拉取并解析一个 Statuspage `status.json`。
///
/// 任何网络或解析失败都返回 [`ServiceStatusError`]，由调用方保留上一份内存值。
pub async fn fetch_status(url: &str) -> Result<ServiceStatus, ServiceStatusError> {
    let response = http::client()
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|_| ServiceStatusError)?;

    if !response.status().is_success() {
        return Err(ServiceStatusError);
    }

    let payload: StatusPayload = response.json().await.map_err(|_| ServiceStatusError)?;

    Ok(ServiceStatus {
        indicator: indicator_from_str(&payload.status.indicator),
        description: payload.status.description,
        updated_at: payload.page.and_then(|page| page.updated_at),
        fetched_at: Utc::now().to_rfc3339(),
    })
}

fn indicator_from_str(raw: &str) -> ServiceStatusIndicator {
    match raw {
        INDICATOR_NONE => ServiceStatusIndicator::None,
        INDICATOR_MINOR => ServiceStatusIndicator::Minor,
        INDICATOR_MAJOR => ServiceStatusIndicator::Major,
        INDICATOR_CRITICAL => ServiceStatusIndicator::Critical,
        INDICATOR_MAINTENANCE => ServiceStatusIndicator::Maintenance,
        _ => ServiceStatusIndicator::Unknown,
    }
}

/// Statuspage.io v2 `status.json` 的最小解码结构；未知字段忽略。
#[derive(serde::Deserialize)]
struct StatusPayload {
    page: Option<Page>,
    status: Status,
}

#[derive(serde::Deserialize)]
struct Page {
    updated_at: Option<String>,
}

#[derive(serde::Deserialize)]
struct Status {
    indicator: String,
    description: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_indicators_map_to_their_enum() {
        assert_eq!(indicator_from_str("none"), ServiceStatusIndicator::None);
        assert_eq!(indicator_from_str("minor"), ServiceStatusIndicator::Minor);
        assert_eq!(indicator_from_str("major"), ServiceStatusIndicator::Major);
        assert_eq!(indicator_from_str("critical"), ServiceStatusIndicator::Critical);
        assert_eq!(
            indicator_from_str("maintenance"),
            ServiceStatusIndicator::Maintenance
        );
    }

    #[test]
    fn unknown_indicators_and_case_differences_become_unknown() {
        for raw in ["", "degraded", "None", "PERFORMANCE"] {
            assert_eq!(indicator_from_str(raw), ServiceStatusIndicator::Unknown, "{raw:?}");
        }
    }

    #[test]
    fn status_payload_decodes_with_optional_fields() {
        let payload: StatusPayload = serde_json::from_str(
            r#"{"page":{"id":"x","updated_at":"2026-08-09T01:02:03Z"},"status":{"indicator":"minor","description":"We are investigating"}}"#,
        )
        .expect("payload parses");
        assert_eq!(payload.status.indicator, "minor");
        assert_eq!(payload.status.description.as_deref(), Some("We are investigating"));
        assert_eq!(payload.page.unwrap().updated_at.as_deref(), Some("2026-08-09T01:02:03Z"));
    }

    #[test]
    fn missing_page_or_description_is_tolerated() {
        let payload: StatusPayload =
            serde_json::from_str(r#"{"status":{"indicator":"none"}}"#).expect("payload parses");
        assert_eq!(payload.status.indicator, "none");
        assert!(payload.status.description.is_none());
        assert!(payload.page.is_none());
    }
}
