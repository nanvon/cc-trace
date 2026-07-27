//! 单个 Provider 的刷新运行时：三维状态转换、请求合并、节流与退避。
//!
//! 本模块是 `docs/状态与错误模型.md` 第 2 节**组合矩阵**在代码里的实现，
//! 也是唯一被允许改写三个状态维度的地方。测试按矩阵逐行覆盖。

use chrono::{DateTime, Duration, Utc};

use super::backoff::Backoff;
use super::params::{MANUAL_REFRESH_MIN_INTERVAL_SECS, STALE_AGE_INTERVAL_MULTIPLIER};
use crate::contracts::{
    AppError, ProviderAvailability, ProviderId, ProviderIdentity, ProviderSnapshot, QuotaSnapshot,
    RefreshState, SnapshotFreshness,
};
use crate::providers::ProviderFetchOutcome;

/// 刷新的触发来源。三种来源都不能绕过退避。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefreshTrigger {
    /// 启动或系统唤醒后的首次刷新。
    Startup,
    /// 自动刷新定时器。
    Auto,
    /// 用户在紧凑面板、主窗口或原生菜单触发。
    Manual,
}

/// 是否真的要发起一次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshDecision {
    Start,
    /// 已有同 Provider 的任务在飞，合并到那一个，不排队重发。
    Merged,
    /// 手动刷新在最小间隔内。
    Throttled,
    /// 退避期内。界面展示可再次尝试时间，但不发起真实请求。
    Blocked {
        retry_at: DateTime<Utc>,
    },
}

/// 一个 Provider 的完整运行时状态。
#[derive(Debug, Clone)]
pub struct ProviderRuntime {
    pub snapshot: ProviderSnapshot,
    backoff: Backoff,
    in_flight: bool,
    last_manual_refresh: Option<DateTime<Utc>>,
    last_success_instant: Option<DateTime<Utc>>,
    /// 上一次成功刷新时的身份指纹。只在 Rust 内比较，不进 command 载荷。
    identity_key: Option<String>,
}

impl ProviderRuntime {
    pub fn new(provider: ProviderId) -> Self {
        Self {
            snapshot: ProviderSnapshot::initial(provider),
            backoff: Backoff::default(),
            in_flight: false,
            last_manual_refresh: None,
            last_success_instant: None,
            identity_key: None,
        }
    }

    pub fn identity_key(&self) -> Option<&str> {
        self.identity_key.as_deref()
    }

    /// 从额度缓存恢复快照时一并恢复身份指纹，让重启后的第一次刷新也能发现身份变化。
    pub fn restore_identity_key(&mut self, identity_key: Option<String>) {
        self.identity_key = identity_key;
    }

    /// 用缓存里的最新有效快照初始化，供启动时先展示再刷新。
    ///
    /// 缓存内容按定义不是本次会话取得的，因此新鲜度是 `stale` 而不是 `live`——
    /// 「有数值」不等于「刷新成功」，见 `docs/状态与错误模型.md` 第 7 节。
    pub fn restore_from_cache(
        &mut self,
        identity: Option<ProviderIdentity>,
        identity_key: Option<String>,
        snapshot: QuotaSnapshot,
        last_success_at: Option<DateTime<Utc>>,
    ) {
        self.identity_key = identity_key;
        self.last_success_instant = last_success_at;
        self.snapshot.identity = identity;
        self.snapshot.snapshot = Some(snapshot);
        self.snapshot.refresh = RefreshState::Idle;
        self.snapshot.freshness = SnapshotFreshness::Stale;
        self.snapshot.availability = ProviderAvailability::Ready;
        self.snapshot.last_success_at = last_success_at.map(|value| value.to_rfc3339());
    }

    pub fn provider(&self) -> ProviderId {
        self.snapshot.provider
    }

    /// 决定是否发起请求。不改变任何状态。
    pub fn decide(&self, trigger: RefreshTrigger, now: DateTime<Utc>) -> RefreshDecision {
        if self.in_flight {
            return RefreshDecision::Merged;
        }

        // 手动刷新不得绕过退避，见 docs/状态与错误模型.md 第 5 节。
        let blocked = self
            .backoff
            .retry_at()
            .filter(|_| self.backoff.is_blocked(now));
        if let Some(retry_at) = blocked {
            return RefreshDecision::Blocked { retry_at };
        }

        let throttled = trigger == RefreshTrigger::Manual
            && self.last_manual_refresh.is_some_and(|last| {
                now - last < Duration::seconds(MANUAL_REFRESH_MIN_INTERVAL_SECS)
            });
        if throttled {
            return RefreshDecision::Throttled;
        }

        RefreshDecision::Start
    }

