use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contracts::{UsageSource, UsageSpeed};

use super::model::{InferenceGeo, RepriceRow, UsageEntry};
use super::pricing_remote::{PricingCachePayload, RemoteModelPrice, RemotePricingStore};
pub(crate) use super::pricing_remote::{PricingRefreshMode, PricingRefreshOutcome};

const CATALOG_SCHEMA_VERSION: u32 = 1;
const BUNDLED_CATALOG: &str = include_str!("pricing-catalog.v1.json");
const LOCAL_OVERRIDE_PRIORITY: u8 = 0;
const REMOTE_PRIORITY: u8 = 1;
const LOCAL_FALLBACK_PRIORITY: u8 = 2;
const PRICING_POLICY_VERSION: u32 = 2;

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogDocument {
    schema_version: u32,
    as_of: String,
    sources: Vec<String>,
    entries: Vec<CatalogEntry>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CatalogEntry {
    source: UsageSource,
    model_prefix: String,
    speed: UsageSpeed,
    #[serde(default)]
    effective_from: Option<String>,
    #[serde(default)]
    effective_until: Option<String>,
    uncached_input_usd_per_m_tok: String,
    cache_read_usd_per_m_tok: String,
    cache_write_5m_usd_per_m_tok: String,
    cache_write_1h_usd_per_m_tok: String,
    output_usd_per_m_tok: String,
    us_inference_multiplier_bps: u32,
    #[serde(default)]
    long_context_threshold_tokens: Option<i64>,
    #[serde(default)]
    long_context_input_multiplier_bps: Option<u32>,
    #[serde(default)]
    long_context_output_multiplier_bps: Option<u32>,
    #[serde(default = "default_true")]
    long_context_priced: bool,
}

#[derive(Clone)]
struct CompiledEntry {
    priority: u8,
    source: UsageSource,
    model_prefix: String,
    speed: UsageSpeed,
    effective_from: Option<DateTime<Utc>>,
    effective_until: Option<DateTime<Utc>>,
    uncached_input_nanos_per_m_tok: u64,
    cache_read_nanos_per_m_tok: u64,
    cache_write_5m_nanos_per_m_tok: u64,
    cache_write_1h_nanos_per_m_tok: u64,
    output_nanos_per_m_tok: u64,
    us_inference_multiplier_bps: u32,
    long_context_threshold_tokens: Option<i64>,
    long_context_input_multiplier_bps: u32,
    long_context_output_multiplier_bps: u32,
    long_context_priced: bool,
}

#[derive(Clone)]
pub struct PricingCatalog {
    entries: Vec<CompiledEntry>,
    fingerprint: String,
}

pub struct PriceEstimate {
    pub cost_nanos: Option<i64>,
    pub assumed_geo: bool,
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct PricingUsageKey {
    pub source: UsageSource,
    pub model: String,
    pub speed: UsageSpeed,
}

impl PricingUsageKey {
    pub(crate) fn new(source: UsageSource, model: &str, speed: UsageSpeed) -> Self {
        let mut model = normalize_model_key(model);
        if source == UsageSource::Claude {
            model = model.replace('.', "-");
        }
        Self {
            source,
            model,
            speed,
        }
    }

    pub(crate) fn persisted_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.source.as_db(),
            self.speed.as_db(),
            self.model
        )
    }
}

