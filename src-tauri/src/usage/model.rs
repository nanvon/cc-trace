use serde::{Deserialize, Serialize};

use crate::contracts::{UsageSource, UsageSpeed};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum InferenceGeo {
    Global,
    Us,
    Unknown,
}

impl InferenceGeo {
    pub fn as_db(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Us => "us",
            Self::Unknown => "unknown",
        }
    }

    pub fn from_db(value: &str) -> Self {
        match value {
            "global" => Self::Global,
            "us" => Self::Us,
            _ => Self::Unknown,
        }
    }
}

#[derive(Clone, Default, PartialEq, Eq)]
pub struct TokenFacts {
    pub uncached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub cache_read_input_tokens: i64,
    pub cache_write_5m_input_tokens: i64,
    pub cache_write_1h_input_tokens: i64,
}

impl TokenFacts {
    pub fn input_tokens(&self) -> i64 {
        self.uncached_input_tokens
            + self.cache_read_input_tokens
            + self.cache_write_5m_input_tokens
            + self.cache_write_1h_input_tokens
    }

    pub fn total_tokens(&self) -> i64 {
        self.input_tokens() + self.output_tokens
    }

    pub fn is_valid(&self) -> bool {
        self.uncached_input_tokens >= 0
            && self.output_tokens >= 0
            && self.reasoning_output_tokens >= 0
            && self.cache_read_input_tokens >= 0
            && self.cache_write_5m_input_tokens >= 0
            && self.cache_write_1h_input_tokens >= 0
            && self.reasoning_output_tokens <= self.output_tokens
    }
}

#[derive(Clone)]
pub struct UsageEntry {
    pub source: UsageSource,
    pub dedup_key: String,
    pub conversation_key: String,
    pub model: Option<String>,
    pub speed: UsageSpeed,
    pub inference_geo: InferenceGeo,
    pub occurred_at: String,
    pub day_local: String,
    pub tokens: TokenFacts,
    pub api_equivalent_cost_nanos: Option<i64>,
    pub billing_equivalent_tokens_nanos: Option<i64>,
    pub fast_multiplier_nanos: Option<i64>,
    pub pricing_fingerprint: Option<String>,
}

#[derive(Clone)]
pub struct ConversationFact {
    pub conversation_key: String,
    pub source: UsageSource,
    pub title: Option<String>,
    pub project_hint: Option<String>,
    pub is_sidechain: bool,
    pub occurred_at: String,
    /// 原始会话 id（会话 UUID），供详情页对话 ID 与标题索引回查。
    pub source_id: Option<String>,
    /// Claude 会话的 git 分支；Codex 不提供。
    pub branch: Option<String>,
}

#[derive(Clone, Default)]
pub struct ScanBatch {
    pub entries: Vec<UsageEntry>,
    pub conversations: Vec<ConversationFact>,
    pub consumed_bytes: u64,
    pub consumed_lines: u64,
    pub invalid_lines: u64,
}

impl ScanBatch {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.conversations.clear();
        self.consumed_bytes = 0;
        self.consumed_lines = 0;
        self.invalid_lines = 0;
    }
}

#[derive(Clone)]
pub struct ScanFileState {
    pub mtime_ms: i64,
    pub size_bytes: u64,
    pub offset_bytes: u64,
    pub prefix_fingerprint: String,
    pub cursor_json: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexCursor {
    pub conversation_key: Option<String>,
    /// session_meta 的原始会话 id，供标题索引查询；会话 UUID 非身份明文。
    #[serde(default)]
    pub source_id: Option<String>,
    pub model: Option<String>,
    pub speed: Option<UsageSpeed>,
    pub last_total_signature: Option<String>,
    /// 首条 user_message 文本的标题兜底；只填首个非空值，消费后保留。
    #[serde(default)]
    pub pending_title: Option<String>,
    /// 会话 cwd 的脱敏项目提示（惰性解析后缓存，避免逐行重复解析）。
    #[serde(default)]
    pub project_hint: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCursor {
    pub conversation_key: Option<String>,
    /// 首个非 sidechain user 行文本的标题兜底；只填首个非空值，消费后保留。
    #[serde(default)]
    pub pending_title: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PiCursor {
    pub conversation_key: Option<String>,
    /// 最近一条 assistant 的 provider/model 标签，供根级 compaction 计入时沿用。
    pub model: Option<String>,
    /// 会话 cwd 的脱敏项目提示（惰性解析后缓存，避免逐行重复解析）。
    pub project_hint: Option<String>,
    /// 首个 user 消息的标题兜底；消费一次后清空，避免后续批次覆盖既有标题。
    pub pending_title: Option<String>,
    /// 文件名末尾 UUID 兜底会话键；session entry 出现后覆盖。
    pub filename_key: Option<String>,
}

/// OpenCode SQLite 会话库的增量扫描状态（非 JSONL，不适用 `scan_files` 字节水位）。
/// 全局去重由 `usage_entries` 的 `(source, dedup_key)` 唯一索引兜底，这里的水位与 seen
/// 集合负责增量与「时间戳回跳」的场景。
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodeScanState {
    /// 已处理消息的最大 `time_created`（Unix 毫秒）。
    pub watermark_ms: i64,
    /// 已见 `message.id`，用于同一水位窗口内重扫去重；按最近 20000 条保留。
    pub seen_ids: Vec<String>,
}

pub enum ParsedLine {
    Ignored,
    Invalid,
    Fact {
        entry: Box<UsageEntry>,
        conversation: Box<ConversationFact>,
    },
}

pub struct RepriceRow {
    pub id: i64,
    pub source: UsageSource,
    pub model: Option<String>,
    pub speed: UsageSpeed,
    pub inference_geo: InferenceGeo,
    pub occurred_at: String,
    pub tokens: TokenFacts,
}
