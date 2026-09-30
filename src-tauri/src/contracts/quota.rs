//! 额度展示契约。
//!
//! 三个状态维度（`RefreshState`、`SnapshotFreshness`、`ProviderAvailability`）各自独立
//! 序列化，不得压成一个互斥枚举，语义见 `docs/状态与错误模型.md` 第 1 节。
//! 额度字段与窗口分层规则见 `docs/额度领域模型.md` 第 1～2 节。
//!
//! 展示单位是**额度主体**（`ProviderSnapshot.subject_id`），不是 Provider：Codex 支持
//! 导入多个副账号，每个账号独立刷新、独立退避、独立失败，见
//! [ADR-0031](../../../docs/决策/ADR-0031-功能基准改为cc-bar-v1.1.1.md)。
//! `ProviderId` 只表达归属与展示分组，顺序固定不随风险重排。
//!
//! 所有载荷都是脱敏 DTO：不含凭据、端点原文、请求头或本机路径。
//! 唯一例外是 `ProviderIdentity.account`：按 [ADR-0025] 展示完整账号
//! 需要它进入 command 载荷与本地缓存；token、凭据与响应原文仍不进入载荷。
//! [ADR-0025]: ../../../../docs/决策/ADR-0025-非隐私模式显示完整邮箱.md

use serde::{Deserialize, Serialize};

use super::error::AppError;

/// Provider 标识。空间顺序固定 Codex → Claude → Antigravity → Cursor → Command Code，
/// 与 cc-bar 菜单栏顺序一致。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProviderId {
    Codex,
    Claude,
    Antigravity,
    Cursor,
    CommandCode,
}

impl ProviderId {
    /// 界面与调度共用的稳定顺序。
    pub const ORDER: [ProviderId; 5] = [
        ProviderId::Codex,
        ProviderId::Claude,
        ProviderId::Antigravity,
        ProviderId::Cursor,
        ProviderId::CommandCode,
    ];

    /// 持久化与命令载荷里使用的稳定短名。
    pub fn key(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Antigravity => "antigravity",
            Self::Cursor => "cursor",
            Self::CommandCode => "commandCode",
        }
    }

    /// 是否支持本地用量统计（Antigravity 与 Command Code 只提供额度）。
    pub fn has_local_usage(self) -> bool {
        matches!(self, Self::Codex | Self::Claude)
    }

    /// 是否有官方 Statuspage 状态链。
    pub fn has_service_status(self) -> bool {
        matches!(self, Self::Codex | Self::Claude | Self::Cursor)
    }
}

/// 额度主体的类型。主账号与导入账号在界面上是同级条目，但导入账号可被删除与排序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaSubjectKind {
    /// 自动发现的当前账号。
    Primary,
    /// 用户粘贴 `auth.json` 导入的副账号。
    Imported,
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
///
/// `Total` / `Auto` / `Api` 是 Cursor 的计量桶，`Monthly` 是 Command Code GOAT 套餐的
/// 月度额度：它们都不是滚动时间窗，但仍是「已用比例 + 重置时刻」的额度过期。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum QuotaWindowKind {
    FiveHour,
    Weekly,
    ModelWeekly,
    Monthly,
    Total,
    Auto,
    Api,
    Unknown,
}

/// 单个额度窗口。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaWindow {
    /// 跨刷新稳定的窗口标识，用于匹配旧快照。不使用展示名或数组下标匹配。
    pub id: String,
    pub kind: QuotaWindowKind,
    /// 只在 `kind` 无法完整表达时使用：模型专项额度的模型名、Antigravity 的
    /// `Gemini` / `Claude` 分组、Cursor 的 `Auto` / `API` 桶名。
    /// 前端优先按 `kind` 取 i18n 文案，此字段作为补充。
    pub display_name: Option<String>,
    pub used_percent: f64,
    /// `clamp(100 - used_percent, 0, 100)`，统一在 Rust 计算，Vue 不重算。
    pub remaining_percent: f64,
    /// ISO 8601 UTC；缺失时前端显示「重置时间未知」，不显示占位符号。
    pub resets_at: Option<String>,
    pub window_seconds: Option<u64>,
    pub is_active: bool,
    /// 是否为返回顺序中的第一项。展示主次始终以 `QuotaSnapshot.windows` 顺序为准。
    pub is_primary: bool,
    /// 无上限额度（Cursor Unlimited）：界面显示 `∞`，不渲染伪造百分比或进度条。
    pub unlimited: bool,
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

/// 展示身份。账号字段按 [ADR-0025] 携带完整邮箱或 account id（隐私模式下由前端隐藏显示，
/// 不在此层过滤）；计划名用于套餐展示。不含 token、凭据或响应原文。
/// [ADR-0025]: ../../../../docs/决策/ADR-0025-非隐私模式显示完整邮箱.md
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderIdentity {
    /// 完整账号，例如 `nanvon@example.com` 或 account id。仅在隐私模式开启时前端不显示。
    pub account: Option<String>,
    pub plan: Option<String>,
}