impl PricingCatalog {
    fn parse(raw: &str) -> Result<Self, ()> {
        let document: CatalogDocument = serde_json::from_str(raw).map_err(|_| ())?;
        if document.schema_version != CATALOG_SCHEMA_VERSION
            || NaiveDate::parse_from_str(&document.as_of, "%Y-%m-%d").is_err()
            || document.entries.is_empty()
            || document.sources.is_empty()
            || document
                .sources
                .iter()
                .any(|source| !source.starts_with("https://"))
        {
            return Err(());
        }

        let mut keys = HashSet::new();
        let mut entries = Vec::with_capacity(document.entries.len());

        for item in document.entries {
            let prefix = item.model_prefix.trim().to_ascii_lowercase();
            if prefix.is_empty()
                || item.speed == UsageSpeed::Unknown
                || !(10_000..=20_000).contains(&item.us_inference_multiplier_bps)
                || item
                    .long_context_threshold_tokens
                    .is_some_and(|value| value <= 0)
                || !(10_000..=50_000)
                    .contains(&item.long_context_input_multiplier_bps.unwrap_or(10_000))
                || !(10_000..=50_000)
                    .contains(&item.long_context_output_multiplier_bps.unwrap_or(10_000))
            {
                return Err(());
            }

            let from = parse_optional_time(item.effective_from.as_deref())?;
            let until = parse_optional_time(item.effective_until.as_deref())?;
            if matches!((from, until), (Some(from), Some(until)) if from >= until) {
                return Err(());
            }

            let key = (
                item.source.as_db(),
                prefix.clone(),
                item.speed.as_db(),
                from,
                until,
            );
            if !keys.insert(key) {
                return Err(());
            }

            let compiled = CompiledEntry {
                priority: local_priority(&item),
                source: item.source,
                model_prefix: prefix,
                speed: item.speed,
                effective_from: from,
                effective_until: until,
                uncached_input_nanos_per_m_tok: parse_usd_rate(&item.uncached_input_usd_per_m_tok)?,
                cache_read_nanos_per_m_tok: parse_usd_rate(&item.cache_read_usd_per_m_tok)?,
                cache_write_5m_nanos_per_m_tok: parse_usd_rate(&item.cache_write_5m_usd_per_m_tok)?,
                cache_write_1h_nanos_per_m_tok: parse_usd_rate(&item.cache_write_1h_usd_per_m_tok)?,
                output_nanos_per_m_tok: parse_usd_rate(&item.output_usd_per_m_tok)?,
                us_inference_multiplier_bps: item.us_inference_multiplier_bps,
                long_context_threshold_tokens: item.long_context_threshold_tokens,
                long_context_input_multiplier_bps: item
                    .long_context_input_multiplier_bps
                    .unwrap_or(10_000),
                long_context_output_multiplier_bps: item
                    .long_context_output_multiplier_bps
                    .unwrap_or(10_000),
                long_context_priced: item.long_context_priced,
            };
            if entries.iter().any(|existing: &CompiledEntry| {
                existing.source == compiled.source
                    && existing.model_prefix == compiled.model_prefix
                    && existing.speed == compiled.speed
                    && ranges_overlap(
                        existing.effective_from.as_ref(),
                        existing.effective_until.as_ref(),
                        compiled.effective_from.as_ref(),
                        compiled.effective_until.as_ref(),
                    )
            }) {
                return Err(());
            }
            entries.push(compiled);
        }

        sort_entries(&mut entries);
        let fingerprint = compiled_fingerprint(&entries);

        Ok(Self {
            entries,
            fingerprint,
        })
    }

    pub fn bundled() -> Self {
        Self::parse(BUNDLED_CATALOG).expect("bundled pricing catalog must be valid")
    }

