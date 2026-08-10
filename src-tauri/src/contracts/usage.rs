//! 本地用量查询与扫描的脱敏 DTO。
//!
//! 契约只返回聚合事实、公开价格估算和不含路径的对话元数据。原始 JSONL、消息正文、
//! 文件名、绝对路径与账号明文永远不跨 command 边界。

use serde::{Deserialize, Serialize};

use super::quota::{ProviderId, QuotaWindowKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    Codex,
    Claude,
    Pi,
    Opencode,
}

impl UsageSource {
    /// 参与在线定价与 fingerprint 的数据源；Pi 与 OpenCode 自带 cost，不参与价格目录。
    pub const ALL: [Self; 2] = [Self::Codex, Self::Claude];

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
            Self::Pi => "pi",
            Self::Opencode => "opencode",
        }
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
    Day,
    Source,
    Model,
    Speed,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilter {
    pub from: Option<String>,
    pub to: Option<String>,
    pub source: Option<UsageSource>,
    pub model: Option<String>,
    pub speed: Option<UsageSpeed>,
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
    pub entry_count: i64,
    pub tokens: UsageTokenTotals,
    pub fast: UsageFastTotals,
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub rows: Vec<UsageSummaryRow>,
    pub entry_count: i64,
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
    /// 精确匹配脱敏项目提示。
    pub project: Option<String>,
    pub sort: Option<UsageConversationSort>,
    /// 可见服务集合；为空或缺失时不额外过滤（由前端保证传可见集合以统一服务过滤）。
    pub sources: Option<Vec<UsageSource>>,
    pub limit: Option<u32>,
    pub offset: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversation {
    pub conversation_key: String,
    pub source: UsageSource,
    pub title: Option<String>,
    pub project_hint: Option<String>,
    pub is_sidechain: bool,
    pub first_at: String,
    pub last_at: String,
    pub entry_count: i64,
    pub tokens: UsageTokenTotals,
    pub fast: UsageFastTotals,
    pub cost: UsageCostTotals,
    /// 原始会话 id（会话 UUID），供详情页展示与复制；非账号明文。
    pub source_id: Option<String>,
    /// Claude 会话的 git 分支（JSONL `gitBranch`）；Codex 不提供。
    pub branch: Option<String>,
    /// 会话涉及的去重模型列表，按模型名排序；不受查询过滤影响。
    pub models: Vec<String>,
}

/// 对话项目筛选选项：脱敏项目名与其可见对话数、最近活动时间。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationProjectOption {
    pub name: String,
    pub conversation_count: i64,
    pub last_at: String,
}

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
