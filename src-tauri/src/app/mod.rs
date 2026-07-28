//! 应用生命周期与用例编排。
//!
//! [`AppCore`] 是设置、Provider 运行时与刷新调度的唯一持有者。所有 command 都经过它，
//! 因此界面永远只有一个状态源：`quota://updated` 与 `quota://refresh-state`。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

#[cfg(debug_assertions)]
use chrono::Duration;
use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter};

use crate::contracts::{
    ProviderId, QuotaState, RefreshState, RefreshStatePayload, Settings, SettingsUpdate,
};
use crate::providers::QuotaProvider;
use crate::providers::claude::ClaudeProvider;
use crate::providers::codex::CodexProvider;
#[cfg(debug_assertions)]
use crate::providers::synthetic::{Scenario, ScenarioHandle, SyntheticProvider};
use crate::scheduler::params::jittered_seconds;
use crate::scheduler::{ProviderRuntime, RefreshDecision, RefreshTrigger};
use crate::storage::{CachedProvider, LoadIssue, QuotaCache, QuotaCacheStore, SettingsStore};

/// 额度数据发生变化。载荷是完整的 [`QuotaState`]，前端不需要自己合并增量。
pub const EVENT_QUOTA_UPDATED: &str = "quota://updated";
/// 某个 Provider 的活动维度发生变化。刷新状态的唯一来源。
pub const EVENT_QUOTA_REFRESH_STATE: &str = "quota://refresh-state";
/// 设置发生变化。
pub const EVENT_SETTINGS_UPDATED: &str = "settings://updated";

/// 预置旧快照时假装它来自多久以前，让 `stale` 场景的「上次成功」时间读起来可信。
#[cfg(debug_assertions)]
const SEEDED_SNAPSHOT_AGE_MINUTES: i64 = 132;

/// 设置写入失败。不携带路径或系统错误原文。
#[derive(Debug, Clone, Copy)]
pub struct SettingsWriteError;

/// 一次设置更新的结果。
pub struct SettingsOutcome {
    pub settings: Settings,
    /// 自动刷新间隔变了，调用方需要重启刷新循环。
    pub schedule_changed: bool,
}

pub struct AppCore {
    store: SettingsStore,
    cache_store: QuotaCacheStore,
    settings: Mutex<Settings>,
    runtimes: Mutex<BTreeMap<ProviderId, ProviderRuntime>>,
    /// 真实额度来源。release 构建里这是唯一的来源。
    providers: BTreeMap<ProviderId, Arc<dyn QuotaProvider>>,
    /// debug 构建的合成来源，只在显式切换到某个验证场景时才被使用。
    #[cfg(debug_assertions)]
    synthetic: BTreeMap<ProviderId, Arc<dyn QuotaProvider>>,
    #[cfg(debug_assertions)]
    scenario: ScenarioHandle,
    /// 每次重启自动刷新循环时自增，旧循环发现代次不符就自行退出。
    schedule_generation: AtomicU64,
}

impl AppCore {
    pub fn new(config_dir: PathBuf) -> (Arc<Self>, Option<LoadIssue>) {
        let store = SettingsStore::new(config_dir.clone());
        let (settings, issue) = store.load();
        let cache_store = QuotaCacheStore::new(config_dir);

        let mut providers: BTreeMap<ProviderId, Arc<dyn QuotaProvider>> = BTreeMap::new();
        providers.insert(ProviderId::Codex, CodexProvider::new());
        providers.insert(ProviderId::Claude, ClaudeProvider::new());

        let mut runtimes = BTreeMap::new();
        for provider in ProviderId::ORDER {
            runtimes.insert(provider, ProviderRuntime::new(provider));
        }
        restore_cached_snapshots(&cache_store, &mut runtimes);

        #[cfg(debug_assertions)]
        let scenario = ScenarioHandle::default();
        #[cfg(debug_assertions)]
        let synthetic = ProviderId::ORDER
            .iter()
            .map(|provider| {
                let source: Arc<dyn QuotaProvider> =
                    Arc::new(SyntheticProvider::new(*provider, scenario.clone()));
                (*provider, source)
            })
            .collect();

        let core = Arc::new(Self {
            store,
            cache_store,
            settings: Mutex::new(settings),
            runtimes: Mutex::new(runtimes),
            providers,
            #[cfg(debug_assertions)]
            synthetic,
            #[cfg(debug_assertions)]
            scenario,
            schedule_generation: AtomicU64::new(0),
        });

        (core, issue)
    }

    /// 本次刷新该用哪个来源。
    ///
    /// release 构建里永远是真实 Provider。debug 构建里只有显式切到某个合成验证场景时
    /// 才用合成来源，切回 [`Scenario::Live`] 立刻恢复真实数据——合成数据不是第二套
    /// 业务状态源，见 [ADR-0009](../../../docs/决策/ADR-0009-合成数据下沉Rust并提前使用正式契约.md)。
    fn source_for(&self, provider: ProviderId) -> Option<Arc<dyn QuotaProvider>> {
        #[cfg(debug_assertions)]
        if self.scenario.get() != Scenario::Live {
            return self.synthetic.get(&provider).cloned();
        }

        self.providers.get(&provider).cloned()
    }

