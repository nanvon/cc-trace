//! Antigravity 额度来源：loadCodeAssist 三阶段富化 + 标准化额度窗口。
//!
//! 端点、请求头与字段映射对齐 cc-bar v1.1.1 的 `AntigravityQuotaClient`：
//!
//! 1. `v1internal:loadCodeAssist`（先 daily 域，再主域回退）给套餐与基础额度；
//! 2. 没有分组额度时用 `v1internal:fetchAvailableModels` 补 Gemini 轮换窗口；
//! 3. 再用 `v1internal:retrieveUserQuotaSummary` 补四组权威额度；
//! 4. 最后用 `v1internal:retrieveUserQuota` 按模型分桶兜底。
//!
//! 分组源（`retrieveUserQuotaSummary`）语义最明确：Gemini 组给 5h 与周窗口，
//! `3p-` 组给第三方（Claude／GPT）的 5h 与周窗口。没有分组源时才退化成
//! 「5h 主条 + 周副条」的旧行为。
//!
//! User-Agent 与 `X-Goog-Api-Client` 照搬 cc-bar 的取值（`antigravity/1.0 darwin/arm64
//! google-api-nodejs-client/10.3.0`）。这是对官方客户端流量的模拟，服务端按它决定
//! 套餐池路由，因此不做「平台真实化」改写；Windows 侧未实机验证。

use std::sync::Arc;

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use tokio::sync::Mutex;

use super::credentials::antigravity::{self, AntigravityAccount};
use super::credentials::{Discovery, Secret};
use super::{BoxFuture, ProviderFetchOutcome, QuotaProvider, http};
use crate::contracts::{
    ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot, QuotaWindow, QuotaWindowKind,
};

const DAILY_BASE: &str = "https://daily-cloudcode-pa.googleapis.com/v1internal";
const MAIN_BASE: &str = "https://cloudcode-pa.googleapis.com/v1internal";
const USERINFO_ENDPOINT: &str = "https://www.googleapis.com/oauth2/v3/userinfo";
const USER_AGENT: &str = "antigravity/1.0 darwin/arm64 google-api-nodejs-client/10.3.0";
const GOOG_API_CLIENT: &str = "gl-node/20.0.0";
const FIVE_HOUR_SECONDS: u64 = 18_000;
const WEEKLY_SECONDS: u64 = 604_800;

#[derive(Clone)]
pub struct AntigravityProvider {
    refresh_lock: Arc<Mutex<()>>,
}

impl std::fmt::Debug for AntigravityProvider {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("AntigravityProvider").finish()
    }
}

impl AntigravityProvider {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            refresh_lock: Arc::new(Mutex::new(())),
        })
    }
}

impl QuotaProvider for AntigravityProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Antigravity
    }

    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome> {
        Box::pin(async move {
            // 续期是「读—改—写」序列，且跨越网络请求，整个 fetch 期间持锁。
            let _guard = self.refresh_lock.lock().await;
            self.fetch_once().await
        })
    }
}

impl AntigravityProvider {
    async fn fetch_once(&self) -> ProviderFetchOutcome {
        let account = match antigravity::discover() {
            Discovery::Found(account) => account,
            Discovery::Missing => return ProviderFetchOutcome::NoCredentials,
            Discovery::Unsupported => return ProviderFetchOutcome::Unsupported,
            Discovery::Unreadable | Discovery::Expired => {
                return ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Credentials,
                };
            }
        };

        let account = match ensure_fresh(account).await {
            Ok(account) => account,
            Err(outcome) => return outcome,
        };
        let Some(access_token) = account.access_token.as_ref() else {
            return ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            };
        };

        let mut parsed = match fetch_quota(access_token, Utc::now()).await {
            Ok(parsed) => parsed,
            Err(outcome) => return outcome,
        };

        // 邮箱是可选的展示信息：拿不到就留空，不让它影响额度本身。
        let email = match account.email.clone() {
            Some(email) => Some(email),
            None => fetch_email(access_token).await,
        };
        parsed.identity = Some(ProviderIdentity {
            account: email.clone(),
            plan: parsed.identity.and_then(|identity| identity.plan),
            credential_source: Some(account.source.key().to_owned()),
        });
        if parsed.identity_key.is_none() {
            parsed.identity_key = super::credentials::identity_fingerprint(
                "antigravity",
                &[email.as_deref().map(Secret::new).as_ref()],
            );
        }

        ProviderFetchOutcome::Success {
            identity: parsed.identity,
            identity_key: parsed.identity_key,
            snapshot: parsed.snapshot,
        }
    }
}

/// 到期就续期。没有 refresh token 且已过期时报凭据类错误：
/// 用户需要重新在 Antigravity 里登录，而不是等我们重试。
async fn ensure_fresh(
    account: AntigravityAccount,
) -> Result<AntigravityAccount, ProviderFetchOutcome> {
    let now = Utc::now();
    if !account.is_expired(now) {
        return Ok(account);
    }
    let Some(refresh_token) = account.refresh_token.clone() else {
        return Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Credentials,
        });
    };

    match antigravity::refresh(&refresh_token, now).await {
        Ok(refreshed) => {
            // 回写失败不阻断本次使用：新令牌已经在手，按 ADR-0014 的语义，
            // 回写是为了下次启动能复用，失败只记在日志。
            let _ = antigravity::write_back(&account, &refreshed);
            Ok(AntigravityAccount {
                access_token: Some(refreshed.access_token.clone()),
                refresh_token: refreshed
                    .refresh_token
                    .clone()
                    .or(account.refresh_token.clone()),
                expires_at: Some(refreshed.expires_at),
                ..account
            })
        }
        Err(antigravity::RefreshFailure::Rejected) => Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Credentials,
        }),
        Err(antigravity::RefreshFailure::Transport) => Err(ProviderFetchOutcome::Offline),
        Err(antigravity::RefreshFailure::Protocol) => Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Protocol,
        }),
    }
}

