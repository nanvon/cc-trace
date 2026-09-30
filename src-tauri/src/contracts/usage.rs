//! 本地用量查询与扫描的脱敏 DTO。
//!
//! 契约只返回聚合事实、公开价格估算和不含路径的对话元数据。原始 JSONL、消息正文、
//! 文件名、绝对路径与账号明文永远不跨 command 边界。

use serde::{Deserialize, Serialize};

use super::quota::{ProviderId, QuotaWindowKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    Codex,
    Claude,
    Pi,
    Opencode,
    Dsh,
    /// Cursor 没有本机会话：用量来自账号级远端计量，单独入库、单独补拉。
    Cursor,
}

impl UsageSource {
    /// 参与在线定价与 fingerprint 的数据源。
    ///
    /// DSH 日志只有 token 用量、没有可当作账单的费用，按 cc-bar 的做法用价格表估算，
    /// 因此它也是价格目录的参与者；Pi 与 OpenCode 的日志自带 cost、
    /// Cursor 用服务端计量费用，三者都不参与。
    pub const ALL: [Self; 3] = [Self::Codex, Self::Claude, Self::Dsh];

    /// 需要本地扫描的数据源；Cursor 不在其中。
    pub const LOCAL_SCAN: [Self; 5] = [
        Self::Codex,
        Self::Claude,
        Self::Pi,
        Self::Opencode,
        Self::Dsh,
    ];

