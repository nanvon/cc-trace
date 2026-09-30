//! 应用生命周期与用例编排。
//!
//! [`AppCore`] 是设置、Provider 运行时与刷新调度的唯一持有者。所有 command 都经过它，
//! 因此界面永远只有一个状态源：`quota://updated` 与 `quota://refresh-state`。

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration as StdDuration;

#[cfg(debug_assertions)]
use chrono::Duration;
use chrono::{DateTime, Utc};
use tauri::{AppHandle, Emitter};

use crate::contracts::{
    ProviderId, QuotaState, QuotaSubject, RefreshState, RefreshStatePayload, ServiceStatusState,
    Settings, SettingsUpdate,
};
use crate::providers::QuotaProvider;
use crate::providers::antigravity::AntigravityProvider;
use crate::providers::claude::ClaudeProvider;
use crate::providers::codex::CodexProvider;
use crate::providers::command_code::CommandCodeProvider;
use crate::providers::cursor::CursorProvider;
#[cfg(debug_assertions)]
use crate::providers::synthetic::{Scenario, ScenarioHandle, SyntheticProvider};
use crate::scheduler::params::jittered_seconds;
use crate::scheduler::{ProviderRuntime, RefreshDecision, RefreshTrigger};
use crate::storage::{
    CachedProvider, ImportedCodexAccount, ImportedCodexStore, LoadIssue, QuotaCache,
    QuotaCacheStore, SettingsStore,
};
use crate::usage::UsageService;

/// 额度数据发生变化。载荷是完整的 [`QuotaState`]，前端不需要自己合并增量。
pub const EVENT_QUOTA_UPDATED: &str = "quota://updated";
/// 某个 Provider 的活动维度发生变化。刷新状态的唯一来源。
pub const EVENT_QUOTA_REFRESH_STATE: &str = "quota://refresh-state";
/// 官方服务状态发生变化。载荷是完整的 [`ServiceStatusState`]；与额度状态是
/// 两条独立状态链，见 [ADR-0026](../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md)。
pub const EVENT_SERVICE_STATUS_UPDATED: &str = "service-status://updated";
/// 设置发生变化。
pub const EVENT_SETTINGS_UPDATED: &str = "settings://updated";

/// 预置旧快照时假装它来自多久以前，让 `stale` 场景的「上次成功」时间读起来可信。
#[cfg(debug_assertions)]
const SEEDED_SNAPSHOT_AGE_MINUTES: i64 = 132;
/// 桌面端没有可靠的跨平台系统唤醒事件；用短周期墙钟检测睡眠造成的时间跳变。
const WAKE_MONITOR_INTERVAL_SECS: u64 = 30;
const WAKE_MONITOR_GAP_SECS: i64 = 45;

/// 设置写入失败。不携带路径或系统错误原文。
#[derive(Debug, Clone, Copy)]
pub struct SettingsWriteError;

/// 一次设置更新的结果。
pub struct SettingsOutcome {
    pub settings: Settings,
    /// 自动刷新间隔变了，调用方需要重启刷新循环。
    pub schedule_changed: bool,
}

/// 一个额度主体的来源与其静态描述。导入账号增删只改这张表。
pub(crate) struct ProviderSlot {
    pub subject: QuotaSubject,
    pub source: Arc<dyn QuotaProvider>,
}

pub struct AppCore {
    store: SettingsStore,
    cache_store: QuotaCacheStore,
    /// 导入的 Codex 副账号：元数据在 JSON，凭据在系统秘密存储。
    imported_codex: Arc<ImportedCodexStore>,
    usage: Arc<UsageService>,
    settings: Arc<Mutex<Settings>>,
    runtimes: Mutex<BTreeMap<String, ProviderRuntime>>,
    /// 真实额度来源。release 构建里这是唯一的来源。键是额度主体标识。
    providers: Mutex<BTreeMap<String, ProviderSlot>>,
    /// debug 构建的合成来源，只在显式切换到某个验证场景时才被使用。
    #[cfg(debug_assertions)]
    synthetic: Mutex<BTreeMap<String, ProviderSlot>>,
    #[cfg(debug_assertions)]
    scenario: ScenarioHandle,
    /// 每次重启自动刷新循环时自增，旧循环发现代次不符就自行退出。
    schedule_generation: AtomicU64,
    /// 官方服务状态的内存快照。公开信息、可随时重建，因此不落盘；
    /// 失败保留上一份值，不清空，见 [ADR-0026](../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md)。
    service_status: Mutex<ServiceStatusState>,
}

