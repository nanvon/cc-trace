//! 额度展示契约。
//!
//! 三个状态维度（`RefreshState`、`SnapshotFreshness`、`ProviderAvailability`）各自独立
//! 序列化，不得压成一个互斥枚举，语义见 `docs/状态与错误模型.md` 第 1 节。
//! 额度字段与窗口分层规则见 `docs/额度领域模型.md` 第 1～2 节。
//!
//! 所有载荷都是脱敏 DTO：不含凭据、端点原文、请求头或本机路径。

use serde::{Deserialize, Serialize};

use super::error::AppError;

/// Provider 标识。空间顺序固定 Codex → Claude Code，不随风险重排。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderId {
    Codex,
    Claude,
}

impl ProviderId {
    /// 界面与调度共用的稳定顺序。
    pub const ORDER: [ProviderId; 2] = [ProviderId::Codex, ProviderId::Claude];
}

/// 活动维度：现在是否正在工作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshState {
    Idle,
    /// 首次加载，此时没有可展示的快照。
    Loading,
    /// 已有快照的刷新，快照必须保留。
    Refreshing,
}

/// 快照新鲜度维度：当前展示的数据是否可信、是否为旧数据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SnapshotFreshness {
    Empty,
    Live,
    Stale,
}

/// 可用性维度：为什么能或不能取得新数据。
///
/// `NoCredentials`、`Unsupported`、`RateLimited` 不得降级成普通 `Error`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderAvailability {
    Ready,
    NoCredentials,
    Unsupported,
    Offline,
    RateLimited,
    Error,
}

/// 额度窗口类型。无法判定时保留 `Unknown`，不得猜成 `FiveHour` 或 `Weekly`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaWindowKind {
    FiveHour,
    Weekly,
    ModelWeekly,
    Unknown,
}

/// 单个额度窗口。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// 跨刷新稳定的窗口标识，用于匹配旧快照。不使用展示名或数组下标匹配。
    pub id: String,
    pub kind: QuotaWindowKind,
    /// 只在 `kind` 无法完整表达时使用，例如 `ModelWeekly` 的模型或场景名。
    /// 前端优先按 `kind` 取 i18n 文案，此字段作为补充。
    pub display_name: Option<String>,
    pub used_percent: f64,
    /// `clamp(100 - used_percent, 0, 100)`，统一在 Rust 计算，Vue 不重算。
    pub remaining_percent: f64,
    /// ISO 8601 UTC；缺失时前端显示「重置时间未知」，不显示占位符号。
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
    pub is_active: bool,
    /// 主要额度：紧凑面板与主窗口的首要判断依据。由 Rust 按窗口分层规则判定。
    pub is_primary: bool,
}

impl QuotaWindow {
    /// 按 `docs/额度领域模型.md` 第 4 节统一 clamp 剩余百分比。
    pub fn normalized_remaining(used_percent: f64) -> f64 {
        (100.0 - used_percent).clamp(0.0, 100.0)
    }
}

/// 一个 Provider 在某一时刻的全部额度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSnapshot {
    pub windows: Vec<QuotaWindow>,
    /// 本快照的采集时刻，ISO 8601 UTC。
    pub captured_at: String,
}

/// 脱敏身份提示。不含完整邮箱、account id 或 token。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentity {
    /// 已脱敏的账号提示，例如 `n***@example.com`。
    pub account_hint: Option<String>,
    pub plan: Option<String>,
}

/// 一个 Provider 的完整展示状态：数据 + 三个独立维度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub provider: ProviderId,
    pub refresh: RefreshState,
    pub freshness: SnapshotFreshness,
    pub availability: ProviderAvailability,
    pub identity: Option<ProviderIdentity>,
    /// 刷新失败时保留上一份有效快照，不清空。
    pub snapshot: Option<QuotaSnapshot>,
    pub last_success_at: Option<String>,
    pub last_attempt_at: Option<String>,
    /// 退避期内可再次尝试的时刻，ISO 8601 UTC。手动刷新不得绕过它。
    pub retry_after: Option<String>,
    /// 只在 `availability == Error` 时出现，用于区分凭据类与协议类文案。
    pub error: Option<AppError>,
}

impl ProviderSnapshot {
    /// 尚无任何数据时的初始状态：首次加载。
    pub fn initial(provider: ProviderId) -> Self {
        Self {
            provider,
            refresh: RefreshState::Loading,
            freshness: SnapshotFreshness::Empty,
            availability: ProviderAvailability::Ready,
            identity: None,
            snapshot: None,
            last_success_at: None,
            last_attempt_at: None,
            retry_after: None,
            error: None,
        }
    }

    pub fn has_snapshot(&self) -> bool {
        self.snapshot.is_some()
    }
}

/// `quota_get_snapshot` 的返回值。Provider 顺序固定为 `ProviderId::ORDER`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaState {
    pub providers: Vec<ProviderSnapshot>,
}

/// `quota://refresh-state` 事件载荷。刷新状态的唯一来源。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshStatePayload {
    pub provider: ProviderId,
    pub refresh: RefreshState,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remaining_percent_is_clamped() {
        assert_eq!(QuotaWindow::normalized_remaining(0.0), 100.0);
        assert_eq!(QuotaWindow::normalized_remaining(27.0), 73.0);
        assert_eq!(QuotaWindow::normalized_remaining(140.0), 0.0);
        assert_eq!(QuotaWindow::normalized_remaining(-10.0), 100.0);
    }

    #[test]
    fn three_dimensions_serialize_independently() {
        let snapshot = ProviderSnapshot {
            refresh: RefreshState::Refreshing,
            freshness: SnapshotFreshness::Stale,
            availability: ProviderAvailability::RateLimited,
            ..ProviderSnapshot::initial(ProviderId::Codex)
        };

        let json = serde_json::to_value(&snapshot).expect("snapshot serializes");
        assert_eq!(json["refresh"], "refreshing");
        assert_eq!(json["freshness"], "stale");
        assert_eq!(json["availability"], "rate_limited");
    }

    #[test]
    fn failure_reasons_keep_their_own_wire_values() {
        for (availability, expected) in [
            (ProviderAvailability::NoCredentials, "no_credentials"),
            (ProviderAvailability::Unsupported, "unsupported"),
            (ProviderAvailability::RateLimited, "rate_limited"),
            (ProviderAvailability::Offline, "offline"),
            (ProviderAvailability::Error, "error"),
            (ProviderAvailability::Ready, "ready"),
        ] {
            let json = serde_json::to_value(availability).expect("availability serializes");
            assert_eq!(json, expected, "{availability:?} must not be downgraded");
        }
    }

    #[test]
    fn provider_order_is_stable() {
        assert_eq!(ProviderId::ORDER, [ProviderId::Codex, ProviderId::Claude]);
    }
}
