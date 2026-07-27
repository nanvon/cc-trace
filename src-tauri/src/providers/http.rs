//! Provider 请求的共享 HTTP 客户端与失败分类。
//!
//! 超时取自 `docs/技术架构.md`「首版参数基线」。状态码到失败原因的映射是
//! `docs/状态与错误模型.md` 第 4 节失败分类表在代码里的唯一实现：
//!
//! | 来源 | 结局 |
//! |---|---|
//! | 无网络、DNS、连接被拒、超时、传输中断 | `Offline` |
//! | 429 | `RateLimited`，优先采用服务端 `Retry-After` |
//! | 401、403 | 凭据类 `Failed` |
//! | 其他非 2xx、响应无法解析 | 协议类 `Failed` |
//!
//! 失败值不携带响应体、请求头或端点原文。

use std::sync::OnceLock;
use std::time::Duration as StdDuration;

use chrono::Duration;

use super::ProviderFetchOutcome;
use crate::contracts::ErrorKind;
use crate::scheduler::params::REQUEST_TIMEOUT_SECS;

/// 进程内共享一个连接池。每次请求新建 client 会丢掉 TLS 会话复用，
/// 也会让两个 Provider 的并发请求互相抬高握手成本。
pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(StdDuration::from_secs(REQUEST_TIMEOUT_SECS))
            // 首版不需要重定向：两个 Provider 的端点都是直接返回 JSON 的 API。
            .redirect(reqwest::redirect::Policy::none())
            .build()
            // 构建失败只会源于 TLS 后端不可用，此时退回默认配置仍比放弃请求好。
            .unwrap_or_default()
    })
}

/// 传输层失败一律是网络类，不得伪装成凭据或协议问题。
pub fn classify_transport(_error: &reqwest::Error) -> ProviderFetchOutcome {
    ProviderFetchOutcome::Offline
}

/// 把非 2xx 响应映射成失败结局。2xx 返回 `None`，由调用方继续解析响应体。
pub fn classify_response(response: &reqwest::Response) -> Option<ProviderFetchOutcome> {
    let retry_after = response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|value| value.to_str().ok());

    classify_status(response.status(), retry_after)
}

/// 分类规则本身。与 [`reqwest::Response`] 解耦，因此可以直接按状态码测试。
pub fn classify_status(
    status: reqwest::StatusCode,
    retry_after: Option<&str>,
) -> Option<ProviderFetchOutcome> {
    if status.is_success() {
        return None;
    }

    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return Some(ProviderFetchOutcome::RateLimited {
            retry_after: parse_retry_after(retry_after),
        });
    }

    if matches!(
        status,
        reqwest::StatusCode::UNAUTHORIZED | reqwest::StatusCode::FORBIDDEN
    ) {
        return Some(ProviderFetchOutcome::Failed {
            kind: ErrorKind::Credentials,
        });
    }

    Some(ProviderFetchOutcome::Failed {
        kind: ErrorKind::Protocol,
    })
}

/// 读取 `Retry-After` 的秒数形式。HTTP-date 形式不解析，交给退避阶梯兜底。
fn parse_retry_after(value: Option<&str>) -> Option<Duration> {
    let seconds = value?.trim().parse::<i64>().ok()?;
    (seconds >= 0).then(|| Duration::seconds(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(code: u16) -> reqwest::StatusCode {
        reqwest::StatusCode::from_u16(code).expect("valid status code")
    }

    #[test]
    fn success_leaves_the_body_to_the_caller() {
        assert!(classify_status(status(200), None).is_none());
    }

    #[test]
    fn rate_limiting_prefers_the_servers_retry_after() {
        assert_eq!(
            classify_status(status(429), Some("90")),
            Some(ProviderFetchOutcome::RateLimited {
                retry_after: Some(Duration::seconds(90)),
            })
        );
    }

    #[test]
    fn rate_limiting_without_a_usable_header_falls_back_to_the_backoff_ladder() {
        for header in [None, Some("Wed, 21 Oct 2026 07:28:00 GMT"), Some("-5")] {
            assert_eq!(
                classify_status(status(429), header),
                Some(ProviderFetchOutcome::RateLimited { retry_after: None }),
                "{header:?}"
            );
        }
    }

    #[test]
    fn auth_failures_are_credential_errors_and_never_offline() {
        for code in [401, 403] {
            assert_eq!(
                classify_status(status(code), None),
                Some(ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Credentials,
                }),
                "HTTP {code} must not be downgraded to a network failure"
            );
        }
    }

    #[test]
    fn other_statuses_are_protocol_errors() {
        for code in [400, 404, 418, 500, 503] {
            assert_eq!(
                classify_status(status(code), None),
                Some(ProviderFetchOutcome::Failed {
                    kind: ErrorKind::Protocol,
                }),
                "HTTP {code}"
            );
        }
    }
}