impl AppCore {
    pub fn new(config_dir: PathBuf) -> (Arc<Self>, Option<LoadIssue>) {
        let store = SettingsStore::new(config_dir.clone());
        let (settings, issue) = store.load();
        let cache_store = QuotaCacheStore::new(config_dir.clone());
        let imported_codex = Arc::new(ImportedCodexStore::new(config_dir.clone()));
        let usage = UsageService::new(config_dir);

        let settings = Arc::new(Mutex::new(settings));
        let providers = build_slots(&imported_codex, &settings);

        let mut runtimes = BTreeMap::new();
        for subject in providers.values().map(|slot| slot.subject.clone()) {
            runtimes.insert(subject.subject_id.clone(), ProviderRuntime::new(&subject));
        }
        restore_cached_snapshots(&cache_store, &mut runtimes);

        #[cfg(debug_assertions)]
        let scenario = ScenarioHandle::default();
        #[cfg(debug_assertions)]
        let synthetic: BTreeMap<String, ProviderSlot> = ProviderId::ORDER
            .iter()
            .map(|provider| {
                let subject = QuotaSubject::primary(*provider);
                let source: Arc<dyn QuotaProvider> =
                    Arc::new(SyntheticProvider::new(*provider, scenario.clone()));
                (subject.subject_id.clone(), ProviderSlot { subject, source })
            })
            .collect();

        let core = Arc::new(Self {
            store,
            cache_store,
            imported_codex,
            usage,
            settings,
            runtimes: Mutex::new(runtimes),
            providers: Mutex::new(providers),
            #[cfg(debug_assertions)]
            synthetic: Mutex::new(synthetic),
            #[cfg(debug_assertions)]
            scenario,
            schedule_generation: AtomicU64::new(0),
            service_status: Mutex::new(ServiceStatusState::default()),
        });

        (core, issue)
    }

    /// 本次刷新该用哪个来源。
    ///
    /// release 构建里永远是真实 Provider。debug 构建里只有显式切到某个合成验证场景时
    /// 才用合成来源，切回 [`Scenario::Live`] 立刻恢复真实数据——合成数据不是第二套
    /// 业务状态源，见 [ADR-0009](../../../docs/决策/ADR-0009-合成数据下沉Rust并提前使用正式契约.md)。
    fn source_for(&self, subject_id: &str) -> Option<Arc<dyn QuotaProvider>> {
        #[cfg(debug_assertions)]
        if self.scenario.get() != Scenario::Live {
            return self
                .synthetic
                .lock()
                .expect("synthetic lock")
                .get(subject_id)
                .map(|slot| Arc::clone(&slot.source));
        }

        self.providers
            .lock()
            .expect("providers lock")
            .get(subject_id)
            .map(|slot| Arc::clone(&slot.source))
    }

    /// 改 Command Code 的凭据偏好并落盘。命令层在手动 Key 写入／清除后调用，
    /// 保证「凭据变了」与「偏好指向它」是同一个动作。
    pub fn set_command_code_credential_preference(
        &self,
        preference: crate::contracts::CommandCodeCredentialPreference,
    ) -> Result<(), SettingsWriteError> {
        let mut settings = self.settings.lock().expect("settings lock").clone();
        if settings.command_code_credential == preference {
            return Ok(());
        }
        settings.command_code_credential = preference;
        self.persist(&settings)?;
        *self.settings.lock().expect("settings lock") = settings;
        Ok(())
    }

    /// 导入账号的元数据与凭据存储句柄。命令层用它读写凭据槽位。
    pub fn imported_codex_store(&self) -> &Arc<ImportedCodexStore> {
        &self.imported_codex
    }

    /// 导入的 Codex 副账号元数据（不含凭据）。
    pub fn imported_codex_accounts(&self) -> Vec<ImportedCodexAccount> {
        self.imported_codex.load()
    }

    /// 写入导入账号元数据并重建额度来源表。
    ///
    /// 重建保留仍然存在的额度主体的运行时（快照、身份、退避），只丢掉被删除主体的
    /// 运行时，因此增删账号不会让其他账号的展示闪回空态。
    pub fn replace_imported_codex_accounts(
        &self,
        accounts: &[ImportedCodexAccount],
    ) -> Result<(), io::Error> {
        self.imported_codex.save(accounts)?;
        self.reload_providers();
        Ok(())
    }

