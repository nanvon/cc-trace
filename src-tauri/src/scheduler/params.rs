//! 刷新参数基线。
//!
//! 这些常量是 `docs/技术架构.md`「首版参数基线」表在代码里的**唯一真值**。
//! 调整任何一项都必须同时改那张表，不允许只改代码常量。

/// 自动刷新抖动比例：间隔的 ±10%，避免两个 Provider 固定同刻请求。
pub const AUTO_REFRESH_JITTER_RATIO: f64 = 0.10;

/// 本地 Token 用量增量扫描间隔（秒）。首版固定为 5 分钟，后续由设置页接管。
pub const USAGE_SCAN_INTERVAL_SECS: u64 = 5 * 60;

/// 同一 Provider 手动刷新最小间隔（秒）。期间的重复触发合并到当前任务，不排队重发。
pub const MANUAL_REFRESH_MIN_INTERVAL_SECS: i64 = 60;

/// 单次 Provider 请求超时（秒）。超时归为网络类失败。
pub const REQUEST_TIMEOUT_SECS: u64 = 15;

/// 429 退避阶梯（秒）。服务端提供 `Retry-After` 时优先采用服务端值。
pub const RATE_LIMIT_BACKOFF_STEPS_SECS: [i64; 3] = [60, 120, 300];

/// 429 退避上限（秒）。
pub const RATE_LIMIT_BACKOFF_CAP_SECS: i64 = 15 * 60;

/// 网络与临时错误退避阶梯（秒）。
pub const TRANSIENT_BACKOFF_STEPS_SECS: [i64; 3] = [30, 60, 120];

/// 网络与临时错误退避上限（秒）。
pub const TRANSIENT_BACKOFF_CAP_SECS: i64 = 5 * 60;

/// 快照降级为 `stale` 的阈值：快照年龄超过当前自动刷新间隔的这个倍数。
/// 即使上次刷新成功也降级，并触发一次刷新。
pub const STALE_AGE_INTERVAL_MULTIPLIER: u32 = 2;

/// Codex token 刷新提前量（秒）：access token 距过期少于这个值就先续期。
pub const CODEX_TOKEN_REFRESH_SKEW_SECS: i64 = 300;

/// Claude Code token 刷新提前量（秒）。
///
/// 明显小于 Codex：CC Trace 是只读监控，把 refresh token 的使用主动权让给
/// Claude Code CLI，避免和它抢同一个一次性 refresh token，见
/// `docs/额度领域模型.md` 第 5.2 节。
pub const CLAUDE_TOKEN_REFRESH_SKEW_SECS: i64 = 30;

/// 对给定间隔施加 ±`AUTO_REFRESH_JITTER_RATIO` 抖动。
///
/// 不引入随机数依赖：用调用序号和进程启动以来的纳秒做低成本扰动即可满足
/// 「避免两个 Provider 固定同刻请求」这一唯一目的。
pub fn jittered_seconds(base_seconds: u64, salt: u64) -> u64 {
    if base_seconds == 0 {
        return 0;
    }

    let spread = (base_seconds as f64 * AUTO_REFRESH_JITTER_RATIO) as i64;
    if spread == 0 {
        return base_seconds;
    }

    let offset = (salt % (spread as u64 * 2 + 1)) as i64 - spread;
    (base_seconds as i64 + offset).max(1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_stays_within_ten_percent() {
        let base = 1800;
        let spread = (base as f64 * AUTO_REFRESH_JITTER_RATIO) as i64;

        for salt in 0..500 {
            let value = jittered_seconds(base, salt) as i64;
            assert!(
                (base as i64 - spread..=base as i64 + spread).contains(&value),
                "salt {salt} produced {value}, outside ±10% of {base}"
            );
        }
    }

    #[test]
    fn jitter_never_returns_zero() {
        assert_eq!(jittered_seconds(0, 7), 0);
        assert!(jittered_seconds(1, 7) >= 1);
    }

    #[test]
    fn baseline_values_match_the_architecture_table() {
        assert_eq!(USAGE_SCAN_INTERVAL_SECS, 300);
        assert_eq!(MANUAL_REFRESH_MIN_INTERVAL_SECS, 60);
        assert_eq!(REQUEST_TIMEOUT_SECS, 15);
        assert_eq!(RATE_LIMIT_BACKOFF_STEPS_SECS, [60, 120, 300]);
        assert_eq!(RATE_LIMIT_BACKOFF_CAP_SECS, 900);
        assert_eq!(TRANSIENT_BACKOFF_STEPS_SECS, [30, 60, 120]);
        assert_eq!(TRANSIENT_BACKOFF_CAP_SECS, 300);
        assert_eq!(STALE_AGE_INTERVAL_MULTIPLIER, 2);
        assert_eq!(CODEX_TOKEN_REFRESH_SKEW_SECS, 300);
        assert_eq!(CLAUDE_TOKEN_REFRESH_SKEW_SECS, 30);
    }
}
