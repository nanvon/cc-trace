use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Local, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::contracts::{UsageSource, UsageSpeed};

use super::model::{
    ClaudeCursor, CodexCursor, ConversationFact, InferenceGeo, ParsedLine, PiCursor, TokenFacts,
    UsageEntry,
};
use super::pricing::PricingCatalog;

pub fn parse_codex_line(
    line: &[u8],
    cursor: &mut CodexCursor,
    catalog: &PricingCatalog,
    titles: &HashMap<String, String>,
) -> ParsedLine {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return ParsedLine::Invalid;
    };
    let kind = value.get("type").and_then(Value::as_str);
    let payload = value.get("payload").unwrap_or(&Value::Null);

    match kind {
        Some("session_meta") => {
            if let Some(id) = payload
                .get("id")
                .or_else(|| payload.get("session_id"))
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
            {
                cursor.source_id = Some(id.to_owned());
                cursor.conversation_key = Some(opaque_key("codex-conversation", id));
            }
            if let Some(model) = payload.get("model").and_then(Value::as_str) {
                cursor.model = normalized_optional(model);
            }
            if let Some(cwd) = payload.get("cwd").and_then(Value::as_str) {
                cursor.project_hint = project_hint(cwd);
            }
            ParsedLine::Ignored
        }
        Some("turn_context") => {
            if let Some(model) = payload.get("model").and_then(Value::as_str) {
                cursor.model = normalized_optional(model);
            }
            if let Some(tier) = payload
                .get("service_tier")
                .or_else(|| payload.pointer("/thread_settings/service_tier"))
                .and_then(Value::as_str)
            {
                cursor.speed = Some(normalize_speed(Some(tier)));
            }
            ParsedLine::Ignored
        }
        Some("event_msg") => {
            if payload.get("type").and_then(Value::as_str) == Some("user_message") {
                if cursor.pending_title.is_none() {
                    cursor.pending_title = payload
                        .get("message")
                        .and_then(content_text)
                        .or_else(|| payload.get("text_elements").and_then(text_elements_title));
                }
                return ParsedLine::Ignored;
            }

            if payload.get("type").and_then(Value::as_str) == Some("thread_settings_applied") {
                if let Some(model) = payload
                    .pointer("/thread_settings/model")
                    .and_then(Value::as_str)
                {
                    cursor.model = normalized_optional(model);
                }
                if let Some(tier) = payload
                    .pointer("/thread_settings/service_tier")
                    .and_then(Value::as_str)
                {
                    cursor.speed = Some(normalize_speed(Some(tier)));
                }
                return ParsedLine::Ignored;
            }

            if payload.get("type").and_then(Value::as_str) != Some("token_count") {
                return ParsedLine::Ignored;
            }

            let Some(info) = payload.get("info") else {
                return ParsedLine::Invalid;
            };
            let Some(last) = info.get("last_token_usage") else {
                return ParsedLine::Ignored;
            };
            let Some(total) = info.get("total_token_usage") else {
                return ParsedLine::Invalid;
            };

            let Some(total_signature) = token_signature(total) else {
                return ParsedLine::Invalid;
            };
            if cursor.last_total_signature.as_deref() == Some(&total_signature) {
                return ParsedLine::Ignored;
            }

            let Some(tokens) = codex_tokens(last) else {
                return ParsedLine::Invalid;
            };
            if !codex_total_is_valid(total) {
                return ParsedLine::Invalid;
            }
            let Some(conversation_key) = cursor.conversation_key.clone() else {
                return ParsedLine::Invalid;
            };
            let Some((occurred_at, day_local)) = normalized_time(
                value
                    .get("timestamp")
                    .and_then(Value::as_str)
                    .or_else(|| payload.get("timestamp").and_then(Value::as_str)),
            ) else {
                return ParsedLine::Invalid;
            };

            let model = cursor.model.clone();
            let speed = cursor.speed.unwrap_or(UsageSpeed::Standard);
            let dedup_key = hash_parts(&[
                "codex-entry",
                &conversation_key,
                &occurred_at,
                model.as_deref().unwrap_or(""),
                speed.as_db(),
                &total_signature,
            ]);
            let mut entry = UsageEntry {
                source: UsageSource::Codex,
                dedup_key,
                conversation_key: conversation_key.clone(),
                model,
                speed,
                inference_geo: InferenceGeo::Global,
                occurred_at: occurred_at.clone(),
                day_local,
                tokens,
                api_equivalent_cost_nanos: None,
                billing_equivalent_tokens_nanos: None,
                fast_multiplier_nanos: None,
                pricing_fingerprint: None,
            };
            apply_price(&mut entry, catalog);
            cursor.last_total_signature = Some(total_signature);

            ParsedLine::Fact {
                entry: Box::new(entry),
                conversation: Box::new(ConversationFact {
                    conversation_key,
                    source: UsageSource::Codex,
                    title: cursor
                        .source_id
                        .as_deref()
                        .and_then(|id| titles.get(id))
                        .cloned()
                        .or_else(|| cursor.pending_title.clone()),
                    project_hint: cursor.project_hint.clone(),
                    is_sidechain: false,
                    occurred_at,
                    source_id: cursor.source_id.clone(),
                    branch: None,
                }),
            }
        }
        _ => ParsedLine::Ignored,
    }
}