    /// 按当前 `codex-accounts.json` 重建额度来源表。
    pub fn reload_providers(&self) {
        let rebuilt = build_slots(&self.imported_codex, &self.settings);
        let mut next_runtimes: BTreeMap<String, ProviderRuntime> = BTreeMap::new();
        {
            let runtimes = self.runtimes.lock().expect("runtimes lock");
            for (subject_id, slot) in &rebuilt {
                let runtime = match runtimes.get(subject_id) {
                    Some(existing) => {
                        let mut runtime = existing.clone();
                        // 别名／顺序变了要跟着走，快照与退避不变。
                        runtime.snapshot.label = slot.subject.label.clone();
                        runtime
                    }
                    None => ProviderRuntime::new(&slot.subject),
                };
                next_runtimes.insert(subject_id.clone(), runtime);
            }
        }
        *self.runtimes.lock().expect("runtimes lock") = next_runtimes;
        *self.providers.lock().expect("providers lock") = rebuilt;
        // 缓存按运行时的主体集合重写：被删除账号的历史读数随之消失，
        // 但 `usage.db` 里的额度历史与用量保留，需要时仍能在额度页看到。
        self.persist_cache();
    }

    /// 按 Provider 顺序、主账号在前、导入账号按用户顺序排列的额度主体清单。
    /// 展示、调度与缓存都使用这一个顺序，不在别处再排一次。
    fn ordered_subjects(&self) -> Vec<QuotaSubject> {
        let providers = self.providers.lock().expect("providers lock");
        let mut subjects: Vec<QuotaSubject> = providers
            .values()
            .map(|slot| slot.subject.clone())
            .collect();
        subjects.sort_by(|left, right| {
            provider_rank(left.provider)
                .cmp(&provider_rank(right.provider))
                .then(left.order_index.cmp(&right.order_index))
                .then(left.subject_id.cmp(&right.subject_id))
        });
        subjects
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
            let mut next = settings.clone();
            let previous_interval = settings.refresh_interval;
            update.apply_to(&mut next);
            let changed = previous_interval != next.refresh_interval;

            self.persist(&next)?;
            *settings = next.clone();
            (next, changed)
        };

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
            let mut next = settings.clone();
            next.onboarding.completed = true;
            next.onboarding.completed_at = Some(Utc::now().to_rfc3339());

