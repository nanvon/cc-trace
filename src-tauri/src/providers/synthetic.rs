//! 桌面壳阶段的合成额度来源。
//!
//! 它只服务于验证：让 Tray、窗口、状态矩阵、退避与刷新编排可以在没有任何凭据和网络的
//! 情况下走通。它**不是第二套业务状态源**——所有数据都经过与真实 Provider 相同的
//! [`ProviderFetchOutcome`] 与 `crate::contracts`，第 12 阶段整份文件被真实实现替换。
//!
//! 这里不读取任何凭据文件、不发起任何网络请求、不接触 Swift 版 cc-bar 的数据。

use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::{BoxFuture, ProviderFetchOutcome, QuotaProvider};
use crate::contracts::{
    ErrorKind, ProviderId, ProviderIdentity, QuotaSnapshot, QuotaWindow, QuotaWindowKind,
};

/// 合成请求的模拟耗时。足够看清刷新状态的进入与退出，又不至于让走查变慢。
const SYNTHETIC_LATENCY: StdDuration = StdDuration::from_millis(700);

/// 可切换的验证场景，覆盖 `docs/设计方向与状态规范.md` 第 8 节状态视觉矩阵的每一行，
/// 并额外覆盖「一个 Provider 失败不影响另一个」的故障隔离要求。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scenario {
    /// 使用真实 Provider，不产生任何合成数据。这是默认值：debug 构建启动时也走真实闭环，
    /// 合成场景只有显式切换才生效。
    #[default]
    Live,
    /// 两个 Provider 都刷新成功。
    Healthy,
    /// 清空快照后重新加载，用于 `loading + empty`。
    FirstLoad,
    /// 两个 Provider 都没有可用凭据。
    NoCredentials,
    /// Codex 凭据形式不受支持，Claude Code 正常。
    Unsupported,
    /// 两个 Provider 离线，但都保留着上一份快照。
    OfflineStale,
    /// 两个 Provider 离线且没有任何快照。
    OfflineEmpty,
    /// Codex 触发 429 并保留旧快照，Claude Code 正常——故障隔离。
    RateLimited,
    /// Codex 凭据失效并保留旧快照，Claude Code 正常。
    ErrorStale,
    /// Codex 协议错误且没有快照，Claude Code 正常。
    ErrorEmpty,
}

impl Scenario {
    /// 切换到该场景时，这个 Provider 是否需要预置一份「上一次成功」的快照。
    ///
    /// `stale` 分支必须先有旧快照才能验证「保留数值、只改新鲜度」。
    pub fn seeds_snapshot(self, provider: ProviderId) -> bool {
        match self {
            Self::OfflineStale => true,
            Self::RateLimited | Self::ErrorStale => provider == ProviderId::Codex,
            _ => false,
        }
    }

    /// 该场景下这个 Provider 的请求结局。
    fn outcome_for(self, provider: ProviderId, now: DateTime<Utc>) -> ProviderFetchOutcome {
        match (self, provider) {
            // `Live` 下 `AppCore` 根本不会选中合成来源；这里只是防御性兜底，
            // 保证即便被误调用也不会 panic，也不会产生看起来像故障的数据。
            (Self::Live | Self::Healthy | Self::FirstLoad, _) => success(provider, now),

            (Self::NoCredentials, _) => ProviderFetchOutcome::NoCredentials,

            (Self::Unsupported, ProviderId::Codex) => ProviderFetchOutcome::Unsupported,
            (Self::Unsupported, ProviderId::Claude) => success(provider, now),

            (Self::OfflineStale | Self::OfflineEmpty, _) => ProviderFetchOutcome::Offline,

            (Self::RateLimited, ProviderId::Codex) => ProviderFetchOutcome::RateLimited {
                retry_after: Some(Duration::minutes(12)),
            },
            (Self::RateLimited, ProviderId::Claude) => success(provider, now),

            (Self::ErrorStale, ProviderId::Codex) => ProviderFetchOutcome::Failed {
                kind: ErrorKind::Credentials,
            },
            (Self::ErrorStale, ProviderId::Claude) => success(provider, now),

            (Self::ErrorEmpty, ProviderId::Codex) => ProviderFetchOutcome::Failed {
                kind: ErrorKind::Protocol,
            },
            (Self::ErrorEmpty, ProviderId::Claude) => success(provider, now),
        }
    }
}

/// 当前选中的场景。由 dev 专用命令切换，release 构建里始终是默认值。
#[derive(Debug, Clone, Default)]
pub struct ScenarioHandle(Arc<Mutex<Scenario>>);

impl ScenarioHandle {
    pub fn get(&self) -> Scenario {
        *self.0.lock().expect("scenario lock is never poisoned")
    }

    pub fn set(&self, scenario: Scenario) {
        *self.0.lock().expect("scenario lock is never poisoned") = scenario;
    }
}

/// 一个 Provider 的合成实现。
pub struct SyntheticProvider {
    id: ProviderId,
    scenario: ScenarioHandle,
}

impl SyntheticProvider {
    pub fn new(id: ProviderId, scenario: ScenarioHandle) -> Self {
        Self { id, scenario }
    }

    /// 直接产出一份成功快照，用于为 `stale` 场景预置「上一次成功」的数据。
    pub fn seed_snapshot(id: ProviderId, now: DateTime<Utc>) -> ProviderFetchOutcome {
        success(id, now)
    }
}

impl QuotaProvider for SyntheticProvider {
    fn id(&self) -> ProviderId {
        self.id
    }

