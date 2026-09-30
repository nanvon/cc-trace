//! Cursor 远端用量计量（Dashboard `get-filtered-usage-events`）。
//!
//! Cursor 没有本机会话日志：用量只存在于服务端。这一支把 Dashboard 的事件流拉下来，
//! 按「自然日 × 模型」聚成**按天粒度**的事实，写进 `usage_entries`
//! （`granularity = 'day'`、`request_count` 记事件条数）。协议与字段映射对齐 cc-bar
//! v1.1.1 的 `CursorUsageFetcher`：
//!
//! - 事件没有稳定 id，因此分页只有「服务端总数的前后一致 + 短页/空页确认结束 +
//!   边界重叠可按总数精确消解」三条同时成立才发布；
//! - 一页装不下就按自然日中点二分递归，单日仍然装不下才算失败；
//! - 拉取范围按自然日**原子替换**：某天拉全了就整天替换，不做增量叠加，
//!   这样重复拉取不会把同一天算两遍；
//! - `chargedCents` 缺失或非法只标记「费用不完整」，token 与请求数照常入账。
//!
//! 拉取范围的规划（拉哪些天）也在这里：以「最近两天 + 本周 + 计费周期起点」为下限，
//! 只补覆盖表里没有的日子，并按自然月切块（Dashboard 对单次范围有页数上限）。

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Days, Local, NaiveDate, TimeZone, Utc};
use serde_json::Value;

use crate::providers::credentials::Secret;

/// Dashboard 事件流端点。
pub const ENDPOINT: &str = "https://cursor.com/api/dashboard/get-filtered-usage-events";
/// 单页事件数。服务端上限约 1000，超过会被截断而不报错。
pub const PAGE_SIZE: u32 = 1_000;
/// 单次范围的最大页数。到顶就二分范围，避免一次拉取把内存拉爆。
pub const MAX_PAGES: u32 = 200;

/// 一次拉取的失败原因。调用方据此决定退避与文案，不猜测。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorFetchError {
    /// 范围为空或方向反了。
    InvalidRange,
    /// 网络或超时。
    Transport,
    /// HTTP 状态码非 2xx。429 由调用方映射成退避。
    Http(u16),
    /// 载荷结构不符合预期。
    InvalidPage,
    /// 分页之间服务端总数变了：整份结果作废，等下次。
    PaginationInconsistent,
    /// 页数到顶：调用方二分范围后重试。
    PageLimitReached,
    /// 单日范围内事件仍然装不下。
    SingleDayTooDense,
    /// 数值累加溢出（服务端返回异常大的数）。
    NumericOverflow,
}

impl CursorFetchError {
    /// 是否值得按「稍后重试」处理。
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Transport | Self::Http(_) | Self::PaginationInconsistent
        )
    }

    /// 是否被限流。调用方按 10 分钟退避处理。
    pub fn is_rate_limited(&self) -> bool {
        matches!(self, Self::Http(429))
    }
}

/// Dashboard 的一条用量事件。
#[derive(Debug, Clone, PartialEq)]
pub struct CursorUsageEvent {
    pub timestamp_ms: i64,
    pub model: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// 服务端计费金额（美分，十进制字符串）。缺失或非法时为 `None`，
    /// 此时该日桶标记「费用不完整」，token 与请求数照常入账。
    pub charged_cents: Option<i64>,
}

/// 一页响应。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CursorUsagePage {
    /// 服务端给出的范围内事件总数。缺失与非法都算结构问题。
    pub total: Option<i64>,
    pub events: Vec<CursorUsageEvent>,
}

/// 按天聚合后的桶。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorUsageBucket {
    /// 本地自然日 `YYYY-MM-DD`。
    pub day_local: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// 服务端计费金额，纳秒单位（美分 × 10^7）。
    pub charged_nanos: i64,
    pub request_count: i64,
    /// 这个桶里至少有一条事件没给出有效计费金额。
    pub cost_incomplete: bool,
}

