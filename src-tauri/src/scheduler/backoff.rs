//! 按 Provider 独立计算的退避状态机。
//!
//! 规则见 `docs/状态与错误模型.md` 第 5 节，数值见 `super::params`。
//! 两条硬规则由本模块保证：
//!
//! - 每个 Provider 独立退避，一个 Provider 的退避不影响另一个（本类型每个 Provider 一份）。
//! - **手动刷新不得绕过退避**：`blocked_until` 对自动与手动刷新一视同仁。

use chrono::{DateTime, Duration, Utc};

use super::params::{
    RATE_LIMIT_BACKOFF_CAP_SECS, RATE_LIMIT_BACKOFF_STEPS_SECS, TRANSIENT_BACKOFF_CAP_SECS,
    TRANSIENT_BACKOFF_STEPS_SECS,
};

/// 退避阶梯。超出阶梯长度后停在上限。
#[derive(Debug, Clone, Copy)]
struct Ladder {
    steps: &'static [i64],
    cap_seconds: i64,
}

impl Ladder {
    const RATE_LIMIT: Self = Self {
        steps: &RATE_LIMIT_BACKOFF_STEPS_SECS,
        cap_seconds: RATE_LIMIT_BACKOFF_CAP_SECS,
    };

    const TRANSIENT: Self = Self {
        steps: &TRANSIENT_BACKOFF_STEPS_SECS,
        cap_seconds: TRANSIENT_BACKOFF_CAP_SECS,
    };

    /// `attempt` 从 0 开始计数：第一次失败取阶梯首项。
    fn seconds_for(&self, attempt: usize) -> i64 {
        let value = self
            .steps
            .get(attempt)
            .copied()
            .unwrap_or_else(|| self.steps.last().copied().unwrap_or(self.cap_seconds));
        value.min(self.cap_seconds)
    }
}

/// 单个 Provider 的退避状态。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Backoff {
    rate_limit_attempts: usize,
    transient_attempts: usize,
    retry_at: Option<DateTime<Utc>>,
}

impl Backoff {
    /// 记录一次 429。优先采用服务端 `Retry-After`，缺失时按阶梯递增。
    pub fn record_rate_limited(
        &mut self,
        now: DateTime<Utc>,
        server_retry_after: Option<Duration>,
    ) -> DateTime<Utc> {
        let wait = match server_retry_after {
            Some(duration) if duration > Duration::zero() => {
                duration.min(Duration::seconds(RATE_LIMIT_BACKOFF_CAP_SECS))
            }
            _ => Duration::seconds(Ladder::RATE_LIMIT.seconds_for(self.rate_limit_attempts)),
        };

        self.rate_limit_attempts = self.rate_limit_attempts.saturating_add(1);
        let retry_at = now + wait;
        self.retry_at = Some(retry_at);
        retry_at
    }

    /// 记录一次网络或临时错误。
    pub fn record_transient(&mut self, now: DateTime<Utc>) -> DateTime<Utc> {
        let wait = Duration::seconds(Ladder::TRANSIENT.seconds_for(self.transient_attempts));
        self.transient_attempts = self.transient_attempts.saturating_add(1);
        let retry_at = now + wait;
        self.retry_at = Some(retry_at);
        retry_at
    }

    /// 一次成功刷新后重置该 Provider 的退避。身份变化时同样调用本方法。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 退避解除时刻。界面据此展示「可再次尝试」时间。
    pub fn retry_at(&self) -> Option<DateTime<Utc>> {
        self.retry_at
    }

    /// 是否仍在退避期内。退避期内不得发起真实请求，手动刷新也不例外。
    pub fn is_blocked(&self, now: DateTime<Utc>) -> bool {
        self.retry_at.is_some_and(|retry_at| now < retry_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn rate_limit_follows_the_ladder_without_retry_after() {
        let mut backoff = Backoff::default();
        let now = at(0);

        assert_eq!(backoff.record_rate_limited(now, None), at(60));
        assert_eq!(backoff.record_rate_limited(now, None), at(120));
        assert_eq!(backoff.record_rate_limited(now, None), at(300));
        assert_eq!(
            backoff.record_rate_limited(now, None),
            at(300),
            "past the ladder the wait holds at its last step"
        );
    }

    #[test]
    fn server_retry_after_wins_and_is_capped() {
        let mut backoff = Backoff::default();
        let now = at(0);

        assert_eq!(
            backoff.record_rate_limited(now, Some(Duration::seconds(42))),
            at(42)
        );
        assert_eq!(
            backoff.record_rate_limited(now, Some(Duration::seconds(9_999))),
            at(RATE_LIMIT_BACKOFF_CAP_SECS),
            "server values are still capped at 15 minutes"
        );
    }

    #[test]
    fn transient_errors_use_their_own_shorter_ladder() {
        let mut backoff = Backoff::default();
        let now = at(0);

        assert_eq!(backoff.record_transient(now), at(30));
        assert_eq!(backoff.record_transient(now), at(60));
        assert_eq!(backoff.record_transient(now), at(120));
        assert_eq!(backoff.record_transient(now), at(120));
    }

    #[test]
    fn manual_refresh_cannot_bypass_the_backoff_window() {
        let mut backoff = Backoff::default();
        let retry_at = backoff.record_rate_limited(at(0), None);

        assert!(backoff.is_blocked(at(59)));
        assert!(!backoff.is_blocked(at(60)));
        assert_eq!(backoff.retry_at(), Some(retry_at));
    }

    #[test]
    fn success_resets_both_ladders() {
        let mut backoff = Backoff::default();
        backoff.record_rate_limited(at(0), None);
        backoff.record_transient(at(0));

        backoff.reset();

        assert!(!backoff.is_blocked(at(0)));
        assert_eq!(backoff.retry_at(), None);
        assert_eq!(backoff.record_rate_limited(at(0), None), at(60));
    }

    #[test]
    fn ladders_are_tracked_separately() {
        let mut backoff = Backoff::default();
        backoff.record_rate_limited(at(0), None);
        backoff.record_rate_limited(at(0), None);

        assert_eq!(
            backoff.record_transient(at(0)),
            at(30),
            "a transient failure must not inherit the rate-limit attempt count"
        );
    }
}