    fn fetch(&self) -> BoxFuture<'_, ProviderFetchOutcome> {
        Box::pin(async move {
            tokio::time::sleep(SYNTHETIC_LATENCY).await;
            self.scenario.get().outcome_for(self.id, Utc::now())
        })
    }
}

fn success(provider: ProviderId, now: DateTime<Utc>) -> ProviderFetchOutcome {
    let (identity, windows) = match provider {
        ProviderId::Codex => (
            ProviderIdentity {
                account: Some("demo@example.com".to_owned()),
                plan: Some("Plus".to_owned()),
            },
            vec![
                window(
                    "codex.primary",
                    QuotaWindowKind::FiveHour,
                    None,
                    27.0,
                    Some(now + Duration::minutes(252)),
                    Some(18_000),
                    true,
                ),
                window(
                    "codex.secondary",
                    QuotaWindowKind::Weekly,
                    None,
                    59.0,
                    Some(now + Duration::days(3)),
                    Some(604_800),
                    false,
                ),
            ],
        ),
        ProviderId::Claude => (
            ProviderIdentity {
                account: Some("demo@example.com".to_owned()),
                plan: Some("Max".to_owned()),
            },
            vec![
                window(
                    "claude.session",
                    QuotaWindowKind::FiveHour,
                    None,
                    62.0,
                    Some(now + Duration::minutes(125)),
                    Some(18_000),
                    true,
                ),
                window(
                    "claude.weekly-all",
                    QuotaWindowKind::Weekly,
                    None,
                    38.0,
                    Some(now + Duration::days(4)),
                    Some(604_800),
                    false,
                ),
                window(
                    "claude.weekly-opus",
                    QuotaWindowKind::ModelWeekly,
                    Some("Opus"),
                    12.0,
                    Some(now + Duration::days(4)),
                    Some(604_800),
                    false,
                ),
            ],
        ),
    };

    ProviderFetchOutcome::Success {
        identity: Some(identity),
        // 合成来源没有真实身份，指纹固定，因此场景切换不会触发身份变化丢弃。
        identity_key: Some(format!("synthetic:{provider:?}")),
        snapshot: QuotaSnapshot {
            windows,
            captured_at: now.to_rfc3339(),
        },
    }
}

fn window(
    id: &str,
    kind: QuotaWindowKind,
    display_name: Option<&str>,
    used_percent: f64,
    resets_at: Option<DateTime<Utc>>,
    window_seconds: Option<u64>,
    is_primary: bool,
) -> QuotaWindow {
    QuotaWindow {
        id: id.to_owned(),
        kind,
        display_name: display_name.map(str::to_owned),
        used_percent,
        remaining_percent: QuotaWindow::normalized_remaining(used_percent),
        resets_at: resets_at.map(|value| value.to_rfc3339()),
        window_seconds,
        is_active: true,
        is_primary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_700_000_000, 0).expect("valid timestamp")
    }

    #[test]
    fn every_scenario_resolves_both_providers_without_a_catch_all() {
        for scenario in [
            Scenario::Healthy,
            Scenario::FirstLoad,
            Scenario::NoCredentials,
            Scenario::Unsupported,
            Scenario::OfflineStale,
            Scenario::OfflineEmpty,
            Scenario::RateLimited,
            Scenario::ErrorStale,
            Scenario::ErrorEmpty,
        ] {
            for provider in ProviderId::ORDER {
                let outcome = scenario.outcome_for(provider, now());
                if let ProviderFetchOutcome::Success { snapshot, .. } = &outcome {
                    assert!(
                        snapshot
                            .windows
                            .first()
                            .is_some_and(|window| window.is_primary),
                        "{scenario:?}/{provider:?} must mark the first window as primary"
                    );
                }
            }
        }
    }

    #[test]
    fn single_provider_failures_leave_the_other_provider_healthy() {
        for scenario in [
            Scenario::RateLimited,
            Scenario::ErrorStale,
            Scenario::ErrorEmpty,
            Scenario::Unsupported,
        ] {
            assert!(
                matches!(
                    scenario.outcome_for(ProviderId::Claude, now()),
                    ProviderFetchOutcome::Success { .. }
                ),
                "{scenario:?} must keep Claude Code succeeding to prove fault isolation"
            );
            assert!(
                !matches!(
                    scenario.outcome_for(ProviderId::Codex, now()),
                    ProviderFetchOutcome::Success { .. }
                ),
                "{scenario:?} must fail Codex"
            );
        }
    }

    #[test]
    fn stale_scenarios_seed_a_previous_snapshot() {
        assert!(Scenario::OfflineStale.seeds_snapshot(ProviderId::Codex));
        assert!(Scenario::OfflineStale.seeds_snapshot(ProviderId::Claude));
        assert!(Scenario::RateLimited.seeds_snapshot(ProviderId::Codex));
        assert!(!Scenario::RateLimited.seeds_snapshot(ProviderId::Claude));
        assert!(!Scenario::OfflineEmpty.seeds_snapshot(ProviderId::Codex));
        assert!(!Scenario::ErrorEmpty.seeds_snapshot(ProviderId::Codex));
    }

    #[test]
    fn remaining_percent_is_normalized_at_the_source() {
        let ProviderFetchOutcome::Success { snapshot, .. } = success(ProviderId::Codex, now())
        else {
            panic!("codex synthesizes a successful snapshot");
        };

        let primary = snapshot.windows.first().expect("primary window");
        assert_eq!(primary.remaining_percent, 73.0);
        assert_eq!(primary.kind, QuotaWindowKind::FiveHour);
    }
}
