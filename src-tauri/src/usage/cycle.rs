//! 额度周期：从已记录的额度事件推导周期、额度片段与用满预估。
//!
//! 推导而不是维护状态：`quota_events` 已经是「每次采样一条」的事实表，
//! 周期与片段是它的纯函数派生结果（与 [ADR-0018] 的划分一致）。这样做的好处是
//! 同一份事件在两次查询之间一定给出同一组周期，不会出现「状态机漏记一次采样、
//! 周期永久对不上」的情况。
//!
//! 规则逐条对齐 cc-bar v1.1.1 的 `QuotaCycleStore`／`CycleUsage`：
//!
//! - 周期边界来自采样里的 `resetsAt`，不是采样时刻；没有 `resetsAt` 的采样只能
//!   更新当前周期的读数，不会新开周期（那样得到的边界是猜的）；
//! - `resetsAt` 的秒级抖动不建新周期（服务端实测 1～3 秒漂移），
//!   漂移超过半小时才认为是真实滚动；
//! - 同一周期内已用比例明显回落 = **额外重置**，开一个新额度片段而不是新周期；
//! - 周期时长明显超过窗口长度（×1.15）说明起点被漂移污染，按
//!   「重置时刻 − 窗口长度」回写起点；
//! - 用满预估只用**当前片段**的本地用量除以官方已用比例，再补上此前的实际用量；
//!   可信度按官方已用比例分档，比例太低时不给出预估（0～10% 外推出来的数没有意义）。
//!
//! [ADR-0018]: ../../../../docs/决策/ADR-0018-用量数据用SQLite与JSON分域.md

use chrono::{DateTime, Duration, Utc};

use crate::contracts::{ProviderId, QuotaWindowKind};

/// 服务端 `resetsAt` 的秒级抖动容差：同一周期的相邻采样落在这个范围内算同一次重置。
const RESET_JITTER_TOLERANCE_SECS: i64 = 60;
/// 活跃周期的 `resetsAt` 分钟级漂移容差。超过它才算真实滚动。
const ACTIVE_RESET_DRIFT_TOLERANCE_SECS: i64 = 30 * 60;
/// 「额外重置」的已用比例最小回落。
const EXTRA_RESET_MINIMUM_DROP: f64 = 10.0;
/// 接近零用量时的最小回落：低用量周期的小幅回落也是额外重置。
const NEAR_ZERO_USED_PERCENT: f64 = 2.0;
const NEAR_ZERO_MINIMUM_DROP: f64 = 3.0;
/// 用满预估的可信度分档上限（官方已用比例）。
const CONFIDENCE_RELIABLE_PERCENT: f64 = 80.0;
const CONFIDENCE_REFERENCE_PERCENT: f64 = 30.0;
const CONFIDENCE_ROUGH_PERCENT: f64 = 10.0;

/// 一次额度采样。对应 `quota_events` 的一行。
#[derive(Debug, Clone, PartialEq)]
pub struct CycleSample {
    pub observed_at: DateTime<Utc>,
    /// 该窗口当时的剩余比例（0～100）。
    pub remaining_percent: f64,
    /// 该窗口当时声明的重置时刻。
    pub resets_at: Option<DateTime<Utc>>,
    /// 窗口长度（秒）。缺失时按窗口类型取常量。
    pub window_seconds: Option<u64>,
}

/// 片段起点为什么开：周期开始，还是周期内的一次额外重置。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentStartReason {
    Initial,
    ExtraReset,
}

impl SegmentStartReason {
    pub fn key(self) -> &'static str {
        match self {
            Self::Initial => "initial",
            Self::ExtraReset => "extraReset",
        }
    }
}

/// 周期边界的可信度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundaryQuality {
    /// 我们亲眼看到了重置（前一个周期的结束由真实滚动推出）。
    Observed,
    /// 起点是按「重置时刻 − 窗口长度」推出来的。
    Inferred,
}

impl BoundaryQuality {
    pub fn key(self) -> &'static str {
        match self {
            Self::Observed => "observed",
            Self::Inferred => "inferred",
        }
    }
}