/// 解析一页响应。
pub fn parse_page(root: &Value) -> Result<CursorUsagePage, CursorFetchError> {
    let events = root
        .get("usageEventsDisplay")
        .and_then(Value::as_array)
        .ok_or(CursorFetchError::InvalidPage)?;

    let total = match root.get("totalUsageEventsCount") {
        None | Some(Value::Null) => None,
        // 布尔与字符串都不算合法总数：前者会把 true 读成 1，后者是另一种协议漂移。
        Some(value) => Some(
            value
                .as_i64()
                .filter(|value| *value >= 0)
                .ok_or(CursorFetchError::InvalidPage)?,
        ),
    };

    let mut parsed = Vec::with_capacity(events.len());
    for event in events {
        parsed.push(parse_event(event)?);
    }
    Ok(CursorUsagePage {
        total,
        events: parsed,
    })
}

fn parse_event(root: &Value) -> Result<CursorUsageEvent, CursorFetchError> {
    let timestamp_ms = root
        .get("timestamp")
        .and_then(as_integer)
        .filter(|value| *value > 0)
        .ok_or(CursorFetchError::InvalidPage)?;
    let usage = root.get("tokenUsage");
    let token = |name: &str| usage.and_then(|usage| usage.get(name)).and_then(as_integer);

    Ok(CursorUsageEvent {
        timestamp_ms,
        model: root
            .get("model")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned),
        input_tokens: token("inputTokens").unwrap_or(0).max(0),
        output_tokens: token("outputTokens").unwrap_or(0).max(0),
        cache_read_tokens: token("cacheReadTokens").unwrap_or(0).max(0),
        cache_write_tokens: token("cacheWriteTokens").unwrap_or(0).max(0),
        charged_cents: root.get("chargedCents").and_then(as_cents),
    })
}

/// 整数字段：数字或数字字符串都接受，布尔一律拒绝。
fn as_integer(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.trim().parse::<i64>().ok(),
        _ => None,
    }
}

/// 把美分金额解析成整数纳秒。
///
/// 服务端把它当十进制字符串给出（`"12.5"`）。走字符串解析而不是 f64：
/// 金额是账，浮点舍入在这里没有理由。
fn as_cents(value: &Value) -> Option<i64> {
    let text = match value {
        Value::String(text) => text.trim().to_owned(),
        Value::Number(number) => number.to_string(),
        _ => return None,
    };
    decimals_to_nanos(&text)
}

/// 十进制字符串 → 纳秒（×10^7）。负数视为非法金额（退款不属于用量计费）。
pub fn decimals_to_nanos(text: &str) -> Option<i64> {
    let text = text.trim();
    if text.is_empty() || text.starts_with('-') {
        return None;
    }
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    if whole.is_empty() && fraction.is_empty() {
        return None;
    }
    if !whole.chars().all(|character| character.is_ascii_digit())
        || !fraction.chars().all(|character| character.is_ascii_digit())
    {
        return None;
    }

    let whole: i64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    // 纳秒精度是 7 位小数；多出来的位数截断而不是四舍五入，账不虚增。
    let mut fraction_digits = fraction.chars().take(7).collect::<String>();
    while fraction_digits.len() < 7 {
        fraction_digits.push('0');
    }
    let fraction: i64 = fraction_digits.parse().ok()?;

    whole.checked_mul(10_000_000)?.checked_add(fraction)
}