    /// 进入刷新态。已有快照时是 `refreshing`，快照不被清空。
    pub fn begin(&mut self, trigger: RefreshTrigger, now: DateTime<Utc>) -> RefreshState {
        self.in_flight = true;
        if trigger == RefreshTrigger::Manual {
            self.last_manual_refresh = Some(now);
        }

        let state = if self.snapshot.has_snapshot() {
            RefreshState::Refreshing
        } else {
            RefreshState::Loading
        };
        self.snapshot.refresh = state;
        state
    }

    /// 把一次请求结局写进三个维度。
    pub fn apply(&mut self, outcome: ProviderFetchOutcome, now: DateTime<Utc>) {
        // 身份变化必须先于任何写入：丢弃该 Provider 的快照与退避，界面一刻也不能展示
        // 上一个身份的额度，见 docs/额度领域模型.md 第 4.1 节。
        if let ProviderFetchOutcome::Success { identity_key, .. } = &outcome
            && self.identity_changed(identity_key.as_deref())
        {
            self.discard_for_identity_change();
        }

        self.in_flight = false;
        self.snapshot.refresh = RefreshState::Idle;
        self.snapshot.last_attempt_at = Some(now.to_rfc3339());

        match outcome {
            ProviderFetchOutcome::Success {
                identity,
                identity_key,
                snapshot,
            } => {
                self.identity_key = identity_key;
                self.backoff.reset();
                self.snapshot.identity = identity;
                self.snapshot.snapshot = Some(snapshot);
                self.snapshot.freshness = SnapshotFreshness::Live;
                self.snapshot.availability = ProviderAvailability::Ready;
                self.snapshot.error = None;
                self.snapshot.retry_after = None;
                self.snapshot.last_success_at = Some(now.to_rfc3339());
                self.last_success_instant = Some(now);
            }

            // 凭据缺失或形式不受支持时不展示任何额度数值：界面用说明替换额度区域，
            // 而不是把它显示成 0%，见 docs/状态与错误模型.md 第 7 节。
            ProviderFetchOutcome::NoCredentials => {
                self.clear_snapshot(ProviderAvailability::NoCredentials);
            }
            ProviderFetchOutcome::Unsupported => {
                self.clear_snapshot(ProviderAvailability::Unsupported);
            }

            ProviderFetchOutcome::Offline => {
                let retry_at = self.backoff.record_transient(now);
                self.degrade(ProviderAvailability::Offline, Some(retry_at));
                self.snapshot.error = None;
            }

            ProviderFetchOutcome::RateLimited { retry_after } => {
                let retry_at = self.backoff.record_rate_limited(now, retry_after);
                self.degrade(ProviderAvailability::RateLimited, Some(retry_at));
                self.snapshot.error = None;
            }

            // 凭据类与协议类错误同样进入退避，避免自动刷新在持续失败时反复打 Provider。
            // 退避数值沿用「网络与临时错误」阶梯，见 docs/技术架构.md 首版参数基线。
            ProviderFetchOutcome::Failed { kind } => {
                let retry_at = self.backoff.record_transient(now);
                self.degrade(ProviderAvailability::Error, Some(retry_at));
                self.snapshot.error = Some(AppError { kind });
            }
        }
    }

    /// 快照年龄超过自动刷新间隔的固定倍数时降级为 `stale`，并保持上一次的失败原因。
    /// 返回是否发生了降级——调用方据此决定要不要触发一次刷新。
    pub fn expire_if_stale(&mut self, now: DateTime<Utc>, auto_refresh_interval_secs: u64) -> bool {
        if self.snapshot.freshness != SnapshotFreshness::Live {
            return false;
        }

        let Some(last_success) = self.last_success_instant else {
            return false;
        };

        let threshold = Duration::seconds(
            (auto_refresh_interval_secs * STALE_AGE_INTERVAL_MULTIPLIER as u64) as i64,
        );
        if now - last_success <= threshold {
            return false;
        }

        self.snapshot.freshness = SnapshotFreshness::Stale;
        true
    }

    /// 身份是否与上一次成功刷新不同。
    ///
    /// 任一侧认不出身份时返回 `false`：「没法比较」不等于「变了」，误丢一份有效快照
    /// 比多显示一次更糟。
    fn identity_changed(&self, next: Option<&str>) -> bool {
        match (self.identity_key.as_deref(), next) {
            (Some(previous), Some(next)) => previous != next,
            _ => false,
        }
    }