    fn merged(remote: &PricingCachePayload) -> Self {
        let mut catalog = Self::bundled();
        let local_entries = catalog.entries.clone();
        catalog
            .entries
            .extend(compile_remote_entries(remote, &local_entries));
        sort_entries(&mut catalog.entries);
        catalog.fingerprint = compiled_fingerprint(&catalog.entries);
        catalog
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn scoped_to_known_usage(mut self, known_usage: &HashSet<PricingUsageKey>) -> Self {
        let relevant = self
            .entries
            .iter()
            .filter(|entry| {
                known_usage.iter().any(|key| {
                    let matches = key.source == entry.source
                        && key.speed == entry.speed
                        && model_matches(&key.model, &entry.model_prefix);
                    matches
                        && self
                            .entries
                            .iter()
                            .filter(|candidate| {
                                candidate.source == key.source
                                    && candidate.speed == key.speed
                                    && model_matches(&key.model, &candidate.model_prefix)
                            })
                            .map(|candidate| candidate.priority)
                            .min()
                            == Some(entry.priority)
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut multiplier_body = String::new();
        let mut keys = known_usage.iter().collect::<Vec<_>>();
        keys.sort_by_key(|key| key.persisted_key());
        for key in keys {
            use std::fmt::Write as _;
            let (_, multiplier) =
                self.fast_billing_equivalent(key.source, Some(&key.model), key.speed, 1);
            let _ = write!(multiplier_body, "{}={:?};", key.persisted_key(), multiplier);
        }
        self.fingerprint = hex_sha256(
            format!(
                "policy={PRICING_POLICY_VERSION};rates={};multipliers={multiplier_body}",
                compiled_fingerprint(&relevant)
            )
            .as_bytes(),
        );
        self
    }

    pub fn estimate_entry(&self, entry: &UsageEntry) -> PriceEstimate {
        self.estimate(
            entry.source,
            entry.model.as_deref(),
            entry.speed,
            entry.inference_geo,
            &entry.occurred_at,
            &entry.tokens,
        )
    }

    pub fn estimate_row(&self, row: &RepriceRow) -> PriceEstimate {
        self.estimate(
            row.source,
            row.model.as_deref(),
            row.speed,
            row.inference_geo,
            &row.occurred_at,
            &row.tokens,
        )
    }

    fn estimate(
        &self,
        source: UsageSource,
        model: Option<&str>,
        speed: UsageSpeed,
        geo: InferenceGeo,
        occurred_at: &str,
        tokens: &super::model::TokenFacts,
    ) -> PriceEstimate {
        let Some(model) = model.map(|value| {
            let normalized = normalize_model_key(value);
            if source == UsageSource::Claude {
                normalized.replace('.', "-")
            } else {
                normalized
            }
        }) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };
        if speed == UsageSpeed::Unknown {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        }
        let Ok(time) = DateTime::parse_from_rfc3339(occurred_at) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };
        let time = time.with_timezone(&Utc);

        if source == UsageSource::Codex
            && speed == UsageSpeed::Fast
            && tokens.input_tokens() > 272_000
        {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        }

        let Some(price) = self.entries.iter().find(|item| {
            item.source == source
                && item.speed == speed
                && model_matches(&model, &item.model_prefix)
                && item.effective_from.is_none_or(|from| time >= from)
                && item.effective_until.is_none_or(|until| time < until)
        }) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };

        let input_dimensions = [
            (
                tokens.uncached_input_tokens,
                price.uncached_input_nanos_per_m_tok,
            ),
            (
                tokens.cache_read_input_tokens,
                price.cache_read_nanos_per_m_tok,
            ),
            (
                tokens.cache_write_5m_input_tokens,
                price.cache_write_5m_nanos_per_m_tok,
            ),
            (
                tokens.cache_write_1h_input_tokens,
                price.cache_write_1h_nanos_per_m_tok,
            ),
        ];

        let mut input_numerator = 0_u128;
        for (count, rate) in input_dimensions {
            let Ok(count) = u128::try_from(count) else {
                return PriceEstimate {
                    cost_nanos: None,
                    assumed_geo: false,
                };
            };
            let Some(value) = count.checked_mul(u128::from(rate)) else {
                return PriceEstimate {
                    cost_nanos: None,
                    assumed_geo: false,
                };
            };
            let Some(next) = input_numerator.checked_add(value) else {
                return PriceEstimate {
                    cost_nanos: None,
                    assumed_geo: false,
                };
            };
            input_numerator = next;
        }
        let Ok(output_count) = u128::try_from(tokens.output_tokens) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };
        let Some(output_numerator) =
            output_count.checked_mul(u128::from(price.output_nanos_per_m_tok))
        else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };

        let long_context = price
            .long_context_threshold_tokens
            .is_some_and(|threshold| tokens.input_tokens() > threshold);
        if long_context && !price.long_context_priced {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        }
        let input_multiplier = if long_context {
            price.long_context_input_multiplier_bps
        } else {
            10_000
        };
        let output_multiplier = if long_context {
            price.long_context_output_multiplier_bps
        } else {
            10_000
        };
        let Some(input_scaled) = input_numerator.checked_mul(u128::from(input_multiplier)) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };
        let Some(output_scaled) = output_numerator.checked_mul(u128::from(output_multiplier))
        else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };
        let Some(numerator) = input_scaled.checked_add(output_scaled) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo: false,
            };
        };

        let multiplier = if geo == InferenceGeo::Us {
            price.us_inference_multiplier_bps
        } else {
            10_000
        };
        let assumed_geo =
            source == UsageSource::Claude && geo == InferenceGeo::Unknown && multiplier == 10_000;
        let Some(scaled) = numerator.checked_mul(u128::from(multiplier)) else {
            return PriceEstimate {
                cost_nanos: None,
                assumed_geo,
            };
        };
        let divisor = 1_000_000_u128 * 10_000_u128 * 10_000_u128;
        let rounded = (scaled + divisor / 2) / divisor;

        PriceEstimate {
            cost_nanos: i64::try_from(rounded).ok(),
            assumed_geo,
        }
    }

    pub(crate) fn needs_remote_refresh(
        &self,
        source: UsageSource,
        model: &str,
        speed: UsageSpeed,
    ) -> bool {
        if speed == UsageSpeed::Unknown {
            return false;
        }
        let mut model = normalize_model_key(model);
        if source == UsageSource::Claude {
            model = model.replace('.', "-");
        }
        if model.is_empty()
            || model == "codex-auto-review"
            || model == "<synthetic>"
            || model.starts_with('<')
        {
            return false;
        }
        !self.entries.iter().any(|item| {
            item.source == source
                && item.speed == speed
                && model_matches(&model, &item.model_prefix)
        })
    }

    /// Fast 原始 Token 对应的计费等效 Token。返回值均以 1e-9 为固定精度，
    /// 不使用浮点数，也不会覆盖原始 Token 事实。
    pub(crate) fn fast_billing_equivalent(
        &self,
        source: UsageSource,
        model: Option<&str>,
        speed: UsageSpeed,
        total_tokens: i64,
    ) -> (Option<i64>, Option<i64>) {
        if speed != UsageSpeed::Fast || total_tokens < 0 {
            return (None, None);
        }
        let Some(mut model) = model.map(normalize_model_key) else {
            return (None, None);
        };
        if source == UsageSource::Claude {
            model = model.replace('.', "-");
        }
        let multiplier_nanos = match source {
            UsageSource::Codex => codex_fast_multiplier_nanos(&model),
            UsageSource::Claude => claude_fast_multiplier_nanos(&model)
                .or_else(|| self.derived_claude_fast_multiplier_nanos(&model)),
            // Pi 无 Fast 概念，且不进价格目录；此路径只在断言失败时到达。
            UsageSource::Pi => None,
        };
        let Some(multiplier_nanos) = multiplier_nanos else {
            return (None, None);
        };
        let equivalent = i128::from(total_tokens)
            .checked_mul(i128::from(multiplier_nanos))
            .and_then(|value| i64::try_from(value).ok());
        (equivalent, equivalent.map(|_| multiplier_nanos))
    }

    fn derived_claude_fast_multiplier_nanos(&self, model: &str) -> Option<i64> {
        let now = Utc::now();
        let standard = self.current_entry(UsageSource::Claude, model, UsageSpeed::Standard, now)?;
        let fast = self.current_entry(UsageSource::Claude, model, UsageSpeed::Fast, now)?;
        let pairs = [
            (
                standard.uncached_input_nanos_per_m_tok,
                fast.uncached_input_nanos_per_m_tok,
            ),
            (standard.output_nanos_per_m_tok, fast.output_nanos_per_m_tok),
            (
                standard.cache_read_nanos_per_m_tok,
                fast.cache_read_nanos_per_m_tok,
            ),
            (
                standard.cache_write_5m_nanos_per_m_tok,
                fast.cache_write_5m_nanos_per_m_tok,
            ),
        ];
        let mut ratio = None;
        for (base, premium) in pairs {
            if base == 0 {
                if premium != 0 {
                    return None;
                }
                continue;
            }
            let numerator = u128::from(premium).checked_mul(1_000_000_000)?;
            if numerator % u128::from(base) != 0 {
                return None;
            }
            let current = i64::try_from(numerator / u128::from(base)).ok()?;
            if ratio.is_some_and(|value| value != current) {
                return None;
            }
            ratio = Some(current);
        }
        ratio
    }

    fn current_entry(
        &self,
        source: UsageSource,
        model: &str,
        speed: UsageSpeed,
        time: DateTime<Utc>,
    ) -> Option<&CompiledEntry> {
        self.entries.iter().find(|item| {
            item.source == source
                && item.speed == speed
                && model_matches(model, &item.model_prefix)
                && item.effective_from.is_none_or(|from| time >= from)
                && item.effective_until.is_none_or(|until| time < until)
        })
    }
}