/// 一个额度片段：周期内的一次额度分配。
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedSegment {
    pub start_at: DateTime<Utc>,
    /// 活动片段为 `None`。
    pub end_at: Option<DateTime<Utc>>,
    pub baseline_used_percent: f64,
    pub latest_used_percent: f64,
    pub maximum_used_percent: f64,
    pub first_sample_at: DateTime<Utc>,
    pub last_sample_at: DateTime<Utc>,
    pub start_reason: SegmentStartReason,
}

impl DerivedSegment {
    /// 官方已用比例的观察值：`latest - baseline`，不低于 0。
    pub fn observed_used_percent(&self) -> f64 {
        (self.latest_used_percent - self.baseline_used_percent).max(0.0)
    }
}

/// 一个推导出的周期。
#[derive(Debug, Clone, PartialEq)]
pub struct DerivedCycle {
    pub provider: ProviderId,
    pub identity_key: String,
    pub window_kind: QuotaWindowKind,
    pub window_id: String,
    pub window_seconds: Option<u64>,
    pub start_at: DateTime<Utc>,
    pub end_at: DateTime<Utc>,
    pub scheduled_end_at: DateTime<Utc>,
    pub first_sample_at: DateTime<Utc>,
    pub last_sample_at: DateTime<Utc>,
    pub latest_used_percent: f64,
    pub boundary_quality: BoundaryQuality,
    pub segments: Vec<DerivedSegment>,
    /// 推导期间的采样缓存。片段切分要用到逐条采样，切完即清空；
    /// 私有字段也保证外部无法凭空构造一个周期。
    samples: Vec<SampleReading>,
}

impl DerivedCycle {
    /// 周期标识：`provider|identity|window|重置时刻`。纯函数推导下它必须稳定，
    /// 否则同一周期在两次查询里会得到两个 id，前端的选中态与对照都会失效。
    pub fn id(&self) -> String {
        format!(
            "{}|{}|{}|{}",
            self.provider.key(),
            self.identity_key,
            self.window_id,
            self.scheduled_end_at.timestamp()
        )
    }

    /// 当前（最后一个）额度片段。
    pub fn active_segment(&self) -> Option<&DerivedSegment> {
        self.segments.last()
    }

    pub fn extra_reset_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| segment.start_reason == SegmentStartReason::ExtraReset)
            .count()
    }

    /// 周期内官方口径的「见过的最多用度」：初始片段与各额外片段的最大值之和。
    pub fn peak_used_percent(&self) -> f64 {
        let initial = self
            .segments
            .iter()
            .find(|segment| segment.start_reason == SegmentStartReason::Initial)
            .map(|segment| segment.maximum_used_percent)
            .unwrap_or(0.0);
        let extra: f64 = self
            .segments
            .iter()
            .filter(|segment| segment.start_reason == SegmentStartReason::ExtraReset)
            .map(|segment| segment.maximum_used_percent)
            .sum();
        (initial + extra).min(100.0)
    }
}

/// 用满预估的可信度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForecastConfidence {
    /// 官方已用比例 < 10%：外推没有意义。
    Early,
    Rough,
    Reference,
    Reliable,
}

impl ForecastConfidence {
    pub fn key(self) -> &'static str {
        match self {
            Self::Early => "early",
            Self::Rough => "rough",
            Self::Reference => "reference",
            Self::Reliable => "reliable",
        }
    }

    fn from_observed(observed_percent: f64) -> Self {
        if observed_percent >= CONFIDENCE_RELIABLE_PERCENT {
            Self::Reliable
        } else if observed_percent >= CONFIDENCE_REFERENCE_PERCENT {
            Self::Reference
        } else if observed_percent >= CONFIDENCE_ROUGH_PERCENT {
            Self::Rough
        } else {
            Self::Early
        }
    }
}

/// 片段里的本地用量（由调用方从 `usage_entries` 汇总后填入）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SegmentUsage {
    pub tokens: i64,
    pub cost_nanos: i64,
    pub request_count: i64,
}

