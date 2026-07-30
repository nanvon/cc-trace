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
    pub model: Option<String>,
    pub speed: Option<UsageSpeed>,
    pub last_total_signature: Option<String>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeCursor {
    pub conversation_key: Option<String>,
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