pub fn parse_claude_line(
    line: &[u8],
    cursor: &mut ClaudeCursor,
    catalog: &PricingCatalog,
    titles: &HashMap<String, String>,
) -> ParsedLine {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return ParsedLine::Invalid;
    };
    if value.get("type").and_then(Value::as_str) == Some("user") {
        if cursor.pending_title.is_none()
            && !value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        {
            cursor.pending_title = value.get("message").and_then(user_message_title);
        }
        return ParsedLine::Ignored;
    }
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return ParsedLine::Ignored;
    }

    let Some(session_id) = value
        .get("sessionId")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return ParsedLine::Invalid;
    };
    let conversation_key = opaque_key("claude-conversation", session_id);
    cursor.conversation_key = Some(conversation_key.clone());

    let Some(message) = value.get("message") else {
        return ParsedLine::Invalid;
    };
    let Some(message_id) = message
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return ParsedLine::Invalid;
    };
    if message.get("stop_reason").and_then(Value::as_str).is_none() {
        return ParsedLine::Ignored;
    }
    let Some(usage) = message.get("usage") else {
        return ParsedLine::Invalid;
    };
    let Some(tokens) = claude_tokens(usage) else {
        return ParsedLine::Invalid;
    };
    let Some((occurred_at, day_local)) =
        normalized_time(value.get("timestamp").and_then(Value::as_str))
    else {
        return ParsedLine::Invalid;
    };

    let model = message
        .get("model")
        .and_then(Value::as_str)
        .and_then(normalized_optional);
    let speed = normalize_speed(usage.get("speed").and_then(Value::as_str));
    let inference_geo = match usage.get("inference_geo").and_then(Value::as_str) {
        Some(value) if value.eq_ignore_ascii_case("us") => InferenceGeo::Us,
        Some(value) if value.eq_ignore_ascii_case("global") => InferenceGeo::Global,
        _ => InferenceGeo::Unknown,
    };
    let mut entry = UsageEntry {
        source: UsageSource::Claude,
        dedup_key: hash_parts(&["claude-entry", message_id]),
        conversation_key: conversation_key.clone(),
        model,
        speed,
        inference_geo,
        occurred_at: occurred_at.clone(),
        day_local,
        tokens,
        api_equivalent_cost_nanos: None,
        billing_equivalent_tokens_nanos: None,
        fast_multiplier_nanos: None,
        pricing_fingerprint: None,
    };
    apply_price(&mut entry, catalog);

    let project_hint = value
        .get("cwd")
        .and_then(Value::as_str)
        .and_then(project_hint);

    ParsedLine::Fact {
        entry: Box::new(entry),
        conversation: Box::new(ConversationFact {
            conversation_key,
            source: UsageSource::Claude,
            title: titles
                .get(session_id)
                .cloned()
                .or_else(|| cursor.pending_title.clone()),
            project_hint,
            is_sidechain: value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            occurred_at,
            source_id: Some(session_id.to_owned()),
            branch: value
                .get("gitBranch")
                .and_then(Value::as_str)
                .and_then(normalized_optional),
        }),
    }
}

/// 解析 Pi 会话 JSONL 的一行。pi 的 usage 与 cost 都随消息自带，不走价格表。
///
/// 规则按 `docs/Pi数据源.md`：仅 assistant 消息与根级带 usage 的 compaction／branch_summary
/// 计入；`toolResult` 嵌套 usage 不计；会话键 `pi:<session id>`（缺失时用文件名 UUID 兜底）；
/// 全局去重由 `usage_entries` 的 `(source, dedup_key)` 唯一索引承担，键为 entry id@entry 时间戳。
pub fn parse_pi_line(line: &[u8], cursor: &mut PiCursor, filename_key: &str) -> ParsedLine {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return ParsedLine::Invalid;
    };
    let Some(entry_id) = value
        .get("id")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return ParsedLine::Invalid;
    };
    let entry_timestamp = value.get("timestamp").and_then(Value::as_str);
    let kind = value.get("type").and_then(Value::as_str);

    match kind {
        Some("session") => {
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                cursor.conversation_key = Some(opaque_key("pi-conversation", id));
            }
            if let Some(cwd) = value.get("cwd").and_then(Value::as_str) {
                cursor.project_hint = project_hint(cwd);
            }
            ParsedLine::Ignored
        }
        Some("message") => {
            let Some(message) = value.get("message") else {
                return ParsedLine::Ignored;
            };
            match message.get("role").and_then(Value::as_str) {
                Some("user") => {
                    if cursor.pending_title.is_none() {
                        cursor.pending_title = user_message_title(message);
                    }
                    ParsedLine::Ignored
                }
                Some("assistant") => {
                    let provider = message
                        .get("provider")
                        .and_then(Value::as_str)
                        .and_then(normalized_optional);
                    let model = message
                        .get("model")
                        .and_then(Value::as_str)
                        .and_then(normalized_optional);
                    if model.is_some() {
                        cursor.model = model.clone();
                    }
                    pi_usage_fact(
                        message,
                        provider,
                        model,
                        cursor,
                        entry_id,
                        entry_timestamp,
                        filename_key,
                    )
                }
                _ => ParsedLine::Ignored,
            }
        }
        Some("compaction") | Some("branch_summary") => {
            if value.get("usage").is_none() {
                return ParsedLine::Ignored;
            }
            // 生成摘要的 LLM 开销：模型沿用文件内最近一条 assistant 的标签。
            let model = cursor.model.clone();
            pi_usage_fact(
                &value,
                None,
                model,
                cursor,
                entry_id,
                entry_timestamp,
                filename_key,
            )
        }
        _ => ParsedLine::Ignored,
    }
}