pub struct PricingCatalogStore {
    remote: RemotePricingStore,
}

impl PricingCatalogStore {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            remote: RemotePricingStore::new(directory),
        }
    }

    pub fn load(&self) -> io::Result<PricingCatalog> {
        Ok(PricingCatalog::merged(&self.remote.active()))
    }

    pub(crate) fn load_for_known_usage(
        &self,
        known_usage: &HashSet<PricingUsageKey>,
    ) -> io::Result<PricingCatalog> {
        Ok(PricingCatalog::merged(&self.remote.active()).scoped_to_known_usage(known_usage))
    }

    pub(crate) fn is_refresh_due(&self) -> bool {
        self.remote.is_due()
    }

    pub(crate) async fn refresh(&self, mode: PricingRefreshMode) -> PricingRefreshOutcome {
        self.remote.refresh(mode).await
    }

    pub(crate) fn mark_missing_refresh_attempts(
        &self,
        keys: &HashSet<PricingUsageKey>,
    ) -> io::Result<bool> {
        let persisted = keys.iter().map(PricingUsageKey::persisted_key).collect();
        self.remote.mark_missing_refresh_attempts(&persisted)
    }

    pub(crate) fn commit_pending(&self) {
        let _ = self.remote.commit_pending();
    }
}

fn local_priority(item: &CatalogEntry) -> u8 {
    let fixed_standard = item.speed == UsageSpeed::Standard
        && (item.effective_from.is_some()
            || item.effective_until.is_some()
            || item.long_context_threshold_tokens.is_some()
            || item.model_prefix == "gpt-5.5-pro");
    let fixed_fast = item.speed == UsageSpeed::Fast
        && matches!(
            item.model_prefix.as_str(),
            "gpt-5.5" | "gpt-5.5-codex" | "claude-opus-4-7" | "claude-opus-4-6"
        );
    if fixed_standard || fixed_fast {
        LOCAL_OVERRIDE_PRIORITY
    } else {
        LOCAL_FALLBACK_PRIORITY
    }
}

fn codex_fast_multiplier_nanos(model: &str) -> Option<i64> {
    match model {
        "gpt-5.6" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-5.5"
        | "gpt-5.5-codex" => Some(2_500_000_000),
        "gpt-5.4" | "gpt-5.4-codex" => Some(2_000_000_000),
        _ => None,
    }
}

fn claude_fast_multiplier_nanos(model: &str) -> Option<i64> {
    match model {
        "claude-opus-5" | "claude-opus-4-8" => Some(2_000_000_000),
        "claude-opus-4-7" | "claude-opus-4-6" => Some(6_000_000_000),
        _ => None,
    }
}

fn sort_entries(entries: &mut [CompiledEntry]) {
    entries.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then_with(|| right.model_prefix.len().cmp(&left.model_prefix.len()))
            .then_with(|| left.source.as_db().cmp(right.source.as_db()))
            .then_with(|| left.speed.as_db().cmp(right.speed.as_db()))
            .then_with(|| left.model_prefix.cmp(&right.model_prefix))
            .then_with(|| left.effective_from.cmp(&right.effective_from))
    });
}