/// 用满预估。`None` 表示不给预估（官方已用比例太低，或本地没有用量可外推）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Forecast {
    pub confidence: ForecastConfidence,
    /// 官方已用比例（当前片段观察值，四舍五入到整数）。
    pub observed_percent: i64,
    /// 按当前片段外推的整段额度总量。
    pub estimated_full_tokens: Option<i64>,
    pub estimated_full_cost_nanos: Option<i64>,
    /// 补齐此前片段的实际用量后的整周期预估值。
    pub projected_cycle_tokens: Option<i64>,
    pub projected_cycle_cost_nanos: Option<i64>,
}

/// 计算用满预估。
///
/// 与 cc-bar 一致的两条硬规则：官方已用比例为 0 时不给预估（除零没有意义），
/// 当前片段的本地用量为 0 时也不给（没有分子可外推）。
pub fn forecast(
    cycle: &DerivedCycle,
    current_allowance: SegmentUsage,
    cycle_totals: SegmentUsage,
) -> Option<Forecast> {
    let observed = cycle.active_segment()?.observed_used_percent();
    if observed <= 0.0 {
        return None;
    }
    if current_allowance.tokens <= 0 && current_allowance.cost_nanos <= 0 {
        return None;
    }

    let confidence = ForecastConfidence::from_observed(observed);
    let observed_percent = observed.round() as i64;

    // 只在已用比例足够高时才给具体数字：低比例外推的量级误差太大，
    // 显式返回 None 让界面显示「样本不足」，好过给一个看起来很确定的错数。
    let (estimated_tokens, projected_tokens) = if confidence == ForecastConfidence::Early {
        (None, None)
    } else {
        let estimated = scale(current_allowance.tokens, observed);
        let prior = (cycle_totals.tokens - current_allowance.tokens).max(0);
        (estimated, estimated.map(|value| prior + value))
    };
    let (estimated_cost, projected_cost) = if confidence == ForecastConfidence::Early {
        (None, None)
    } else {
        let estimated = scale(current_allowance.cost_nanos, observed);
        let prior = (cycle_totals.cost_nanos - current_allowance.cost_nanos).max(0);
        (estimated, estimated.map(|value| prior + value))
    };

    Some(Forecast {
        confidence,
        observed_percent,
        estimated_full_tokens: estimated_tokens,
        estimated_full_cost_nanos: estimated_cost,
        projected_cycle_tokens: projected_tokens,
        projected_cycle_cost_nanos: projected_cost,
    })
}

/// `value × 100 / observed`，溢出与非法值返回 `None`。
fn scale(value: i64, observed_percent: f64) -> Option<i64> {
    if value <= 0 || observed_percent <= 0.0 {
        return None;
    }
    let estimate = f64::from(i32::try_from(value).ok()?) * 100.0 / observed_percent;
    if !estimate.is_finite() || estimate > i64::MAX as f64 {
        return None;
    }
    Some(estimate.round() as i64)
}

/// 一组采样的推导结果。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DerivedCycles {
    /// 按重置时刻升序；最后一个是当前周期。
    pub cycles: Vec<DerivedCycle>,
}

impl DerivedCycles {
    pub fn active(&self) -> Option<&DerivedCycle> {
        self.cycles.last()
    }
}