/// 从 pi 的 usage 对象构造用量事实。`usage_holder` 是携带 `usage` 的 JSON 节点
/// （message 或根级 compaction）；`usage_holder` 为根级时 `provider` 为 `None`。
fn pi_usage_fact(
    usage_holder: &Value,
    provider: Option<String>,
    model: Option<String>,
    cursor: &mut PiCursor,
    entry_id: &str,
    entry_timestamp: Option<&str>,
    filename_key: &str,
) -> ParsedLine {
    let Some(usage) = usage_holder.get("usage") else {
        return ParsedLine::Invalid;
    };
    let Some(tokens) = pi_tokens(usage) else {
        return ParsedLine::Invalid;
    };
    let cost_nanos = pi_cost_nanos(usage);
    if tokens.total_tokens() <= 0 && cost_nanos.is_none() {
        return ParsedLine::Invalid;
    }
    let Some((occurred_at, day_local)) = pi_time(usage_holder, entry_timestamp) else {
        return ParsedLine::Invalid;
    };
    let conversation_key = cursor
        .conversation_key
        .clone()
        .unwrap_or_else(|| filename_conversation_key(filename_key));
    let model = pi_model_label(provider, model);
    let dedup_key = hash_parts(&[
        "pi-entry",
        entry_id,
        entry_timestamp.unwrap_or(&occurred_at),
    ]);

    let fact_entry = UsageEntry {
        source: UsageSource::Pi,
        dedup_key,
        conversation_key: conversation_key.clone(),
        model,
        speed: UsageSpeed::Standard,
        inference_geo: InferenceGeo::Global,
        occurred_at: occurred_at.clone(),
        day_local,
        tokens,
        api_equivalent_cost_nanos: cost_nanos,
        billing_equivalent_tokens_nanos: None,
        fast_multiplier_nanos: None,
        pricing_fingerprint: None,
    };

    let project_hint = cursor.project_hint.clone();
    let title = cursor.pending_title.take();
    ParsedLine::Fact {
        entry: Box::new(fact_entry),
        conversation: Box::new(ConversationFact {
            conversation_key,
            source: UsageSource::Pi,
            title,
            project_hint,
            is_sidechain: false,
            occurred_at,
            source_id: None,
            branch: None,
        }),
    }
}

/// 模型标签：`provider/model`，provider 缺失时只有模型名；均缺失时为 `None`。
fn pi_model_label(provider: Option<String>, model: Option<String>) -> Option<String> {
    match (provider, model) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (Some(provider), None) => Some(provider),
        (None, Some(model)) => Some(model),
        (None, None) => None,
    }
}

fn pi_tokens(usage: &Value) -> Option<TokenFacts> {
    let input = integer_or_zero(usage, "input")?;
    let output = integer_or_zero(usage, "output")?;
    let cache_read = integer_or_zero(usage, "cacheRead")?;
    let cache_write = integer_or_zero(usage, "cacheWrite")?;
    let reasoning = integer_or_zero(usage, "reasoning")?;
    let total = integer_or_zero(usage, "totalTokens")?;
    if total
        != input
            .saturating_add(output)
            .saturating_add(cache_read)
            .saturating_add(cache_write)
    {
        return None;
    }
    if reasoning > output {
        return None;
    }
    let facts = TokenFacts {
        uncached_input_tokens: input,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        cache_read_input_tokens: cache_read,
        cache_write_5m_input_tokens: cache_write,
        cache_write_1h_input_tokens: 0,
    };
    facts.is_valid().then_some(facts)
}

/// `usage.cost.total`（美元小数）换算为整数 USD nanos。缺失或非数值返回 `None`（未定价）；
/// `0` 是 pi 明确算出的真实零值，保留为 `Some(0)`。
fn pi_cost_nanos(usage: &Value) -> Option<i64> {
    let total = usage
        .get("cost")
        .and_then(|cost| cost.get("total"))
        .and_then(Value::as_f64)?;
    Some((total * 1_000_000_000.0).round() as i64)
}

/// 消息时间优先 `message.timestamp`（Unix 毫秒），缺失时退回 entry 的 ISO 时间戳。
fn pi_time(usage_holder: &Value, entry_timestamp: Option<&str>) -> Option<(String, String)> {
    if let Some(ts) = usage_holder.get("timestamp").and_then(Value::as_u64) {
        let utc = DateTime::from_timestamp_millis(i64::try_from(ts).ok()?)?;
        let local = utc.with_timezone(&Local);
        return Some((
            utc.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            local.format("%Y-%m-%d").to_string(),
        ));
    }
    normalized_time(entry_timestamp)
}

fn filename_conversation_key(filename_key: &str) -> String {
    opaque_key("pi-conversation", filename_key)
}

/// 标题清理：空白折叠、去掉 `<` 前缀、截 80 字符；空结果返回 `None`。
/// 语义对齐 cc-bar `ConversationTitleIndex.clean`，供标题索引与消息兜底共用。
pub(crate) fn clean_title(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let without_prefix = collapsed.strip_prefix('<').unwrap_or(&collapsed).trim();
    if without_prefix.is_empty() {
        return None;
    }
    Some(without_prefix.chars().take(80).collect())
}