    /// 身份变化时丢弃该 Provider 的快照与退避，见 docs/额度领域模型.md 第 4.1 节。
    pub fn discard_for_identity_change(&mut self) {
        let provider = self.snapshot.provider;
        self.snapshot = ProviderSnapshot::initial(provider);
        self.backoff.reset();
        self.last_success_instant = None;
        self.identity_key = None;
    }

    /// 场景切换时预置一份「上一次成功」的快照，只供桌面壳验证使用。
    pub fn seed_success(&mut self, outcome: ProviderFetchOutcome, now: DateTime<Utc>) {
        self.apply(outcome, now);
    }

    pub fn reset(&mut self) {
        self.discard_for_identity_change();
        self.in_flight = false;
        self.last_manual_refresh = None;
    }

    fn clear_snapshot(&mut self, availability: ProviderAvailability) {
        self.backoff.reset();
        self.snapshot.identity = None;
        self.snapshot.snapshot = None;
        self.snapshot.freshness = SnapshotFreshness::Empty;
        self.snapshot.availability = availability;
        self.snapshot.error = None;
        self.snapshot.retry_after = None;
        self.last_success_instant = None;
    }

    /// 失败但可能有旧快照：保留数值，只改新鲜度与原因。
    fn degrade(&mut self, availability: ProviderAvailability, retry_at: Option<DateTime<Utc>>) {
        self.snapshot.freshness = if self.snapshot.has_snapshot() {
            SnapshotFreshness::Stale
        } else {
            SnapshotFreshness::Empty
        };
        self.snapshot.availability = availability;
        self.snapshot.retry_after = retry_at.map(|value| value.to_rfc3339());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ErrorKind, QuotaSnapshot, QuotaWindow, QuotaWindowKind};

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    fn snapshot_with(remaining: f64, now: DateTime<Utc>) -> QuotaSnapshot {
        QuotaSnapshot {
            windows: vec![QuotaWindow {
                id: "test.primary".to_owned(),
                kind: QuotaWindowKind::FiveHour,
                display_name: None,
                used_percent: 100.0 - remaining,
                remaining_percent: remaining,
                resets_at: Some((now + Duration::hours(4)).to_rfc3339()),
                window_seconds: Some(18_000),
                is_active: true,
                is_primary: true,
            }],
            captured_at: now.to_rfc3339(),
        }
    }

    fn success(now: DateTime<Utc>) -> ProviderFetchOutcome {
        success_as("identity-one", now)
    }

    fn success_as(identity_key: &str, now: DateTime<Utc>) -> ProviderFetchOutcome {
        ProviderFetchOutcome::Success {
            identity: None,
            identity_key: Some(identity_key.to_owned()),
            snapshot: snapshot_with(73.0, now),
        }
    }

    fn refreshed_runtime() -> ProviderRuntime {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.begin(RefreshTrigger::Startup, at(0));
        runtime.apply(success(at(0)), at(0));
        runtime
    }

    // --- docs/状态与错误模型.md 第 2 节组合矩阵，逐行覆盖 ---

