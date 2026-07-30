use std::path::Path;

use chrono::{DateTime, Local, Utc};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::contracts::{UsageSource, UsageSpeed};

use super::model::{
    ClaudeCursor, CodexCursor, ConversationFact, InferenceGeo, ParsedLine, TokenFacts, UsageEntry,
};
use super::pricing::PricingCatalog;

pub fn parse_codex_line(
    line: &[u8],
    cursor: &mut CodexCursor,
    catalog: &PricingCatalog,
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
                cursor.conversation_key = Some(opaque_key("codex-conversation", id));
            }
            if let Some(model) = payload.get("model").and_then(Value::as_str) {
                cursor.model = normalized_optional(model);
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
                    title: None,
                    project_hint: None,
                    is_sidechain: false,
                    occurred_at,
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
) -> ParsedLine {
    let Ok(value) = serde_json::from_slice::<Value>(line) else {
        return ParsedLine::Invalid;
    };
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
            title: None,
            project_hint,
            is_sidechain: value
                .get("isSidechain")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            occurred_at,
        }),
    }
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

fn project_hint(value: &str) -> Option<String> {
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

    #[test]
    fn codex_subtracts_cache_facts_and_does_not_double_reasoning() {
        let mut cursor = CodexCursor {
            conversation_key: Some("conversation".to_owned()),
            model: Some("gpt-5.6-sol".to_owned()),
            speed: Some(UsageSpeed::Standard),
            last_total_signature: None,
        };
        let line = br#"{"timestamp":"2026-07-30T01:02:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":30,"cached_input_tokens":40,"cache_write_input_tokens":10,"reasoning_output_tokens":5,"total_tokens":130},"total_token_usage":{"input_tokens":100,"output_tokens":30,"cached_input_tokens":40,"cache_write_input_tokens":10,"reasoning_output_tokens":5,"total_tokens":130}}}}"#;

        let ParsedLine::Fact { entry, .. } =
            parse_codex_line(line, &mut cursor, &PricingCatalog::bundled())
        else {
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
        };
        let line = br#"{"timestamp":"2026-07-30T01:02:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#;

        assert!(matches!(
            parse_codex_line(line, &mut cursor, &PricingCatalog::bundled()),
            ParsedLine::Fact { .. }
        ));
        assert!(matches!(
            parse_codex_line(line, &mut cursor, &PricingCatalog::bundled()),
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
                &catalog
            ),
            ParsedLine::Ignored
        ));
        assert!(matches!(
            parse_codex_line(
                br#"{"type":"turn_context","payload":{"service_tier":"priority"}}"#,
                &mut cursor,
                &catalog
            ),
            ParsedLine::Ignored
        ));
        let ParsedLine::Fact { entry: fast, .. } = parse_codex_line(
            br#"{"timestamp":"2026-07-30T01:02:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#,
            &mut cursor,
            &catalog,
        ) else {
            panic!("expected fast fact");
        };
        assert_eq!(fast.speed, UsageSpeed::Fast);

        assert!(matches!(
            parse_codex_line(
                br#"{"type":"event_msg","payload":{"type":"thread_settings_applied","thread_settings":{"service_tier":"standard"}}}"#,
                &mut cursor,
                &catalog
            ),
            ParsedLine::Ignored
        ));
        let ParsedLine::Fact {
            entry: standard, ..
        } = parse_codex_line(
            br#"{"timestamp":"2026-07-30T01:03:03Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":2,"output_tokens":2,"total_tokens":4},"total_token_usage":{"input_tokens":2,"output_tokens":2,"total_tokens":4}}}}"#,
            &mut cursor,
            &catalog,
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
                &PricingCatalog::bundled()
            ),
            ParsedLine::Ignored
        ));
    }
}