/// 从消息节点的 `content` 提取纯文本标题兜底：字符串内容，或数组里首个 `text` 项。
fn content_text(content: &Value) -> Option<String> {
    content.as_str().and_then(clean_title).or_else(|| {
        content.as_array().and_then(|items| {
            items
                .iter()
                .find_map(|item| item.get("text").and_then(Value::as_str))
                .and_then(clean_title)
        })
    })
}

/// Codex `user_message` 的 `text_elements` 数组标题兜底：取首个非空 `text` 项。
fn text_elements_title(value: &Value) -> Option<String> {
    value.as_array().and_then(|items| {
        items
            .iter()
            .find_map(|item| item.get("text").and_then(Value::as_str))
            .and_then(clean_title)
    })
}

/// 首条 user 消息的纯文本标题兜底：`message.content` 的字符串或数组 `text` 项。
fn user_message_title(message: &Value) -> Option<String> {
    message.get("content").and_then(content_text)
}

fn codex_tokens(value: &Value) -> Option<TokenFacts> {
    let input = integer(value, "input_tokens")?;
    let output = integer(value, "output_tokens")?;
    let cached = integer_or_zero(value, "cached_input_tokens")?;
    let cache_write = codex_cache_write_tokens(value)?;
    let reasoning = integer_or_zero(value, "reasoning_output_tokens")?;
    let total = integer(value, "total_tokens")?;

    let uncached = input.checked_sub(cached)?.checked_sub(cache_write)?;
    if total != input.checked_add(output)? || reasoning > output {
        return None;
    }
    let facts = TokenFacts {
        uncached_input_tokens: uncached,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        cache_read_input_tokens: cached,
        cache_write_5m_input_tokens: cache_write,
        cache_write_1h_input_tokens: 0,
    };
    facts.is_valid().then_some(facts)
}

fn codex_total_is_valid(value: &Value) -> bool {
    let Some(input) = integer(value, "input_tokens") else {
        return false;
    };
    let Some(output) = integer(value, "output_tokens") else {
        return false;
    };
    let Some(total) = integer(value, "total_tokens") else {
        return false;
    };
    let Some(cached) = integer_or_zero(value, "cached_input_tokens") else {
        return false;
    };
    let Some(cache_write) = codex_cache_write_tokens(value) else {
        return false;
    };
    let Some(reasoning) = integer_or_zero(value, "reasoning_output_tokens") else {
        return false;
    };

    total == input.saturating_add(output)
        && input >= cached.saturating_add(cache_write)
        && output >= reasoning
}

fn claude_tokens(value: &Value) -> Option<TokenFacts> {
    let input = integer_or_zero(value, "input_tokens")?;
    let output = integer_or_zero(value, "output_tokens")?;
    let cache_read = integer_or_zero(value, "cache_read_input_tokens")?;
    let nested = value.get("cache_creation");
    let aggregate = optional_integer(value, "cache_creation_input_tokens")?;
    let detail_5m = match nested {
        Some(item) => optional_integer(item, "ephemeral_5m_input_tokens")?,
        None => None,
    };
    let detail_1h = match nested {
        Some(item) => optional_integer(item, "ephemeral_1h_input_tokens")?,
        None => None,
    };
    let (write_5m, write_1h) = if let Some(total) = aggregate {
        let write_1h = detail_1h.unwrap_or(0).min(total);
        (total - write_1h, write_1h)
    } else {
        (detail_5m.unwrap_or(0), detail_1h.unwrap_or(0))
    };

    let facts = TokenFacts {
        uncached_input_tokens: input,
        output_tokens: output,
        reasoning_output_tokens: 0,
        cache_read_input_tokens: cache_read,
        cache_write_5m_input_tokens: write_5m,
        cache_write_1h_input_tokens: write_1h,
    };
    facts.is_valid().then_some(facts)
}

fn token_signature(value: &Value) -> Option<String> {
    let fields = [
        integer(value, "input_tokens")?,
        integer(value, "output_tokens")?,
        integer_or_zero(value, "cached_input_tokens")?,
        codex_cache_write_tokens(value)?,
        integer_or_zero(value, "reasoning_output_tokens")?,
        integer(value, "total_tokens")?,
    ];
    Some(hash_parts(&[
        "codex-total",
        &fields[0].to_string(),
        &fields[1].to_string(),
        &fields[2].to_string(),
        &fields[3].to_string(),
        &fields[4].to_string(),
        &fields[5].to_string(),
    ]))
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| i64::try_from(value).ok())
}

fn integer_or_zero(value: &Value, key: &str) -> Option<i64> {
    value.get(key).map_or(Some(0), |value| {
        value.as_u64().and_then(|value| i64::try_from(value).ok())
    })
}

fn optional_integer(value: &Value, key: &str) -> Option<Option<i64>> {
    value.get(key).map_or(Some(None), |value| {
        value
            .as_u64()
            .and_then(|value| i64::try_from(value).ok())
            .map(Some)
    })
}