fn compile_remote_entries(
    remote: &PricingCachePayload,
    local_entries: &[CompiledEntry],
) -> Vec<CompiledEntry> {
    let mut output = Vec::new();
    let standard_keys = union_keys([
        &remote.lite_llm.standard_rates,
        &remote.models_dev.standard_rates,
    ]);
    for key in standard_keys {
        let Some(price) = remote
            .lite_llm
            .standard_rates
            .get(&key)
            .or_else(|| remote.models_dev.standard_rates.get(&key))
        else {
            continue;
        };
        for source in UsageSource::ALL {
            output.push(remote_entry(
                source,
                &key,
                UsageSpeed::Standard,
                price,
                local_entries,
            ));
        }
    }

    let codex_fast_keys = union_keys([
        &remote.models_dev.codex_fast_rates,
        &remote.lite_llm.codex_fast_rates,
    ]);
    for key in codex_fast_keys {
        if let Some(price) = remote
            .models_dev
            .codex_fast_rates
            .get(&key)
            .or_else(|| remote.lite_llm.codex_fast_rates.get(&key))
        {
            output.push(remote_entry(
                UsageSource::Codex,
                &key,
                UsageSpeed::Fast,
                price,
                local_entries,
            ));
        }
    }

    let claude_fast_keys = union_keys([
        &remote.models_dev.claude_fast_rates,
        &remote.lite_llm.claude_fast_rates,
    ]);
    for key in claude_fast_keys {
        if let Some(price) = remote
            .models_dev
            .claude_fast_rates
            .get(&key)
            .or_else(|| remote.lite_llm.claude_fast_rates.get(&key))
        {
            output.push(remote_entry(
                UsageSource::Claude,
                &key,
                UsageSpeed::Fast,
                price,
                local_entries,
            ));
        }
    }
    output
}

fn union_keys<const N: usize>(maps: [&HashMap<String, RemoteModelPrice>; N]) -> Vec<String> {
    let mut keys = HashSet::new();
    for map in maps {
        keys.extend(map.keys().cloned());
    }
    let mut keys: Vec<_> = keys.into_iter().collect();
    keys.sort();
    keys
}

fn remote_entry(
    source: UsageSource,
    model: &str,
    speed: UsageSpeed,
    price: &RemoteModelPrice,
    local_entries: &[CompiledEntry],
) -> CompiledEntry {
    let inherited_geo = local_entries
        .iter()
        .find(|entry| {
            entry.source == source
                && entry.speed == speed
                && model_matches(model, &entry.model_prefix)
        })
        .map_or(10_000, |entry| entry.us_inference_multiplier_bps);
    let cache_write_1h = if source == UsageSource::Claude {
        price.uncached_input_nanos_per_m_tok.saturating_mul(2)
    } else {
        price.cache_write_nanos_per_m_tok
    };
    CompiledEntry {
        priority: REMOTE_PRIORITY,
        source,
        model_prefix: model.to_owned(),
        speed,
        effective_from: None,
        effective_until: None,
        uncached_input_nanos_per_m_tok: price.uncached_input_nanos_per_m_tok,
        cache_read_nanos_per_m_tok: price.cache_read_nanos_per_m_tok,
        cache_write_5m_nanos_per_m_tok: price.cache_write_nanos_per_m_tok,
        cache_write_1h_nanos_per_m_tok: cache_write_1h,
        output_nanos_per_m_tok: price.output_nanos_per_m_tok,
        us_inference_multiplier_bps: inherited_geo,
        long_context_threshold_tokens: None,
        long_context_input_multiplier_bps: 10_000,
        long_context_output_multiplier_bps: 10_000,
        long_context_priced: true,
    }
}

fn compiled_fingerprint(entries: &[CompiledEntry]) -> String {
    let mut body = String::new();
    for entry in entries {
        use std::fmt::Write as _;
        let _ = write!(
            body,
            "{}|{}|{}|{}|{:?}|{:?}|{}|{}|{}|{}|{}|{}|{:?}|{}|{}|{};",
            entry.priority,
            entry.source.as_db(),
            entry.model_prefix,
            entry.speed.as_db(),
            entry.effective_from,
            entry.effective_until,
            entry.uncached_input_nanos_per_m_tok,
            entry.cache_read_nanos_per_m_tok,
            entry.cache_write_5m_nanos_per_m_tok,
            entry.cache_write_1h_nanos_per_m_tok,
            entry.output_nanos_per_m_tok,
            entry.us_inference_multiplier_bps,
            entry.long_context_threshold_tokens,
            entry.long_context_input_multiplier_bps,
            entry.long_context_output_multiplier_bps,
            entry.long_context_priced,
        );
    }
    hex_sha256(body.as_bytes())
}

