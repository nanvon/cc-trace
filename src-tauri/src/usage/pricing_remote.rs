//! LiteLLM / models.dev 公开价格目录的缓存、解码与刷新。
//!
//! 磁盘文件只保存可重建的远端状态；应用内置价格政策始终随二进制升级，不会被旧缓存遮住。
//! 读侧始终使用 `active` 冻结快照，网络刷新只写 `pending`，由用量扫描安全边界统一提交。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Duration, Utc};
use reqwest::header::{ACCEPT, ETAG, IF_NONE_MATCH};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use super::pricing::normalize_model_key;

const CACHE_FILE: &str = "pricing-catalog.json";
const TEMP_FILE: &str = "pricing-catalog.json.tmp";
const CACHE_SCHEMA_VERSION: u32 = 2;
const REFRESH_HOURS: i64 = 24;
const FAILURE_RETRY_MINUTES: i64 = 30;
const MISSING_RETRY_MINUTES: i64 = 30;
const MISSING_RETENTION_DAYS: i64 = 7;
const SUSPICIOUS_SHRINK_MINIMUM: usize = 20;
const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;

const LITELLM_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const MODELS_DEV_URL: &str = "https://models.dev/api.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteModelPrice {
    /// USD nanos / 百万 Token。
    pub uncached_input_nanos_per_m_tok: u64,
    pub output_nanos_per_m_tok: u64,
    pub cache_read_nanos_per_m_tok: u64,
    pub cache_write_nanos_per_m_tok: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PricingSourceState {
    pub etag: Option<String>,
    pub fetched_at: Option<String>,
    pub failed_at: Option<String>,
    pub standard_rates: HashMap<String, RemoteModelPrice>,
    pub codex_fast_rates: HashMap<String, RemoteModelPrice>,
    pub claude_fast_rates: HashMap<String, RemoteModelPrice>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PricingCachePayload {
    pub schema_version: u32,
    pub lite_llm: PricingSourceState,
    pub models_dev: PricingSourceState,
    pub missing_refresh_attempts: HashMap<String, String>,
}

impl Default for PricingCachePayload {
    fn default() -> Self {
        Self {
            schema_version: CACHE_SCHEMA_VERSION,
            lite_llm: PricingSourceState::default(),
            models_dev: PricingSourceState::default(),
            missing_refresh_attempts: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct DecodedPricingRates {
    standard: HashMap<String, RemoteModelPrice>,
    codex_fast: HashMap<String, RemoteModelPrice>,
    claude_fast: HashMap<String, RemoteModelPrice>,
}

#[derive(Clone, Copy)]
enum PricingSource {
    LiteLlm,
    ModelsDev,
}

/// 刷新触发的来源决定是否可以绕过成功间隔或失败退避。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PricingRefreshMode {
    Scheduled,
    MissingPrice,
    Manual,
}

/// 对外只暴露目录刷新是否完整、部分或完全失败；不泄露上游错误原文。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PricingRefreshOutcome {
    Complete,
    Partial,
    Failed,
}

impl PricingRefreshOutcome {
    pub(crate) fn did_update(self) -> bool {
        !matches!(self, Self::Failed)
    }
}

#[derive(Clone, Copy)]
enum SourceRefreshOutcome {
    Success,
    Failed,
    Skipped,
}

impl PricingSource {
    fn state<'a>(self, payload: &'a PricingCachePayload) -> &'a PricingSourceState {
        match self {
            Self::LiteLlm => &payload.lite_llm,
            Self::ModelsDev => &payload.models_dev,
        }
    }

    fn state_mut<'a>(self, payload: &'a mut PricingCachePayload) -> &'a mut PricingSourceState {
        match self {
            Self::LiteLlm => &mut payload.lite_llm,
            Self::ModelsDev => &mut payload.models_dev,
        }
    }
}

struct CatalogState {
    active: PricingCachePayload,
    pending: Option<PricingCachePayload>,
}

pub(crate) struct RemotePricingStore {
    directory: PathBuf,
    state: Mutex<CatalogState>,
    refresh_lock: AsyncMutex<()>,
}