/// 按「本地自然日 × 模型」聚合成桶。
pub fn make_buckets(
    events: &[CursorUsageEvent],
) -> Result<Vec<CursorUsageBucket>, CursorFetchError> {
    let mut buckets: std::collections::BTreeMap<(String, String), CursorUsageBucket> =
        std::collections::BTreeMap::new();

    for event in events {
        let timestamp = DateTime::from_timestamp_millis(event.timestamp_ms)
            .ok_or(CursorFetchError::InvalidPage)?;
        let day_local = timestamp
            .with_timezone(&Local)
            .date_naive()
            .format("%Y-%m-%d")
            .to_string();
        let model = event.model.clone().unwrap_or_else(|| "unknown".to_owned());
        let key = (day_local.clone(), model.clone());
        let bucket = buckets.entry(key).or_insert_with(|| CursorUsageBucket {
            day_local,
            model,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            charged_nanos: 0,
            request_count: 0,
            cost_incomplete: false,
        });

        bucket.input_tokens = bucket
            .input_tokens
            .checked_add(event.input_tokens)
            .ok_or(CursorFetchError::NumericOverflow)?;
        bucket.output_tokens = bucket
            .output_tokens
            .checked_add(event.output_tokens)
            .ok_or(CursorFetchError::NumericOverflow)?;
        bucket.cache_read_tokens = bucket
            .cache_read_tokens
            .checked_add(event.cache_read_tokens)
            .ok_or(CursorFetchError::NumericOverflow)?;
        bucket.cache_write_tokens = bucket
            .cache_write_tokens
            .checked_add(event.cache_write_tokens)
            .ok_or(CursorFetchError::NumericOverflow)?;
        bucket.request_count = bucket
            .request_count
            .checked_add(1)
            .ok_or(CursorFetchError::NumericOverflow)?;
        match event.charged_cents {
            Some(nanos) => {
                bucket.charged_nanos = bucket
                    .charged_nanos
                    .checked_add(nanos)
                    .ok_or(CursorFetchError::NumericOverflow)?;
            }
            None => bucket.cost_incomplete = true,
        }
    }

    Ok(buckets.into_values().collect())
}

/// 把多个页的事件合成一份完整结果。
///
/// 服务端按页返回时会在页边界重复少量事件（游标语义），因此原始条数常常大于
/// `expectedTotal`。这里按「相邻页的后缀与前缀重叠」逐个消解，消解不掉就判不一致——
/// 宁可不发布，也不把重复事件算进用量。
pub fn reconcile_pages(
    pages: &[Vec<CursorUsageEvent>],
    expected_total: i64,
) -> Result<Vec<CursorUsageEvent>, CursorFetchError> {
    let raw_count: i64 = pages.iter().map(|page| page.len() as i64).sum();
    if raw_count < expected_total {
        return Err(CursorFetchError::PaginationInconsistent);
    }
    if raw_count == expected_total {
        return Ok(pages.iter().flatten().cloned().collect());
    }

    let mut remaining = raw_count - expected_total;
    let mut reconciled: Vec<CursorUsageEvent> = pages.first().cloned().unwrap_or_default();
    for index in 1..pages.len() {
        let overlap = boundary_overlap(&pages[index - 1], &pages[index]) as i64;
        let removal = overlap.min(remaining);
        reconciled.extend(pages[index].iter().skip(removal as usize).cloned());
        remaining -= removal;
    }

    if remaining != 0 || reconciled.len() as i64 != expected_total {
        return Err(CursorFetchError::PaginationInconsistent);
    }
    Ok(reconciled)
}

/// 前页后缀与后页前缀的最长重叠长度。
fn boundary_overlap(previous: &[CursorUsageEvent], current: &[CursorUsageEvent]) -> usize {
    let limit = previous.len().min(current.len());
    for count in (1..=limit).rev() {
        if previous[previous.len() - count..] == current[..count] {
            return count;
        }
    }
    0
}

/// 拉取一段范围的全部事件（分页 + 超页数时二分）。
///
/// `from_ms` 含、`to_ms` 不含。
pub async fn fetch_range(
    cookie: &Secret,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<CursorUsageEvent>, CursorFetchError> {
    if from_ms >= to_ms {
        return Err(CursorFetchError::InvalidRange);
    }
    fetch_window(cookie, from_ms, to_ms, 0).await
}

fn fetch_window<'a>(
    cookie: &'a Secret,
    from_ms: i64,
    to_ms: i64,
    depth: usize,
) -> std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<Vec<CursorUsageEvent>, CursorFetchError>>
            + Send
            + 'a,
    >,
> {
    Box::pin(async move {
        match fetch_pages(cookie, from_ms, to_ms).await {
            Err(CursorFetchError::PageLimitReached) => {
                // 到页数上限就把范围二分；单日仍然装不下才算失败。
                let day_ms = 86_400_000_i64;
                let span = to_ms - from_ms;
                if span <= day_ms || depth > 40 {
                    return Err(CursorFetchError::SingleDayTooDense);
                }
                let midpoint = from_ms + span / 2;
                if midpoint <= from_ms || midpoint >= to_ms {
                    return Err(CursorFetchError::SingleDayTooDense);
                }
                let left = fetch_window(cookie, from_ms, midpoint, depth + 1).await?;
                let right = fetch_window(cookie, midpoint, to_ms, depth + 1).await?;
                let mut merged = left;
                merged.extend(right);
                Ok(merged)
            }
            other => other,
        }
    })
}