pub(crate) fn normalize_model_key(model: &str) -> String {
    let mut value = model.trim().to_ascii_lowercase();
    for prefix in ["openai/", "anthropic/", "deepseek/"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped.to_owned();
            break;
        }
    }
    if let Some((base, suffix)) = value.split_once('@')
        && is_compact_date(suffix)
    {
        value = base.to_owned();
    }
    if value.len() >= 11 {
        let split = value.len() - 11;
        let suffix = &value[split..];
        if suffix.starts_with('-') && is_dashed_date(&suffix[1..]) {
            value.truncate(split);
        }
    }
    if value.len() >= 9 {
        let split = value.len() - 9;
        let suffix = &value[split..];
        if suffix.starts_with('-') && is_compact_date(&suffix[1..]) {
            value.truncate(split);
        }
    }
    value
}

fn is_compact_date(value: &str) -> bool {
    value.len() == 8 && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_dashed_date(value: &str) -> bool {
    value.len() == 10
        && value.as_bytes()[4] == b'-'
        && value.as_bytes()[7] == b'-'
        && value
            .bytes()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
}

fn parse_optional_time(value: Option<&str>) -> Result<Option<DateTime<Utc>>, ()> {
    value
        .map(|value| {
            DateTime::parse_from_rfc3339(value)
                .map(|time| time.with_timezone(&Utc))
                .map_err(|_| ())
        })
        .transpose()
}

fn default_true() -> bool {
    true
}

fn ranges_overlap(
    left_from: Option<&DateTime<Utc>>,
    left_until: Option<&DateTime<Utc>>,
    right_from: Option<&DateTime<Utc>>,
    right_until: Option<&DateTime<Utc>>,
) -> bool {
    left_until.is_none_or(|until| right_from.is_none_or(|from| from < until))
        && right_until.is_none_or(|until| left_from.is_none_or(|from| from < until))
}

fn model_matches(model: &str, prefix: &str) -> bool {
    if model == prefix {
        return true;
    }
    let Some(snapshot) = model
        .strip_prefix(prefix)
        .and_then(|suffix| suffix.strip_prefix('-'))
    else {
        return false;
    };
    NaiveDate::parse_from_str(snapshot, "%Y-%m-%d").is_ok()
        || NaiveDate::parse_from_str(snapshot, "%Y%m%d").is_ok()
}

fn parse_usd_rate(value: &str) -> Result<u64, ()> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return Err(());
    }
    let (whole, fractional) = value.split_once('.').unwrap_or((value, ""));
    if fractional.len() > 9 || !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    }
    if !fractional.chars().all(|c| c.is_ascii_digit()) {
        return Err(());
    }

    let whole: u64 = whole.parse().map_err(|_| ())?;
    let fraction = if fractional.is_empty() {
        0
    } else {
        let parsed: u64 = fractional.parse().map_err(|_| ())?;
        parsed
            .checked_mul(10_u64.pow(9 - fractional.len() as u32))
            .ok_or(())?
    };
    whole
        .checked_mul(1_000_000_000)
        .and_then(|value| value.checked_add(fraction))
        .ok_or(())
}