/// 一个额度主体的完整展示状态：数据 + 三个独立维度。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    /// 额度主体标识：主账号用 Provider 短名（`codex`），导入账号用 `codex:<序号>`。
    /// 它是缓存、退避与历史序列的键，不承载账号明文。
    pub subject_id: String,
    pub provider: ProviderId,
    pub kind: QuotaSubjectKind,
    /// 导入账号的用户可见名称（脱敏邮箱）；主账号为 `None`，账号在 `identity` 里。
    pub label: Option<String>,
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
    /// 主账号的初始状态。真正开始请求后由调度层进入 `loading`。
    pub fn initial(provider: ProviderId) -> Self {
        Self::for_subject(
            provider.key().to_owned(),
            provider,
            QuotaSubjectKind::Primary,
            None,
        )
    }

    /// 任一度量主体的初始状态。
    pub fn for_subject(
        subject_id: String,
        provider: ProviderId,
        kind: QuotaSubjectKind,
        label: Option<String>,
    ) -> Self {
        Self {
            subject_id,
            provider,
            kind,
            label,
            refresh: RefreshState::Idle,
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

/// 一个额度主体的静态描述：身份、归属与展示顺序信息。运行时用它建 `ProviderSnapshot`
/// 与刷新运行时，不含任何凭据。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaSubject {
    pub subject_id: String,
    pub provider: ProviderId,
    pub kind: QuotaSubjectKind,
    /// 导入账号的展示名；主账号为 `None`。
    pub label: Option<String>,
    /// 导入账号的用户排序位置；主账号恒为 0。
    pub order_index: u32,
}

impl QuotaSubject {
    /// 主账号：每个 Provider 至少有一个，`subject_id` 就是 Provider 短名。
    pub fn primary(provider: ProviderId) -> Self {
        Self {
            subject_id: provider.key().to_owned(),
            provider,
            kind: QuotaSubjectKind::Primary,
            label: None,
            order_index: 0,
        }
    }

    /// 导入的 Codex 副账号。`identity_hash` 是账号身份的短哈希，
    /// **不是下标**：删除或重排账号时下标会变，用下标做标识会把 A 的缓存与历史读成 B 的。
    pub fn imported_codex(identity_hash: &str, label: Option<String>, order_index: u32) -> Self {
        Self {
            subject_id: format!("codex:imported:{identity_hash}"),
            provider: ProviderId::Codex,
            kind: QuotaSubjectKind::Imported,
            label,
            order_index,
        }
    }
}

/// `quota_get_snapshot` 的返回值。顺序固定：Provider 顺序，主账号在前，导入账号按用户顺序在后。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaState {
    pub providers: Vec<ProviderSnapshot>,
}

/// `quota://refresh-state` 事件载荷。刷新状态的唯一来源。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshStatePayload {
    pub subject_id: String,
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
        assert_eq!(json["subjectId"], "codex");
        assert_eq!(json["kind"], "primary");
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
        assert_eq!(
            ProviderId::ORDER,
            [
                ProviderId::Codex,
                ProviderId::Claude,
                ProviderId::Antigravity,
                ProviderId::Cursor,
                ProviderId::CommandCode
            ]
        );
    }

    #[test]
    fn window_kinds_cover_the_remote_quota_shapes() {
        for (kind, expected) in [
            (QuotaWindowKind::FiveHour, "fiveHour"),
            (QuotaWindowKind::Weekly, "weekly"),
            (QuotaWindowKind::ModelWeekly, "modelWeekly"),
            (QuotaWindowKind::Monthly, "monthly"),
            (QuotaWindowKind::Total, "total"),
            (QuotaWindowKind::Auto, "auto"),
            (QuotaWindowKind::Api, "api"),
            (QuotaWindowKind::Unknown, "unknown"),
        ] {
            let json = serde_json::to_value(kind).expect("kind serializes");
            assert_eq!(json, expected);
        }
    }

    #[test]
    fn imported_subjects_keep_their_own_identity() {
        let subject = QuotaSubject::imported_codex("0123456789abcdef", None, 2);
        assert_eq!(subject.subject_id, "codex:imported:0123456789abcdef");
        assert_eq!(subject.order_index, 2);

        let imported = ProviderSnapshot::for_subject(
            "codex:imported:0123456789abcdef".to_owned(),
            ProviderId::Codex,
            QuotaSubjectKind::Imported,
            Some("second@example.com".to_owned()),
        );
        assert_eq!(imported.subject_id, "codex:imported:0123456789abcdef");
        assert_eq!(imported.kind, QuotaSubjectKind::Imported);
        assert_eq!(imported.label.as_deref(), Some("second@example.com"));
        assert!(imported.identity.is_none());
    }
}