/// 从同一序列的采样推导周期。调用方必须先按
/// `(provider, identity_key, window_kind, window_id)` 分组，并按 `observed_at` 升序排好。
pub fn derive_cycles(
    provider: ProviderId,
    identity_key: &str,
    window_kind: QuotaWindowKind,
    window_id: &str,
    samples: &[CycleSample],
) -> DerivedCycles {
    let mut cycles: Vec<DerivedCycle> = Vec::new();

    for sample in samples {
        let used = (100.0 - sample.remaining_percent).clamp(0.0, 100.0);
        let window_seconds = sample
            .window_seconds
            .or_else(|| default_window_seconds(window_kind));

        match cycles.last_mut() {
            None => {
                if let Some(cycle) = open_cycle(
                    provider,
                    identity_key,
                    window_kind,
                    window_id,
                    window_seconds,
                    sample,
                    used,
                    BoundaryQuality::Inferred,
                ) {
                    cycles.push(cycle);
                }
            }
            Some(active) => {
                let Some(scheduled_end) = sample.resets_at else {
                    // 没有重置时刻的采样只更新读数：拿它新开周期会得到一个猜的边界。
                    apply_reading(active, sample, used);
                    continue;
                };

                let drift = (scheduled_end - active.scheduled_end_at)
                    .num_seconds()
                    .abs();
                if drift <= RESET_JITTER_TOLERANCE_SECS {
                    // 秒级抖动：同一次重置的相邻采样。
                    apply_reading(active, sample, used);
                    continue;
                }
                if drift <= ACTIVE_RESET_DRIFT_TOLERANCE_SECS {
                    // 分钟级漂移：保留既有边界，避免把抖动写成周期滚动。
                    apply_reading(active, sample, used);
                    continue;
                }

                // 真实滚动：关闭当前周期，按新的重置时刻开新周期。
                let new_start = start_from(
                    scheduled_end,
                    window_seconds.or(active.window_seconds),
                    sample.observed_at,
                );
                close_cycle(active, new_start, sample);
                if let Some(cycle) = open_cycle(
                    provider,
                    identity_key,
                    window_kind,
                    window_id,
                    window_seconds.or(active.window_seconds),
                    sample,
                    used,
                    BoundaryQuality::Observed,
                ) {
                    cycles.push(cycle);
                }
                continue;
            }
        }
    }

    // 额外重置：同一周期内已用比例明显回落 → 追加一个额度片段。
    split_extra_resets(&mut cycles);

    DerivedCycles { cycles }
}

/// 沿采样序列切出额外重置片段。
///
/// 这一步与周期滚动分开：额度回落说明「这一份额度用完了，服务端又给了一份」，
/// 周期本身还没结束，因此它是周期内的新片段，不是新周期。
fn split_extra_resets(cycles: &mut [DerivedCycle]) {
    for cycle in cycles.iter_mut() {
        let samples = cycle.samples.clone();
        if samples.len() < 2 {
            cycle.samples.clear();
            continue;
        }
        let mut segments: Vec<DerivedSegment> = Vec::new();
        for sample in &samples {
            match segments.last_mut() {
                None => segments.push(new_segment(sample, SegmentStartReason::Initial)),
                Some(active) => {
                    let previous_used = active.latest_used_percent;
                    if is_extra_reset(previous_used, sample.used_percent) {
                        // 上一片段在回落时刻收口。
                        active.end_at = Some(sample.observed_at);
                        segments.push(new_segment(sample, SegmentStartReason::ExtraReset));
                    } else {
                        active.latest_used_percent = sample.used_percent;
                        active.maximum_used_percent =
                            active.maximum_used_percent.max(sample.used_percent);
                        active.last_sample_at = sample.observed_at;
                    }
                }
            }
        }
        if let Some(first) = segments.first_mut() {
            // 片段是从采样重建的，采样时刻只是「我们第一次看到它」；
            // 初始片段代表这一份额度的有效期，起点必须回到周期起点，否则
            // 片段用量区间会漏掉周期开头的一段（跨重启后尤其明显）。
            first.start_at = cycle.start_at;
        }
        cycle.segments = segments;
        cycle.samples.clear();
    }
}

/// 周期内的采样（中间态；推导结束后清空，只留周期与片段）。
#[derive(Debug, Clone, Copy, PartialEq)]
struct SampleReading {
    observed_at: DateTime<Utc>,
    used_percent: f64,
}

fn new_segment(sample: &SampleReading, reason: SegmentStartReason) -> DerivedSegment {
    DerivedSegment {
        start_at: sample.observed_at,
        end_at: None,
        baseline_used_percent: sample.used_percent,
        latest_used_percent: sample.used_percent,
        maximum_used_percent: sample.used_percent,
        first_sample_at: sample.observed_at,
        last_sample_at: sample.observed_at,
        start_reason: reason,
    }
}