async fn fetch_pages(
    cookie: &Secret,
    from_ms: i64,
    to_ms: i64,
) -> Result<Vec<CursorUsageEvent>, CursorFetchError> {
    let mut pages: Vec<Vec<CursorUsageEvent>> = Vec::new();
    let mut expected_total: Option<i64> = None;
    let mut completed = false;

    for page_number in 1..=MAX_PAGES {
        let page = fetch_page(cookie, page_number, from_ms, to_ms).await?;
        let total = page.total.ok_or(CursorFetchError::InvalidPage)?;
        match expected_total {
            Some(expected) if expected != total => {
                return Err(CursorFetchError::PaginationInconsistent);
            }
            _ => expected_total = Some(total),
        }

        if page.events.is_empty() {
            completed = true;
            break;
        }
        let short_page = page.events.len() < PAGE_SIZE as usize;
        pages.push(page.events);
        if short_page {
            completed = true;
            break;
        }
    }

    if !completed {
        return Err(CursorFetchError::PageLimitReached);
    }
    let expected_total = expected_total.ok_or(CursorFetchError::InvalidPage)?;
    reconcile_pages(&pages, expected_total)
}

async fn fetch_page(
    cookie: &Secret,
    page: u32,
    from_ms: i64,
    to_ms: i64,
) -> Result<CursorUsagePage, CursorFetchError> {
    let body = serde_json::json!({
        "page": page,
        "pageSize": PAGE_SIZE,
        // 服务端按毫秒字符串解析这两个字段。
        "startDate": from_ms.to_string(),
        "endDate": to_ms.to_string(),
    });

    let response = crate::providers::http::client()
        .post(ENDPOINT)
        .timeout(std::time::Duration::from_secs(30))
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .header(reqwest::header::ACCEPT, "application/json")
        .header(reqwest::header::ORIGIN, "https://cursor.com")
        .header(reqwest::header::COOKIE, cookie.expose())
        .body(body.to_string())
        .send()
        .await
        .map_err(|_| CursorFetchError::Transport)?;

    let status = response.status();
    if !status.is_success() {
        return Err(CursorFetchError::Http(status.as_u16()));
    }
    let text = response
        .text()
        .await
        .map_err(|_| CursorFetchError::Transport)?;
    let root: Value = serde_json::from_str(&text).map_err(|_| CursorFetchError::InvalidPage)?;
    parse_page(&root)
}

/// 覆盖表：已经拉全的自然日。
pub type CursorCoverage = BTreeSet<NaiveDate>;

/// 规划要拉取的 [from_ms, to_ms) 区间。
///
/// 规则（与 cc-bar 的刷新策略同构）：
///
/// - 刷新窗 = 「最近两天」∪「本周（周一起算）」；计费周期已知时再向回到周期起点，
///   这样换了周期或换了账号之后不会留下空洞；
/// - 只补覆盖表里没有的日子；
/// - 连续的日子合并，再按自然月切块：Dashboard 对单次范围有页数上限，
///   按月切是最稳的边界（也是 cc-bar 的做法）。
pub fn plan_fetch_ranges(
    today: NaiveDate,
    coverage: &CursorCoverage,
    billing_cycle_start: Option<NaiveDate>,
) -> Vec<(i64, i64)> {
    let recent_start = today - Days::new(2);
    let week_start = start_of_iso_week(today);
    let mut earliest = recent_start.min(week_start);
    if let Some(billing_start) = billing_cycle_start
        && billing_start < earliest
    {
        earliest = billing_start;
    }

    let mut ranges: Vec<(i64, i64)> = Vec::new();
    let mut cursor = earliest;
    while cursor <= today {
        if coverage.contains(&cursor) {
            cursor = cursor + Days::new(1);
            continue;
        }
        let start = cursor;
        let mut end = cursor;
        let mut probe = cursor + Days::new(1);
        while probe <= today && !coverage.contains(&probe) {
            // 跨自然月就断开：单次请求的范围不跨月，页数上限更容易满足。
            if probe.month() != end.month() {
                break;
            }
            end = probe;
            probe = probe + Days::new(1);
        }

        if let (Some(from), Some(to)) = (day_start_ms(start), day_start_ms(end + Days::new(1))) {
            ranges.push((from, to));
        }
        cursor = end + Days::new(1);
    }

    ranges
}