fn codex_cache_write_tokens(value: &Value) -> Option<i64> {
    for path in [
        "/cache_write_input_tokens",
        "/cache_write_tokens",
        "/input_tokens_details/cache_write_tokens",
        "/prompt_tokens_details/cache_write_tokens",
        "/token_details/cache_write_tokens",
    ] {
        if let Some(item) = value.pointer(path) {
            return item.as_u64().and_then(|value| i64::try_from(value).ok());
        }
    }
    Some(0)
}

fn normalized_time(value: Option<&str>) -> Option<(String, String)> {
    let value = value?;
    let parsed = DateTime::parse_from_rfc3339(value).ok()?;
    let utc = parsed.with_timezone(&Utc);
    let local = utc.with_timezone(&Local);
    Some((
        utc.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        local.format("%Y-%m-%d").to_string(),
    ))
}

fn normalized_optional(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn normalize_speed(value: Option<&str>) -> UsageSpeed {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("default") | Some("standard") => UsageSpeed::Standard,
        Some("fast") | Some("priority") => UsageSpeed::Fast,
        _ => UsageSpeed::Unknown,
    }
}

pub(crate) fn project_hint(value: &str) -> Option<String> {
    Path::new(value)
        .file_name()
        .and_then(|value| value.to_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(120).collect())
}