/// 额外重置判定：明显回落，或本就在接近零用量处的小幅回落。
fn is_extra_reset(previous_used_percent: f64, used_percent: f64) -> bool {
    let drop = previous_used_percent - used_percent;
    drop >= EXTRA_RESET_MINIMUM_DROP
        || (used_percent <= NEAR_ZERO_USED_PERCENT && drop >= NEAR_ZERO_MINIMUM_DROP)
}

/// 周期起点：能用窗口长度回推就回推，不能就用**首次观察时刻**。
///
/// 退回首次观察时刻而不是重置时刻：窗口长度未知（Cursor 的计费周期桶）时，
/// 我们唯一确知的事实是「这个周期至少从我们第一次看到它时就在了」；
/// 把起点放在重置时刻会把整个周期压成一个点。
fn start_from(
    scheduled_end: DateTime<Utc>,
    window_seconds: Option<u64>,
    observed_at: DateTime<Utc>,
) -> DateTime<Utc> {
    match window_seconds {
        Some(seconds) if seconds > 0 => {
            scheduled_end - Duration::seconds(i64::try_from(seconds).unwrap_or(i64::MAX))
        }
        _ => observed_at,
    }
}

#[allow(clippy::too_many_arguments)]
fn open_cycle(
    provider: ProviderId,
    identity_key: &str,
    window_kind: QuotaWindowKind,
    window_id: &str,
    window_seconds: Option<u64>,
    sample: &CycleSample,
    used: f64,
    quality: BoundaryQuality,
) -> Option<DerivedCycle> {
    let scheduled_end_at = sample.resets_at?;
    let start_at = start_from(scheduled_end_at, window_seconds, sample.observed_at);
    let reading = SampleReading {
        observed_at: sample.observed_at,
        used_percent: used,
    };
    let mut cycle = DerivedCycle {
        provider,
        identity_key: identity_key.to_owned(),
        window_kind,
        window_id: window_id.to_owned(),
        window_seconds,
        start_at,
        end_at: scheduled_end_at,
        scheduled_end_at,
        first_sample_at: sample.observed_at,
        last_sample_at: sample.observed_at,
        latest_used_percent: used,
        boundary_quality: quality,
        segments: Vec::new(),
        samples: vec![reading],
    };
    let mut segment = new_segment(&reading, SegmentStartReason::Initial);
    // 初始片段代表「这一份额度的有效期」，它从周期起点开始；first_sample_at 才是
    // 我们第一次看到的时刻。两者混同会让片段用量区间漏掉周期开头的一段。
    segment.start_at = start_at;
    cycle.segments = vec![segment];
    Some(cycle)
}

fn apply_reading(cycle: &mut DerivedCycle, sample: &CycleSample, used: f64) {
    cycle.last_sample_at = sample.observed_at;
    cycle.latest_used_percent = used;
    cycle.samples.push(SampleReading {
        observed_at: sample.observed_at,
        used_percent: used,
    });
}

fn close_cycle(cycle: &mut DerivedCycle, new_start: DateTime<Utc>, sample: &CycleSample) {
    // 相邻周期的边界由各自的 resetsAt 推出，可能出现数秒重叠：抖动不截断。
    if new_start > cycle.end_at + Duration::seconds(RESET_JITTER_TOLERANCE_SECS) {
        cycle.end_at = new_start;
    }
    apply_reading(cycle, sample, cycle.latest_used_percent);
}

// cc-bar 还需要一步「超长周期修复」：它把周期边界持久化到 JSON，跨重启累积的
// `resetsAt` 漂移会把起点留在旧位置，只能事后按「重置时刻 − 窗口长度」纠回来。
// 这里不需要：起点每次都由 `resetsAt − 窗口长度` 现算，边界不落盘，
// 也就不存在「被污染的起点」这种状态。