            self.persist(&next)?;
            *settings = next.clone();
            next
        };

        Ok(next)
    }

    fn persist(&self, settings: &Settings) -> Result<(), SettingsWriteError> {
        self.store.save(settings).map_err(|_| SettingsWriteError)
    }

    // --- 额度状态 ---

    pub fn usage(&self) -> Arc<UsageService> {
        Arc::clone(&self.usage)
    }

    /// 当前展示状态。读取时顺带检查快照是否已经老到该降级为 `stale`。
    pub fn quota_state(&self) -> QuotaState {
        let interval = self.settings().refresh_interval.seconds();
        let now = Utc::now();
        let mut runtimes = self.runtimes.lock().expect("runtimes lock");

        let providers = self
            .ordered_subjects()
            .iter()
            .filter_map(|subject| {
                let runtime = runtimes.get_mut(&subject.subject_id)?;
                runtime.expire_if_stale(now, interval);
                Some(runtime.snapshot.clone())
            })
            .collect();

        QuotaState { providers }
    }

    /// 刷新全部 Provider。两个 Provider 互不等待，一个失败不影响另一个。
    pub fn refresh_all(self: &Arc<Self>, app: &AppHandle, trigger: RefreshTrigger) {
        for subject in self.ordered_subjects() {
            self.refresh_subject(app, &subject.subject_id, trigger);
        }
    }

    /// 系统从睡眠恢复后，只刷新已经超过一倍自动刷新间隔的 Provider。
    ///
    /// 真正发请求仍经过 [`Self::refresh_provider`]，所以在飞请求会合并，退避也不会被绕过。
    pub fn refresh_stale_after_wake(self: &Arc<Self>, app: &AppHandle) {
        let settings = self.settings();
        if !settings.onboarding.completed {
            return;
        }
        let interval = settings.refresh_interval.seconds();
        let now = Utc::now();
        let stale = {
            let mut runtimes = self.runtimes.lock().expect("runtimes lock");
            self.ordered_subjects()
                .iter()
                .map(|subject| subject.subject_id.clone())
                .filter(|subject_id| {
                    let Some(runtime) = runtimes.get_mut(subject_id) else {
                        return false;
                    };
                    let should_refresh = runtime.is_older_than(now, interval);
                    runtime.expire_if_stale(now, interval);
                    should_refresh
                })
                .collect::<Vec<_>>()
        };

        if stale.is_empty() {
            return;
        }

        self.emit_quota_state(app);
        for subject_id in stale {
            self.refresh_subject(app, &subject_id, RefreshTrigger::Startup);
        }
    }

    /// 刷新一个额度主体。并发触发合并到已在飞的任务；退避期内不发起真实请求。
    pub fn refresh_subject(
        self: &Arc<Self>,
        app: &AppHandle,
        subject_id: &str,
        trigger: RefreshTrigger,
    ) {
        let now = Utc::now();
        // 关闭额度轮询的服务不发请求；界面继续展示上一次的快照。
        if let Some(subject) = self
            .ordered_subjects()
            .into_iter()
            .find(|subject| subject.subject_id == subject_id)
            && !self.settings().quota_enabled(subject.provider)
        {
            return;
        }

        let started = {
            let mut runtimes = self.runtimes.lock().expect("runtimes lock");
            let Some(runtime) = runtimes.get_mut(subject_id) else {
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

        // `begin` 同时清除上一次的可用性错误与重试时间，必须先广播完整三维状态；
        // 否则现有窗口和系统区域会在请求期间继续展示旧错误。
        self.emit_quota_state(app);
        self.emit_refresh_state(app, subject_id, refresh_state);

        let Some(source) = self.source_for(subject_id) else {
            return;
        };
        let subject_id = subject_id.to_owned();
        let core = Arc::clone(self);
        let app = app.clone();

        tauri::async_runtime::spawn(async move {
            let outcome = source.fetch().await;
            let quota_event = if core.uses_live_data() {
                match &outcome {
                    crate::providers::ProviderFetchOutcome::Success {
                        identity_key: Some(identity_key),
                        snapshot,
                        ..
                    } => Some((identity_key.clone(), snapshot.clone())),
                    _ => None,
                }
            } else {
                None
            };

            let mut provider = None;
            {
                let mut runtimes = core.runtimes.lock().expect("runtimes lock");
                if let Some(runtime) = runtimes.get_mut(&subject_id) {
                    provider = Some(runtime.snapshot.provider);
                    runtime.apply(outcome, Utc::now());
                }
            }

            if let (Some((identity_key, snapshot)), Some(provider)) = (quota_event, provider) {
                core.usage
                    .record_quota_snapshot(provider, &identity_key, &snapshot);
            }
            core.persist_cache();
            core.emit_refresh_state(&app, &subject_id, RefreshState::Idle);
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
            self.ordered_subjects()
                .iter()
                .filter_map(|subject| {
                    let runtime = runtimes.get(&subject.subject_id)?;
                    Some(CachedProvider {
                        subject_id: subject.subject_id.clone(),
                        provider: subject.provider,
                        kind: subject.kind,
                        label: subject.label.clone(),
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
                let Some(runtime) = runtimes.get_mut(provider.key()) else {
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

    // --- 官方服务状态（Statuspage 状态链） ---

    /// 当前服务状态快照。
    pub fn service_status(&self) -> ServiceStatusState {
        self.service_status
            .lock()
            .expect("service status lock")
            .clone()
    }

    /// 拉取 OpenAI／Anthropic 官方状态页。两个请求并发，互不影响；
    /// 单个失败保留上一份内存值，不清空。
    pub fn refresh_service_status(self: &Arc<Self>, app: &AppHandle) {
        let core = Arc::clone(self);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            // 三条状态链并发发起；单请求超时互不影响（共享 HTTP Client、15 秒超时不变）。
            let (codex, claude, cursor) = tokio::join!(
                crate::providers::service_status::fetch_status(
                    crate::providers::service_status::OPENAI_STATUS_URL,
                ),
                crate::providers::service_status::fetch_status(
                    crate::providers::service_status::ANTHROPIC_STATUS_URL,
                ),
                crate::providers::service_status::fetch_status(
                    crate::providers::service_status::CURSOR_STATUS_URL,
                ),
            );

            {
                let mut state = core.service_status.lock().expect("service status lock");
                if let Ok(status) = codex {
                    state.set(ProviderId::Codex, status);
                }
                if let Ok(status) = claude {
                    state.set(ProviderId::Claude, status);
                }
                if let Ok(status) = cursor {
                    state.set(ProviderId::Cursor, status);
                }
            }

            core.emit_service_status(&app);
        });
    }

    // --- 事件 ---

    /// 界面与系统区域用同一份状态：菜单栏徽标不是第二个数据源，
    /// 因此它在这里更新，而不是自己订阅事件再算一遍。
    pub fn emit_quota_state(&self, app: &AppHandle) {
        let state = self.quota_state();
        let settings = self.settings();
        let menu_bar: Vec<ProviderId> = ProviderId::ORDER
            .iter()
            .copied()
            .filter(|provider| settings.services.get(*provider).menu_bar)
            .collect();
        crate::platform::tray::present_quota(app, &state, &menu_bar);
        let _ = app.emit(EVENT_QUOTA_UPDATED, state);
    }

    pub fn emit_settings(&self, app: &AppHandle, settings: &Settings) {
        let _ = app.emit(EVENT_SETTINGS_UPDATED, settings);
    }

    pub fn emit_service_status(&self, app: &AppHandle) {
        let _ = app.emit(EVENT_SERVICE_STATUS_UPDATED, self.service_status());
    }

    fn emit_refresh_state(&self, app: &AppHandle, subject_id: &str, refresh: RefreshState) {
        let provider = self
            .runtimes
            .lock()
            .expect("runtimes lock")
            .get(subject_id)
            .map(|runtime| runtime.snapshot.provider);
        let Some(provider) = provider else {
            return;
        };
        let _ = app.emit(
            EVENT_QUOTA_REFRESH_STATE,
            RefreshStatePayload {
                subject_id: subject_id.to_owned(),
                provider,
                refresh,
            },
        );
    }
}

/// 用缓存里的最新有效快照预热运行时，让界面在第一次刷新完成前就有内容可看。
///
/// 恢复出来的快照一律是 `stale`：它不是本次会话取得的，见
/// `docs/技术架构.md`「UI 启动顺序」。
fn restore_cached_snapshots(
    cache_store: &QuotaCacheStore,
    runtimes: &mut BTreeMap<String, ProviderRuntime>,
) {
    let Some(cache) = cache_store.load() else {
        return;
    };

    for cached in cache.providers {
        let Some(runtime) = runtimes.get_mut(&cached.subject_id) else {
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

    {
        let core = Arc::clone(core);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut previous_tick = Utc::now();
            loop {
                tokio::time::sleep(StdDuration::from_secs(WAKE_MONITOR_INTERVAL_SECS)).await;

                if core.schedule_generation.load(Ordering::SeqCst) != generation {
                    break;
                }

                let now = Utc::now();
                if (now - previous_tick).num_seconds() > WAKE_MONITOR_GAP_SECS {
                    core.refresh_stale_after_wake(&app);
                }
                previous_tick = now;
            }
        });
    }

    for (index, subject) in core.ordered_subjects().into_iter().enumerate() {
        let core = Arc::clone(core);
        let app = app.clone();
        let subject_id = subject.subject_id.clone();

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

                core.refresh_subject(&app, &subject_id, RefreshTrigger::Auto);
                tick = tick.wrapping_add(1);
            }
        });
    }
}

/// 每 5 分钟发起一次本地 Token 用量增量扫描。
///
/// 启动调度后立即执行首次扫描，之后每隔一个完整间隔执行；扫描已经运行时
/// `start_default_scan` 返回 busy，本轮直接跳过，不排队制造紧接着的第二次扫描。
pub fn start_auto_usage_scan(core: &Arc<AppCore>) {
    // 先同步把状态切到 running，再启动周期任务。扫描本身仍在独立阻塞线程执行；
    // 这里只消除 WebView 首次读取状态时仍看到 idle 的启动竞态。
    let _ = core.usage.start_default_scan();

    let core = Arc::clone(core);
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(StdDuration::from_secs(
                crate::scheduler::params::USAGE_SCAN_INTERVAL_SECS,
            ))
            .await;
            let _ = core.usage.start_default_scan();
            // 远端计量有自己的一套节流与退避（5 分钟最小间隔、429 后 10 分钟退避），
            // 因此跟着扫描循环走不会把请求打密：它自己决定这一轮发不发。
            let _ = core.usage.refresh_cursor_remote(false).await;
        }
    });
}

/// 每 5 分钟拉一次官方服务状态，启动后立即拉一次。
///
/// 固定间隔、不加抖动（公开只读端点，不与额度刷新争峰）；失败保留旧值，
/// 不进入退避体系，见 [ADR-0026](../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md)。
pub fn start_auto_service_status(core: &Arc<AppCore>, app: &AppHandle) {
    core.refresh_service_status(app);

    let core = Arc::clone(core);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(StdDuration::from_secs(
                crate::scheduler::params::SERVICE_STATUS_INTERVAL_SECS,
            ))
            .await;
            core.refresh_service_status(&app);
        }
    });
}