/// 一次取数的结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedAntigravityUsage {
    pub identity: Option<ProviderIdentity>,
    pub identity_key: Option<String>,
    pub snapshot: QuotaSnapshot,
}

/// 按「daily 域优先、主域回退」取额度。
async fn fetch_quota(
    access_token: &Secret,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedAntigravityUsage, ProviderFetchOutcome> {
    let mut last = ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    };

    for (base, rich_body) in [(DAILY_BASE, false), (MAIN_BASE, true)] {
        match fetch_from(base, access_token, rich_body, fetched_at).await {
            Ok(parsed) => return Ok(parsed),
            Err(outcome) => {
                // 鉴权失败是终局：换域也不会好，直接把原因交给上层。
                if matches!(
                    outcome,
                    ProviderFetchOutcome::Failed {
                        kind: ErrorKind::Credentials
                    }
                ) {
                    return Err(outcome);
                }
                last = outcome;
            }
        }
    }
    Err(last)
}

async fn fetch_from(
    base: &str,
    access_token: &Secret,
    rich_body: bool,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedAntigravityUsage, ProviderFetchOutcome> {
    // daily 域保持与官方客户端一致的空 body；主域回退带项目上下文。
    let payload = if rich_body {
        serde_json::json!({
            "cloudaicompanionProject": "aicode-consumers",
            "metadata": { "ideName": "antigravity" }
        })
    } else {
        serde_json::json!({})
    };

    let root = post(base, "loadCodeAssist", access_token, &payload, 30).await?;
    let mut parsed = parse_raw(&root, fetched_at);

    if !parsed.has_grouped_data {
        for endpoint in [
            "fetchAvailableModels",
            "retrieveUserQuotaSummary",
            "retrieveUserQuota",
        ] {
            if let Ok(enriched) =
                post(base, endpoint, access_token, &serde_json::json!({}), 15).await
            {
                // 富化失败只是少一档数据，不算整体失败；成功则合并。
                parsed = merge_raw(parsed, parse_raw(&enriched, fetched_at));
            }
        }
    }

    to_usage(parsed, fetched_at)
}

async fn post(
    base: &str,
    method: &str,
    access_token: &Secret,
    payload: &Value,
    timeout_secs: u64,
) -> Result<Value, ProviderFetchOutcome> {
    let body = serde_json::to_string(payload).unwrap_or_else(|_| "{}".to_owned());
    let response = http::client()
        .post(format!("{base}:{method}"))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", access_token.expose()),
        )
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::USER_AGENT, USER_AGENT)
        .header("X-Goog-Api-Client", GOOG_API_CLIENT)
        .body(body)
        .send()
        .await
        .map_err(|error| http::classify_transport(&error))?;

    if let Some(failure) = http::classify_response(&response) {
        return Err(failure);
    }
    let text = response
        .text()
        .await
        .map_err(|error| http::classify_transport(&error))?;
    serde_json::from_str(&text).map_err(|_| ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    })
}

/// 取登录邮箱。失败返回 `None`：邮箱不是额度链路的必需项。
async fn fetch_email(access_token: &Secret) -> Option<String> {
    let response = http::client()
        .get(USERINFO_ENDPOINT)
        .header(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", access_token.expose()),
        )
        .header("X-Goog-Api-Client", GOOG_API_CLIENT)
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let root: Value = serde_json::from_str(&response.text().await.ok()?).ok()?;
    non_empty(root.get("email").and_then(Value::as_str))
}

