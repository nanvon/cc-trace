//! 检查更新：查询 GitHub Releases 最新版本并与当前版本比较。
//!
//! 只提示与给出下载链接，不做自动安装（ADR-0031 范围）。失败分类沿用 Provider 的
//! 网络约定：无网络与超时是 `failed`，HTTP 403／429（GitHub 未认证限流）是 `rateLimited`，
//! 两者都可以稍后重试，且不影响任何额度功能。

use std::cmp::Ordering;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::providers::http;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/nanvon/cc-trace/releases/latest";
const RELEASE_URL_PREFIX: &str = "https://github.com/nanvon/cc-trace/";
pub const RELEASES_PAGE: &str = "https://github.com/nanvon/cc-trace/releases/latest";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateStatus {
    /// 还没有检查过。
    Idle,
    UpToDate {
        #[serde(rename = "latestVersion")]
        latest_version: String,
    },
    Available {
        #[serde(rename = "latestVersion")]
        latest_version: String,
    },
    Failed,
    RateLimited,
}

static LAST: Mutex<UpdateStatus> = Mutex::new(UpdateStatus::Idle);

pub fn last_status() -> UpdateStatus {
    LAST.lock()
        .map(|status| status.clone())
        .unwrap_or(UpdateStatus::Idle)
}

fn remember(status: &UpdateStatus) {
    if let Ok(mut last) = LAST.lock() {
        *last = status.clone();
    }
}

#[derive(Deserialize)]
struct ReleasePayload {
    tag_name: String,
}

/// `v0.1.12`、`0.2.0-beta.1` 等 tag 的解析结果：数字核心 + 是否带预发布后缀。
fn parse_version(raw: &str) -> Option<([u64; 3], bool)> {
    let trimmed = raw.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match trimmed.split_once(['-', '+']) {
        Some((core, _)) => (core, trimmed.contains('-')),
        None => (trimmed, false),
    };
    let mut parts = [0u64; 3];
    let mut count = 0;
    for piece in core.split('.') {
        if count >= 3 {
            return None;
        }
        parts[count] = piece.parse().ok()?;
        count += 1;
    }
    (count >= 1).then_some((parts, pre))
}

/// `latest` 是否比 `current` 新。预发布版本低于同号正式版；无法解析时按「不更新」处理。
pub fn is_newer(latest: &str, current: &str) -> bool {
    let (Some((latest, latest_pre)), Some((current, current_pre))) =
        (parse_version(latest), parse_version(current))
    else {
        return false;
    };
    match latest.cmp(&current) {
        Ordering::Greater => true,
        Ordering::Less => false,
        Ordering::Equal => current_pre && !latest_pre,
    }
}

pub fn evaluate(payload_tag: &str, current: &str) -> UpdateStatus {
    let latest_version = payload_tag.trim().trim_start_matches(['v', 'V']).to_owned();
    if is_newer(payload_tag, current) {
        UpdateStatus::Available { latest_version }
    } else {
        UpdateStatus::UpToDate { latest_version }
    }
}

/// 请求 GitHub 并记住结果。
pub async fn check(current: &str) -> UpdateStatus {
    let status = fetch(current).await;
    remember(&status);
    let label = match &status {
        UpdateStatus::Available { .. } => "available",
        UpdateStatus::UpToDate { .. } => "up_to_date",
        UpdateStatus::RateLimited => "rate_limited",
        UpdateStatus::Failed => "failed",
        UpdateStatus::Idle => "idle",
    };
    super::log::info("commands", "update_check", &[("result", label)]);
    status
}

async fn fetch(current: &str) -> UpdateStatus {
    let response = http::client()
        .get(LATEST_RELEASE_API)
        .header(reqwest::header::USER_AGENT, "cc-trace-update-check")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await;
    let Ok(response) = response else {
        return UpdateStatus::Failed;
    };
    let code = response.status();
    if code == reqwest::StatusCode::FORBIDDEN || code == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return UpdateStatus::RateLimited;
    }
    if !code.is_success() {
        return UpdateStatus::Failed;
    }
    match response.json::<ReleasePayload>().await {
        // `/releases/latest` 只返回最新的正式版，不含草稿与预发布。
        Ok(payload) => evaluate(&payload.tag_name, current),
        Err(_) => UpdateStatus::Failed,
    }
}

/// 只允许打开本仓库的 Release 页面，前端传不进任意 URL。
pub fn release_url_is_allowed(url: &str) -> bool {
    url.starts_with(RELEASE_URL_PREFIX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.1.13", "0.1.12"));
        assert!(is_newer("0.2.0", "0.1.99"));
        assert!(!is_newer("0.1.12", "0.1.12"));
        assert!(!is_newer("0.1.11", "0.1.12"));
        assert!(is_newer("1.0.0", "1.0.0-beta.1"));
        assert!(!is_newer("1.0.0-beta.1", "1.0.0"));
    }

    #[test]
    fn unparsable_versions_are_not_updates() {
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("0.1.0", "abc"));
    }

    #[test]
    fn evaluate_strips_v_prefix() {
        assert_eq!(
            evaluate("v0.2.0", "0.1.0"),
            UpdateStatus::Available {
                latest_version: "0.2.0".to_owned()
            }
        );
        assert_eq!(
            evaluate("v0.1.0", "0.1.0"),
            UpdateStatus::UpToDate {
                latest_version: "0.1.0".to_owned()
            }
        );
    }

    #[test]
    fn release_url_is_limited_to_repo() {
        assert!(release_url_is_allowed(RELEASES_PAGE));
        assert!(!release_url_is_allowed("https://evil.example/cc-trace/"));
    }
}