    /// 全部数据源，顺序固定：本地扫描源在前，远端计量源在后。
    pub const ORDER: [Self; 6] = [
        Self::Codex,
        Self::Claude,
        Self::Pi,
        Self::Opencode,
        Self::Dsh,
        Self::Cursor,
    ];

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
            Self::Dsh => "dsh",
            Self::Cursor => "cursor",
        }
    }

    /// 是否来自本地日志扫描（决定重建、重计价与扫描水位的适用范围）。
    pub fn is_local_scan(self) -> bool {
        !matches!(self, Self::Cursor)
    }

    /// 是否自带费用真值、不参与价格表（Pi／OpenCode 是日志自带，Cursor 是服务端计量）。
    pub fn carries_own_cost(self) -> bool {
        matches!(self, Self::Pi | Self::Opencode | Self::Cursor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSpeed {
    Standard,
    Fast,
    Unknown,
}

impl UsageSpeed {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::Fast => "fast",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageGroupBy {
    /// 自然日桶。
    Day,
    /// 自然周桶，周一为周起点。
    Week,
    /// 自然月桶。
    Month,
    Source,
    /// 模型提供商归属（展示层推导，见 [ADR-0031]）。
    /// [ADR-0031]: ../../../../docs/决策/ADR-0031-功能基准改为cc-bar-v1.1.1.md
    Provider,
    Model,
    Project,
    Speed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilter {
    pub from: Option<String>,
    pub to: Option<String>,
    /// 可见服务集合。`None` 或空表示不过滤；空数组在规范化时视为 `None`。
    pub sources: Option<Vec<UsageSource>>,
    pub model: Option<String>,
    pub speed: Option<UsageSpeed>,
    /// 项目身份键（规范化项目路径）。`None` 表示不按项目过滤。
    pub project: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryQuery {
    #[serde(default)]
    pub filter: UsageFilter,
    pub group_by: UsageGroupBy,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTokenTotals {
    pub uncached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_write_5m_input_tokens: i64,
    pub cache_write_1h_input_tokens: i64,
    pub input_tokens: i64,
    pub total_tokens: i64,
}

impl UsageTokenTotals {
    pub(crate) fn add_assign(&mut self, other: &Self) {
        self.uncached_input_tokens += other.uncached_input_tokens;
        self.output_tokens += other.output_tokens;
        self.reasoning_output_tokens += other.reasoning_output_tokens;
        self.cache_read_input_tokens += other.cache_read_input_tokens;
        self.cache_write_5m_input_tokens += other.cache_write_5m_input_tokens;
        self.cache_write_1h_input_tokens += other.cache_write_1h_input_tokens;
        self.input_tokens += other.input_tokens;
        self.total_tokens += other.total_tokens;
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageCostTotals {
    pub api_equivalent_cost_nanos: i64,
    pub priced_entries: i64,
    pub unpriced_entries: i64,
    pub assumed_geo_entries: i64,
    pub pricing_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFastTotals {
    /// Fast 档位的原始 Token，不包含任何倍率。
    pub raw_tokens: i64,
    /// 十进制定点字符串，避免跨 Rust / JavaScript 边界丢失精度。
    pub billing_equivalent_tokens: String,
    /// 混合模型时分别返回最小倍率与最大倍率；未知倍率不猜测。
    pub minimum_multiplier: Option<String>,
    pub maximum_multiplier: Option<String>,
    pub has_unpriced_equivalent: bool,
}

impl Default for UsageFastTotals {
    fn default() -> Self {
        Self {
            raw_tokens: 0,
            billing_equivalent_tokens: "0".to_owned(),
            minimum_multiplier: None,
            maximum_multiplier: None,
            has_unpriced_equivalent: false,
        }
    }
}

impl UsageFastTotals {
    pub(crate) fn add_assign(&mut self, other: &Self) {
        self.raw_tokens += other.raw_tokens;
        self.billing_equivalent_tokens = decimal_nanos_string(
            parse_decimal_nanos(&self.billing_equivalent_tokens)
                .saturating_add(parse_decimal_nanos(&other.billing_equivalent_tokens)),
        );
        self.minimum_multiplier = decimal_option_min(
            self.minimum_multiplier.as_deref(),
            other.minimum_multiplier.as_deref(),
        );
        self.maximum_multiplier = decimal_option_max(
            self.maximum_multiplier.as_deref(),
            other.maximum_multiplier.as_deref(),
        );
        self.has_unpriced_equivalent |= other.has_unpriced_equivalent;
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummaryRow {
    pub key: String,
    /// 事实行数（日粒度远端计量算一行）。
    pub entry_count: i64,
    /// 请求数；日粒度远端计量的请求数为 0。
    pub request_count: i64,
    pub tokens: UsageTokenTotals,
    pub fast: UsageFastTotals,
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub rows: Vec<UsageSummaryRow>,
    pub entry_count: i64,
    pub request_count: i64,
    pub tokens: UsageTokenTotals,
    pub fast: UsageFastTotals,
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageConversationSort {
    /// 最近活动优先。
    Recent,
    /// 总 Token 降序。
    Tokens,
    /// API 等值费用降序。
    Cost,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationQuery {
    #[serde(default)]
    pub filter: UsageFilter,
    pub search: Option<String>,
    /// 项目身份键；`None` 表示不过滤。
    pub project: Option<String>,
    pub sort: Option<UsageConversationSort>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversation {
    pub conversation_key: String,
    pub source: UsageSource,
    pub title: Option<String>,
    /// 脱敏展示名（项目路径尾段），未归属对话为 `None`。
    pub project_hint: Option<String>,
    /// 项目身份键（规范化项目路径）；未归属或未解析时为 `None`。
    pub project_key: Option<String>,
    /// 对话自身的工作目录路径，用于 worktree 明细与「在文件管理器中显示」。
    pub worktree_path: Option<String>,
    /// 未归属：Cursor 远端计量、补录与早期按天汇总历史。
    pub unattributed: bool,
    pub is_sidechain: bool,
    pub first_at: String,
    pub last_at: String,
    /// 事实行数。
    pub entry_count: i64,
    /// 请求数；日粒度远端计量为 0。
    pub request_count: i64,
    pub tokens: UsageTokenTotals,
    pub fast: UsageFastTotals,
    pub cost: UsageCostTotals,
    /// 原始会话 id（会话 UUID），供详情页展示与复制；非账号明文。
    pub source_id: Option<String>,
    /// 会话 git 分支；只有记录分支的数据源提供。
    pub branch: Option<String>,
    /// 会话涉及的去重模型列表，按模型名排序；不受查询过滤影响。
    pub models: Vec<String>,
}

/// 对话项目筛选选项：项目身份键、展示名与其可见对话数、最近活动时间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationProjectOption {
    /// 项目身份键；未归属项目为 `None`。
    pub key: Option<String>,
    pub name: String,
    pub conversation_count: i64,
    pub last_at: String,
}

/// 额度周期查询。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCycleQuery {
    /// 只取某个服务；`None` 表示全部。
    pub provider: Option<ProviderId>,
    /// 只取某个额度主体（导入账号）；`None` 表示全部。
    pub identity_key: Option<String>,
    /// 回看天数；缺失按 30 天。
    pub days: Option<u32>,
}

/// 周期的用量读数。没有本地用量的服务（Antigravity／Cursor／Command Code）全为 0。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCycleUsage {
    pub tokens: UsageTokenTotals,
    pub cost: UsageCostTotals,
    pub request_count: i64,
}

/// 一个额度片段（周期内的份额度）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCycleSegmentView {
    pub start_at: String,
    /// 活动片段为 `None`。
    pub end_at: Option<String>,
    pub baseline_used_percent: i64,
    pub latest_used_percent: i64,
    pub maximum_used_percent: i64,
    /// 官方口径的观察值：`latest - baseline`。
    pub observed_used_percent: i64,
    /// `initial` 或 `extraReset`。
    pub start_reason: String,
    pub active: bool,
    pub usage: QuotaCycleUsage,
}

/// 用满预估。`confidence` 为 `early` 时不给具体数字。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCycleForecast {
    pub confidence: String,
    pub observed_percent: i64,
    pub estimated_full_tokens: Option<i64>,
    /// 十进制定点纳秒字符串，避免 JSON 数字精度丢失。
    pub estimated_full_cost_nanos: Option<String>,
    pub projected_cycle_tokens: Option<i64>,
    pub projected_cycle_cost_nanos: Option<String>,
}

/// 一个额度周期。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCycleView {
    /// 稳定标识：同一周期在两次查询之间必须一致。
    pub id: String,
    pub provider: ProviderId,
    pub identity_key: String,
    /// 主体展示名（导入账号的别名或邮箱）；主账号为 `None`。
    pub identity_label: Option<String>,
    pub window_kind: QuotaWindowKind,
    pub window_id: String,
    pub window_seconds: Option<u64>,
    pub start_at: String,
    pub end_at: String,
    pub scheduled_end_at: String,
    pub first_sample_at: String,
    pub last_sample_at: String,
    pub latest_used_percent: i64,
    /// 官方口径的峰值（初始片段与额外片段之和，封顶 100）。
    pub peak_used_percent: i64,
    /// `observed`（亲眼看到重置）或 `inferred`（起点由重置时刻回推）。
    pub boundary_quality: String,
    pub extra_reset_count: i64,
    /// 是不是当前正在走的周期。
    pub active: bool,
    /// 整周期用量。
    pub usage: QuotaCycleUsage,
    /// 当前片段用量。
    pub current_allowance: QuotaCycleUsage,
    pub segments: Vec<QuotaCycleSegmentView>,
    pub forecast: Option<QuotaCycleForecast>,
}

/// `usage_quota_cycles` 的返回值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCyclePage {
    /// 按窗口类型与服务排序；同一序列内按周期起点升序。
    pub cycles: Vec<QuotaCycleView>,
    pub generated_at: String,
}

/// 项目列表排序。默认值跟随排行口径，由前端显式传入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageProjectSort {
    Recent,
    Tokens,
    Cost,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProjectQuery {
    #[serde(default)]
    pub filter: UsageFilter,
    /// 按项目名或路径搜索。
    pub search: Option<String>,
    pub sort: Option<UsageProjectSort>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

/// 一个项目的用量汇总。`key` 为空串表示未归属（Cursor 远端计量与补录历史）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProjectSummary {
    /// 项目身份键：规范化后的仓库根路径；未归属项目为空串。
    pub key: String,
    /// 展示名（路径尾段）；未归属项目是固定文案。
    pub name: String,
    /// 完整项目路径；未归属项目为 `None`。
    pub path: Option<String>,
    pub unattributed: bool,
    pub conversation_count: i64,
    pub active_days: i64,
    pub first_at: Option<String>,
    pub last_at: String,
    /// 事实行数。
    pub entry_count: i64,
    /// 请求数；日粒度远端计量为 0。
    pub request_count: i64,
    pub tokens: UsageTokenTotals,
    pub fast: UsageFastTotals,
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageProjectPage {
    pub items: Vec<UsageProjectSummary>,
    pub total: i64,
    pub limit: u32,
    pub offset: u64,
}

/// 把十进制定点纳秒值序列化成不丢精度的字符串。
pub(crate) fn decimal_nanos_string(value: i64) -> String {
    let whole = value / 1_000_000_000;
    let fraction = value % 1_000_000_000;
    if fraction == 0 {
        return whole.to_string();
    }
    format!("{whole}.{fraction:09}")
        .trim_end_matches('0')
        .to_owned()
}

fn parse_decimal_nanos(value: &str) -> i64 {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole.parse::<i64>().unwrap_or(0);
    let mut fraction = fraction.chars().take(9).collect::<String>();
    fraction.extend(std::iter::repeat_n('0', 9 - fraction.len()));
    whole
        .saturating_mul(1_000_000_000)
        .saturating_add(fraction.parse::<i64>().unwrap_or(0))
}

fn decimal_option_min(left: Option<&str>, right: Option<&str>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(decimal_nanos_string(
            parse_decimal_nanos(left).min(parse_decimal_nanos(right)),
        )),
        (Some(value), None) | (None, Some(value)) => Some(value.to_owned()),
        (None, None) => None,
    }
}

fn decimal_option_max(left: Option<&str>, right: Option<&str>) -> Option<String> {
    match (left, right) {
        (Some(left), Some(right)) => Some(decimal_nanos_string(
            parse_decimal_nanos(left).max(parse_decimal_nanos(right)),
        )),
        (Some(value), None) | (None, Some(value)) => Some(value.to_owned()),
        (None, None) => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationPage {
    pub items: Vec<UsageConversation>,
    pub total: i64,
    pub limit: u32,
    pub offset: u64,
}

/// 单个对话详情里的模型／速度拆分行；列布局与 `UsageSummaryRow` 一致。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationBreakdown {
    /// 按模型聚合。
    pub models: Vec<UsageSummaryRow>,
    /// 按速度档位聚合。
    pub speeds: Vec<UsageSummaryRow>,
}

/// 额度历史中的单个事件点。`remaining_percent` 是当时该窗口的整数剩余值。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaHistoryEvent {
    pub provider: ProviderId,
    /// 不可逆身份指纹，只用于把事件归到同一账号序列，不承载账号明文。
    pub identity_key: String,
    pub window_kind: QuotaWindowKind,
    pub window_id: Option<String>,
    pub remaining_percent: i64,
    /// ISO 8601 UTC。
    pub observed_at: String,
    /// 事件时点该窗口的重置时间（ISO 8601 UTC）；缺失或旧数据为 `None`。
    pub resets_at: Option<String>,
    /// 该窗口的长度（秒）。周期起点按「重置时刻 − 窗口长度」回推，因此必须随事件一起
    /// 带出来；旧行与「窗口长度未知」的服务为 `None`（那时退回首次观察时刻）。
    pub window_seconds: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaHistoryQuery {
    pub provider: Option<ProviderId>,
    /// 起始观察时间（含）。
    pub from: Option<String>,
    /// 结束观察时间（不含）。
    pub to: Option<String>,
    /// 最多返回事件数；按最近优先截取，默认 200，上限 500。
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaHistory {
    pub events: Vec<QuotaHistoryEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageScanState {
    Idle,
    Running,
    Cancelling,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageScanStatus {
    pub state: UsageScanState,
    pub current_source: Option<UsageSource>,
    pub discovered_files: u64,
    pub completed_files: u64,
    pub bytes_read: u64,
    pub inserted_entries: u64,
    pub duplicate_entries: u64,
    pub invalid_lines: u64,
    pub failed_files: u64,
    pub partial_failure: bool,
    pub cancelled: bool,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

impl Default for UsageScanStatus {
    fn default() -> Self {
        Self {
            state: UsageScanState::Idle,
            current_source: None,
            discovered_files: 0,
            completed_files: 0,
            bytes_read: 0,
            inserted_entries: 0,
            duplicate_entries: 0,
            invalid_lines: 0,
            failed_files: 0,
            partial_failure: false,
            cancelled: false,
            started_at: None,
            finished_at: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageRepriceResult {
    pub updated_entries: u64,
    pub priced_entries: u64,
    pub unpriced_entries: u64,
    pub pricing_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PricingCatalogRefreshStatus {
    Complete,
    Partial,
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_contract_does_not_gain_path_or_content_fields() {
        let value = serde_json::to_value(UsageScanStatus::default()).expect("serialize");
        let object = value.as_object().expect("object");

        for forbidden in ["path", "fileName", "content", "body", "errorMessage"] {
            assert!(
                !object.contains_key(forbidden),
                "forbidden field {forbidden}"
            );
        }
    }
}