fn non_empty(value: Option<&str>) -> Option<String> {
    let text = value?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// 解析中间结果：四个语义槽位 + 套餐名 + 是否拿到分组数据。
#[derive(Debug, Clone, Default, PartialEq)]
struct Parsed {
    /// 第三方（Claude／GPT）5h。
    five_hour: Option<QuotaWindow>,
    /// 第三方周额度。
    weekly: Option<QuotaWindow>,
    gemini_window: Option<QuotaWindow>,
    gemini_weekly: Option<QuotaWindow>,
    plan: Option<String>,
    /// 是否来自 `retrieveUserQuotaSummary` 的分组结构。
    has_grouped_data: bool,
}

/// 把响应标准化成额度快照。
pub fn parse(
    root: &Value,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedAntigravityUsage, ProviderFetchOutcome> {
    to_usage(parse_raw(root, fetched_at), fetched_at)
}

/// 富化合并：只在空缺处补，不覆盖已经拿到的槽位。
fn merge_raw(base: Parsed, enrichment: Parsed) -> Parsed {
    Parsed {
        five_hour: base.five_hour.or(enrichment.five_hour),
        weekly: base.weekly.or(enrichment.weekly),
        gemini_window: base.gemini_window.or(enrichment.gemini_window),
        gemini_weekly: base.gemini_weekly.or(enrichment.gemini_weekly),
        plan: base.plan.or(enrichment.plan),
        has_grouped_data: base.has_grouped_data || enrichment.has_grouped_data,
    }
}

/// 组装展示窗口并校验：一个窗口都没有说明结构与预期不符。
fn to_usage(
    parsed: Parsed,
    fetched_at: DateTime<Utc>,
) -> Result<ParsedAntigravityUsage, ProviderFetchOutcome> {
    let windows = assemble(parsed.clone());
    if windows.is_empty() {
        return Err(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Protocol,
        });
    }

    Ok(ParsedAntigravityUsage {
        identity: Some(ProviderIdentity {
            account: None,
            plan: parsed.plan,
            credential_source: None,
        }),
        identity_key: None,
        snapshot: QuotaSnapshot {
            windows,
            captured_at: fetched_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
    })
}

/// 解析但允许没有窗口：富化阶段用它判断「这一档到底拿到了什么」。
fn parse_raw(root: &Value, fetched_at: DateTime<Utc>) -> Parsed {
    let _ = fetched_at;
    let mut parsed = Parsed {
        plan: plan_from(root),
        ..Parsed::default()
    };

    if let Some(groups) = root.get("groups").and_then(Value::as_array) {
        let group = parse_groups(groups, fetched_at);
        parsed.five_hour = group.five_hour;
        parsed.weekly = group.weekly;
        parsed.gemini_window = group.gemini_window;
        parsed.gemini_weekly = group.gemini_weekly;
        parsed.has_grouped_data = parsed.five_hour.is_some()
            || parsed.weekly.is_some()
            || parsed.gemini_window.is_some()
            || parsed.gemini_weekly.is_some();
        if parsed.plan.is_none() {
            parsed.plan = group.plan;
        }
    }

    if let Some(quota) = root.get("quota") {
        let quota_parsed = parse_quota_dict(quota, fetched_at);
        parsed.five_hour = parsed.five_hour.or(quota_parsed.five_hour);
        parsed.weekly = parsed.weekly.or(quota_parsed.weekly);
        parsed.gemini_window = parsed.gemini_window.or(quota_parsed.gemini_window);
        parsed.gemini_weekly = parsed.gemini_weekly.or(quota_parsed.gemini_weekly);
    }

    if let Some(buckets) = root.get("buckets").and_then(Value::as_array) {
        let bucket_parsed = parse_buckets(buckets, fetched_at);
        // buckets 是 retrieveUserQuota 的主数据，信息最全：新值非空时覆盖。
        parsed.five_hour = bucket_parsed.five_hour.or(parsed.five_hour);
        parsed.weekly = bucket_parsed.weekly.or(parsed.weekly);
        parsed.gemini_window = bucket_parsed.gemini_window.or(parsed.gemini_window);
        parsed.gemini_weekly = bucket_parsed.gemini_weekly.or(parsed.gemini_weekly);
    } else {
        if let Some(models) = root.get("models").and_then(Value::as_array) {
            let model_parsed = parse_models(models, fetched_at);
            parsed.five_hour = parsed.five_hour.or(model_parsed.five_hour);
            parsed.weekly = parsed.weekly.or(model_parsed.weekly);
            parsed.gemini_window = parsed.gemini_window.or(model_parsed.gemini_window);
            parsed.gemini_weekly = parsed.gemini_weekly.or(model_parsed.gemini_weekly);
        }
    }

    if let Some(models) = root.get("models").and_then(Value::as_object) {
        let model_parsed = parse_models_dict(models, fetched_at);
        parsed.five_hour = parsed.five_hour.or(model_parsed.five_hour);
        parsed.weekly = parsed.weekly.or(model_parsed.weekly);
        parsed.gemini_window = parsed.gemini_window.or(model_parsed.gemini_window);
        parsed.gemini_weekly = parsed.gemini_weekly.or(model_parsed.gemini_weekly);
    }

    if let Some(available) = root.get("availableModels").and_then(Value::as_array) {
        let model_parsed = parse_models(available, fetched_at);
        parsed.five_hour = parsed.five_hour.or(model_parsed.five_hour);
        parsed.weekly = parsed.weekly.or(model_parsed.weekly);
        parsed.gemini_window = parsed.gemini_window.or(model_parsed.gemini_window);
        parsed.gemini_weekly = parsed.gemini_weekly.or(model_parsed.gemini_weekly);
    }

    if parsed.five_hour.is_none()
        && parsed.weekly.is_none()
        && parsed.gemini_window.is_none()
        && parsed.gemini_weekly.is_none()
        && let Some(info) = root.get("quotaInfo")
    {
        let info_parsed = parse_quota_dict(info, fetched_at);
        parsed.five_hour = info_parsed.five_hour;
        parsed.weekly = info_parsed.weekly;
        parsed.gemini_window = info_parsed.gemini_window;
        parsed.gemini_weekly = info_parsed.gemini_weekly;
    }

    parsed
}

/// 四档语义槽位组装成展示窗口。
///
/// 分组源可用时按官方两组四窗口映射（Gemini 组在前，第三方组在后）；
/// 没有分组源时退化成「5h 主条 + 周副条」。
fn assemble(parsed: Parsed) -> Vec<QuotaWindow> {
    let mut windows = Vec::new();

    if parsed.has_grouped_data {
        if let Some(window) = parsed.gemini_window.clone() {
            windows.push(labelled(
                window,
                "gemini-5h",
                "Gemini 5H",
                QuotaWindowKind::FiveHour,
            ));
        }
        if let Some(window) = parsed.gemini_weekly.clone() {
            windows.push(labelled(
                window,
                "gemini-weekly",
                "Gemini WK",
                QuotaWindowKind::Weekly,
            ));
        }
        if let Some(window) = parsed.five_hour.clone() {
            windows.push(labelled(
                window,
                "claude-5h",
                "Claude 5H",
                QuotaWindowKind::FiveHour,
            ));
        }
        if let Some(window) = parsed.weekly.clone() {
            windows.push(labelled(
                window,
                "claude-weekly",
                "Claude WK",
                QuotaWindowKind::Weekly,
            ));
        }
    } else {
        let primary = parsed.five_hour.clone().or(parsed.weekly.clone());
        if let Some(window) = primary {
            let kind = window_kind(&window, QuotaWindowKind::FiveHour);
            let id = if kind == QuotaWindowKind::FiveHour {
                "antigravity-5h"
            } else {
                "antigravity-weekly"
            };
            let label = if kind == QuotaWindowKind::FiveHour {
                "5HOUR"
            } else {
                "WEEKLY"
            };
            windows.push(labelled(window, id, label, kind));
        }
        if parsed.five_hour.is_some()
            && let Some(window) = parsed.weekly.clone()
        {
            windows.push(labelled(
                window,
                "antigravity-weekly",
                "WEEKLY",
                QuotaWindowKind::Weekly,
            ));
        }
    }

    for (index, window) in windows.iter_mut().enumerate() {
        window.is_primary = index == 0;
    }
    windows
}

fn labelled(
    window: QuotaWindow,
    id: &str,
    display_name: &str,
    kind: QuotaWindowKind,
) -> QuotaWindow {
    QuotaWindow {
        id: id.to_owned(),
        kind,
        display_name: Some(display_name.to_owned()),
        is_active: true,
        is_primary: false,
        unlimited: false,
        ..window
    }
}

/// 新版 loadCodeAssist 的 tier 结构。付费订阅在 `paidTier`：Pro 账号可能同时返回
/// `currentTier=free-tier`，后者是基础 Code Assist 层级，不能覆盖付费订阅。
fn plan_from(root: &Value) -> Option<String> {
    if let Some(paid) = root.get("paidTier")
        && let Some(plan) = non_empty(paid.get("name").and_then(Value::as_str))
            .or_else(|| non_empty(paid.get("id").and_then(Value::as_str)))
    {
        return Some(plan);
    }
    if let Some(current) = root.get("currentTier") {
        if let Some(id) = non_empty(current.get("id").and_then(Value::as_str)) {
            return Some(id);
        }
        if let Some(text) = non_empty(current.as_str()) {
            return Some(text);
        }
    }
    if let Some(allowed) = root.get("allowedTiers").and_then(Value::as_array)
        && let Some(first) = allowed.first()
        && let Some(id) = non_empty(first.get("id").and_then(Value::as_str))
    {
        return Some(id);
    }
    root.get("tier")
        .and_then(|tier| non_empty(tier.get("id").and_then(Value::as_str)))
}

/// `retrieveUserQuotaSummary` 的分组结构：
/// `groups[] = { displayName, buckets: [{ bucketId, window, remainingFraction, resetTime }] }`
fn parse_groups(groups: &[Value], _fetched_at: DateTime<Utc>) -> Parsed {
    let mut parsed = Parsed::default();
    for group in groups {
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        let group_name = group
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_lowercase();

        for bucket in buckets {
            if bucket.get("disabled").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            let Some(remaining) = remaining_fraction(bucket) else {
                continue;
            };
            let id = text_at(bucket, "bucketId");
            let name = text_at(bucket, "displayName");
            let window_tag = text_at(bucket, "window");
            let reset = reset_time(bucket.get("resetTime"));

            // 归属判定：显式 gemini-／3p- 前缀优先，其次组名；旧结构缺前缀时按第三方处理。
            let is_gemini = if id.starts_with("gemini") {
                true
            } else if id.starts_with("3p")
                || id.starts_with("third")
                || id.starts_with("claude")
                || id.starts_with("gpt")
            {
                false
            } else {
                group_name.contains("gemini")
            };
            let is_weekly =
                id.contains("week") || window_tag.contains("week") || name.contains("week");

            let slot = if is_gemini {
                if is_weekly {
                    &mut parsed.gemini_weekly
                } else {
                    &mut parsed.gemini_window
                }
            } else if is_weekly {
                &mut parsed.weekly
            } else {
                &mut parsed.five_hour
            };
            let seconds = if is_weekly {
                WEEKLY_SECONDS
            } else {
                FIVE_HOUR_SECONDS
            };
            adopt_min(slot, remaining, reset, Some(seconds));
        }
    }
    parsed
}

/// `retrieveUserQuota` 的桶数组。语义按 bucketId／modelId 归类。
fn parse_buckets(buckets: &[Value], _fetched_at: DateTime<Utc>) -> Parsed {
    let mut parsed = Parsed::default();
    for bucket in buckets {
        let Some(remaining) = remaining_fraction(bucket) else {
            continue;
        };
        let model = text_at(bucket, "modelId");
        let model = if model.is_empty() {
            text_at(bucket, "model")
        } else {
            model
        };
        let bucket_id = text_at(bucket, "bucketId");
        let reset = reset_time(bucket.get("resetTime").or_else(|| bucket.get("reset_time")));

        let gemini = model.contains("gemini") || model.contains("tab_") || model.contains("chat_");
        let weekly = bucket_id.contains("week") || bucket_id.contains("weekly");
        if !gemini
            && !weekly
            && !(model.contains("claude")
                || model.contains("gpt")
                || model.contains("opus")
                || model.contains("sonnet"))
        {
            // 内部补齐／完整标记类：没有真实用量窗口。
            continue;
        }
        // 未消耗且没有重置时间的桶（新账号从未用过的模型）不是真实窗口，
        // 合成一个未来重置时间会把它显示成 0% 额度。
        if remaining >= 1.0 && reset.is_none() {
            continue;
        }

        let slot = if gemini {
            if weekly {
                &mut parsed.gemini_weekly
            } else {
                &mut parsed.gemini_window
            }
        } else if weekly {
            &mut parsed.weekly
        } else {
            &mut parsed.five_hour
        };
        let seconds = if weekly {
            WEEKLY_SECONDS
        } else {
            FIVE_HOUR_SECONDS
        };
        adopt_min(slot, remaining, reset, Some(seconds));
    }
    parsed
}

/// 模型数组（`fetchAvailableModels` 的 `availableModels` 与 `retrieveUserQuota` 的旧形态）。
///
/// 启发式与 cc-bar 一致：第一个 Gemini 当 5h、第二个当周；非 Gemini 同理。
fn parse_models(models: &[Value], _fetched_at: DateTime<Utc>) -> Parsed {
    let mut parsed = Parsed::default();
    for model in models {
        let Some(remaining) = remaining_fraction(model) else {
            continue;
        };
        let mut model_id = text_at(model, "modelId");
        if model_id.is_empty() {
            model_id = text_at(model, "name");
        }
        let reset = reset_time(model.get("resetTime").or_else(|| model.get("reset_time")));
        let weekly = model_id.contains("week");
        let window = window_from(
            remaining,
            reset,
            Some(if weekly {
                WEEKLY_SECONDS
            } else {
                FIVE_HOUR_SECONDS
            }),
        );

        if model_id.contains("gemini") {
            if parsed.gemini_window.is_none() {
                parsed.gemini_window = window;
            } else {
                parsed.gemini_weekly = window;
            }
        } else if parsed.five_hour.is_none() {
            parsed.five_hour = window;
        } else {
            parsed.weekly = window;
        }
    }
    parsed
}

/// `fetchAvailableModels` 的 `models` 是字典：`{ "<model-id>": { quotaInfo: {...} } }`。
///
/// `quotaInfo` 没有窗口类型标记，按 `resetTime` 距当前时间推断：
/// 2 小时内重置算 5h 轮换窗口（Gemini 与第三方都适用）。
fn parse_models_dict(models: &serde_json::Map<String, Value>, fetched_at: DateTime<Utc>) -> Parsed {
    let mut parsed = Parsed::default();
    let mut gemini_tightest: Option<(f64, QuotaWindow)> = None;

    for (key, value) in models {
        let info = value.get("quotaInfo").unwrap_or(value);
        let Some(remaining) = remaining_fraction(info) else {
            continue;
        };
        let reset = reset_time(info.get("resetTime").or_else(|| info.get("reset_time")));
        let is_five_hour = reset
            .as_ref()
            .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
            .map(|reset| (reset.with_timezone(&Utc) - fetched_at).num_seconds() < 2 * 3600)
            .unwrap_or(false);
        let Some(window) = window_from(remaining, reset, None) else {
            continue;
        };

        let model_id = key.to_lowercase();
        let is_gemini =
            model_id.contains("gemini") || model_id.contains("tab_") || model_id.contains("chat_");
        if is_gemini {
            if is_five_hour
                && gemini_tightest
                    .as_ref()
                    .is_none_or(|(tightest, _)| remaining < *tightest)
            {
                gemini_tightest = Some((remaining, window));
            }
        } else if is_five_hour {
            parsed.five_hour = Some(window);
        }
    }

    if let Some((_, window)) = gemini_tightest {
        // Gemini 组自己那一档一定填；只有第三方 5h 这一轮没拿到东西时才兼任主条，
        // 否则会把 Gemini 的读数贴到「Claude 5H」标签上。
        parsed.gemini_window = Some(window.clone());
        if parsed.five_hour.is_none() {
            parsed.five_hour = Some(window);
        }
    }
    parsed
}

/// 顶层 `quota` 字典（兼容旧语言服务器的 `quotaInfo`）。
fn parse_quota_dict(dict: &Value, _fetched_at: DateTime<Utc>) -> Parsed {
    let mut parsed = Parsed::default();
    let Some(object) = dict.as_object() else {
        return parsed;
    };
    for (key, bucket) in object {
        let Some(remaining) = remaining_fraction(bucket) else {
            continue;
        };
        let key = key.to_lowercase();
        let reset = reset_time(bucket.get("resetTime").or_else(|| bucket.get("reset_time")));
        let weekly = key.contains("week");
        let window = window_from(
            remaining,
            reset,
            Some(if weekly {
                WEEKLY_SECONDS
            } else {
                FIVE_HOUR_SECONDS
            }),
        );

        if key.contains("gemini") {
            if weekly {
                parsed.gemini_weekly = window;
            } else {
                parsed.gemini_window = window;
            }
        } else if weekly {
            parsed.weekly = window;
        } else {
            parsed.five_hour = window;
        }
    }
    parsed
}

/// 保留最紧张的窗口（剩余最少）；并列时优先带重置时间的。
fn adopt_min(
    slot: &mut Option<QuotaWindow>,
    remaining: f64,
    reset: Option<String>,
    window_seconds: Option<u64>,
) {
    let candidate = window_from(remaining, reset, window_seconds);
    match slot {
        None => *slot = candidate,
        Some(existing) => {
            let existing_remaining = existing.remaining_percent / 100.0;
            if remaining < existing_remaining
                || (remaining == existing_remaining
                    && existing.resets_at.is_none()
                    && candidate
                        .as_ref()
                        .is_some_and(|value| value.resets_at.is_some()))
            {
                *slot = candidate;
            }
        }
    }
}

fn window_from(
    remaining: f64,
    resets_at: Option<String>,
    window_seconds: Option<u64>,
) -> Option<QuotaWindow> {
    let used_percent = (100.0 - remaining * 100.0).clamp(0.0, 100.0);
    Some(QuotaWindow {
        id: String::new(),
        kind: QuotaWindowKind::Unknown,
        display_name: None,
        used_percent,
        remaining_percent: QuotaWindow::normalized_remaining(used_percent),
        resets_at,
        window_seconds,
        is_active: true,
        is_primary: false,
        unlimited: false,
    })
}

/// 窗口类型：给出窗口长度就按长度判，否则用调用方给的兜底类型。
fn window_kind(window: &QuotaWindow, fallback: QuotaWindowKind) -> QuotaWindowKind {
    match window.window_seconds {
        Some(seconds) if seconds >= WEEKLY_SECONDS => QuotaWindowKind::Weekly,
        Some(seconds) if seconds >= FIVE_HOUR_SECONDS => QuotaWindowKind::FiveHour,
        _ => fallback,
    }
}

fn text_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_lowercase()
}

/// 剩余比例。字段名与单位都有多种历史写法，取值顺序与 cc-bar 一致。
fn remaining_fraction(bucket: &Value) -> Option<f64> {
    for key in ["remainingFraction", "remaining_fraction"] {
        if let Some(value) = number(bucket.get(key)) {
            return Some(value);
        }
    }
    if let Some(value) = number(bucket.get("remaining")) {
        // 大于 1 视为百分数。
        return Some(if value > 1.0 { value / 100.0 } else { value });
    }
    if let Some(value) = number(bucket.get("remainingPercent")) {
        return Some(1.0 - value / 100.0);
    }
    if let Some(value) = number(bucket.get("usedPercent")) {
        return Some(1.0 - value / 100.0);
    }
    None
}

fn number(value: Option<&Value>) -> Option<f64> {
    let parsed = match value? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    parsed.is_finite().then_some(parsed)
}

/// 重置时间：ISO 8601 字符串，或 epoch 秒／毫秒数值。
fn reset_time(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return None;
            }
            DateTime::parse_from_rfc3339(trimmed).ok().map(|time| {
                time.with_timezone(&Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            })
        }
        Value::Number(_) => {
            let raw = number(value)?;
            if raw <= 0.0 {
                return None;
            }
            let millis = if raw > 10_000_000_000.0 {
                raw.round()
            } else {
                (raw * 1000.0).round()
            };
            Utc.timestamp_millis_opt(millis as i64)
                .single()
                .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 1, 12, 0, 0)
            .single()
            .expect("valid")
    }

    fn ids(windows: &[QuotaWindow]) -> Vec<&str> {
        windows.iter().map(|window| window.id.as_str()).collect()
    }

    #[test]
    fn grouped_quota_maps_four_windows_in_official_order() {
        let root = serde_json::json!({
            "paidTier": { "name": "Google AI Pro" },
            "groups": [
                {
                    "displayName": "Gemini Models",
                    "buckets": [
                        { "bucketId": "gemini-5h", "remainingFraction": 0.75,
                          "resetTime": "2026-10-01T15:00:00Z" },
                        { "bucketId": "gemini-weekly", "remainingFraction": 0.5,
                          "resetTime": "2026-10-06T00:00:00Z" }
                    ]
                },
                {
                    "displayName": "Claude and GPT models",
                    "buckets": [
                        { "bucketId": "3p-5h", "remainingFraction": 0.25,
                          "resetTime": "2026-10-01T14:00:00Z" },
                        { "bucketId": "3p-weekly", "remainingFraction": 0.9,
                          "resetTime": "2026-10-06T00:00:00Z" }
                    ]
                }
            ]
        });

        let parsed = parse(&root, fetched_at()).expect("parses");
        assert_eq!(
            ids(&parsed.snapshot.windows),
            ["gemini-5h", "gemini-weekly", "claude-5h", "claude-weekly"]
        );
        assert!(parsed.snapshot.windows[0].is_primary);
        assert_eq!(parsed.snapshot.windows[0].kind, QuotaWindowKind::FiveHour);
        assert!((parsed.snapshot.windows[0].used_percent - 25.0).abs() < f64::EPSILON);
        assert_eq!(parsed.snapshot.windows[2].kind, QuotaWindowKind::FiveHour);
        assert!((parsed.snapshot.windows[2].used_percent - 75.0).abs() < f64::EPSILON);
        assert_eq!(
            parsed.identity.and_then(|value| value.plan).as_deref(),
            Some("Google AI Pro")
        );
    }

    #[test]
    fn a_disabled_bucket_is_skipped() {
        let root = serde_json::json!({
            "groups": [{
                "displayName": "Gemini Models",
                "buckets": [
                    { "bucketId": "gemini-5h", "disabled": true, "remainingFraction": 0.1 },
                    { "bucketId": "gemini-5h", "remainingFraction": 0.4 }
                ]
            }]
        });

        let parsed = parse(&root, fetched_at()).expect("parses");
        assert!((parsed.snapshot.windows[0].used_percent - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_tightest_bucket_wins_within_a_group() {
        let root = serde_json::json!({
            "groups": [{
                "displayName": "Claude and GPT models",
                "buckets": [
                    { "bucketId": "3p-5h", "remainingFraction": 0.6 },
                    { "bucketId": "3p-5h", "remainingFraction": 0.2,
                      "resetTime": "2026-10-01T14:00:00Z" }
                ]
            }]
        });

        let parsed = parse(&root, fetched_at()).expect("parses");
        let window = &parsed.snapshot.windows[0];
        assert!((window.used_percent - 80.0).abs() < f64::EPSILON);
        assert_eq!(window.resets_at.as_deref(), Some("2026-10-01T14:00:00Z"));
    }

    #[test]
    fn buckets_classify_gemini_and_third_party_by_model_id() {
        let root = serde_json::json!({
            "buckets": [
                { "modelId": "gemini-3-pro", "remainingFraction": 0.8 },
                { "modelId": "gemini-3-pro-weekly", "bucketId": "weekly",
                  "remainingFraction": 0.3 },
                { "modelId": "claude-sonnet-4-5", "remainingFraction": 0.5 },
                { "modelId": "gpt-oss-120b", "bucketId": "weekly", "remainingFraction": 0.6 }
            ]
        });

        let parsed = parse_raw(&root, fetched_at());
        assert!(parsed.five_hour.is_some());
        assert!(parsed.weekly.is_some());
        assert!(parsed.gemini_window.is_some());
        assert!(parsed.gemini_weekly.is_some());
    }

    #[test]
    fn an_unused_bucket_without_a_reset_time_is_not_a_window() {
        let root = serde_json::json!({
            "buckets": [{ "modelId": "gemini-3-pro", "remainingFraction": 1.0 }]
        });

        assert!(matches!(
            parse(&root, fetched_at()),
            Err(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol
            })
        ));
    }

    #[test]
    fn a_model_dict_entry_without_a_quota_is_skipped() {
        let root = serde_json::json!({
            "models": { "gemini-3-pro": { "quotaInfo": {} } }
        });
        assert!(matches!(
            parse(&root, fetched_at()),
            Err(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol
            })
        ));
    }

    #[test]
    fn remaining_may_arrive_as_percentages_or_used_percentages() {
        let as_percent =
            remaining_fraction(&serde_json::json!({ "remaining": 40 })).expect("value");
        assert!((as_percent - 0.4).abs() < f64::EPSILON);

        let as_fraction =
            remaining_fraction(&serde_json::json!({ "remaining": 0.4 })).expect("value");
        assert!((as_fraction - 0.4).abs() < f64::EPSILON);

        let as_remaining_percent =
            remaining_fraction(&serde_json::json!({ "remainingPercent": 60 })).expect("value");
        assert!((as_remaining_percent - 0.4).abs() < f64::EPSILON);

        let as_used = remaining_fraction(&serde_json::json!({ "usedPercent": 60 })).expect("value");
        assert!((as_used - 0.4).abs() < f64::EPSILON);

        let as_string =
            remaining_fraction(&serde_json::json!({ "remainingFraction": "0.4" })).expect("value");
        assert!((as_string - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn booleans_are_never_read_as_numbers() {
        assert!(remaining_fraction(&serde_json::json!({ "remaining": true })).is_none());
    }

    #[test]
    fn model_dicts_use_reset_distance_to_pick_the_five_hour_rotation() {
        let root = serde_json::json!({
            "models": {
                "gemini-3-pro": { "quotaInfo": { "remainingFraction": 0.9,
                    "resetTime": "2026-10-01T13:00:00Z" } },
                "gemini-3-flash": { "quotaInfo": { "remainingFraction": 0.4,
                    "resetTime": "2026-10-01T13:30:00Z" } },
                "claude-sonnet-4-5": { "quotaInfo": { "remainingFraction": 0.7,
                    "resetTime": "2026-10-01T13:00:00Z" } },
                "gpt-weekly-ish": { "quotaInfo": { "remainingFraction": 0.2,
                    "resetTime": "2026-10-06T00:00:00Z" } }
            }
        });

        let parsed = parse_raw(&root, fetched_at());
        // Gemini 组取最紧张的 5h 窗口。
        let gemini = parsed.gemini_window.as_ref().expect("gemini window");
        assert!((gemini.used_percent - 60.0).abs() < f64::EPSILON);
        // 第三方 5h 直接取 2 小时内重置的那条；更远的按周窗口处理，不覆盖它。
        let third_party = parsed.five_hour.as_ref().expect("third party window");
        assert!((third_party.used_percent - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn the_gemini_window_covers_the_primary_slot_only_when_nothing_else_did() {
        let root = serde_json::json!({
            "models": {
                "gemini-3-pro": { "quotaInfo": { "remainingFraction": 0.4,
                    "resetTime": "2026-10-01T13:00:00Z" } }
            }
        });

        let parsed = parse_raw(&root, fetched_at());
        let primary = parsed.five_hour.as_ref().expect("primary window");
        assert!((primary.used_percent - 60.0).abs() < f64::EPSILON);
        assert!(parsed.gemini_window.is_some());
    }

    #[test]
    fn models_arrays_fall_back_to_the_first_and_second_slot_heuristic() {
        let root = serde_json::json!({
            "availableModels": [
                { "name": "models/gemini-3-pro", "remainingFraction": 0.7 },
                { "name": "models/gemini-3-pro-weekly", "remainingFraction": 0.2 },
                { "name": "models/claude-sonnet-4-5", "remainingFraction": 0.5 },
                { "name": "models/gpt-oss", "remainingFraction": 0.1 }
            ]
        });

        let parsed = parse_raw(&root, fetched_at());
        assert!(
            (parsed.gemini_window.as_ref().expect("g").used_percent - 30.0).abs() < f64::EPSILON
        );
        assert!(
            (parsed.gemini_weekly.as_ref().expect("gw").used_percent - 80.0).abs() < f64::EPSILON
        );
        assert!((parsed.five_hour.as_ref().expect("t").used_percent - 50.0).abs() < f64::EPSILON);
        assert!((parsed.weekly.as_ref().expect("w").used_percent - 90.0).abs() < f64::EPSILON);
    }

    #[test]
    fn quota_dicts_split_gemini_from_third_party() {
        let root = serde_json::json!({
            "quota": {
                "gemini-5h": { "remainingFraction": 0.75 },
                "gemini-week": { "remainingFraction": 0.5 },
                "claude-5h": { "remainingFraction": 0.25 },
                "claude-week": { "remainingFraction": 0.9 }
            }
        });

        let parsed = parse_raw(&root, fetched_at());
        assert!(parsed.gemini_window.is_some());
        assert!(parsed.gemini_weekly.is_some());
        assert!((parsed.five_hour.as_ref().expect("t").used_percent - 75.0).abs() < f64::EPSILON);
        assert!((parsed.weekly.as_ref().expect("w").used_percent - 10.0).abs() < f64::EPSILON);
    }

    #[test]
    fn a_non_grouped_payload_falls_back_to_primary_and_secondary_windows() {
        let root = serde_json::json!({
            "buckets": [
                { "modelId": "claude-sonnet-4-5", "remainingFraction": 0.3,
                  "resetTime": "2026-10-01T14:00:00Z" },
                { "modelId": "gpt-oss", "bucketId": "weekly", "remainingFraction": 0.8,
                  "resetTime": "2026-10-06T00:00:00Z" }
            ]
        });

        let parsed = parse(&root, fetched_at()).expect("parses");
        assert_eq!(
            ids(&parsed.snapshot.windows),
            ["antigravity-5h", "antigravity-weekly"]
        );
        assert_eq!(parsed.snapshot.windows[0].kind, QuotaWindowKind::FiveHour);
        assert_eq!(parsed.snapshot.windows[1].kind, QuotaWindowKind::Weekly);
        assert!(parsed.snapshot.windows[0].is_primary);
        assert!(!parsed.snapshot.windows[1].is_primary);
    }

    #[test]
    fn reset_times_accept_iso_seconds_and_milliseconds() {
        let iso = reset_time(Some(&serde_json::json!("2026-10-05T00:00:00.250Z")));
        assert_eq!(iso.as_deref(), Some("2026-10-05T00:00:00Z"));

        let seconds = reset_time(Some(&serde_json::json!(1_790_000_000_u64)));
        let millis = reset_time(Some(&serde_json::json!(1_790_000_000_000_u64)));
        assert_eq!(seconds, millis);
        assert!(reset_time(Some(&serde_json::json!(0))).is_none());
        assert!(reset_time(Some(&serde_json::json!(""))).is_none());
    }

    #[test]
    fn an_empty_payload_is_a_protocol_error() {
        assert!(matches!(
            parse(&serde_json::json!({}), fetched_at()),
            Err(ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol
            })
        ));
    }

    #[test]
    fn a_paid_tier_beats_the_free_current_tier() {
        let root = serde_json::json!({
            "currentTier": { "id": "free-tier" },
            "paidTier": { "name": "Google AI Pro" }
        });
        assert_eq!(plan_from(&root).as_deref(), Some("Google AI Pro"));

        let fallback = serde_json::json!({ "currentTier": { "id": "free-tier" } });
        assert_eq!(plan_from(&fallback).as_deref(), Some("free-tier"));

        let allowed = serde_json::json!({ "allowedTiers": [{ "id": "legacy-tier" }] });
        assert_eq!(plan_from(&allowed).as_deref(), Some("legacy-tier"));

        assert!(plan_from(&serde_json::json!({})).is_none());
    }
}