impl RemotePricingStore {
    pub(crate) fn new(directory: PathBuf) -> Self {
        let active = load_cache(&directory);
        Self {
            directory,
            state: Mutex::new(CatalogState {
                active,
                pending: None,
            }),
            refresh_lock: AsyncMutex::new(()),
        }
    }

    pub(crate) fn active(&self) -> PricingCachePayload {
        self.state
            .lock()
            .expect("pricing catalog state")
            .active
            .clone()
    }

    pub(crate) fn is_due(&self) -> bool {
        let now = Utc::now();
        let state = self.state.lock().expect("pricing catalog state");
        let payload = state.pending.as_ref().unwrap_or(&state.active);
        source_is_due(&payload.lite_llm, now) || source_is_due(&payload.models_dev, now)
    }

    pub(crate) fn commit_pending(&self) -> Option<(PricingCachePayload, PricingCachePayload)> {
        let mut state = self.state.lock().expect("pricing catalog state");
        let pending = state.pending.take()?;
        let previous = std::mem::replace(&mut state.active, pending.clone());
        Some((previous, pending))
    }

    pub(crate) fn mark_missing_refresh_attempts(&self, keys: &HashSet<String>) -> io::Result<bool> {
        let now = Utc::now();
        let mut state = self.state.lock().expect("pricing catalog state");
        let mut payload = state
            .pending
            .clone()
            .unwrap_or_else(|| state.active.clone());
        payload.missing_refresh_attempts.retain(|_, attempted_at| {
            parse_time(attempted_at).is_some_and(|value| {
                now.signed_duration_since(value) < Duration::days(MISSING_RETENTION_DAYS)
            })
        });

        let mut due = false;
        for key in keys {
            let eligible = payload
                .missing_refresh_attempts
                .get(key)
                .and_then(|value| parse_time(value))
                .is_none_or(|attempted_at| {
                    now.signed_duration_since(attempted_at)
                        >= Duration::minutes(MISSING_RETRY_MINUTES)
                });
            if eligible {
                payload
                    .missing_refresh_attempts
                    .insert(key.clone(), now.to_rfc3339());
                due = true;
            }
        }
        if due {
            // 保持 state 锁直至 rename 完成：否则并发刷新可能被旧的缺价冷却副本覆写。
            save_cache(&self.directory, &payload)?;
            state.pending = Some(payload);
        }
        Ok(due)
    }

    pub(crate) async fn refresh(&self, mode: PricingRefreshMode) -> PricingRefreshOutcome {
        let guard = if mode == PricingRefreshMode::Scheduled {
            let Ok(guard) = self.refresh_lock.try_lock() else {
                return PricingRefreshOutcome::Failed;
            };
            guard
        } else {
            self.refresh_lock.lock().await
        };

        let lite_llm = self
            .refresh_source(PricingSource::LiteLlm, LITELLM_URL, mode, decode_litellm)
            .await;
        let models_dev = self
            .refresh_source(
                PricingSource::ModelsDev,
                MODELS_DEV_URL,
                mode,
                decode_models_dev,
            )
            .await;
        drop(guard);
        refresh_outcome(lite_llm, models_dev)
    }