fn hex_sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::usage::model::{InferenceGeo, TokenFacts, UsageEntry};

    fn entry(source: UsageSource, model: &str, speed: UsageSpeed) -> UsageEntry {
        UsageEntry {
            source,
            dedup_key: "dedup".to_owned(),
            conversation_key: "conversation".to_owned(),
            model: Some(model.to_owned()),
            speed,
            inference_geo: InferenceGeo::Global,
            occurred_at: "2026-07-30T00:00:00Z".to_owned(),
            day_local: "2026-07-30".to_owned(),
            tokens: TokenFacts {
                uncached_input_tokens: 1_000_000,
                output_tokens: 1_000_000,
                ..TokenFacts::default()
            },
            api_equivalent_cost_nanos: None,
            billing_equivalent_tokens_nanos: None,
            fast_multiplier_nanos: None,
            pricing_fingerprint: None,
        }
    }

    #[test]
    fn decimal_rates_are_exact_nanos() {
        assert_eq!(parse_usd_rate("0.1"), Ok(100_000_000));
        assert_eq!(parse_usd_rate("3.125"), Ok(3_125_000_000));
        assert_eq!(parse_usd_rate("-1"), Err(()));
    }

    #[test]
    fn codex_standard_and_priority_use_distinct_catalog_rows() {
        let catalog = PricingCatalog::bundled();
        let mut standard_entry = entry(UsageSource::Codex, "gpt-5.6-sol", UsageSpeed::Standard);
        standard_entry.tokens.uncached_input_tokens = 100_000;
        standard_entry.tokens.output_tokens = 100_000;
        let mut fast_entry = standard_entry.clone();
        fast_entry.speed = UsageSpeed::Fast;

        assert_eq!(
            catalog.estimate_entry(&standard_entry).cost_nanos,
            Some(3_500_000_000)
        );
        assert_eq!(
            catalog.estimate_entry(&fast_entry).cost_nanos,
            Some(7_000_000_000)
        );
    }

    #[test]
    fn unknown_model_is_unpriced_instead_of_zero() {
        let estimate = PricingCatalog::bundled().estimate_entry(&entry(
            UsageSource::Claude,
            "claude-unknown",
            UsageSpeed::Standard,
        ));
        assert_eq!(estimate.cost_nanos, None);
        assert_eq!(
            PricingCatalog::bundled()
                .estimate_entry(&entry(
                    UsageSource::Codex,
                    "gpt-5.5-pro",
                    UsageSpeed::Standard
                ))
                .cost_nanos,
            Some(210_000_000_000),
            "gpt-5.5-pro uses its audited fixed local price"
        );
        let mut pro_cache_write = entry(UsageSource::Codex, "gpt-5.5-pro", UsageSpeed::Standard);
        pro_cache_write.tokens = TokenFacts {
            cache_write_5m_input_tokens: 1_000_000,
            ..TokenFacts::default()
        };
        assert_eq!(
            PricingCatalog::bundled()
                .estimate_entry(&pro_cache_write)
                .cost_nanos,
            Some(0),
            "gpt-5.5-pro cache creation is not billed by the cc-bar policy"
        );
        assert_eq!(
            PricingCatalog::bundled()
                .estimate_entry(&entry(
                    UsageSource::Codex,
                    "gpt-5.5-2",
                    UsageSpeed::Standard
                ))
                .cost_nanos,
            None,
            "a numeric minor version is not a dated snapshot"
        );
        assert!(
            PricingCatalog::bundled()
                .estimate_entry(&entry(
                    UsageSource::Codex,
                    "gpt-5.5-2026-04-23",
                    UsageSpeed::Standard
                ))
                .cost_nanos
                .is_some()
        );
    }

    #[test]
    fn bundled_catalog_covers_publicly_priceable_models_seen_in_local_logs() {
        let catalog = PricingCatalog::bundled();
        for model in [
            "gpt-5.5",
            "gpt-5.4",
            "gpt-5.4-mini",
            "gpt-5.3-codex",
            "claude-sonnet-5",
            "claude-opus-5",
            "claude-opus-4.8",
            "claude-sonnet-4.6",
            "claude-fable-5",
            "claude-haiku-4-5-20251001",
            "claude-opus-4.7",
        ] {
            let source = if model.starts_with("gpt-") {
                UsageSource::Codex
            } else {
                UsageSource::Claude
            };
            assert!(
                catalog
                    .estimate_entry(&entry(source, model, UsageSpeed::Standard))
                    .cost_nanos
                    .is_some(),
                "{model} should have an official standard price"
            );
        }
    }

    #[test]
    fn overlapping_effective_ranges_are_rejected() {
        let raw = r#"{
          "schemaVersion": 1,
          "asOf": "2026-07-30",
          "sources": ["https://example.com/pricing"],
          "entries": [
            {
              "source": "codex",
              "modelPrefix": "model",
              "speed": "standard",
              "effectiveUntil": "2026-08-01T00:00:00Z",
              "uncachedInputUsdPerMTok": "1",
              "cacheReadUsdPerMTok": "0.1",
              "cacheWrite5mUsdPerMTok": "1",
              "cacheWrite1hUsdPerMTok": "1",
              "outputUsdPerMTok": "2",
              "usInferenceMultiplierBps": 10000
            },
            {
              "source": "codex",
              "modelPrefix": "model",
              "speed": "standard",
              "effectiveFrom": "2026-07-31T00:00:00Z",
              "uncachedInputUsdPerMTok": "1",
              "cacheReadUsdPerMTok": "0.1",
              "cacheWrite5mUsdPerMTok": "1",
              "cacheWrite1hUsdPerMTok": "1",
              "outputUsdPerMTok": "2",
              "usInferenceMultiplierBps": 10000
            }
          ]
        }"#;

        assert!(PricingCatalog::parse(raw).is_err());
    }

    #[test]
    fn claude_us_inference_applies_the_catalog_multiplier() {
        let catalog = PricingCatalog::bundled();
        let mut usage = entry(UsageSource::Claude, "claude-opus-5", UsageSpeed::Standard);
        usage.inference_geo = InferenceGeo::Us;

        assert_eq!(
            catalog.estimate_entry(&usage).cost_nanos,
            Some(33_000_000_000)
        );
    }

    #[test]
    fn sonnet_5_price_switches_at_the_effective_time_boundary() {
        let catalog = PricingCatalog::bundled();
        let mut usage = entry(UsageSource::Claude, "claude-sonnet-5", UsageSpeed::Standard);
        usage.occurred_at = "2026-08-31T23:59:59Z".to_owned();
        assert_eq!(
            catalog.estimate_entry(&usage).cost_nanos,
            Some(12_000_000_000)
        );

        usage.occurred_at = "2026-09-01T00:00:00Z".to_owned();
        assert_eq!(
            catalog.estimate_entry(&usage).cost_nanos,
            Some(18_000_000_000)
        );
    }

    #[test]
    fn sol_long_context_uses_documented_multipliers_and_priority_stays_unpriced() {
        let catalog = PricingCatalog::bundled();
        let mut standard = entry(UsageSource::Codex, "gpt-5.6-sol", UsageSpeed::Standard);
        standard.tokens = TokenFacts {
            uncached_input_tokens: 300_000,
            output_tokens: 100_000,
            ..TokenFacts::default()
        };
        let mut priority = standard.clone();
        priority.speed = UsageSpeed::Fast;

        assert_eq!(
            catalog.estimate_entry(&standard).cost_nanos,
            Some(7_500_000_000)
        );
        assert_eq!(catalog.estimate_entry(&priority).cost_nanos, None);
    }

    #[test]
    fn terra_and_luna_long_context_use_documented_multipliers() {
        let catalog = PricingCatalog::bundled();
        let mut standard = entry(UsageSource::Codex, "gpt-5.6-terra", UsageSpeed::Standard);
        standard.tokens = TokenFacts {
            uncached_input_tokens: 300_000,
            output_tokens: 100_000,
            ..TokenFacts::default()
        };

        assert_eq!(
            catalog.estimate_entry(&standard).cost_nanos,
            Some(3_000_000_000)
        );

        standard.model = Some("gpt-5.6-luna".to_owned());
        assert_eq!(
            catalog.estimate_entry(&standard).cost_nanos,
            Some(300_000_000)
        );
    }

    #[test]
    fn fast_billing_equivalent_uses_known_multiplier_without_changing_raw_tokens() {
        let catalog = PricingCatalog::bundled();
        assert_eq!(
            catalog.fast_billing_equivalent(
                UsageSource::Codex,
                Some("gpt-5.6-terra"),
                UsageSpeed::Fast,
                120,
            ),
            (Some(300_000_000_000), Some(2_500_000_000))
        );
        assert_eq!(
            catalog.fast_billing_equivalent(
                UsageSource::Claude,
                Some("claude-opus-4.7"),
                UsageSpeed::Fast,
                10,
            ),
            (Some(60_000_000_000), Some(6_000_000_000))
        );
    }

    #[test]
    fn scoped_fingerprint_ignores_unrelated_remote_models() {
        let known = HashSet::from([PricingUsageKey::new(
            UsageSource::Claude,
            "claude-opus-5",
            UsageSpeed::Fast,
        )]);
        let base = PricingCatalog::merged(&PricingCachePayload::default())
            .scoped_to_known_usage(&known)
            .fingerprint()
            .to_owned();
        let mut remote = PricingCachePayload::default();
        remote.models_dev.standard_rates.insert(
            "unrelated-model".to_owned(),
            RemoteModelPrice {
                uncached_input_nanos_per_m_tok: 1,
                output_nanos_per_m_tok: 2,
                ..RemoteModelPrice::default()
            },
        );
        let next = PricingCatalog::merged(&remote)
            .scoped_to_known_usage(&known)
            .fingerprint()
            .to_owned();

        assert_eq!(base, next);
    }

    #[test]
    fn scoped_fingerprint_changes_for_related_fast_price() {
        let known = HashSet::from([PricingUsageKey::new(
            UsageSource::Claude,
            "claude-future",
            UsageSpeed::Fast,
        )]);
        let base = PricingCatalog::merged(&PricingCachePayload::default())
            .scoped_to_known_usage(&known)
            .fingerprint()
            .to_owned();
        let mut remote = PricingCachePayload::default();
        remote.models_dev.claude_fast_rates.insert(
            "claude-future".to_owned(),
            RemoteModelPrice {
                uncached_input_nanos_per_m_tok: 10,
                output_nanos_per_m_tok: 50,
                ..RemoteModelPrice::default()
            },
        );
        let next = PricingCatalog::merged(&remote)
            .scoped_to_known_usage(&known)
            .fingerprint()
            .to_owned();

        assert_ne!(base, next);
    }

    #[test]
    fn scoped_fingerprint_ignores_remote_price_shadowed_by_local_policy() {
        let known = HashSet::from([PricingUsageKey::new(
            UsageSource::Codex,
            "gpt-5.5",
            UsageSpeed::Standard,
        )]);
        let base = PricingCatalog::merged(&PricingCachePayload::default())
            .scoped_to_known_usage(&known)
            .fingerprint()
            .to_owned();
        let mut remote = PricingCachePayload::default();
        remote.lite_llm.standard_rates.insert(
            "gpt-5.5".to_owned(),
            RemoteModelPrice {
                uncached_input_nanos_per_m_tok: 1,
                output_nanos_per_m_tok: 1,
                ..RemoteModelPrice::default()
            },
        );
        let next = PricingCatalog::merged(&remote)
            .scoped_to_known_usage(&known)
            .fingerprint()
            .to_owned();

        assert_eq!(base, next);
    }

    #[test]
    fn legacy_external_catalog_is_retained_and_migrated_to_v2_cache() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join("pricing-catalog.json"), BUNDLED_CATALOG)
            .expect("seed legacy catalog");
        let store = PricingCatalogStore::new(dir.path().to_path_buf());

        let catalog = store.load().expect("recover catalog");
        assert_eq!(
            catalog.fingerprint(),
            PricingCatalog::bundled().fingerprint()
        );
        let names = fs::read_dir(dir.path())
            .expect("read directory")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == "pricing-catalog.json"));
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("pricing-catalog.json.legacy-v1-"))
        );
    }
}