/// 窗口类型对应的名义长度。只在采样没带长度时使用。
fn default_window_seconds(kind: QuotaWindowKind) -> Option<u64> {
    match kind {
        QuotaWindowKind::FiveHour => Some(18_000),
        QuotaWindowKind::Weekly | QuotaWindowKind::ModelWeekly => Some(604_800),
        QuotaWindowKind::Monthly => Some(30 * 86_400),
        // Cursor 的三档是计费周期桶：长度随周期变化，只能用采样自带的长度。
        QuotaWindowKind::Total | QuotaWindowKind::Api | QuotaWindowKind::Auto => None,
        QuotaWindowKind::Unknown => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_000_000 + seconds, 0).expect("valid")
    }

    fn sample(offset: i64, remaining: f64, resets: Option<i64>) -> CycleSample {
        CycleSample {
            observed_at: at(offset),
            remaining_percent: remaining,
            resets_at: resets.map(at),
            window_seconds: Some(18_000),
        }
    }

    fn derive(samples: &[CycleSample]) -> DerivedCycles {
        derive_cycles(
            ProviderId::Codex,
            "identity",
            QuotaWindowKind::FiveHour,
            "codex.five-hour",
            samples,
        )
    }

    #[test]
    fn a_single_sample_opens_an_inferred_cycle() {
        let cycles = derive(&[sample(0, 60.0, Some(18_000))]);

        assert_eq!(cycles.cycles.len(), 1);
        let cycle = cycles.active().expect("active");
        assert_eq!(cycle.boundary_quality, BoundaryQuality::Inferred);
        assert_eq!(cycle.start_at, at(0));
        assert_eq!(cycle.scheduled_end_at, at(18_000));
        assert_eq!(cycle.latest_used_percent, 40.0);
        assert_eq!(cycle.segments.len(), 1);
        assert_eq!(cycle.segments[0].start_reason, SegmentStartReason::Initial);
        // 起点是推出来的，因此片段的起点也按窗口长度回推。
        assert_eq!(cycle.segments[0].start_at, at(0));
    }

    #[test]
    fn a_reset_within_the_jitter_tolerance_does_not_open_a_cycle() {
        let cycles = derive(&[
            sample(0, 60.0, Some(18_000)),
            // 服务端 resetsAt 抖动 3 秒。
            sample(300, 55.0, Some(18_003)),
        ]);

        assert_eq!(cycles.cycles.len(), 1);
        assert_eq!(cycles.active().expect("active").latest_used_percent, 45.0);
    }

    #[test]
    fn a_minute_level_drift_keeps_the_existing_boundary() {
        let cycles = derive(&[
            sample(0, 60.0, Some(18_000)),
            sample(300, 50.0, Some(18_000 + 600)),
        ]);

        assert_eq!(cycles.cycles.len(), 1, "分钟级漂移不是滚动");
        assert_eq!(
            cycles.active().expect("active").scheduled_end_at,
            at(18_000)
        );
    }

    #[test]
    fn a_real_rollover_closes_the_previous_cycle_at_the_new_start() {
        let cycles = derive(&[
            sample(0, 60.0, Some(18_000)),
            sample(600, 40.0, Some(18_000)),
            // 重置时刻跳到下一个窗口：真实滚动。
            sample(18_000, 99.0, Some(36_000)),
        ]);

        assert_eq!(cycles.cycles.len(), 2);
        let previous = &cycles.cycles[0];
        assert_eq!(previous.end_at, at(18_000));
        let active = cycles.active().expect("active");
        assert_eq!(active.boundary_quality, BoundaryQuality::Observed);
        assert_eq!(active.start_at, at(18_000));
        assert_eq!(active.latest_used_percent, 1.0);
    }

    #[test]
    fn a_sample_without_a_reset_time_only_updates_the_reading() {
        let cycles = derive(&[sample(0, 60.0, Some(18_000)), sample(300, 50.0, None)]);

        assert_eq!(cycles.cycles.len(), 1);
        assert_eq!(cycles.active().expect("active").latest_used_percent, 50.0);
        assert_eq!(
            cycles.active().expect("active").scheduled_end_at,
            at(18_000)
        );
    }

    #[test]
    fn a_leading_sample_without_a_reset_time_cannot_open_a_cycle() {
        // 没有重置时刻就推不出周期边界：宁可不出周期，也不给一个猜的起点。
        let cycles = derive(&[sample(0, 60.0, None)]);
        assert!(cycles.cycles.is_empty());
    }

    #[test]
    fn a_usage_drop_inside_a_cycle_opens_an_extra_allowance_segment() {
        let cycles = derive(&[
            sample(0, 60.0, Some(18_000)),
            sample(600, 5.0, Some(18_000)),
            // 已用从 95% 回落到 20%：额外重置。
            sample(900, 80.0, Some(18_000)),
            sample(1_200, 70.0, Some(18_000)),
        ]);

        let cycle = cycles.active().expect("active");
        assert_eq!(cycle.segments.len(), 2);
        assert_eq!(cycle.extra_reset_count(), 1);
        assert_eq!(
            cycle.segments[1].start_reason,
            SegmentStartReason::ExtraReset
        );
        assert_eq!(cycle.segments[0].end_at, Some(at(900)));
        assert_eq!(cycle.segments[1].baseline_used_percent, 20.0);
        assert_eq!(cycle.segments[1].latest_used_percent, 30.0);
        // 官方口径的峰值是初始片段与额外片段之和。
        assert_eq!(cycle.peak_used_percent(), 100.0);
    }

    #[test]
    fn a_small_drop_near_zero_usage_is_still_an_extra_reset() {
        let cycles = derive(&[
            sample(0, 100.0, Some(18_000)),
            // 已用 5% → 0.5%：回落只有 4.5 个点，但落在接近零用量的区间里。
            sample(600, 95.0, Some(18_000)),
            sample(900, 99.5, Some(18_000)),
        ]);

        let cycle = cycles.active().expect("active");
        assert_eq!(
            cycle.segments.len(),
            2,
            "接近零用量时的小幅回落也是额外重置"
        );
        assert_eq!(
            cycle.segments[1].start_reason,
            SegmentStartReason::ExtraReset
        );
    }

    #[test]
    fn a_small_drop_at_high_usage_is_just_noise() {
        let cycles = derive(&[
            sample(0, 40.0, Some(18_000)),
            sample(600, 43.0, Some(18_000)),
        ]);

        let cycle = cycles.active().expect("active");
        assert_eq!(cycle.segments.len(), 1, "3% 的回落在高用量下是噪声");
    }

    #[test]
    fn the_cycle_start_is_always_recomputed_from_the_reset_time() {
        let cycles = derive(&[
            // 起点被污染的场景（跨重启累积漂移）在这里不存在：起点每次现算。
            sample(0, 90.0, Some(12 * 3600)),
            sample(12 * 3600, 5.0, Some(43_200)),
        ]);

        let previous = &cycles.cycles[0];
        assert_eq!(previous.start_at, at(12 * 3600 - 18_000));
        assert_eq!(previous.segments[0].start_at, at(12 * 3600 - 18_000));
    }

    #[test]
    fn cycle_ids_are_stable_across_queries() {
        let samples = vec![
            sample(0, 60.0, Some(18_000)),
            sample(600, 50.0, Some(18_000)),
        ];
        let first = derive(&samples);
        let second = derive(&samples);

        assert_eq!(
            first.active().expect("active").id(),
            second.active().expect("active").id()
        );
        assert!(first.active().expect("active").id().contains("codex"));
    }

    #[test]
    fn the_forecast_scales_the_active_allowance_and_adds_prior_usage() {
        let cycles = derive(&[
            sample(0, 60.0, Some(18_000)),
            sample(600, 5.0, Some(18_000)),
            sample(900, 80.0, Some(18_000)),
            sample(1_200, 70.0, Some(18_000)),
        ]);
        let cycle = cycles.active().expect("active");
        // 当前片段：基线 20% → 现在 30%，官方已用 10%。
        assert_eq!(
            cycle
                .active_segment()
                .expect("segment")
                .observed_used_percent(),
            10.0
        );

        let current = SegmentUsage {
            tokens: 1_000,
            cost_nanos: 2_000_000_000,
            request_count: 4,
        };
        let totals = SegmentUsage {
            tokens: 9_000,
            cost_nanos: 5_000_000_000,
            request_count: 12,
        };
        let forecast = forecast(cycle, current, totals).expect("forecast");

        assert_eq!(forecast.confidence, ForecastConfidence::Rough);
        assert_eq!(forecast.observed_percent, 10);
        // 10% 对应 1000 tokens → 整段 10000；此前实际 8000 → 整周期 18000。
        assert_eq!(forecast.estimated_full_tokens, Some(10_000));
        assert_eq!(forecast.projected_cycle_tokens, Some(18_000));
        assert_eq!(forecast.estimated_full_cost_nanos, Some(20_000_000_000));
        assert_eq!(forecast.projected_cycle_cost_nanos, Some(23_000_000_000));
    }

    #[test]
    fn confidence_tiers_follow_the_observed_percent() {
        let cases = [
            (95.0, ForecastConfidence::Reliable),
            (80.0, ForecastConfidence::Reliable),
            (50.0, ForecastConfidence::Reference),
            (30.0, ForecastConfidence::Reference),
            (20.0, ForecastConfidence::Rough),
            (5.0, ForecastConfidence::Early),
        ];
        for (observed, expected) in cases {
            assert_eq!(
                ForecastConfidence::from_observed(observed),
                expected,
                "{observed}%"
            );
        }
    }

    #[test]
    fn an_early_forecast_reports_no_numbers() {
        let cycles = derive(&[
            sample(0, 100.0, Some(18_000)),
            sample(600, 96.0, Some(18_000)),
        ]);
        let cycle = cycles.active().expect("active");
        let usage = SegmentUsage {
            tokens: 100,
            cost_nanos: 0,
            request_count: 1,
        };

        let forecast = forecast(cycle, usage, usage).expect("forecast");
        assert_eq!(forecast.confidence, ForecastConfidence::Early);
        assert_eq!(forecast.estimated_full_tokens, None);
        assert_eq!(forecast.projected_cycle_tokens, None);
    }

    #[test]
    fn no_forecast_without_an_official_reading_or_local_usage() {
        let cycles = derive(&[sample(0, 100.0, Some(18_000))]);
        let cycle = cycles.active().expect("active");

        assert!(forecast(cycle, SegmentUsage::default(), SegmentUsage::default()).is_none());
        assert!(
            forecast(
                cycle,
                SegmentUsage {
                    tokens: 10,
                    cost_nanos: 0,
                    request_count: 1
                },
                SegmentUsage::default()
            )
            .is_none(),
            "官方已用比例为 0 时不给预估"
        );
    }

    #[test]
    fn cursor_buckets_without_a_window_length_use_the_sample_boundary() {
        // Cursor 的计费周期桶没有固定长度：起点退回采样时刻，而不是猜一个月。
        let cycles = derive_cycles(
            ProviderId::Cursor,
            "identity",
            QuotaWindowKind::Total,
            "cursor.total",
            &[CycleSample {
                observed_at: at(0),
                remaining_percent: 70.0,
                resets_at: Some(at(2_592_000)),
                window_seconds: None,
            }],
        );

        let cycle = cycles.active().expect("active");
        assert_eq!(cycle.start_at, at(0));
        assert_eq!(cycle.window_seconds, None);
    }

    #[test]
    fn a_sample_carried_window_length_beats_the_type_default() {
        let cycles = derive_cycles(
            ProviderId::Codex,
            "identity",
            QuotaWindowKind::FiveHour,
            "codex.five-hour",
            &[CycleSample {
                observed_at: at(0),
                remaining_percent: 70.0,
                resets_at: Some(at(3_600)),
                window_seconds: Some(3_600),
            }],
        );

        let cycle = cycles.active().expect("active");
        assert_eq!(cycle.start_at, at(0));
        assert_eq!(cycle.window_seconds, Some(3_600));
    }
}