    async fn refresh_source(
        &self,
        source: PricingSource,
        url: &str,
        mode: PricingRefreshMode,
        decode: fn(&[u8]) -> Result<DecodedPricingRates, ()>,
    ) -> SourceRefreshOutcome {
        let now = Utc::now();
        let previous = {
            let state = self.state.lock().expect("pricing catalog state");
            let payload = state.pending.as_ref().unwrap_or(&state.active);
            source.state(payload).clone()
        };
        if !source_should_refresh(&previous, now, mode) {
            return SourceRefreshOutcome::Skipped;
        }

        match fetch(url, previous.etag.as_deref()).await {
            Ok(FetchOutcome::NotModified) => {
                if self
                    .update_pending(|payload| {
                        let current = source.state_mut(payload);
                        current.fetched_at = Some(now.to_rfc3339());
                        current.failed_at = None;
                    })
                    .is_ok()
                {
                    SourceRefreshOutcome::Success
                } else {
                    SourceRefreshOutcome::Failed
                }
            }
            Ok(FetchOutcome::Updated { body, etag }) => {
                let Ok(rates) = decode(&body) else {
                    self.record_source_failure(source, now, true);
                    return SourceRefreshOutcome::Failed;
                };
                if suspicious_shrink(&rates.standard, &previous.standard_rates) {
                    self.record_source_failure(source, now, true);
                    return SourceRefreshOutcome::Failed;
                }
                if self
                    .update_pending(|payload| {
                        let current = source.state_mut(payload);
                        current.standard_rates = rates.standard;
                        // Fast 价格保留 last-known：上游移除型号不能破坏历史日志计价。
                        current.codex_fast_rates.extend(rates.codex_fast);
                        current.claude_fast_rates.extend(rates.claude_fast);
                        current.etag = etag;
                        current.fetched_at = Some(now.to_rfc3339());
                        current.failed_at = None;
                    })
                    .is_ok()
                {
                    SourceRefreshOutcome::Success
                } else {
                    SourceRefreshOutcome::Failed
                }
            }
            Err(()) => {
                self.record_source_failure(source, now, false);
                SourceRefreshOutcome::Failed
            }
        }
    }

    fn record_source_failure(&self, source: PricingSource, now: DateTime<Utc>, clear_etag: bool) {
        let _ = self.update_pending(|payload| {
            let current = source.state_mut(payload);
            current.failed_at = Some(now.to_rfc3339());
            if clear_etag {
                current.etag = None;
            }
        });
    }

    fn update_pending(&self, update: impl FnOnce(&mut PricingCachePayload)) -> io::Result<()> {
        let mut state = self.state.lock().expect("pricing catalog state");
        let mut payload = state
            .pending
            .clone()
            .unwrap_or_else(|| state.active.clone());
        update(&mut payload);
        // 先持久化、后发布内存快照；磁盘不可写时既不伪报成功，也不留下不可恢复的 pending。
        save_cache(&self.directory, &payload)?;
        state.pending = Some(payload);
        Ok(())
    }
}

fn refresh_outcome(
    left: SourceRefreshOutcome,
    right: SourceRefreshOutcome,
) -> PricingRefreshOutcome {
    let succeeded = [left, right]
        .into_iter()
        .filter(|outcome| matches!(outcome, SourceRefreshOutcome::Success))
        .count();
    let failed = [left, right]
        .into_iter()
        .filter(|outcome| matches!(outcome, SourceRefreshOutcome::Failed))
        .count();
    match (succeeded, failed) {
        (0, _) => PricingRefreshOutcome::Failed,
        (_, 0) => PricingRefreshOutcome::Complete,
        _ => PricingRefreshOutcome::Partial,
    }
}

fn source_is_due(source: &PricingSourceState, now: DateTime<Utc>) -> bool {
    if source_failed_recently(source, now) {
        return false;
    }
    source
        .fetched_at
        .as_deref()
        .and_then(parse_time)
        .is_none_or(|fetched_at| {
            now.signed_duration_since(fetched_at) >= Duration::hours(REFRESH_HOURS)
        })
}

fn source_failed_recently(source: &PricingSourceState, now: DateTime<Utc>) -> bool {
    source
        .failed_at
        .as_deref()
        .and_then(parse_time)
        .is_some_and(|failed_at| {
            now.signed_duration_since(failed_at) < Duration::minutes(FAILURE_RETRY_MINUTES)
        })
}

fn source_should_refresh(
    source: &PricingSourceState,
    now: DateTime<Utc>,
    mode: PricingRefreshMode,
) -> bool {
    match mode {
        PricingRefreshMode::Scheduled => source_is_due(source, now),
        // 缺价需要立即补齐，但同一来源刚失败时仍遵守全局失败退避。
        PricingRefreshMode::MissingPrice => !source_failed_recently(source, now),
        // 用户明确的手动更新可以绕过所有时间门槛。
        PricingRefreshMode::Manual => true,
    }
}

fn suspicious_shrink(
    new_rates: &HashMap<String, RemoteModelPrice>,
    previous_rates: &HashMap<String, RemoteModelPrice>,
) -> bool {
    previous_rates.len() >= SUSPICIOUS_SHRINK_MINIMUM
        && new_rates.len().saturating_mul(2) < previous_rates.len()
}

