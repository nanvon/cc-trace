//! 本地用量查询与扫描的脱敏 DTO。
//!
//! 契约只返回聚合事实、公开价格估算和不含路径的对话元数据。原始 JSONL、消息正文、
//! 文件名、绝对路径与账号明文永远不跨 command 边界。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum UsageSource {
    Codex,
    Claude,
}

impl UsageSource {
    pub const ALL: [Self; 2] = [Self::Codex, Self::Claude];

    pub fn as_db(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::Claude => "claude",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct UsageSummaryRow {
    pub key: String,
    pub entry_count: i64,
    pub tokens: UsageTokenTotals,
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub rows: Vec<UsageSummaryRow>,
    pub entry_count: i64,
    pub tokens: UsageTokenTotals,
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationQuery {
    #[serde(default)]
    pub filter: UsageFilter,
    pub search: Option<String>,
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
    pub cost: UsageCostTotals,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageConversationPage {
    pub items: Vec<UsageConversation>,
    pub total: i64,
    pub limit: u32,
    pub offset: u64,
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