    #[test]
    fn matrix_first_load() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        assert_eq!(
            runtime.begin(RefreshTrigger::Startup, at(0)),
            RefreshState::Loading
        );
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Empty);
        assert_eq!(runtime.snapshot.availability, ProviderAvailability::Ready);
    }

    #[test]
    fn matrix_refresh_with_existing_snapshot_keeps_the_values() {
        let mut runtime = refreshed_runtime();
        assert_eq!(
            runtime.begin(RefreshTrigger::Manual, at(60)),
            RefreshState::Refreshing
        );
        assert!(
            runtime.snapshot.has_snapshot(),
            "an in-flight refresh must not clear the snapshot"
        );
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Live);
    }

    #[test]
    fn matrix_refresh_succeeds() {
        let runtime = refreshed_runtime();
        assert_eq!(runtime.snapshot.refresh, RefreshState::Idle);
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Live);
        assert_eq!(runtime.snapshot.availability, ProviderAvailability::Ready);
        assert!(runtime.snapshot.last_success_at.is_some());
        assert!(runtime.snapshot.error.is_none());
    }

    #[test]
    fn matrix_no_credentials_shows_no_quota_at_all() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Manual, at(60));
        runtime.apply(ProviderFetchOutcome::NoCredentials, at(60));

        assert_eq!(
            runtime.snapshot.availability,
            ProviderAvailability::NoCredentials
        );
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Empty);
        assert!(
            runtime.snapshot.snapshot.is_none(),
            "no_credentials must not keep values that would read as a real quota"
        );
        assert!(runtime.snapshot.retry_after.is_none());
    }

    #[test]
    fn matrix_unsupported_is_not_downgraded_to_error() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.begin(RefreshTrigger::Startup, at(0));
        runtime.apply(ProviderFetchOutcome::Unsupported, at(0));

        assert_eq!(
            runtime.snapshot.availability,
            ProviderAvailability::Unsupported
        );
        assert!(runtime.snapshot.error.is_none());
    }

    #[test]
    fn matrix_offline_with_snapshot_goes_stale_and_keeps_values() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(ProviderFetchOutcome::Offline, at(60));

        assert_eq!(runtime.snapshot.availability, ProviderAvailability::Offline);
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Stale);
        assert!(runtime.snapshot.has_snapshot());
        assert_eq!(runtime.snapshot.retry_after, Some(at(90).to_rfc3339()));
    }

    #[test]
    fn matrix_offline_without_snapshot_stays_empty() {
        let mut runtime = ProviderRuntime::new(ProviderId::Claude);
        runtime.begin(RefreshTrigger::Startup, at(0));
        runtime.apply(ProviderFetchOutcome::Offline, at(0));

        assert_eq!(runtime.snapshot.availability, ProviderAvailability::Offline);
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Empty);
    }

    #[test]
    fn matrix_rate_limited_with_snapshot_reports_a_retry_time() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(
            ProviderFetchOutcome::RateLimited {
                retry_after: Some(Duration::minutes(12)),
            },
            at(60),
        );

        assert_eq!(
            runtime.snapshot.availability,
            ProviderAvailability::RateLimited
        );
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Stale);
        assert_eq!(
            runtime.snapshot.retry_after,
            Some(at(60 + 720).to_rfc3339())
        );
        assert!(runtime.snapshot.has_snapshot());
    }

    #[test]
    fn matrix_error_with_snapshot_distinguishes_credential_from_protocol() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(
            ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            },
            at(60),
        );

        assert_eq!(runtime.snapshot.availability, ProviderAvailability::Error);
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Stale);
        assert_eq!(
            runtime.snapshot.error.as_ref().map(|error| error.kind),
            Some(ErrorKind::Credentials)
        );
        assert!(runtime.snapshot.has_snapshot());
    }

    #[test]
    fn matrix_error_without_snapshot_stays_empty() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.begin(RefreshTrigger::Startup, at(0));
        runtime.apply(
            ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol,
            },
            at(0),
        );

        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Empty);
        assert_eq!(
            runtime.snapshot.error.as_ref().map(|error| error.kind),
            Some(ErrorKind::Protocol)
        );
    }

    #[test]
    fn matrix_expired_snapshot_goes_stale_and_keeps_the_previous_reason() {
        let mut runtime = refreshed_runtime();
        let interval = 1_800;

        assert!(!runtime.expire_if_stale(at(interval as i64), interval));
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Live);

        assert!(runtime.expire_if_stale(at(interval as i64 * 2 + 1), interval));
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Stale);
        assert_eq!(
            runtime.snapshot.availability,
            ProviderAvailability::Ready,
            "ageing out must not invent a failure reason"
        );
    }

    // --- 合并、节流与退避 ---

    #[test]
    fn concurrent_refreshes_merge_into_the_in_flight_task() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.begin(RefreshTrigger::Manual, at(0));

        assert_eq!(
            runtime.decide(RefreshTrigger::Manual, at(1)),
            RefreshDecision::Merged
        );
        assert_eq!(
            runtime.decide(RefreshTrigger::Auto, at(1)),
            RefreshDecision::Merged
        );
    }

    #[test]
    fn manual_refresh_is_throttled_for_thirty_seconds() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Manual, at(100));
        runtime.apply(success(at(100)), at(100));

        assert_eq!(
            runtime.decide(RefreshTrigger::Manual, at(129)),
            RefreshDecision::Throttled
        );
        assert_eq!(
            runtime.decide(RefreshTrigger::Manual, at(130)),
            RefreshDecision::Start
        );
        assert_eq!(
            runtime.decide(RefreshTrigger::Auto, at(101)),
            RefreshDecision::Start,
            "the manual throttle must not block the automatic schedule"
        );
    }

    #[test]
    fn manual_refresh_cannot_bypass_backoff() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(
            ProviderFetchOutcome::RateLimited { retry_after: None },
            at(60),
        );

        let decision = runtime.decide(RefreshTrigger::Manual, at(70));
        assert_eq!(decision, RefreshDecision::Blocked { retry_at: at(120) });
        assert_eq!(
            runtime.decide(RefreshTrigger::Manual, at(120)),
            RefreshDecision::Start
        );
    }

    #[test]
    fn a_successful_refresh_clears_the_backoff() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(ProviderFetchOutcome::Offline, at(60));
        assert!(matches!(
            runtime.decide(RefreshTrigger::Auto, at(70)),
            RefreshDecision::Blocked { .. }
        ));

        runtime.begin(RefreshTrigger::Auto, at(200));
        runtime.apply(success(at(200)), at(200));

        assert_eq!(
            runtime.decide(RefreshTrigger::Auto, at(201)),
            RefreshDecision::Start
        );
        assert!(runtime.snapshot.retry_after.is_none());
    }

    #[test]
    fn a_new_identity_discards_the_previous_snapshot_before_writing_the_new_one() {
        let mut runtime = refreshed_runtime();
        assert!(runtime.snapshot.has_snapshot());

        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(success_as("identity-two", at(60)), at(60));

        assert_eq!(runtime.identity_key(), Some("identity-two"));
        assert_eq!(
            runtime.snapshot.freshness,
            SnapshotFreshness::Live,
            "the new identity's own snapshot must still be written"
        );
        assert_eq!(
            runtime.snapshot.last_success_at,
            Some(at(60).to_rfc3339()),
            "the discarded run must not leave the previous success time behind"
        );
    }

    #[test]
    fn the_same_identity_keeps_the_snapshot_and_the_backoff_history() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(success(at(60)), at(60));

        assert_eq!(runtime.identity_key(), Some("identity-one"));
        assert!(runtime.snapshot.has_snapshot());
    }

    #[test]
    fn an_unidentifiable_provider_never_triggers_a_discard() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(
            ProviderFetchOutcome::Success {
                identity: None,
                identity_key: None,
                snapshot: snapshot_with(50.0, at(60)),
            },
            at(60),
        );

        assert!(
            runtime.snapshot.has_snapshot(),
            "an absent fingerprint means 'cannot compare', not 'changed'"
        );
        assert_eq!(runtime.identity_key(), None);
    }

    #[test]
    fn a_cached_snapshot_is_restored_as_stale_never_as_a_fresh_success() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.restore_from_cache(None, None, snapshot_with(73.0, at(0)), Some(at(0)));

        assert_eq!(runtime.snapshot.refresh, RefreshState::Idle);
        assert_eq!(
            runtime.snapshot.freshness,
            SnapshotFreshness::Stale,
            "a cached snapshot was not fetched in this session"
        );
        assert_eq!(runtime.snapshot.availability, ProviderAvailability::Ready);
        assert!(runtime.snapshot.has_snapshot());
    }

    #[test]
    fn a_refresh_over_a_restored_snapshot_is_refreshing_not_loading() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.restore_from_cache(None, None, snapshot_with(73.0, at(0)), Some(at(0)));

        assert_eq!(
            runtime.begin(RefreshTrigger::Startup, at(1)),
            RefreshState::Refreshing,
            "the user already sees numbers, so this is not a first load"
        );
    }

    #[test]
    fn a_restored_identity_key_lets_the_first_refresh_after_restart_detect_a_change() {
        let mut runtime = ProviderRuntime::new(ProviderId::Codex);
        runtime.restore_identity_key(Some("identity-one".to_owned()));

        runtime.begin(RefreshTrigger::Startup, at(0));
        runtime.apply(success_as("identity-two", at(0)), at(0));

        assert_eq!(runtime.identity_key(), Some("identity-two"));
    }

    #[test]
    fn identity_change_discards_snapshot_and_backoff() {
        let mut runtime = refreshed_runtime();
        runtime.begin(RefreshTrigger::Auto, at(60));
        runtime.apply(ProviderFetchOutcome::Offline, at(60));

        runtime.discard_for_identity_change();

        assert!(runtime.snapshot.snapshot.is_none());
        assert_eq!(runtime.snapshot.freshness, SnapshotFreshness::Empty);
        assert_eq!(
            runtime.decide(RefreshTrigger::Auto, at(61)),
            RefreshDecision::Start
        );
    }
}