fn parse_time(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

enum FetchOutcome {
    NotModified,
    Updated { body: Vec<u8>, etag: Option<String> },
}

async fn fetch(url: &str, etag: Option<&str>) -> Result<FetchOutcome, ()> {
    let mut request = crate::providers::http::client()
        .get(url)
        .header(ACCEPT, "application/json");
    if let Some(etag) = etag.filter(|value| !value.is_empty()) {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let response = request.send().await.map_err(|_| ())?;
    if response.status() == reqwest::StatusCode::NOT_MODIFIED {
        return Ok(FetchOutcome::NotModified);
    }
    if !response.status().is_success() {
        return Err(());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_CATALOG_BYTES)
    {
        return Err(());
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned);
    let body = response.bytes().await.map_err(|_| ())?;
    if body.len() as u64 > MAX_CATALOG_BYTES {
        return Err(());
    }
    Ok(FetchOutcome::Updated {
        body: body.to_vec(),
        etag,
    })
}

fn load_cache(directory: &Path) -> PricingCachePayload {
    let path = directory.join(CACHE_FILE);
    let Ok(raw) = fs::read_to_string(&path) else {
        return PricingCachePayload::default();
    };
    if let Ok(payload) = serde_json::from_str::<PricingCachePayload>(&raw)
        && payload.schema_version == CACHE_SCHEMA_VERSION
    {
        return payload;
    }

    // CC Trace v1 是一张最终价格表，不是远端缓存。升级时保留原文件作诊断；
    // 其他无效或不支持版本按 corrupt 保留。运行时统一使用内置政策 + 空 v2 缓存。
    let kind = serde_json::from_str::<Value>(&raw)
        .ok()
        .and_then(|value| value.get("schemaVersion").and_then(Value::as_u64))
        .filter(|version| *version == 1)
        .map_or("corrupt", |_| "legacy-v1");
    let suffix = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
    let legacy = directory.join(format!("{CACHE_FILE}.{kind}-{suffix}"));
    let _ = fs::rename(&path, legacy);
    let payload = PricingCachePayload::default();
    let _ = save_cache(directory, &payload);
    payload
}

fn save_cache(directory: &Path, payload: &PricingCachePayload) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    let serialized = serde_json::to_vec_pretty(payload)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let temp = directory.join(TEMP_FILE);
    {
        let mut file = fs::File::create(&temp)?;
        file.write_all(&serialized)?;
        file.sync_all()?;
    }
    fs::rename(temp, directory.join(CACHE_FILE))
}

fn decode_litellm(data: &[u8]) -> Result<DecodedPricingRates, ()> {
    let root = serde_json::from_slice::<Value>(data).map_err(|_| ())?;
    let entries = root.as_object().ok_or(())?;
    let allowed = ["anthropic", "openai", "deepseek"];
    let mut standard = Vec::new();
    let mut codex_fast = Vec::new();

    for (raw_key, value) in entries {
        if raw_key.contains('/') {
            continue;
        }
        let Some(entry) = value.as_object() else {
            continue;
        };
        let Some(provider) = entry.get("litellm_provider").and_then(Value::as_str) else {
            continue;
        };
        if !allowed.contains(&provider) {
            continue;
        }
        if let Some(price) = parse_litellm_rates(entry, "") {
            standard.push((raw_key.clone(), price));
        }
        if provider == "openai"
            && let Some(price) = parse_litellm_rates(entry, "_priority")
        {
            codex_fast.push((raw_key.clone(), price));
        }
    }

    let standard = resolve_candidates(standard);
    if standard.is_empty() {
        return Err(());
    }
    Ok(DecodedPricingRates {
        standard,
        codex_fast: resolve_candidates(codex_fast),
        claude_fast: HashMap::new(),
    })
}

fn parse_litellm_rates(
    entry: &serde_json::Map<String, Value>,
    suffix: &str,
) -> Option<RemoteModelPrice> {
    let input = per_token_nanos(entry.get(&format!("input_cost_per_token{suffix}"))?)?;
    let output = per_token_nanos(entry.get(&format!("output_cost_per_token{suffix}"))?)?;
    let cache_read = entry
        .get(&format!("cache_read_input_token_cost{suffix}"))
        .and_then(per_token_nanos)
        .unwrap_or(0);
    let cache_write = entry
        .get(&format!("cache_creation_input_token_cost{suffix}"))
        .and_then(per_token_nanos)
        .unwrap_or(0);
    Some(RemoteModelPrice {
        uncached_input_nanos_per_m_tok: input,
        output_nanos_per_m_tok: output,
        cache_read_nanos_per_m_tok: cache_read,
        cache_write_nanos_per_m_tok: cache_write,
    })
}

fn decode_models_dev(data: &[u8]) -> Result<DecodedPricingRates, ()> {
    let root = serde_json::from_slice::<Value>(data).map_err(|_| ())?;
    let providers = root.as_object().ok_or(())?;
    let allowed = ["anthropic", "openai", "deepseek"];
    let mut standard = Vec::new();
    let mut codex_fast = Vec::new();
    let mut claude_fast = Vec::new();

    for (priority, provider_id) in allowed.into_iter().enumerate() {
        let Some(models) = providers
            .get(provider_id)
            .and_then(|value| value.get("models"))
            .and_then(Value::as_object)
        else {
            continue;
        };
        for (model_id, entry) in models {
            if let Some(price) = entry.get("cost").and_then(parse_models_dev_rates) {
                standard.push((model_id.clone(), priority, price));
            }
            let fast = entry
                .pointer("/experimental/modes/fast/cost")
                .and_then(parse_models_dev_rates);
            if let Some(price) = fast {
                match provider_id {
                    "openai" => codex_fast.push((model_id.clone(), priority, price)),
                    "anthropic" => claude_fast.push((model_id.clone(), priority, price)),
                    _ => {}
                }
            }
        }
    }

    let standard = resolve_prioritized_candidates(standard);
    if standard.is_empty() {
        return Err(());
    }
    Ok(DecodedPricingRates {
        standard,
        codex_fast: resolve_prioritized_candidates(codex_fast),
        claude_fast: resolve_prioritized_candidates(claude_fast),
    })
}

fn parse_models_dev_rates(value: &Value) -> Option<RemoteModelPrice> {
    let cost = value.as_object()?;
    let input = per_million_nanos(cost.get("input")?)?;
    let output = per_million_nanos(cost.get("output")?)?;
    let cache_read = cost
        .get("cache_read")
        .and_then(per_million_nanos)
        .unwrap_or(0);
    let cache_write = cost
        .get("cache_write")
        .and_then(per_million_nanos)
        .unwrap_or(0);
    Some(RemoteModelPrice {
        uncached_input_nanos_per_m_tok: input,
        output_nanos_per_m_tok: output,
        cache_read_nanos_per_m_tok: cache_read,
        cache_write_nanos_per_m_tok: cache_write,
    })
}

fn per_token_nanos(value: &Value) -> Option<u64> {
    scaled_decimal_nanos(value, 15)
}

fn per_million_nanos(value: &Value) -> Option<u64> {
    scaled_decimal_nanos(value, 9)
}

fn scaled_decimal_nanos(value: &Value, scale_power: i32) -> Option<u64> {
    let raw = match value {
        Value::Number(number) => number.to_string(),
        Value::String(value) => value.trim().to_owned(),
        _ => return None,
    };
    if raw.is_empty() || raw.starts_with('-') || raw.starts_with('+') {
        return None;
    }
    let exponent_index = raw.find('e').or_else(|| raw.find('E'));
    let (mantissa, exponent) = if let Some(index) = exponent_index {
        (&raw[..index], raw[index + 1..].parse::<i32>().ok()?)
    } else {
        (raw.as_str(), 0_i32)
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let coefficient = digits.parse::<u128>().ok()?;
    let decimal_power = exponent
        .checked_sub(i32::try_from(fraction.len()).ok()?)?
        .checked_add(scale_power)?;
    let scaled = if decimal_power >= 0 {
        coefficient.checked_mul(10_u128.checked_pow(decimal_power as u32)?)?
    } else {
        let divisor = 10_u128.checked_pow(decimal_power.unsigned_abs())?;
        let quotient = coefficient / divisor;
        let remainder = coefficient % divisor;
        quotient.checked_add(u128::from(remainder >= (divisor + 1) / 2))?
    };
    u64::try_from(scaled).ok()
}

fn resolve_candidates(
    mut candidates: Vec<(String, RemoteModelPrice)>,
) -> HashMap<String, RemoteModelPrice> {
    candidates.sort_by(|left, right| left.0.cmp(&right.0));
    let prioritized = candidates
        .into_iter()
        .map(|(key, price)| (key, 0_usize, price))
        .collect();
    resolve_prioritized_candidates(prioritized)
}

fn resolve_prioritized_candidates(
    mut candidates: Vec<(String, usize, RemoteModelPrice)>,
) -> HashMap<String, RemoteModelPrice> {
    candidates.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut output = HashMap::new();
    for (raw_key, _, price) in &candidates {
        let normalized = normalize_model_key(raw_key);
        if normalized == raw_key.to_ascii_lowercase() {
            output.entry(normalized).or_insert_with(|| price.clone());
        }
    }
    for (raw_key, _, price) in candidates {
        output.entry(normalize_model_key(&raw_key)).or_insert(price);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn litellm_separates_standard_and_openai_priority() {
        let decoded = decode_litellm(
            br#"{
              "gpt-future": {
                "litellm_provider": "openai",
                "input_cost_per_token": 0.000005,
                "output_cost_per_token": 0.000030,
                "cache_read_input_token_cost": 0.0000005,
                "input_cost_per_token_priority": 0.000010,
                "output_cost_per_token_priority": 0.000060,
                "cache_read_input_token_cost_priority": 0.000001
              },
              "claude-future": {
                "litellm_provider": "anthropic",
                "input_cost_per_token": 0.000005,
                "output_cost_per_token": 0.000025,
                "input_cost_per_token_priority": 0.000010,
                "output_cost_per_token_priority": 0.000050
              }
            }"#,
        )
        .expect("decode");

        assert_eq!(
            decoded.standard["gpt-future"].uncached_input_nanos_per_m_tok,
            5_000_000_000
        );
        assert_eq!(
            decoded.codex_fast["gpt-future"].output_nanos_per_m_tok,
            60_000_000_000
        );
        assert_eq!(
            decoded.standard["claude-future"].cache_write_nanos_per_m_tok, 0,
            "缺少缓存子价格按产品口径视为 $0"
        );
        assert!(!decoded.codex_fast.contains_key("claude-future"));
        assert!(decoded.claude_fast.is_empty());
    }

    #[test]
    fn models_dev_reads_official_fast_and_ignores_reseller() {
        let decoded = decode_models_dev(
            br#"{
              "anthropic": {
                "models": {
                  "claude-opus-5": {
                    "cost": {"input": 5, "output": 25, "cache_read": 0.5, "cache_write": 6.25},
                    "experimental": {"modes": {"fast": {"cost": {
                      "input": 10, "output": 50, "cache_read": 1, "cache_write": 12.5
                    }}}}
                  }
                }
              },
              "reseller": {
                "models": {
                  "claude-opus-5": {
                    "cost": {"input": 1, "output": 1},
                    "experimental": {"modes": {"fast": {"cost": {"input": 2, "output": 2}}}}
                  }
                }
              }
            }"#,
        )
        .expect("decode");

        let fast = &decoded.claude_fast["claude-opus-5"];
        assert_eq!(fast.uncached_input_nanos_per_m_tok, 10_000_000_000);
        assert_eq!(fast.output_nanos_per_m_tok, 50_000_000_000);
        assert_eq!(fast.cache_read_nanos_per_m_tok, 1_000_000_000);
        assert_eq!(fast.cache_write_nanos_per_m_tok, 12_500_000_000);
        assert!(!decoded.codex_fast.contains_key("claude-opus-5"));
    }

    #[test]
    fn decimal_decoder_keeps_catalog_rates_exact() {
        assert_eq!(
            per_token_nanos(&serde_json::json!(0.000000075)),
            Some(75_000_000)
        );
        assert_eq!(
            per_token_nanos(&serde_json::json!(1.25e-5)),
            Some(12_500_000_000)
        );
        assert_eq!(
            per_million_nanos(&serde_json::json!("3.125")),
            Some(3_125_000_000)
        );
    }

    #[test]
    fn source_due_respects_success_and_failure_windows() {
        let now = Utc::now();
        let fresh = PricingSourceState {
            fetched_at: Some((now - Duration::hours(23)).to_rfc3339()),
            ..PricingSourceState::default()
        };
        let expired = PricingSourceState {
            fetched_at: Some((now - Duration::hours(24)).to_rfc3339()),
            ..PricingSourceState::default()
        };
        let failed = PricingSourceState {
            failed_at: Some((now - Duration::minutes(29)).to_rfc3339()),
            ..PricingSourceState::default()
        };

        assert!(!source_is_due(&fresh, now));
        assert!(source_is_due(&expired, now));
        assert!(!source_is_due(&failed, now));
        assert!(source_should_refresh(
            &fresh,
            now,
            PricingRefreshMode::MissingPrice
        ));
        assert!(!source_should_refresh(
            &failed,
            now,
            PricingRefreshMode::MissingPrice
        ));
        assert!(source_should_refresh(
            &failed,
            now,
            PricingRefreshMode::Manual
        ));
    }

    #[test]
    fn missing_refresh_cooldown_is_persisted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let key = HashSet::from(["claude|fast|claude-future".to_owned()]);
        let store = RemotePricingStore::new(dir.path().to_path_buf());

        assert!(
            store
                .mark_missing_refresh_attempts(&key)
                .expect("persist first cooldown")
        );
        assert!(
            !store
                .mark_missing_refresh_attempts(&key)
                .expect("read persisted cooldown")
        );

        let restored = RemotePricingStore::new(dir.path().to_path_buf());
        assert!(
            !restored
                .mark_missing_refresh_attempts(&key)
                .expect("restore cooldown")
        );
    }

    #[test]
    fn pending_rates_do_not_replace_active_until_committed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = RemotePricingStore::new(dir.path().to_path_buf());
        let price = RemoteModelPrice {
            uncached_input_nanos_per_m_tok: 10_000_000_000,
            output_nanos_per_m_tok: 50_000_000_000,
            cache_read_nanos_per_m_tok: 1_000_000_000,
            cache_write_nanos_per_m_tok: 12_500_000_000,
        };

        store
            .update_pending(|payload| {
                payload
                    .models_dev
                    .claude_fast_rates
                    .insert("claude-opus-5".to_owned(), price.clone());
            })
            .expect("persist pending");

        assert!(store.active().models_dev.claude_fast_rates.is_empty());
        store.commit_pending().expect("commit pending");
        assert_eq!(
            store
                .active()
                .models_dev
                .claude_fast_rates
                .get("claude-opus-5"),
            Some(&price)
        );
    }

    #[test]
    fn refresh_outcome_reports_partial_source_success() {
        assert_eq!(
            refresh_outcome(SourceRefreshOutcome::Success, SourceRefreshOutcome::Success),
            PricingRefreshOutcome::Complete
        );
        assert_eq!(
            refresh_outcome(SourceRefreshOutcome::Success, SourceRefreshOutcome::Failed),
            PricingRefreshOutcome::Partial
        );
        assert_eq!(
            refresh_outcome(SourceRefreshOutcome::Failed, SourceRefreshOutcome::Failed),
            PricingRefreshOutcome::Failed
        );
    }

    #[test]
    fn pending_state_is_not_published_when_cache_cannot_be_written() {
        let file = tempfile::NamedTempFile::new().expect("file");
        let store = RemotePricingStore::new(file.path().to_path_buf());

        assert!(
            store
                .update_pending(|payload| {
                    payload
                        .missing_refresh_attempts
                        .insert("codex|fast|future".to_owned(), Utc::now().to_rfc3339());
                })
                .is_err()
        );
        assert!(store.active().missing_refresh_attempts.is_empty());
    }
}