/// Provider 在固定展示顺序里的位置。
fn provider_rank(provider: ProviderId) -> usize {
    ProviderId::ORDER
        .iter()
        .position(|candidate| *candidate == provider)
        .unwrap_or(usize::MAX)
}

/// 真实额度来源的全部主体：每个 Provider 的主账号，外加用户导入的 Codex 副账号。
///
/// 这是唯一一处构造额度来源的地方；新增 Provider 在这里加一行，顺序由
/// [`ProviderId::ORDER`] 与 [`QuotaSubject::order_index`] 决定，界面与调度不再各自排序。
fn build_slots(
    imported_codex: &Arc<ImportedCodexStore>,
    settings: &Arc<Mutex<Settings>>,
) -> BTreeMap<String, ProviderSlot> {
    let mut slots = BTreeMap::new();
    let mut insert = |subject: QuotaSubject, source: Arc<dyn QuotaProvider>| {
        slots.insert(subject.subject_id.clone(), ProviderSlot { subject, source });
    };

    insert(
        QuotaSubject::primary(ProviderId::Codex),
        CodexProvider::new(),
    );
    insert(
        QuotaSubject::primary(ProviderId::Claude),
        ClaudeProvider::new(),
    );
    insert(
        QuotaSubject::primary(ProviderId::Antigravity),
        AntigravityProvider::new(),
    );
    insert(
        QuotaSubject::primary(ProviderId::Cursor),
        CursorProvider::new(),
    );
    insert(
        QuotaSubject::primary(ProviderId::CommandCode),
        CommandCodeProvider::new(Arc::clone(settings)),
    );

    for (index, account) in imported_codex.load().iter().enumerate() {
        let identity = account.identity_hash();
        let order = u32::try_from(index).unwrap_or(u32::MAX);
        let subject = QuotaSubject::imported_codex(
            &identity,
            Some(account.display_name(&default_account_name(index))),
            order,
        );
        let source = CodexProvider::imported(
            identity,
            // PAT 不发 `ChatGPT-Account-Id`，见 `credentials::codex::parse_imported`。
            (!account.personal_access_token).then(|| account.chatgpt_account_id().to_owned()),
            Arc::clone(imported_codex),
        );
        insert(subject, source);
    }

    slots
}

