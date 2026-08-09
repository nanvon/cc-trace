//! 官方服务状态契约（Statuspage.io 状态链）。
//!
//! 这条状态链与额度三维状态（`docs/状态与错误模型.md`）是**两条独立状态链**：
//! 它报告 OpenAI／Anthropic 官方服务的公开故障，不参与 `ProviderAvailability`、
//! Overall Signal、退避与刷新调度，见 [ADR-0026]。
//! [ADR-0026]: ../../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md
//!
//! 载荷是公开信息：不含凭据、端点原文或本机路径。

use serde::{Deserialize, Serialize};

/// Statuspage 的 `status.indicator`。解析失败或枚举值无法识别时归为 `Unknown`，
/// 前端对 `Unknown` 不绘制圆点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceStatusIndicator {
    None,
    Minor,
    Major,
    Critical,
    Maintenance,
    Unknown,
}

/// 一个 Provider 的官方服务状态。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub indicator: ServiceStatusIndicator,
    /// Statuspage 的 `status.description`，缺失时由界面回退到 indicator 的文案。
    pub description: Option<String>,
    /// Statuspage 的 `page.updated_at`，ISO 8601 UTC；缺失时 tooltip 不显示更新时间。
    pub updated_at: Option<String>,
    /// 本地抓取时刻，ISO 8601 UTC。
    pub fetched_at: String,
}

/// `service_status_get` 的返回值与 `service-status://updated` 事件载荷。
///
/// 两个 Provider 独立存在：一个失败只影响自己的 `None`，不影响另一个。
/// 语义是「没有可展示的服务状态」，不是错误。
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatusState {
    pub codex: Option<ServiceStatus>,
    pub claude: Option<ServiceStatus>,
}

impl ServiceStatusState {
    /// 取某个 Provider 的状态；未知 Provider 返回 `None`。
    pub fn get(&self, provider: ProviderId) -> Option<&ServiceStatus> {
        match provider {
            ProviderId::Codex => self.codex.as_ref(),
            ProviderId::Claude => self.claude.as_ref(),
        }
    }

    /// 更新某个 Provider 的状态；未知 Provider 是 no-op。
    pub fn set(&mut self, provider: ProviderId, status: ServiceStatus) {
        match provider {
            ProviderId::Codex => self.codex = Some(status),
            ProviderId::Claude => self.claude = Some(status),
        }
    }
}

use crate::contracts::ProviderId;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_wire_values_stay_snake_case() {
        for (indicator, expected) in [
            (ServiceStatusIndicator::None, "none"),
            (ServiceStatusIndicator::Minor, "minor"),
            (ServiceStatusIndicator::Major, "major"),
            (ServiceStatusIndicator::Critical, "critical"),
            (ServiceStatusIndicator::Maintenance, "maintenance"),
            (ServiceStatusIndicator::Unknown, "unknown"),
        ] {
            let json = serde_json::to_value(indicator).expect("indicator serializes");
            assert_eq!(json, expected, "{indicator:?}");
        }
    }

    #[test]
    fn unknown_indicator_is_covered_by_the_parser_layer() {
        // `indicator_from_str` 在 `providers::service_status` 里测试；契约层
        // 只序列化 Rust 侧已有的取值，不负责容忍未知枚举。
        let json = serde_json::to_value(ServiceStatusIndicator::Unknown).expect("serializes");
        assert_eq!(json, "unknown");
    }

    #[test]
    fn state_keeps_providers_independent() {
        let mut state = ServiceStatusState::default();
        assert!(state.codex.is_none());
        assert!(state.claude.is_none());

        state.set(
            ProviderId::Codex,
            ServiceStatus {
                indicator: ServiceStatusIndicator::Minor,
                description: None,
                updated_at: None,
                fetched_at: "2026-08-09T00:00:00Z".to_owned(),
            },
        );

        assert_eq!(
            state.get(ProviderId::Codex).map(|s| s.indicator),
            Some(ServiceStatusIndicator::Minor)
        );
        assert_eq!(state.get(ProviderId::Claude).map(|s| s.indicator), None);
    }
}