    /// 当前是否在使用真实数据。合成场景下不写额度缓存，避免污染真实快照。
    fn uses_live_data(&self) -> bool {
        #[cfg(debug_assertions)]
        {
            self.scenario.get() == Scenario::Live
        }
        #[cfg(not(debug_assertions))]
        {
            true
        }
    }

    // --- 设置 ---

    pub fn settings(&self) -> Settings {
        self.settings.lock().expect("settings lock").clone()
    }

    /// 合并部分更新并持久化。写入失败时保留原值，由调用方向用户明确提示。
    pub fn update_settings(
        &self,
        update: &SettingsUpdate,
    ) -> Result<SettingsOutcome, SettingsWriteError> {
        let (next, schedule_changed) = {
            let mut settings = self.settings.lock().expect("settings lock");
            let previous_interval = settings.refresh_interval;
            update.apply_to(&mut settings);
            let changed = previous_interval != settings.refresh_interval;
            (settings.clone(), changed)
        };

        self.persist(&next)?;
        if schedule_changed {
            self.schedule_generation.fetch_add(1, Ordering::SeqCst);
        }

        Ok(SettingsOutcome {
            settings: next,
            schedule_changed,
        })
    }

    /// 写入首次启动完成标记。这是唯一的写入入口。
    pub fn complete_onboarding(&self) -> Result<Settings, SettingsWriteError> {
        let next = {
            let mut settings = self.settings.lock().expect("settings lock");
            settings.onboarding.completed = true;
            settings.onboarding.completed_at = Some(Utc::now().to_rfc3339());
            settings.clone()
        };

        self.persist(&next)?;
        Ok(next)
    }

    fn persist(&self, settings: &Settings) -> Result<(), SettingsWriteError> {
        self.store.save(settings).map_err(|_| SettingsWriteError)
    }

    // --- 额度状态 ---

    /// 当前展示状态。读取时顺带检查快照是否已经老到该降级为 `stale`。
    pub fn quota_state(&self) -> QuotaState {
        let interval = self.settings().refresh_interval.seconds();
        let now = Utc::now();
        let mut runtimes = self.runtimes.lock().expect("runtimes lock");

        let providers = ProviderId::ORDER
            .iter()
            .filter_map(|provider| {
                let runtime = runtimes.get_mut(provider)?;
                runtime.expire_if_stale(now, interval);
                Some(runtime.snapshot.clone())
            })
            .collect();

        QuotaState { providers }
    }

    /// 刷新全部 Provider。两个 Provider 互不等待，一个失败不影响另一个。
    pub fn refresh_all(self: &Arc<Self>, app: &AppHandle, trigger: RefreshTrigger) {
        for provider in ProviderId::ORDER {
            self.refresh_provider(app, provider, trigger);
        }
    }

    /// 刷新一个 Provider。并发触发合并到已在飞的任务；退避期内不发起真实请求。
    pub fn refresh_provider(
        self: &Arc<Self>,
        app: &AppHandle,
        provider: ProviderId,
        trigger: RefreshTrigger,
    ) {
        let now = Utc::now();

        let started = {
            let mut runtimes = self.runtimes.lock().expect("runtimes lock");
            let Some(runtime) = runtimes.get_mut(&provider) else {
                return;
            };

            match runtime.decide(trigger, now) {
                RefreshDecision::Start => Some(runtime.begin(trigger, now)),
                RefreshDecision::Merged
                | RefreshDecision::Throttled
                | RefreshDecision::Blocked { .. } => None,
            }
        };

        // 合并、节流与退避都不发起请求，但界面仍要拿到当前的可重试时间。
        let Some(refresh_state) = started else {
            self.emit_quota_state(app);
            return;
        };

        self.emit_refresh_state(app, provider, refresh_state);

        let Some(source) = self.source_for(provider) else {
            return;
        };
        let core = Arc::clone(self);
        let app = app.clone();

        tauri::async_runtime::spawn(async move {
            let outcome = source.fetch().await;

            {
                let mut runtimes = core.runtimes.lock().expect("runtimes lock");
                if let Some(runtime) = runtimes.get_mut(&provider) {
                    runtime.apply(outcome, Utc::now());
                }
            }

            core.persist_cache();
            core.emit_refresh_state(&app, provider, RefreshState::Idle);
            core.emit_quota_state(&app);
        });
    }