/// 导入账号没有别名也没有邮箱时的兜底展示名，只用于界面。
fn default_account_name(index: usize) -> String {
    format!("Codex 账号 {}", index + 1)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::contracts::RefreshInterval;

    fn core_with_unwritable_config_path() -> (tempfile::TempDir, Arc<AppCore>) {
        let dir = tempfile::tempdir().expect("temp dir");
        let config_path = dir.path().join("not-a-directory");
        fs::write(&config_path, "blocks directory creation").expect("seed blocking file");
        let (core, _) = AppCore::new(config_path);
        (dir, core)
    }

    #[test]
    fn failed_settings_write_does_not_commit_the_in_memory_update() {
        let (_dir, core) = core_with_unwritable_config_path();
        let original = core.settings();
        let update = SettingsUpdate {
            refresh_interval: Some(RefreshInterval::OneMinute),
            ..SettingsUpdate::default()
        };

        assert!(core.update_settings(&update).is_err());
        assert_eq!(core.settings(), original);
    }

    #[test]
    fn failed_onboarding_write_keeps_onboarding_incomplete() {
        let (_dir, core) = core_with_unwritable_config_path();

        assert!(core.complete_onboarding().is_err());
        assert!(!core.settings().onboarding.completed);
        assert!(core.settings().onboarding.completed_at.is_none());
    }
}
