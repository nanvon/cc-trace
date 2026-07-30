use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::contracts::{UsageSource, UsageSpeed};

use super::model::{InferenceGeo, RepriceRow, UsageEntry};

const CATALOG_FILE: &str = "pricing-catalog.json";
const TEMP_FILE: &str = "pricing-catalog.json.tmp";
const CATALOG_SCHEMA_VERSION: u32 = 1;
const BUNDLED_CATALOG: &str = include_str!("pricing-catalog.v1.json");

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

        let canonical = serde_json::to_vec(&document).map_err(|_| ())?;
        let fingerprint = hex_sha256(&canonical);
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

        entries.sort_by(|left, right| {
            right
                .model_prefix
                .len()
                .cmp(&left.model_prefix.len())
                .then_with(|| left.effective_from.cmp(&right.effective_from))
        });

        Ok(Self {
            entries,
            fingerprint,
        })
    }

    pub fn bundled() -> Self {
        Self::parse(BUNDLED_CATALOG).expect("bundled pricing catalog must be valid")
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
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
            let normalized = value.to_ascii_lowercase();
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
}

pub struct PricingCatalogStore {
    directory: PathBuf,
    load_lock: Mutex<()>,
}

impl PricingCatalogStore {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            load_lock: Mutex::new(()),
        }
    }

    pub fn load(&self) -> io::Result<PricingCatalog> {
        let _guard = self.load_lock.lock().expect("pricing catalog load lock");
        fs::create_dir_all(&self.directory)?;
        let path = self.directory.join(CATALOG_FILE);

        match fs::read_to_string(&path) {
            Ok(raw) => match PricingCatalog::parse(&raw) {
                Ok(catalog) => Ok(catalog),
                Err(()) => {
                    self.quarantine(&path);
                    self.write_bundled()?;
                    Ok(PricingCatalog::bundled())
                }
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                self.write_bundled()?;
                Ok(PricingCatalog::bundled())
            }
            Err(error) => Err(error),
        }
    }

    fn write_bundled(&self) -> io::Result<()> {
        let temp = self.directory.join(TEMP_FILE);
        {
            let mut file = fs::File::create(&temp)?;
            file.write_all(BUNDLED_CATALOG.as_bytes())?;
            file.sync_all()?;
        }
        fs::rename(temp, self.directory.join(CATALOG_FILE))
    }

    fn quarantine(&self, path: &std::path::Path) {
        let suffix = Utc::now().format("%Y%m%dT%H%M%S%.3fZ");
        let target = self
            .directory
            .join(format!("{CATALOG_FILE}.corrupt-{suffix}"));
        let _ = fs::rename(path, target);
    }
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
            None,
            "a broad family prefix must not misprice a distinct model variant"
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
    fn corrupt_external_catalog_is_retained_and_reseeded() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(CATALOG_FILE), "{not json").expect("seed corrupt");
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
        assert!(names.iter().any(|name| name == CATALOG_FILE));
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("pricing-catalog.json.corrupt-"))
        );
    }
}