    /// 把每个 Provider 的最新有效快照写进缓存，供下次启动先展示后刷新。
    ///
    /// 写入失败不影响本次展示：缓存缺失只是下次启动慢一步，不是错误。
    fn persist_cache(&self) {
        if !self.uses_live_data() {
            return;
        }

        let providers: Vec<CachedProvider> = {
            let runtimes = self.runtimes.lock().expect("runtimes lock");
            ProviderId::ORDER
                .iter()
                .filter_map(|provider| {
                    let runtime = runtimes.get(provider)?;
                    Some(CachedProvider {
                        provider: *provider,
                        identity: runtime.snapshot.identity.clone(),
                        identity_key: runtime.identity_key().map(str::to_owned),
                        snapshot: runtime.snapshot.snapshot.clone()?,
                        last_success_at: runtime.snapshot.last_success_at.clone()?,
                    })
                })
                .collect()
        };

        let _ = self.cache_store.save(&QuotaCache::new(providers));
    }

    // --- 桌面壳验证场景（合成数据） ---

    /// 切换验证场景：重置两个 Provider，按需要预置一份旧快照，再走一次正常刷新。
    ///
    /// 切到 [`Scenario::Live`] 会丢掉合成快照并重新向真实 Provider 取一次数据。
    #[cfg(debug_assertions)]
    pub fn apply_scenario(self: &Arc<Self>, app: &AppHandle, scenario: Scenario) {
        self.scenario.set(scenario);

        {
            let seeded_at = Utc::now() - Duration::minutes(SEEDED_SNAPSHOT_AGE_MINUTES);
            let mut runtimes = self.runtimes.lock().expect("runtimes lock");
            for provider in ProviderId::ORDER {
                let Some(runtime) = runtimes.get_mut(&provider) else {
                    continue;
                };
                runtime.reset();
                if scenario.seeds_snapshot(provider) {
                    runtime.seed_success(
                        SyntheticProvider::seed_snapshot(provider, seeded_at),
                        seeded_at,
                    );
                }
            }
        }

        self.emit_quota_state(app);
        self.refresh_all(app, RefreshTrigger::Startup);
    }

    // --- 事件 ---

    /// 界面与系统区域用同一份状态：菜单栏徽标不是第二个数据源，
    /// 因此它在这里更新，而不是自己订阅事件再算一遍。
    pub fn emit_quota_state(&self, app: &AppHandle) {
        let state = self.quota_state();
        crate::platform::tray::present_quota(app, &state);
        let _ = app.emit(EVENT_QUOTA_UPDATED, state);
    }

    pub fn emit_settings(&self, app: &AppHandle, settings: &Settings) {
        let _ = app.emit(EVENT_SETTINGS_UPDATED, settings);
    }

    fn emit_refresh_state(&self, app: &AppHandle, provider: ProviderId, refresh: RefreshState) {
        let _ = app.emit(
            EVENT_QUOTA_REFRESH_STATE,
            RefreshStatePayload { provider, refresh },
        );
    }
}

/// 用缓存里的最新有效快照预热运行时，让界面在第一次刷新完成前就有内容可看。
///
/// 恢复出来的快照一律是 `stale`：它不是本次会话取得的，见
/// `docs/技术架构.md`「UI 启动顺序」。
fn restore_cached_snapshots(
    cache_store: &QuotaCacheStore,
    runtimes: &mut BTreeMap<ProviderId, ProviderRuntime>,
) {
    let Some(cache) = cache_store.load() else {
        return;
    };

    for cached in cache.providers {
        let Some(runtime) = runtimes.get_mut(&cached.provider) else {
            continue;
        };

        let last_success_at = DateTime::parse_from_rfc3339(&cached.last_success_at)
            .ok()
            .map(|value| value.with_timezone(&Utc));
        runtime.restore_from_cache(
            cached.identity,
            cached.identity_key,
            cached.snapshot,
            last_success_at,
        );
    }
}

/// 为每个 Provider 启动一个独立的自动刷新循环。
///
/// 每个循环各自加抖动，两个 Provider 不会固定同刻请求。间隔变化时旧循环自行退出，
/// 调用方在 `settings_update` 之后重新调用本函数即可。
pub fn start_auto_refresh(core: &Arc<AppCore>, app: &AppHandle) {
    let generation = core.schedule_generation.load(Ordering::SeqCst);

    for (index, provider) in ProviderId::ORDER.iter().copied().enumerate() {
        let core = Arc::clone(core);
        let app = app.clone();

        tauri::async_runtime::spawn(async move {
            let mut tick: u64 = 0;
            loop {
                let interval = core.settings().refresh_interval.seconds();
                let salt = tick
                    .wrapping_mul(31)
                    .wrapping_add(index as u64 * 7)
                    .wrapping_add(interval);

                tokio::time::sleep(StdDuration::from_secs(jittered_seconds(interval, salt))).await;

                if core.schedule_generation.load(Ordering::SeqCst) != generation {
                    break;
                }

                core.refresh_provider(&app, provider, RefreshTrigger::Auto);
                tick = tick.wrapping_add(1);
            }
        });
    }
}