/// 本周起点（周一）。ISO 周与 cc-bar 的周一起算保持一致。
fn start_of_iso_week(day: NaiveDate) -> NaiveDate {
    let weekday = day.weekday().num_days_from_monday();
    day - Days::new(u64::from(weekday))
}

/// 本地自然日零点对应的 UTC 毫秒。夏令时下「零点不存在」的日子取当天的第一个有效时刻。
fn day_start_ms(day: NaiveDate) -> Option<i64> {
    let naive = day.and_hms_opt(0, 0, 0)?;
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(time) => Some(time.timestamp_millis()),
        chrono::LocalResult::Ambiguous(first, _second) => Some(first.timestamp_millis()),
        // 零点被夏令时跳过（如巴西旧规则）：取当天 01:00。
        chrono::LocalResult::None => Local
            .from_local_datetime(&day.and_hms_opt(1, 0, 0)?)
            .earliest()
            .map(|time| time.timestamp_millis()),
    }
}

/// 把 UTC 毫秒换成它落在的本地自然日。
pub fn local_day_of(timestamp_ms: i64) -> Option<NaiveDate> {
    let timestamp = Utc.timestamp_millis_opt(timestamp_ms).single()?;
    Some(timestamp.with_timezone(&Local).date_naive())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn day(value: &str) -> NaiveDate {
        NaiveDate::parse_from_str(value, "%Y-%m-%d").expect("valid date")
    }

    #[test]
    fn a_page_without_the_events_array_is_invalid() {
        assert_eq!(
            parse_page(&serde_json::json!({ "totalUsageEventsCount": 0 })),
            Err(CursorFetchError::InvalidPage)
        );
    }

    #[test]
    fn a_boolean_total_is_not_a_number() {
        assert_eq!(
            parse_page(&serde_json::json!({
                "usageEventsDisplay": [],
                "totalUsageEventsCount": true
            })),
            Err(CursorFetchError::InvalidPage)
        );
        assert_eq!(
            parse_page(&serde_json::json!({
                "usageEventsDisplay": [],
                "totalUsageEventsCount": 2
            }))
            .expect("parses")
            .total,
            Some(2)
        );
    }

    #[test]
    fn events_are_parsed_with_their_token_split_and_charge() {
        let page = parse_page(&serde_json::json!({
            "usageEventsDisplay": [{
                "timestamp": 1_790_000_000_000_i64,
                "model": "claude-4.5-sonnet",
                "tokenUsage": {
                    "inputTokens": 100,
                    "outputTokens": 40,
                    "cacheReadTokens": 10,
                    "cacheWriteTokens": 5
                },
                "chargedCents": "12.5"
            }],
            "totalUsageEventsCount": 1
        }))
        .expect("parses");

        let event = &page.events[0];
        assert_eq!(event.input_tokens, 100);
        assert_eq!(event.cache_write_tokens, 5);
        assert_eq!(
            event.charged_cents,
            Some(125_000_000),
            "12.5 美分 = 0.125 美元"
        );
    }

    #[test]
    fn a_missing_timestamp_invalidates_the_page() {
        assert_eq!(
            parse_page(&serde_json::json!({
                "usageEventsDisplay": [{ "model": "m" }],
                "totalUsageEventsCount": 1
            })),
            Err(CursorFetchError::InvalidPage)
        );
    }

    #[test]
    fn decimal_amounts_convert_without_floating_point() {
        assert_eq!(decimals_to_nanos("0"), Some(0));
        assert_eq!(decimals_to_nanos("1"), Some(10_000_000));
        assert_eq!(decimals_to_nanos("12.5"), Some(125_000_000));
        assert_eq!(decimals_to_nanos("0.0000001"), Some(1));
        // 多出的位数截断，不四舍五入：账不虚增。
        assert_eq!(decimals_to_nanos("0.00000019"), Some(1));
        assert_eq!(decimals_to_nanos(""), None);
        assert_eq!(decimals_to_nanos("-1"), None);
        assert_eq!(decimals_to_nanos("abc"), None);
        assert_eq!(decimals_to_nanos("1.2.3"), None);
    }

    #[test]
    fn buckets_group_by_day_and_model_and_flag_incomplete_costs() {
        let events = vec![
            CursorUsageEvent {
                timestamp_ms: 1_790_000_000_000,
                model: Some("a".to_owned()),
                input_tokens: 10,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                charged_cents: Some(10_000_000),
            },
            CursorUsageEvent {
                timestamp_ms: 1_790_000_060_000,
                model: Some("a".to_owned()),
                input_tokens: 5,
                output_tokens: 2,
                cache_read_tokens: 1,
                cache_write_tokens: 1,
                charged_cents: None,
            },
            CursorUsageEvent {
                timestamp_ms: 1_790_000_060_000,
                model: None,
                input_tokens: 1,
                output_tokens: 1,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                charged_cents: Some(0),
            },
        ];

        let buckets = make_buckets(&events).expect("buckets");
        let model_a = buckets
            .iter()
            .find(|bucket| bucket.model == "a")
            .expect("bucket a");
        assert_eq!(model_a.request_count, 2);
        assert_eq!(model_a.input_tokens, 15);
        assert_eq!(model_a.cache_read_tokens, 1);
        assert_eq!(model_a.charged_nanos, 10_000_000);
        assert!(model_a.cost_incomplete, "缺一次计费金额就要标出来");

        let unknown = buckets
            .iter()
            .find(|bucket| bucket.model == "unknown")
            .expect("unknown bucket");
        assert!(!unknown.cost_incomplete, "0 是合法金额，不是缺失");
    }

    #[test]
    fn overlapping_pages_are_reconciled_by_count() {
        let event = |timestamp_ms: i64| CursorUsageEvent {
            timestamp_ms,
            model: Some("m".to_owned()),
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            charged_cents: Some(0),
        };
        let pages = vec![vec![event(1), event(2), event(3)], vec![event(3), event(4)]];

        let reconciled = reconcile_pages(&pages, 4).expect("reconciles");
        assert_eq!(reconciled.len(), 4);
        assert_eq!(reconciled[3].timestamp_ms, 4);
    }

    #[test]
    fn a_short_page_set_is_inconsistent() {
        let event = |timestamp_ms: i64| CursorUsageEvent {
            timestamp_ms,
            model: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            charged_cents: None,
        };

        assert_eq!(
            reconcile_pages(&[vec![event(1)]], 3),
            Err(CursorFetchError::PaginationInconsistent)
        );
        // 条数多于总数却没有重叠可消解：拒绝，而不是把重复事件算进用量。
        assert_eq!(
            reconcile_pages(&[vec![event(1), event(2)], vec![event(3)]], 2),
            Err(CursorFetchError::PaginationInconsistent)
        );
        // 条数正好等于总数时不需要消解。
        assert_eq!(
            reconcile_pages(&[vec![event(1), event(2)], vec![event(3)]], 3)
                .expect("reconciles")
                .len(),
            3
        );
    }

    #[test]
    fn ranges_cover_the_recent_window_and_the_current_week() {
        // 2026-10-20 是周二：本周起点 10-19，最近两天是 10-18 → 最早 10-18，都在十月内。
        let ranges = plan_fetch_ranges(day("2026-10-20"), &CursorCoverage::new(), None);

        assert_eq!(ranges.len(), 1, "连着的日子合成一段（同月）");
        let (from, to) = ranges[0];
        assert_eq!(local_day_of(from), Some(day("2026-10-18")));
        assert_eq!(local_day_of(to - 1), Some(day("2026-10-20")));
    }

    #[test]
    fn a_window_crossing_a_month_boundary_is_split() {
        // 2026-10-02 是周五：本周起点 09-28、最近两天 09-30 → 最早 09-28，跨月。
        let ranges = plan_fetch_ranges(day("2026-10-02"), &CursorCoverage::new(), None);

        assert_eq!(ranges.len(), 2, "跨月要切块");
        assert_eq!(local_day_of(ranges[0].0), Some(day("2026-09-28")));
        assert_eq!(local_day_of(ranges[0].1 - 1), Some(day("2026-09-30")));
        assert_eq!(local_day_of(ranges[1].0), Some(day("2026-10-01")));
        assert_eq!(local_day_of(ranges[1].1 - 1), Some(day("2026-10-02")));
    }

    #[test]
    fn covered_days_are_skipped_and_the_rest_split_by_month() {
        let mut coverage = CursorCoverage::new();
        for date in ["2026-09-28", "2026-09-29", "2026-09-30", "2026-10-01"] {
            coverage.insert(day(date));
        }

        let ranges = plan_fetch_ranges(day("2026-10-02"), &coverage, None);
        // 只有 10-02 缺，而且它是周五，最近两天是 09-30（已覆盖）。
        assert_eq!(ranges.len(), 1);
        assert_eq!(local_day_of(ranges[0].0), Some(day("2026-10-02")));

        // 整段覆盖 → 不需要拉。
        let mut full = CursorCoverage::new();
        for offset in 0..40 {
            full.insert(day("2026-09-01") + Days::new(offset));
        }
        assert!(plan_fetch_ranges(day("2026-10-01"), &full, None).is_empty());
    }

    #[test]
    fn a_billing_cycle_start_pulls_the_whole_cycle_once() {
        // 周期从九月开始：整段都要补，并按自然月切块。
        let ranges = plan_fetch_ranges(
            day("2026-10-20"),
            &CursorCoverage::new(),
            Some(day("2026-09-25")),
        );

        assert_eq!(ranges.len(), 2, "跨月要切块");
        assert_eq!(local_day_of(ranges[0].0), Some(day("2026-09-25")));
        assert_eq!(local_day_of(ranges[0].1 - 1), Some(day("2026-09-30")));
        assert_eq!(local_day_of(ranges[1].0), Some(day("2026-10-01")));
        assert_eq!(local_day_of(ranges[1].1 - 1), Some(day("2026-10-20")));
    }

    #[test]
    fn a_billing_cycle_inside_one_month_is_a_single_range() {
        let ranges = plan_fetch_ranges(
            day("2026-10-20"),
            &CursorCoverage::new(),
            Some(day("2026-10-15")),
        );

        assert_eq!(ranges.len(), 1);
        assert_eq!(local_day_of(ranges[0].0), Some(day("2026-10-15")));
        assert_eq!(local_day_of(ranges[0].1 - 1), Some(day("2026-10-20")));
    }

    #[test]
    fn covered_days_split_a_range_at_the_month_boundary() {
        let mut coverage = CursorCoverage::new();
        coverage.insert(day("2026-09-30"));
        let ranges = plan_fetch_ranges(day("2026-10-01"), &coverage, Some(day("2026-09-28")));

        // 09-28、09-29 与 10-01 缺，09-30 已覆盖 → 09 月一段、10 月一段。
        assert_eq!(ranges.len(), 2);
        assert_eq!(local_day_of(ranges[0].0), Some(day("2026-09-28")));
        assert_eq!(local_day_of(ranges[0].1 - 1), Some(day("2026-09-29")));
        assert_eq!(local_day_of(ranges[1].0), Some(day("2026-10-01")));
    }

    #[test]
    fn errors_are_classified_for_retry_and_backoff() {
        assert!(CursorFetchError::Http(429).is_rate_limited());
        assert!(!CursorFetchError::Http(500).is_rate_limited());
        assert!(CursorFetchError::Http(500).is_retryable());
        assert!(CursorFetchError::Transport.is_retryable());
        assert!(!CursorFetchError::InvalidPage.is_retryable());
        assert!(!CursorFetchError::InvalidRange.is_retryable());
    }
}