fn opaque_key(namespace: &str, value: &str) -> String {
    hash_parts(&[namespace, value])
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn apply_price(entry: &mut UsageEntry, catalog: &PricingCatalog) {
    let estimate = catalog.estimate_entry(entry);
    let (billing_equivalent, multiplier) = catalog.fast_billing_equivalent(
        entry.source,
        entry.model.as_deref(),
        entry.speed,
        entry.tokens.total_tokens(),
    );
    entry.api_equivalent_cost_nanos = estimate.cost_nanos;
    entry.billing_equivalent_tokens_nanos = billing_equivalent;
    entry.fast_multiplier_nanos = multiplier;
    entry.pricing_fingerprint = estimate
        .cost_nanos
        .map(|_| catalog.fingerprint().to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(value: &str) -> Vec<u8> {
        value.as_bytes().to_vec()
    }

    #[test]
    fn pi_counts_assistant_only_with_own_cost_and_provider_model_label() {
        let mut cursor = PiCursor {
            conversation_key: Some("pi-key".to_owned()),
            model: None,
            project_hint: Some("project".to_owned()),
            pending_title: None,
            filename_key: None,
        };
        let parsed = parse_pi_line(
            &line(
                r#"{"id":"a1","type":"message","timestamp":"2026-08-01T01:00:00Z","message":{"role":"assistant","provider":"deepseek","model":"deepseek-v4-flash","usage":{"input":100,"output":20,"cacheRead":0,"cacheWrite":0,"reasoning":5,"totalTokens":120,"cost":{"total":0.0000196}}}}"#,
            ),
            &mut cursor,
            "20260801-abc",
        );

        let ParsedLine::Fact {
            entry,
            conversation,
        } = parsed
        else {
            panic!("expected fact");
        };
        assert_eq!(entry.source, UsageSource::Pi);
        assert_eq!(entry.tokens.uncached_input_tokens, 100);
        assert_eq!(entry.tokens.output_tokens, 20);
        assert_eq!(entry.tokens.reasoning_output_tokens, 5);
        assert_eq!(entry.tokens.total_tokens(), 120);
        assert_eq!(entry.api_equivalent_cost_nanos, Some(19_600));
        assert_eq!(entry.model.as_deref(), Some("deepseek/deepseek-v4-flash"));
        assert_eq!(entry.speed, UsageSpeed::Standard);
        assert_eq!(entry.pricing_fingerprint, None);
        assert_eq!(conversation.conversation_key, "pi-key");
        assert_eq!(conversation.project_hint.as_deref(), Some("project"));
    }

    #[test]
    fn pi_ignores_tool_result_user_and_label_entries() {
        let mut cursor = PiCursor::default();
        let tool_result = parse_pi_line(
            &line(
                r#"{"id":"t1","type":"message","timestamp":"2026-08-01T01:00:00Z","message":{"role":"toolResult","tool_use_id":"x","usage":{"input":999,"output":999,"cacheRead":0,"cacheWrite":0,"totalTokens":1998,"cost":{"total":0.5}}}}"#,
            ),
            &mut cursor,
            "file",
        );
        assert!(matches!(tool_result, ParsedLine::Ignored));

        let user = parse_pi_line(
            &line(
                r#"{"id":"u1","type":"message","timestamp":"2026-08-01T01:00:00Z","message":{"role":"user","content":"hello"}}"#,
            ),
            &mut cursor,
            "file",
        );
        assert!(matches!(user, ParsedLine::Ignored));
        assert_eq!(cursor.pending_title.as_deref(), Some("hello"));

        let label = parse_pi_line(
            &line(r#"{"id":"l1","type":"label","timestamp":"2026-08-01T01:00:00Z","content":"x"}"#),
            &mut cursor,
            "file",
        );
        assert!(matches!(label, ParsedLine::Ignored));
    }

    #[test]
    fn pi_compaction_uses_cursor_model_and_entry_timestamp() {
        let mut cursor = PiCursor {
            conversation_key: Some("pi-key".to_owned()),
            model: Some("deepseek/deepseek-v4-flash".to_owned()),
            project_hint: None,
            pending_title: None,
            filename_key: None,
        };
        let parsed = parse_pi_line(
            &line(
                r#"{"id":"c1","type":"compaction","timestamp":"2026-08-01T03:00:00Z","usage":{"input":200,"output":50,"cacheRead":0,"cacheWrite":0,"totalTokens":250,"cost":{"total":0.000042}}}"#,
            ),
            &mut cursor,
            "file",
        );

        let ParsedLine::Fact { entry, .. } = parsed else {
            panic!("expected fact");
        };
        assert_eq!(entry.tokens.total_tokens(), 250);
        assert_eq!(entry.api_equivalent_cost_nanos, Some(42_000));
        assert_eq!(entry.model.as_deref(), Some("deepseek/deepseek-v4-flash"));
        assert_eq!(entry.occurred_at, "2026-08-01T03:00:00.000Z");
    }

    #[test]
    fn pi_rejects_inconsistent_total_and_absent_cost_with_zero_tokens() {
        let mut cursor = PiCursor::default();
        let inconsistent = parse_pi_line(
            &line(
                r#"{"id":"i1","type":"message","timestamp":"2026-08-01T01:00:00Z","message":{"role":"assistant","model":"m","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":5,"cost":{"total":0.001}}}}"#,
            ),
            &mut cursor,
            "file",
        );
        assert!(matches!(inconsistent, ParsedLine::Invalid));

        let zero_without_cost = parse_pi_line(
            &line(
                r#"{"id":"z1","type":"message","timestamp":"2026-08-01T01:00:00Z","message":{"role":"assistant","model":"m","usage":{"input":0,"output":0,"cacheRead":0,"cacheWrite":0,"totalTokens":0}}}"#,
            ),
            &mut cursor,
            "file",
        );
        assert!(matches!(zero_without_cost, ParsedLine::Invalid));
    }

    #[test]
    fn pi_falls_back_to_filename_conversation_key_when_session_entry_is_missing() {
        let mut cursor = PiCursor::default();
        let parsed = parse_pi_line(
            &line(
                r#"{"id":"m1","type":"message","timestamp":"2026-08-01T01:00:00Z","message":{"role":"assistant","model":"m","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"totalTokens":2,"cost":{"total":0.000001}}}}"#,
            ),
            &mut cursor,
            "20260801-abc123",
        );

        let ParsedLine::Fact { entry, .. } = parsed else {
            panic!("expected fact");
        };
        assert_eq!(
            entry.conversation_key,
            opaque_key("pi-conversation", "20260801-abc123")
        );
    }

    #[test]
    fn codex_subtracts_cache_facts_and_does_not_double_reasoning() {
        let mut cursor = CodexCursor {
            conversation_key: Some("conversation".to_owned()),
            model: Some("gpt-5.6-sol".to_owned()),
            speed: Some(UsageSpeed::Standard),
            last_total_signature: None,
            ..CodexCursor::default()
        };
        let line = br#"{"timestamp":"2026-07-30T01:02:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":30,"cached_input_tokens":40,"cache_write_input_tokens":10,"reasoning_output_tokens":5,"total_tokens":130},"total_token_usage":{"input_tokens":100,"output_tokens":30,"cached_input_tokens":40,"cache_write_input_tokens":10,"reasoning_output_tokens":5,"total_tokens":130}}}}"#;

        let ParsedLine::Fact { entry, .. } = parse_codex_line(
            line,
            &mut cursor,
            &PricingCatalog::bundled(),
            &HashMap::new(),
        ) else {
            panic!("expected fact");
        };

        assert_eq!(entry.tokens.uncached_input_tokens, 50);
        assert_eq!(entry.tokens.cache_read_input_tokens, 40);
        assert_eq!(entry.tokens.cache_write_5m_input_tokens, 10);
        assert_eq!(entry.tokens.total_tokens(), 130);
        assert_eq!(entry.tokens.reasoning_output_tokens, 5);
    }

    #[test]
    fn repeated_codex_total_signature_is_ignored() {
        let mut cursor = CodexCursor {
            conversation_key: Some("conversation".to_owned()),
            model: Some("gpt-5.6-sol".to_owned()),
            speed: Some(UsageSpeed::Standard),
            last_total_signature: None,
            ..CodexCursor::default()
        };
        let line = br#"{"timestamp":"2026-07-30T01:02:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#;

        assert!(matches!(
            parse_codex_line(
                line,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Fact { .. }
        ));
        assert!(matches!(
            parse_codex_line(
                line,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
    }

    #[test]
    fn claude_maps_both_cache_write_ttls() {
        let line = br#"{"type":"assistant","sessionId":"session","timestamp":"2026-07-30T01:02:03Z","cwd":"/private/project-alpha","isSidechain":false,"message":{"id":"message","model":"claude-opus-5","stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":10,"cache_read_input_tokens":30,"cache_creation_input_tokens":11,"cache_creation":{"ephemeral_5m_input_tokens":7,"ephemeral_1h_input_tokens":4},"speed":"standard","inference_geo":"us"}}}"#;
        let ParsedLine::Fact {
            entry,
            conversation,
        } = parse_claude_line(
            line,
            &mut ClaudeCursor::default(),
            &PricingCatalog::bundled(),
            &HashMap::new(),
        )
        else {
            panic!("expected fact");
        };

        assert_eq!(entry.tokens.uncached_input_tokens, 20);
        assert_eq!(entry.tokens.cache_write_5m_input_tokens, 7);
        assert_eq!(entry.tokens.cache_write_1h_input_tokens, 4);
        assert_eq!(conversation.project_hint.as_deref(), Some("project-alpha"));
    }

    #[test]
    fn claude_aggregate_cache_write_is_authoritative() {
        let facts = claude_tokens(&serde_json::json!({
            "input_tokens": 1,
            "output_tokens": 1,
            "cache_creation_input_tokens": 10,
            "cache_creation": {
                "ephemeral_5m_input_tokens": 9,
                "ephemeral_1h_input_tokens": 4
            }
        }))
        .expect("valid facts");

        assert_eq!(facts.cache_write_5m_input_tokens, 6);
        assert_eq!(facts.cache_write_1h_input_tokens, 4);
    }

    #[test]
    fn claude_without_aggregate_sums_cache_write_details() {
        let facts = claude_tokens(&serde_json::json!({
            "input_tokens": 1,
            "output_tokens": 1,
            "cache_creation": {
                "ephemeral_5m_input_tokens": 7,
                "ephemeral_1h_input_tokens": 4
            }
        }))
        .expect("valid facts");

        assert_eq!(facts.cache_write_5m_input_tokens, 7);
        assert_eq!(facts.cache_write_1h_input_tokens, 4);
    }

    #[test]
    fn codex_accepts_all_supported_cache_write_paths() {
        for value in [
            serde_json::json!({
                "input_tokens": 100,
                "output_tokens": 20,
                "cached_input_tokens": 30,
                "cache_write_input_tokens": 10,
                "total_tokens": 120
            }),
            serde_json::json!({
                "input_tokens": 100,
                "output_tokens": 20,
                "cached_input_tokens": 30,
                "cache_write_tokens": 10,
                "total_tokens": 120
            }),
            serde_json::json!({
                "input_tokens": 100,
                "output_tokens": 20,
                "cached_input_tokens": 30,
                "input_tokens_details": { "cache_write_tokens": 10 },
                "total_tokens": 120
            }),
            serde_json::json!({
                "input_tokens": 100,
                "output_tokens": 20,
                "cached_input_tokens": 30,
                "prompt_tokens_details": { "cache_write_tokens": 10 },
                "total_tokens": 120
            }),
            serde_json::json!({
                "input_tokens": 100,
                "output_tokens": 20,
                "cached_input_tokens": 30,
                "token_details": { "cache_write_tokens": 10 },
                "total_tokens": 120
            }),
        ] {
            let facts = codex_tokens(&value).expect("valid facts");

            assert_eq!(facts.uncached_input_tokens, 60);
            assert_eq!(facts.cache_read_input_tokens, 30);
            assert_eq!(facts.cache_write_5m_input_tokens, 10);
        }
    }

    #[test]
    fn codex_speed_state_switches_between_fast_and_standard_entries() {
        let catalog = PricingCatalog::bundled();
        let mut cursor = CodexCursor::default();
        assert!(matches!(
            parse_codex_line(
                br#"{"type":"session_meta","payload":{"id":"session","model":"gpt-5.6-sol"}}"#,
                &mut cursor,
                &catalog,
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        assert!(matches!(
            parse_codex_line(
                br#"{"type":"turn_context","payload":{"service_tier":"priority"}}"#,
                &mut cursor,
                &catalog,
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        let ParsedLine::Fact { entry: fast, .. } = parse_codex_line(
            br#"{"timestamp":"2026-07-30T01:02:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#,
            &mut cursor,
            &catalog,
            &HashMap::new(),
        ) else {
            panic!("expected fast fact");
        };
        assert_eq!(fast.speed, UsageSpeed::Fast);

        assert!(matches!(
            parse_codex_line(
                br#"{"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"service_tier":"standard"}}}"#,
                &mut cursor,
                &catalog,
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        let ParsedLine::Fact {
            entry: standard, ..
        } = parse_codex_line(
            br#"{"timestamp":"2026-07-30T01:03:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":2,"output_tokens":2,"total_tokens":4},"total_token_usage":{"input_tokens":2,"output_tokens":2,"total_tokens":4}}}}"#,
            &mut cursor,
            &catalog,
            &HashMap::new(),
        ) else {
            panic!("expected standard fact");
        };
        assert_eq!(standard.speed, UsageSpeed::Standard);
    }

    #[test]
    fn claude_missing_speed_is_unknown_and_unpriced() {
        let line = br#"{"type":"assistant","sessionId":"session","timestamp":"2026-07-30T01:02:03Z","message":{"id":"message","model":"claude-opus-5","stop_reason":"end_turn","usage":{"input_tokens":20,"output_tokens":10}}}"#;
        let ParsedLine::Fact { entry, .. } = parse_claude_line(
            line,
            &mut ClaudeCursor::default(),
            &PricingCatalog::bundled(),
            &HashMap::new(),
        ) else {
            panic!("expected fact");
        };

        assert_eq!(entry.speed, UsageSpeed::Unknown);
        assert_eq!(entry.api_equivalent_cost_nanos, None);
    }

    #[test]
    fn claude_requires_a_complete_stop_reason() {
        let line = br#"{"type":"assistant","sessionId":"session","timestamp":"2026-07-30T01:02:03Z","message":{"id":"message","model":"claude-opus-5","stop_reason":null,"usage":{"input_tokens":20,"output_tokens":0}}}"#;

        assert!(matches!(
            parse_claude_line(
                line,
                &mut ClaudeCursor::default(),
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
    }

    #[test]
    fn codex_user_message_title_is_used_as_fallback_without_index() {
        let mut cursor = CodexCursor::default();
        let meta = br#"{"timestamp":"2026-07-30T01:00:00Z","type":"session_meta","payload":{"id":"session-a"}}"#;
        assert!(matches!(
            parse_codex_line(
                meta,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        let user_message = r#"{"timestamp":"2026-07-30T01:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"  帮我重构一下  登录模块  "}}"#.as_bytes();
        assert!(matches!(
            parse_codex_line(
                user_message,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        assert_eq!(
            cursor.pending_title.as_deref(),
            Some("帮我重构一下 登录模块")
        );

        let token = r#"{"timestamp":"2026-07-30T01:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#.as_bytes();
        let ParsedLine::Fact { conversation, .. } = parse_codex_line(
            token,
            &mut cursor,
            &PricingCatalog::bundled(),
            &HashMap::new(),
        ) else {
            panic!("expected fact");
        };
        assert_eq!(conversation.title.as_deref(), Some("帮我重构一下 登录模块"));
    }

    #[test]
    fn codex_title_index_takes_priority_over_message_fallback() {
        let mut cursor = CodexCursor::default();
        let meta = br#"{"timestamp":"2026-07-30T01:00:00Z","type":"session_meta","payload":{"id":"session-a"}}"#;
        assert!(matches!(
            parse_codex_line(
                meta,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        let user_message = r#"{"timestamp":"2026-07-30T01:00:01Z","type":"event_msg","payload":{"type":"user_message","message":"兜底标题"}}"#.as_bytes();
        assert!(matches!(
            parse_codex_line(
                user_message,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        let titles = HashMap::from([("session-a".to_owned(), "索引标题".to_owned())]);
        let token = br#"{"timestamp":"2026-07-30T01:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#;
        let ParsedLine::Fact { conversation, .. } =
            parse_codex_line(token, &mut cursor, &PricingCatalog::bundled(), &titles)
        else {
            panic!("expected fact");
        };
        assert_eq!(conversation.title.as_deref(), Some("索引标题"));
    }

    #[test]
    fn codex_pending_title_keeps_first_non_empty_value() {
        let mut cursor = CodexCursor::default();
        let first =
            r#"{"type":"event_msg","payload":{"type":"user_message","message":"<前缀注入标题"}}"#
                .as_bytes();
        assert!(matches!(
            parse_codex_line(
                first,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        assert_eq!(cursor.pending_title.as_deref(), Some("前缀注入标题"));

        let second = r#"{"type":"event_msg","payload":{"type":"user_message","message":"后续消息不应覆盖"}}"#.as_bytes();
        assert!(matches!(
            parse_codex_line(
                second,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        assert_eq!(cursor.pending_title.as_deref(), Some("前缀注入标题"));
    }

    #[test]
    fn claude_user_line_title_is_used_as_fallback_and_skips_sidechain() {
        let mut cursor = ClaudeCursor::default();
        let user = r#"{"type":"user","sessionId":"session-a","isSidechain":false,"timestamp":"2026-07-30T02:00:00Z","message":{"role":"user","content":"分析一下 Page.vue 里的布局"}}"#.as_bytes();
        assert!(matches!(
            parse_claude_line(
                user,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        assert_eq!(
            cursor.pending_title.as_deref(),
            Some("分析一下 Page.vue 里的布局")
        );

        let sidechain = r#"{"type":"user","sessionId":"session-b","isSidechain":true,"timestamp":"2026-07-30T02:00:01Z","message":{"role":"user","content":"子任务消息不作为标题"}}"#.as_bytes();
        assert!(matches!(
            parse_claude_line(
                sidechain,
                &mut cursor,
                &PricingCatalog::bundled(),
                &HashMap::new()
            ),
            ParsedLine::Ignored
        ));
        assert_eq!(
            cursor.pending_title.as_deref(),
            Some("分析一下 Page.vue 里的布局")
        );

        let assistant = br#"{"type":"assistant","sessionId":"session-a","timestamp":"2026-07-30T02:01:00Z","message":{"id":"message-a","model":"claude-opus-5","stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}}"#;
        let ParsedLine::Fact { conversation, .. } = parse_claude_line(
            assistant,
            &mut cursor,
            &PricingCatalog::bundled(),
            &HashMap::new(),
        ) else {
            panic!("expected fact");
        };
        assert_eq!(
            conversation.title.as_deref(),
            Some("分析一下 Page.vue 里的布局")
        );
    }

    #[test]
    fn clean_title_collapses_whitespace_and_strips_angle_prefix_and_truncates() {
        assert_eq!(
            clean_title("  a   b\n\tc  ").as_deref(),
            Some("a b c"),
            "空白折叠"
        );
        assert_eq!(clean_title("<前缀").as_deref(), Some("前缀"), "去掉 < 前缀");
        assert_eq!(clean_title("<prefix").as_deref(), Some("prefix"));
        assert_eq!(clean_title("   "), None, "纯空白返回 None");
        assert_eq!(clean_title(""), None);
        let long = "字".repeat(90);
        assert_eq!(
            clean_title(&long).map(|value| value.chars().count()),
            Some(80)
        );
    }
}
