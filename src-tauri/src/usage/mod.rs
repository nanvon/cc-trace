//! Codex／Claude Code 本地 JSONL 的只读索引。
//!
//! 扫描只读外部文件，所有派生数据写入 CC Trace 自己的 SQLite。command 只接收固定
//! 查询参数，不接收路径；测试通过显式临时根目录覆盖，避免触碰真实用户数据。

pub(crate) mod cursor_remote;
mod dsh;
mod dsh_zstd;
pub(crate) mod model;
mod opencode;
mod parser;
pub mod pricing;
mod pricing_remote;
mod title_index;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Local, SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::{
    PricingCatalogRefreshStatus, ProviderId, QuotaHistory, QuotaHistoryQuery, QuotaSnapshot,
    UsageConversation, UsageConversationBreakdown, UsageConversationPage,
    UsageConversationProjectOption, UsageConversationQuery, UsageFilter, UsageRepriceResult,
    UsageScanState, UsageScanStatus, UsageSource, UsageSummary, UsageSummaryQuery,
};
#[cfg(feature = "perf-baseline")]
use crate::storage::PerfStats;
use crate::storage::{CursorRemoteBucket, DshSessionUpsert, UsageDb, UsageDbError};

use model::{ClaudeCursor, CodexCursor, ConversationFact, ParsedLine, PiCursor, ScanBatch};
use parser::{parse_claude_line, parse_codex_line, parse_pi_line};
use pricing::{
    PricingCatalog, PricingCatalogStore, PricingRefreshMode, PricingRefreshOutcome, PricingUsageKey,
};
use title_index::TitleIndex;

const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const BATCH_LINES: u64 = 2_000;
const BATCH_BYTES: u64 = 8 * 1024 * 1024;
const PREFIX_BYTES: u64 = 4_096;
/// 远端计量的最小间隔：Dashboard 对频率敏感，不跟随本地扫描节奏（默认 5 分钟）。
const CURSOR_REMOTE_MIN_INTERVAL_MINUTES: i64 = 5;
/// 被限流后的退避。
const CURSOR_REMOTE_RATE_LIMIT_BACKOFF_MINUTES: i64 = 10;
/// 可重试失败后的退避。
const CURSOR_REMOTE_ERROR_BACKOFF_MINUTES: i64 = 5;
/// 不可重试失败（结构不符、范围非法）后的退避：立刻重试只会拿到同一个结果。
const CURSOR_REMOTE_PERMANENT_BACKOFF_MINUTES: i64 = 60;
/// DSH 父链解析的深度上限。超出即自认根：宁可少归一条，也不做无界遍历。
const DSH_ROOT_MAX_DEPTH: usize = 32;
const DEFAULT_LIMIT: u32 = 50;
const MAX_LIMIT: u32 = 200;
const MAX_FILTER_LENGTH: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageError {
    InvalidQuery,
    Unavailable,
    ScanBusy,
}

impl From<UsageDbError> for UsageError {
    fn from(_: UsageDbError) -> Self {
        Self::Unavailable
    }
}

pub struct UsageService {
    db: UsageDb,
    pricing: PricingCatalogStore,
    status: Mutex<UsageScanStatus>,
    /// Cursor 远端计量的节流与退避状态。远端拉取不参与本地扫描的水位。
    cursor_remote: Mutex<CursorRemoteState>,
    /// 串行化“开始扫描”与“提交价格 + 数据库重计价”，保证两者不会越过安全边界。
    lifecycle: Mutex<()>,
    reprice_pending: AtomicBool,
    cancel: AtomicBool,
}

/// 账号哈希：覆盖表与到条目键都用它，**不落账号明文**。
fn cursor_account_hash(
    credentials: &crate::providers::credentials::cursor::CursorCredentials,
) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(b"cc-trace-cursor-account-v1");
    hasher.update(
        credentials
            .account_key()
            .map(|key| key.expose().to_owned())
            .unwrap_or_else(|| credentials.subject.clone())
            .as_bytes(),
    );
    hasher
        .finalize()
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 取数错误的稳定标签。日志与状态里只留标签与原因分类，不带响应原文。
fn error_label(error: &cursor_remote::CursorFetchError) -> &'static str {
    match error {
        cursor_remote::CursorFetchError::InvalidRange => "invalid-range",
        cursor_remote::CursorFetchError::Transport => "transport",
        cursor_remote::CursorFetchError::Http(_) => "http",
        cursor_remote::CursorFetchError::InvalidPage => "invalid-page",
        cursor_remote::CursorFetchError::PaginationInconsistent => "pagination-inconsistent",
        cursor_remote::CursorFetchError::PageLimitReached => "page-limit",
        cursor_remote::CursorFetchError::SingleDayTooDense => "single-day-too-dense",
        cursor_remote::CursorFetchError::NumericOverflow => "numeric-overflow",
    }
}

/// Cursor 远端计量的对外状态。界面用它说明「为什么现在没有远端数据」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorRemoteStatus {
    pub last_success_at: Option<String>,
    pub last_error: Option<String>,
    /// 退避期内不再发请求的时刻。
    pub retry_after: Option<String>,
    /// 当前账号已经拉全的自然日数。
    pub covered_days: u64,
}

#[derive(Debug, Clone, Default)]
struct CursorRemoteState {
    last_attempt_at: Option<DateTime<Utc>>,
    last_success_at: Option<DateTime<Utc>>,
    retry_after: Option<DateTime<Utc>>,
    last_error: Option<String>,
    /// 上一次拉取用的账号哈希：变了就清掉旧账号的远端账，不把两个账号混在一起。
    account_hash: Option<String>,
}

/// 一次远端刷新的结局。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CursorRemoteOutcome {
    /// 没有可用的 Cursor 登录态：远端计量整体跳过，不是错误。
    NoCredential,
    /// 距上次尝试太近，或还在退避期内。
    Throttled,
    /// 拉取成功（可能只是补齐了部分日子）。
    Updated { ranges: usize, buckets: usize },
    /// 拉取失败：保留上一次的远端数据，等退避结束再试。
    Failed { error: String, retry_after: String },
}

impl UsageService {
    /// 远端计量的状态快照。
    pub fn cursor_remote_status(&self) -> CursorRemoteStatus {
        let state = self.cursor_remote.lock().expect("cursor remote lock");
        let covered_days = state
            .account_hash
            .as_deref()
            .and_then(|account| self.db.cursor_coverage(account).ok())
            .map(|days| days.len() as u64)
            .unwrap_or(0);
        CursorRemoteStatus {
            last_success_at: state
                .last_success_at
                .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true)),
            last_error: state.last_error.clone(),
            retry_after: state
                .retry_after
                .map(|time| time.to_rfc3339_opts(SecondsFormat::Secs, true)),
            covered_days,
        }
    }

    /// 拉取 Cursor 远端计费用量。
    ///
    /// 自己的节流与退避在这里定：远端接口对频率敏感，**不跟随本地扫描节奏**。
    /// `force` 只用于用户手动刷新，仍然绕过不了 429 之后的那段退避。
    pub async fn refresh_cursor_remote(self: &Arc<Self>, force: bool) -> CursorRemoteOutcome {
        let now = Utc::now();
        {
            let state = self.cursor_remote.lock().expect("cursor remote lock");
            if let Some(retry_after) = state.retry_after
                && retry_after > now
            {
                return CursorRemoteOutcome::Throttled;
            }
            if !force
                && state.last_attempt_at.is_some_and(|last| {
                    (now - last).num_minutes() < CURSOR_REMOTE_MIN_INTERVAL_MINUTES
                })
            {
                return CursorRemoteOutcome::Throttled;
            }
        }

        let credentials = match crate::providers::credentials::cursor::discover() {
            crate::providers::credentials::Discovery::Found(credentials) => credentials,
            _ => return CursorRemoteOutcome::NoCredential,
        };
        let account_hash = cursor_account_hash(&credentials);
        self.cursor_remote
            .lock()
            .expect("cursor remote lock")
            .last_attempt_at = Some(now);

        // 换账号：旧账号的远端账整份清掉，覆盖状态也清掉，否则两边会混成一张账。
        let previous_accounts = self.db.cursor_coverage_accounts().unwrap_or_default();
        if previous_accounts
            .iter()
            .any(|account| account != &account_hash)
        {
            for account in previous_accounts {
                if account != account_hash {
                    let _ = self.db.cursor_reset_account(&account);
                }
            }
        }

        let coverage: cursor_remote::CursorCoverage = self
            .db
            .cursor_coverage(&account_hash)
            .unwrap_or_default()
            .iter()
            .filter_map(|day| chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d").ok())
            .collect();
        let today = Local::now().date_naive();
        let ranges = cursor_remote::plan_fetch_ranges(today, &coverage, None);
        if ranges.is_empty() {
            self.mark_cursor_success(now, Some(account_hash));
            return CursorRemoteOutcome::Updated {
                ranges: 0,
                buckets: 0,
            };
        }

        let cookie = credentials.cookie_header();
        let mut total_buckets = 0_usize;
        let mut completed = 0_usize;
        for (from_ms, to_ms) in &ranges {
            match cursor_remote::fetch_range(&cookie, *from_ms, *to_ms).await {
                Ok(events) => {
                    let buckets = match cursor_remote::make_buckets(&events) {
                        Ok(buckets) => buckets,
                        Err(error) => return self.fail_cursor_remote(&error, now),
                    };
                    let Some(from_day) = cursor_remote::local_day_of(*from_ms) else {
                        return self.fail_cursor_remote(
                            &cursor_remote::CursorFetchError::InvalidRange,
                            now,
                        );
                    };
                    let Some(to_day) = cursor_remote::local_day_of(to_ms - 1) else {
                        return self.fail_cursor_remote(
                            &cursor_remote::CursorFetchError::InvalidRange,
                            now,
                        );
                    };
                    let rows: Vec<CursorRemoteBucket> = buckets
                        .iter()
                        .map(|bucket| CursorRemoteBucket {
                            day_local: bucket.day_local.clone(),
                            model: bucket.model.clone(),
                            input_tokens: bucket.input_tokens,
                            output_tokens: bucket.output_tokens,
                            cache_read_tokens: bucket.cache_read_tokens,
                            cache_write_tokens: bucket.cache_write_tokens,
                            charged_nanos: bucket.charged_nanos,
                            request_count: bucket.request_count,
                        })
                        .collect();
                    let conversation_key = format!("cursor:{account_hash}");
                    total_buckets += rows.len();
                    if let Err(_error) = self.db.cursor_replace_days(
                        &account_hash,
                        &from_day.format("%Y-%m-%d").to_string(),
                        &to_day.format("%Y-%m-%d").to_string(),
                        &conversation_key,
                        &rows,
                    ) {
                        return self.fail_cursor_remote(
                            &cursor_remote::CursorFetchError::InvalidPage,
                            now,
                        );
                    }
                    completed += 1;
                }
                Err(error) => return self.fail_cursor_remote(&error, now),
            }
        }

        self.mark_cursor_success(now, Some(account_hash));
        CursorRemoteOutcome::Updated {
            ranges: completed,
            buckets: total_buckets,
        }
    }

    fn mark_cursor_success(&self, now: DateTime<Utc>, account_hash: Option<String>) {
        let mut state = self.cursor_remote.lock().expect("cursor remote lock");
        state.last_success_at = Some(now);
        state.retry_after = None;
        state.last_error = None;
        if account_hash.is_some() {
            state.account_hash = account_hash;
        }
    }

    /// 失败时记退避。退避长度按错误的**性质**分档，不按文案比字符串：
    /// 被限流最久（10 分钟，远端最敏感），可重试的（网络、5xx、分页不一致）中等（5 分钟），
    /// 其余（结构不符、范围非法）更久（60 分钟）——立刻重试只会拿到同一个结果。
    fn fail_cursor_remote(
        self: &Arc<Self>,
        error: &cursor_remote::CursorFetchError,
        now: DateTime<Utc>,
    ) -> CursorRemoteOutcome {
        let backoff = if error.is_rate_limited() {
            CURSOR_REMOTE_RATE_LIMIT_BACKOFF_MINUTES
        } else if error.is_retryable() {
            CURSOR_REMOTE_ERROR_BACKOFF_MINUTES
        } else {
            CURSOR_REMOTE_PERMANENT_BACKOFF_MINUTES
        };
        let label = error_label(error);
        let retry_after = now + chrono::Duration::minutes(backoff);
        {
            let mut state = self.cursor_remote.lock().expect("cursor remote lock");
            state.last_error = Some(label.to_owned());
            state.retry_after = Some(retry_after);
        }
        CursorRemoteOutcome::Failed {
            error: label.to_owned(),
            retry_after: retry_after.to_rfc3339_opts(SecondsFormat::Secs, true),
        }
    }

    pub fn new(config_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            db: UsageDb::new(config_dir.clone()),
            pricing: PricingCatalogStore::new(config_dir),
            status: Mutex::new(UsageScanStatus::default()),
            cursor_remote: Mutex::new(CursorRemoteState::default()),
            lifecycle: Mutex::new(()),
            reprice_pending: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
        })
    }

    pub fn start_default_scan(self: &Arc<Self>) -> Result<UsageScanStatus, UsageError> {
        let roots = ScanRoots::from_environment();
        self.start_scan(roots)
    }

    fn start_scan(self: &Arc<Self>, roots: ScanRoots) -> Result<UsageScanStatus, UsageError> {
        let _lifecycle = self.lifecycle.lock().expect("usage lifecycle");
        self.start_scan_locked(roots)
    }

    /// 假定调用方已持有 lifecycle 锁；`rebuild` 在清库后复用同一启动路径。
    fn start_scan_locked(
        self: &Arc<Self>,
        roots: ScanRoots,
    ) -> Result<UsageScanStatus, UsageError> {
        if self.scan_status().state != UsageScanState::Idle {
            return Err(UsageError::ScanBusy);
        }
        self.apply_pending_pricing_locked()?;
        let status = {
            let mut status = self.status.lock().expect("usage scan status");
            *status = UsageScanStatus {
                state: UsageScanState::Running,
                started_at: Some(now()),
                ..UsageScanStatus::default()
            };
            status.clone()
        };
        self.cancel.store(false, Ordering::SeqCst);

        let service = Arc::clone(self);
        std::thread::spawn(move || service.run_scan(roots));
        self.refresh_pricing_if_needed();
        Ok(status)
    }

    pub fn cancel_scan(&self) -> UsageScanStatus {
        let mut status = self.status.lock().expect("usage scan status");
        if status.state == UsageScanState::Running {
            self.cancel.store(true, Ordering::SeqCst);
            status.state = UsageScanState::Cancelling;
        }
        status.clone()
    }

    pub fn scan_status(&self) -> UsageScanStatus {
        self.status.lock().expect("usage scan status").clone()
    }

    /// perf-baseline 统计快照（feature 门控，生产构建不存在）。
    #[cfg(feature = "perf-baseline")]
    pub fn perf_stats(&self) -> PerfStats {
        self.db.perf_stats()
    }

    /// 性能基线的同步扫描入口（`perf-baseline` feature 门控，见 docs/性能与功耗优化方案.md 阶段 0）。
    /// 路径由基准工具显式提供，不触碰真实用户目录。
    #[cfg(feature = "perf-baseline")]
    pub fn run_benchmark_scan(&self, roots: BenchmarkRoots) -> Result<(), UsageError> {
        self.run_scan_inner(roots.into_scan_roots())
    }

    pub fn summary(&self, mut query: UsageSummaryQuery) -> Result<UsageSummary, UsageError> {
        normalize_filter(&mut query.filter)?;
        self.db.summary(&query).map_err(Into::into)
    }

    pub fn conversations(
        &self,
        mut query: UsageConversationQuery,
    ) -> Result<UsageConversationPage, UsageError> {
        normalize_filter(&mut query.filter)?;
        let search = normalize_optional(&query.search)?;
        let project = normalize_optional(&query.project)?;
        let limit = query.limit.unwrap_or(DEFAULT_LIMIT);
        if !(1..=MAX_LIMIT).contains(&limit) {
            return Err(UsageError::InvalidQuery);
        }
        let offset = query.offset.unwrap_or(0);
        if i64::try_from(offset).is_err() {
            return Err(UsageError::InvalidQuery);
        }
        if let Some(sources) = &query.filter.sources
            && sources.len() > 8
        {
            return Err(UsageError::InvalidQuery);
        }
        self.db
            .conversations(&query, limit, offset, search.as_deref(), project.as_deref())
            .map_err(Into::into)
    }

    pub fn conversation(
        &self,
        conversation_key: String,
    ) -> Result<Option<UsageConversation>, UsageError> {
        let key = conversation_key.trim();
        if key.is_empty() || key.len() > 128 {
            return Err(UsageError::InvalidQuery);
        }
        self.db.conversation(key).map_err(Into::into)
    }

    pub fn conversation_breakdown(
        &self,
        conversation_key: String,
    ) -> Result<Option<UsageConversationBreakdown>, UsageError> {
        let key = conversation_key.trim();
        if key.is_empty() || key.len() > 128 {
            return Err(UsageError::InvalidQuery);
        }
        if self.db.conversation(key)?.is_none() {
            return Ok(None);
        }
        self.db
            .conversation_breakdown(key)
            .map(Some)
            .map_err(Into::into)
    }

    pub fn conversation_projects(
        &self,
        mut query: UsageConversationQuery,
    ) -> Result<Vec<UsageConversationProjectOption>, UsageError> {
        normalize_filter(&mut query.filter)?;
        if let Some(sources) = &query.filter.sources
            && sources.len() > 8
        {
            return Err(UsageError::InvalidQuery);
        }
        self.db.conversation_projects(&query).map_err(Into::into)
    }

    pub fn quota_history(&self, mut query: QuotaHistoryQuery) -> Result<QuotaHistory, UsageError> {
        query.from = normalize_time(query.from.as_deref())?;
        query.to = normalize_time(query.to.as_deref())?;
        if let (Some(from), Some(to)) = (&query.from, &query.to)
            && from >= to
        {
            return Err(UsageError::InvalidQuery);
        }
        let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let events = self.db.quota_history(
            query.provider,
            query.from.as_deref(),
            query.to.as_deref(),
            limit,
        )?;
        Ok(QuotaHistory { events })
    }

    pub fn reprice(&self) -> Result<UsageRepriceResult, UsageError> {
        let _lifecycle = self.lifecycle.lock().expect("usage lifecycle");
        if self.scan_status().state != UsageScanState::Idle {
            return Err(UsageError::ScanBusy);
        }
        let known_usage = self.db.known_usage_keys()?;
        let catalog = self
            .pricing
            .load_for_known_usage(&known_usage)
            .map_err(|_| UsageError::Unavailable)?;
        self.db.reprice(&catalog).map_err(Into::into)
    }

    /// 重建指定数据源：清空其用量条目、对话与扫描水位后立即全量重扫，
    /// 用于修复上游补写或历史解析导致的过期数据。`None` 表示全部数据源。
    pub fn rebuild(
        self: &Arc<Self>,
        sources: Option<Vec<UsageSource>>,
    ) -> Result<UsageScanStatus, UsageError> {
        let roots = ScanRoots::from_environment();
        self.rebuild_with_roots(sources, roots)
    }

    fn rebuild_with_roots(
        self: &Arc<Self>,
        sources: Option<Vec<UsageSource>>,
        roots: ScanRoots,
    ) -> Result<UsageScanStatus, UsageError> {
        let _lifecycle = self.lifecycle.lock().expect("usage lifecycle");
        if self.scan_status().state != UsageScanState::Idle {
            return Err(UsageError::ScanBusy);
        }
        self.db
            .rebuild_data(sources.as_deref())
            .map_err(UsageError::from)?;
        self.start_scan_locked(roots)
    }

    /// 设置页手动更新价格目录：绕过 24 小时与失败退避，等待当前扫描结束后提交并重计价。
    pub async fn refresh_pricing_catalog(&self) -> Result<PricingCatalogRefreshStatus, UsageError> {
        let outcome = self.pricing.refresh(PricingRefreshMode::Manual).await;
        if outcome.did_update() {
            loop {
                if self.scan_status().state == UsageScanState::Idle
                    && (self.apply_pending_pricing_if_idle()?
                        || self.scan_status().state == UsageScanState::Idle)
                {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }
        Ok(match outcome {
            PricingRefreshOutcome::Complete => PricingCatalogRefreshStatus::Complete,
            PricingRefreshOutcome::Partial => PricingCatalogRefreshStatus::Partial,
            PricingRefreshOutcome::Failed => PricingCatalogRefreshStatus::Failed,
        })
    }

    pub fn record_quota_snapshot(
        &self,
        provider: ProviderId,
        identity_key: &str,
        snapshot: &QuotaSnapshot,
    ) {
        if identity_key.is_empty() {
            return;
        }
        let _ = self
            .db
            .record_quota_snapshot(provider, identity_key, snapshot);
    }

    fn run_scan(self: Arc<Self>, roots: ScanRoots) {
        let result = self.run_scan_inner(roots);
        {
            let mut status = self.status.lock().expect("usage scan status");
            if result.is_err() {
                status.partial_failure = true;
                status.failed_files = status.failed_files.saturating_add(1);
            }
            status.cancelled = self.cancel.load(Ordering::SeqCst);
            status.state = UsageScanState::Idle;
            status.current_source = None;
            status.finished_at = Some(now());
        }
        let _ = self.apply_pending_pricing_if_idle();
        self.refresh_missing_pricing_if_needed();
    }

    fn run_scan_inner(&self, roots: ScanRoots) -> Result<(), UsageError> {
        self.db.initialize()?;
        let known_usage = self.db.known_usage_keys()?;
        let catalog = self
            .pricing
            .load_for_known_usage(&known_usage)
            .map_err(|_| UsageError::Unavailable)?;
        let titles = TitleIndex::load(
            roots.codex_title_index.as_deref(),
            roots.claude_history.as_deref(),
        );
        let discovery = discover_files(&roots);
        let previous_states = self.db.scan_file_states()?;
        {
            let mut status = self.status.lock().expect("usage scan status");
            status.discovered_files = discovery.files.len() as u64;
            status.failed_files = status.failed_files.saturating_add(discovery.failures);
            status.partial_failure |= discovery.failures > 0;
        }

        for file in discovery.files {
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            {
                self.status
                    .lock()
                    .expect("usage scan status")
                    .current_source = Some(file.source);
            }

            let previous = previous_states.get(&file.file_key).cloned();
            if self.scan_file(&file, &catalog, &titles, previous).is_err() {
                let mut status = self.status.lock().expect("usage scan status");
                status.failed_files = status.failed_files.saturating_add(1);
                status.partial_failure = true;
            }
            self.status
                .lock()
                .expect("usage scan status")
                .completed_files += 1;
        }

        if let Some(root) = roots.dsh_sessions.as_deref() {
            self.scan_dsh(root, &catalog, &previous_states)?;
        }

        if let Some(db_path) = roots.opencode_db.as_deref() {
            let outcome = opencode::scan_opencode(&self.db, db_path)?;
            {
                let mut status = self.status.lock().expect("usage scan status");
                status.inserted_entries = status.inserted_entries.saturating_add(outcome.inserted);
                status.duplicate_entries =
                    status.duplicate_entries.saturating_add(outcome.duplicates);
            }
        }
        // 一次完整扫描是同一逻辑操作；批次只是其事务边界，扫描结束后统一健康检查一次。
        self.db.verify_after_logical_write()?;
        Ok(())
    }

    fn refresh_pricing_if_needed(self: &Arc<Self>) {
        if !self.pricing.is_refresh_due() {
            return;
        }
        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            if service
                .pricing
                .refresh(PricingRefreshMode::Scheduled)
                .await
                .did_update()
            {
                let _ = service.apply_pending_pricing_if_idle();
            }
        });
    }

    fn refresh_missing_pricing_if_needed(self: &Arc<Self>) {
        let Ok(catalog) = self.pricing.load() else {
            return;
        };
        let Ok(candidates) = self.db.unpriced_usage_keys() else {
            return;
        };
        let missing: HashSet<PricingUsageKey> = candidates
            .into_iter()
            .filter(|key| catalog.needs_remote_refresh(key.source, &key.model, key.speed))
            .collect();
        if missing.is_empty()
            || !self
                .pricing
                .mark_missing_refresh_attempts(&missing)
                .unwrap_or(false)
        {
            return;
        }

        let service = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            if service
                .pricing
                .refresh(PricingRefreshMode::MissingPrice)
                .await
                .did_update()
            {
                let _ = service.apply_pending_pricing_if_idle();
            }
        });
    }

    fn apply_pending_pricing_if_idle(&self) -> Result<bool, UsageError> {
        let _lifecycle = self.lifecycle.lock().expect("usage lifecycle");
        if self.scan_status().state != UsageScanState::Idle {
            return Ok(false);
        }
        self.apply_pending_pricing_locked()
    }

    fn apply_pending_pricing_locked(&self) -> Result<bool, UsageError> {
        self.pricing.commit_pending();
        self.db.initialize()?;
        let known_usage = self.db.known_usage_keys()?;
        let catalog = self
            .pricing
            .load_for_known_usage(&known_usage)
            .map_err(|_| UsageError::Unavailable)?;
        if self.db.pricing_fingerprint()?.as_deref() != Some(catalog.fingerprint()) {
            self.reprice_pending.store(true, Ordering::Release);
        }
        if !self.reprice_pending.load(Ordering::Acquire) {
            return Ok(false);
        }
        self.db.reprice(&catalog)?;
        self.reprice_pending.store(false, Ordering::Release);
        Ok(true)
    }

    /// DSH 会话日志扫描。
    ///
    /// 与行式 JSONL 不同的三点，都是这条路径独立存在的原因：
    /// 日志是多帧 zstd 容器、水位按帧边界推进、一个文件里包含整个会话的记录序列
    /// （槽位替换、种子边界、标题覆盖都要在同一遍里处理）。
    ///
    /// 扫完之后统一做三件事：写会话归属表、解析父链把子代理归到根、
    /// 以及把「日志已经不在了但汇总还在」的会话按天物化回来。
    fn scan_dsh(
        &self,
        root: &Path,
        catalog: &PricingCatalog,
        previous_states: &HashMap<String, model::ScanFileState>,
    ) -> Result<(), UsageError> {
        let had_previous = previous_states.keys().any(|key| key.starts_with("dsh:"));
        let selection = dsh::select_logs(root, had_previous);
        if selection.failed_directories > 0 {
            let mut status = self.status.lock().expect("usage scan status");
            status.failed_files = status
                .failed_files
                .saturating_add(u64::try_from(selection.failed_directories).unwrap_or(u64::MAX));
            status.partial_failure = true;
        }
        if selection.logs.is_empty() {
            // 没有日志也要走归根与物化：清掉日志之后历史要能从汇总恢复。
            self.settle_dsh_sessions(&[], &[])?;
            return Ok(());
        }
        {
            let mut status = self.status.lock().expect("usage scan status");
            status.discovered_files = status
                .discovered_files
                .saturating_add(u64::try_from(selection.logs.len()).unwrap_or(u64::MAX));
            status.current_source = Some(UsageSource::Dsh);
        }

        let mut sessions = Vec::new();
        let mut touched = Vec::new();

        for log in &selection.logs {
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            let previous = previous_states.get(&log.file_key);
            let state = previous
                .and_then(|state| decode_cursor::<dsh::DshScanState>(state.cursor_json.as_deref()));
            let previous_offset = previous.map(|state| state.offset_bytes).unwrap_or(0);
            let reset = match previous {
                None => false,
                Some(previous) => {
                    // 文件变小或前缀指纹变了（原地替换）→ 整份重扫。
                    // 不复用文件身份（device/inode）：Windows 上没有稳定的等价物，
                    // 前缀指纹是仓库既有的跨平台判据。
                    previous.offset_bytes > log.size
                        || prefix_fingerprint(
                            &log.path,
                            previous.size_bytes.min(PREFIX_BYTES).min(log.size),
                        )
                        .map_err(|_| UsageError::Unavailable)?
                            != previous.prefix_fingerprint
                }
            };

            if !reset
                && previous.is_some_and(|state| {
                    state.offset_bytes == log.size && state.mtime_ms == log.mtime_ms
                })
            {
                // 这一轮没有新字节：跳过读取，但会话归属仍从状态里带出来，
                // 否则「日志没变」的那些会话会从归属表里消失，子代理就归不到根上。
                if let Some(state) = state.as_ref()
                    && let Some(session_id) = state.session_id.clone()
                {
                    sessions.push(dsh::DshSessionRow {
                        session_id,
                        parent_session: state.parent_session.clone(),
                        cwd: state.cwd.clone(),
                        title: state.title.clone(),
                        project_key: state
                            .cwd
                            .as_deref()
                            .map(parser::project_identity)
                            .and_then(|identity| identity.path),
                    });
                }
                {
                    let mut status = self.status.lock().expect("usage scan status");
                    status.completed_files += 1;
                }
                continue;
            }

            let output = dsh::scan_log(log, state.as_ref(), previous_offset, reset);
            if output.outcome != dsh::DshFileOutcome::Success {
                let mut status = self.status.lock().expect("usage scan status");
                status.failed_files += 1;
                status.partial_failure = true;
                status.completed_files += 1;
                continue;
            }

            let Some(conversation) = output.conversation.as_ref() else {
                // 成功但没有会话身份：不计入失败，也不写任何东西。
                self.status
                    .lock()
                    .expect("usage scan status")
                    .completed_files += 1;
                continue;
            };
            let session_id = conversation.session_id.clone();
            // 逐请求条目回来了：同一会话此前的按天物化条目必须让位，否则两份会并存。
            let _ = self.db.dsh_drop_contribution_entries(&session_id);
            sessions.push(dsh::DshSessionRow::from_conversation(conversation));
            touched.push(session_id);

            let mut batch = ScanBatch::default();
            for draft in &output.entries {
                let mut entry = dsh::into_usage_entry(draft);
                parser::apply_price(&mut entry, catalog);
                batch.entries.push(entry);
            }
            batch.conversations.push(ConversationFact {
                conversation_key: conversation.conversation_key.clone(),
                source: UsageSource::Dsh,
                title: conversation.title.clone(),
                project_hint: conversation.project_hint.clone(),
                project_key: conversation.project_key.clone(),
                worktree_path: conversation.worktree_path.clone(),
                is_sidechain: conversation.parent_session.is_some(),
                unattributed: false,
                occurred_at: conversation.last_at.clone(),
                source_id: None,
                branch: None,
            });
            batch.consumed_bytes = output.offset.saturating_sub(previous_offset);

            let cursor = encode_cursor(&output.state).map_err(|_| UsageError::Unavailable)?;
            let prefix = prefix_fingerprint(&log.path, log.size.min(PREFIX_BYTES))
                .map_err(|_| UsageError::Unavailable)?;
            let result = self.db.commit_scan_batch(
                &log.file_key,
                UsageSource::Dsh,
                log.mtime_ms,
                log.size,
                output.offset,
                &prefix,
                Some(&cursor),
                reset,
                &batch,
            )?;

            {
                let mut status = self.status.lock().expect("usage scan status");
                status.inserted_entries = status.inserted_entries.saturating_add(result.inserted);
                status.duplicate_entries =
                    status.duplicate_entries.saturating_add(result.duplicates);
                status.completed_files += 1;
            }
        }

        let touched: Vec<String> = {
            let mut unique = touched;
            unique.sort();
            unique.dedup();
            unique
        };
        self.settle_dsh_sessions(&sessions, &touched)?;
        Ok(())
    }

    /// 会话归属落库、父链归根、贡献汇总与「日志已消失」的物化。
    fn settle_dsh_sessions(
        &self,
        sessions: &[dsh::DshSessionRow],
        touched: &[String],
    ) -> Result<(), UsageError> {
        if !sessions.is_empty() {
            let now = chrono::Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true);
            let rows: Vec<DshSessionUpsert> = sessions
                .iter()
                .map(|session| DshSessionUpsert {
                    session_id: session.session_id.clone(),
                    parent_session: session.parent_session.clone(),
                    cwd: session.cwd.clone(),
                    title: session.title.clone(),
                    project_key: session.project_key.clone(),
                    updated_at: now.clone(),
                })
                .collect();
            self.db.dsh_upsert_sessions(&rows)?;
        }

        // 汇总必须在归根之前算：归根会把子会话的条目挪到根上，
        // 之后再算子会话的汇总就只剩空集了。
        for session_id in touched {
            self.db.dsh_recompute_rollup(session_id)?;
        }

        let parents = self.db.dsh_session_parents()?;
        if !parents.is_empty() {
            let roots = dsh::resolve_roots(&parents, DSH_ROOT_MAX_DEPTH);
            let rows: Vec<(String, Option<String>)> = roots
                .iter()
                .map(|(session_id, root)| (session_id.clone(), Some(root.clone())))
                .collect();
            self.db.dsh_set_roots(&rows)?;
            for (session_id, root) in &roots {
                if session_id == root {
                    continue;
                }
                self.db
                    .dsh_rekey_session(&format!("dsh:{session_id}"), &format!("dsh:{root}"))?;
            }
        }

        // 汇总算完之后再物化：日志被清理或条目被重建的会话从这里恢复历史。
        self.db.dsh_materialize_missing()?;
        Ok(())
    }

    fn scan_file(
        &self,
        file: &SourceFile,
        catalog: &PricingCatalog,
        titles: &TitleIndex,
        previous: Option<model::ScanFileState>,
    ) -> Result<(), UsageError> {
        let metadata = fs::metadata(&file.path).map_err(|_| UsageError::Unavailable)?;
        if !metadata.is_file() {
            return Ok(());
        }
        let size = metadata.len();
        let mtime_ms = modified_ms(&metadata);
        let cursor_valid = previous.as_ref().is_none_or(|state| match file.source {
            UsageSource::Codex => {
                decode_cursor::<CodexCursor>(state.cursor_json.as_deref()).is_some()
            }
            UsageSource::Claude => {
                decode_cursor::<ClaudeCursor>(state.cursor_json.as_deref()).is_some()
            }
            UsageSource::Pi => decode_cursor::<PiCursor>(state.cursor_json.as_deref()).is_some(),
            // OpenCode 是库级扫描，不产生文件级 cursor。
            UsageSource::Opencode => true,
            // DSH 是多帧 zstd 容器，有自己的水位语义（批次 3 实装）；
            // Cursor 来自远端计量，不经过文件扫描。两者当前都没有文件级 cursor 需要校验。
            UsageSource::Dsh | UsageSource::Cursor => true,
        });

        let previous_prefix = match &previous {
            Some(state) => Some(
                prefix_fingerprint(&file.path, state.size_bytes.min(PREFIX_BYTES).min(size))
                    .map_err(|_| UsageError::Unavailable)?,
            ),
            None => None,
        };
        let previous_prefix_matches = previous
            .as_ref()
            .zip(previous_prefix.as_ref())
            .is_none_or(|(state, value)| value == &state.prefix_fingerprint);
        let prefix_length = size.min(PREFIX_BYTES);
        let prefix = match (&previous, &previous_prefix) {
            (Some(state), Some(value))
                if state.size_bytes.min(PREFIX_BYTES).min(size) == prefix_length =>
            {
                value.clone()
            }
            _ => prefix_fingerprint(&file.path, prefix_length)
                .map_err(|_| UsageError::Unavailable)?,
        };

        let must_reset = previous.as_ref().is_some_and(|state| {
            state.offset_bytes > size
                || !previous_prefix_matches
                || !cursor_valid
                || (size == state.size_bytes && mtime_ms != state.mtime_ms)
        });
        let offset = previous
            .as_ref()
            .filter(|_| !must_reset)
            .map_or(0, |state| state.offset_bytes);

        if previous.as_ref().is_some_and(|state| {
            !must_reset
                && state.offset_bytes == size
                && state.size_bytes == size
                && state.mtime_ms == mtime_ms
        }) {
            return Ok(());
        }

        let mut codex_cursor = if file.source == UsageSource::Codex && !must_reset {
            decode_cursor(
                previous
                    .as_ref()
                    .and_then(|state| state.cursor_json.as_deref()),
            )
            .unwrap_or_default()
        } else {
            CodexCursor::default()
        };
        let mut claude_cursor = if file.source == UsageSource::Claude && !must_reset {
            decode_cursor(
                previous
                    .as_ref()
                    .and_then(|state| state.cursor_json.as_deref()),
            )
            .unwrap_or_default()
        } else {
            ClaudeCursor::default()
        };
        let mut pi_cursor = if file.source == UsageSource::Pi {
            let mut cursor = if !must_reset {
                decode_cursor(
                    previous
                        .as_ref()
                        .and_then(|state| state.cursor_json.as_deref()),
                )
                .unwrap_or_default()
            } else {
                PiCursor::default()
            };
            if cursor.filename_key.is_none() {
                cursor.filename_key = Some(
                    file.path
                        .file_stem()
                        .and_then(|value| value.to_str())
                        .unwrap_or_default()
                        .to_owned(),
                );
            }
            cursor
        } else {
            PiCursor::default()
        };
        let pi_filename_key = pi_cursor.filename_key.clone().unwrap_or_default();

        let mut handle = fs::File::open(&file.path).map_err(|_| UsageError::Unavailable)?;
        handle
            .seek(SeekFrom::Start(offset))
            .map_err(|_| UsageError::Unavailable)?;
        let mut reader = BufReader::new(handle);
        let mut committed_offset = offset;
        let mut batch = ScanBatch::default();
        let mut line = Vec::new();
        let mut reset_pending = must_reset;

        loop {
            if self.cancel.load(Ordering::SeqCst) {
                break;
            }
            line.clear();
            let next =
                read_complete_line(&mut reader, &mut line).map_err(|_| UsageError::Unavailable)?;
            let (line_bytes, oversized) = match next {
                LineRead::Eof | LineRead::Partial => break,
                LineRead::Complete { bytes, oversized } => (bytes, oversized),
            };

            batch.consumed_bytes = batch.consumed_bytes.saturating_add(line_bytes);
            batch.consumed_lines += 1;
            if oversized {
                batch.invalid_lines += 1;
            } else {
                let parsed = match file.source {
                    UsageSource::Codex => {
                        parse_codex_line(&line, &mut codex_cursor, catalog, &titles.codex)
                    }
                    UsageSource::Claude => {
                        parse_claude_line(&line, &mut claude_cursor, catalog, &titles.claude)
                    }
                    UsageSource::Pi => parse_pi_line(&line, &mut pi_cursor, &pi_filename_key),
                    // OpenCode 走库级扫描，不经过文件解析。
                    UsageSource::Opencode => ParsedLine::Ignored,
                    // DSH 是多帧容器、Cursor 是远端计量，都不走行式 JSONL 解析。
                    UsageSource::Dsh | UsageSource::Cursor => ParsedLine::Ignored,
                };
                match parsed {
                    ParsedLine::Ignored => {}
                    ParsedLine::Invalid => batch.invalid_lines += 1,
                    ParsedLine::Fact {
                        entry,
                        conversation,
                    } => {
                        batch.entries.push(*entry);
                        batch.conversations.push(*conversation);
                    }
                }
            }

            let reached_batch =
                batch.consumed_lines >= BATCH_LINES || batch.consumed_bytes >= BATCH_BYTES;
            if reached_batch || self.cancel.load(Ordering::SeqCst) {
                committed_offset = committed_offset.saturating_add(batch.consumed_bytes);
                self.commit_file_batch(
                    file,
                    mtime_ms,
                    size,
                    committed_offset,
                    &prefix,
                    &codex_cursor,
                    &claude_cursor,
                    &pi_cursor,
                    &mut reset_pending,
                    &mut batch,
                )?;
            }
        }

        if batch.consumed_bytes > 0
            || (!self.cancel.load(Ordering::SeqCst) && (previous.is_none() || must_reset))
        {
            committed_offset = committed_offset.saturating_add(batch.consumed_bytes);
            self.commit_file_batch(
                file,
                mtime_ms,
                size,
                committed_offset,
                &prefix,
                &codex_cursor,
                &claude_cursor,
                &pi_cursor,
                &mut reset_pending,
                &mut batch,
            )?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn commit_file_batch(
        &self,
        file: &SourceFile,
        mtime_ms: i64,
        size: u64,
        offset: u64,
        prefix: &str,
        codex_cursor: &CodexCursor,
        claude_cursor: &ClaudeCursor,
        pi_cursor: &PiCursor,
        reset_pending: &mut bool,
        batch: &mut ScanBatch,
    ) -> Result<(), UsageError> {
        let cursor = match file.source {
            UsageSource::Codex => encode_cursor(codex_cursor),
            UsageSource::Claude => encode_cursor(claude_cursor),
            UsageSource::Pi => encode_cursor(pi_cursor),
            UsageSource::Opencode => encode_cursor(&PiCursor::default()),
            // DSH 与 Cursor 不用行式 cursor；写入一个空对象保持列非空且可解析。
            UsageSource::Dsh | UsageSource::Cursor => encode_cursor(&PiCursor::default()),
        }
        .map_err(|_| UsageError::Unavailable)?;
        let consumed = batch.consumed_bytes;
        let invalid = batch.invalid_lines;
        let result = self.db.commit_scan_batch(
            &file.file_key,
            file.source,
            mtime_ms,
            size,
            offset,
            prefix,
            Some(&cursor),
            *reset_pending,
            batch,
        )?;
        *reset_pending = false;
        {
            let mut status = self.status.lock().expect("usage scan status");
            status.bytes_read = status.bytes_read.saturating_add(consumed);
            status.inserted_entries = status.inserted_entries.saturating_add(result.inserted);
            status.duplicate_entries = status.duplicate_entries.saturating_add(result.duplicates);
            status.invalid_lines = status.invalid_lines.saturating_add(invalid);
        }
        batch.clear();
        // 大文件每批提交后主动让出时间片，降低后台扫描连续占用 CPU 的概率。
        std::thread::yield_now();
        Ok(())
    }
}

/// 性能基线用的扫描根（`perf-baseline` feature 门控）。与产品入口 `ScanRoots` 等价，
/// 但所有路径由基准工具显式提供，不读环境变量、不触碰真实用户目录。
#[cfg(feature = "perf-baseline")]
#[derive(Debug, Clone, Default)]
pub struct BenchmarkRoots {
    pub codex_sessions: Option<PathBuf>,
    pub codex_archived: Option<PathBuf>,
    pub claude_projects: Option<PathBuf>,
    pub pi_sessions: Option<PathBuf>,
    pub dsh_sessions: Option<PathBuf>,
    pub opencode_db: Option<PathBuf>,
    pub codex_title_index: Option<PathBuf>,
    pub claude_history: Option<PathBuf>,
}

#[cfg(feature = "perf-baseline")]
impl BenchmarkRoots {
    fn into_scan_roots(self) -> ScanRoots {
        ScanRoots {
            codex_sessions: self.codex_sessions,
            codex_archived: self.codex_archived,
            claude_projects: self.claude_projects,
            pi_sessions: self.pi_sessions,
            dsh_sessions: self.dsh_sessions,
            opencode_db: self.opencode_db,
            codex_title_index: self.codex_title_index,
            claude_history: self.claude_history,
        }
    }
}

struct ScanRoots {
    codex_sessions: Option<PathBuf>,
    codex_archived: Option<PathBuf>,
    claude_projects: Option<PathBuf>,
    pi_sessions: Option<PathBuf>,
    dsh_sessions: Option<PathBuf>,
    opencode_db: Option<PathBuf>,
    codex_title_index: Option<PathBuf>,
    claude_history: Option<PathBuf>,
}

impl ScanRoots {
    fn from_environment() -> Self {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from);
        Self {
            codex_sessions: home.as_ref().map(|path| path.join(".codex/sessions")),
            codex_archived: home
                .as_ref()
                .map(|path| path.join(".codex/archived_sessions")),
            claude_projects: home.as_ref().map(|path| path.join(".claude/projects")),
            pi_sessions: home.as_ref().map(|path| path.join(".pi/agent/sessions")),
            // DSH 的会话日志根。目录结构是 <项目段>/<会话段>/<日志文件>。
            dsh_sessions: home.as_ref().map(|path| path.join(".dsh/sessions")),
            opencode_db: home
                .as_ref()
                .map(|path| path.join(".local/share/opencode/opencode.db")),
            codex_title_index: home
                .as_ref()
                .map(|path| path.join(".codex/session_index.jsonl")),
            claude_history: home.as_ref().map(|path| path.join(".claude/history.jsonl")),
        }
    }
}

struct SourceFile {
    source: UsageSource,
    path: PathBuf,
    file_key: String,
}

struct Discovery {
    files: Vec<SourceFile>,
    failures: u64,
}

fn discover_files(roots: &ScanRoots) -> Discovery {
    let mut codex = Vec::new();
    let mut failures = 0;
    if let Some(root) = &roots.codex_sessions {
        collect_jsonl(root, &mut codex, &mut failures);
    }
    if let Some(root) = &roots.codex_archived {
        collect_jsonl(root, &mut codex, &mut failures);
    }

    let mut newest = HashMap::<String, PathBuf>::new();
    for path in codex {
        let identity = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_owned();
        newest
            .entry(identity)
            .and_modify(|current| {
                if path_mtime(&path) > path_mtime(current) {
                    *current = path.clone();
                }
            })
            .or_insert(path);
    }

    let mut files = newest
        .into_values()
        .map(|path| source_file(UsageSource::Codex, path, None))
        .collect::<Vec<_>>();
    if let Some(root) = &roots.claude_projects {
        let mut claude = Vec::new();
        collect_jsonl(root, &mut claude, &mut failures);
        files.extend(
            claude
                .into_iter()
                .map(|path| source_file(UsageSource::Claude, path, Some(root))),
        );
    }
    if let Some(root) = &roots.pi_sessions {
        let mut pi = Vec::new();
        collect_jsonl(root, &mut pi, &mut failures);
        files.extend(
            pi.into_iter()
                .map(|path| source_file(UsageSource::Pi, path, Some(root))),
        );
    }
    files.sort_by(|left, right| {
        left.source
            .as_db()
            .cmp(right.source.as_db())
            .then_with(|| left.file_key.cmp(&right.file_key))
    });
    Discovery { files, failures }
}

fn collect_jsonl(root: &Path, output: &mut Vec<PathBuf>, failures: &mut u64) {
    let root_metadata = match fs::symlink_metadata(root) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return,
        Err(_) => {
            *failures = failures.saturating_add(1);
            return;
        }
    };
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return;
    }
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => {
            *failures = failures.saturating_add(1);
            return;
        }
    };
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                *failures = failures.saturating_add(1);
                continue;
            }
        };
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => {
                *failures = failures.saturating_add(1);
                continue;
            }
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            collect_jsonl(&path, output, failures);
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("jsonl"))
        {
            output.push(path);
        }
    }
}

fn source_file(source: UsageSource, path: PathBuf, source_root: Option<&Path>) -> SourceFile {
    let mut hasher = Sha256::new();
    hasher.update(source.as_db().as_bytes());
    hasher.update([0]);
    match source {
        UsageSource::Codex => {
            hasher.update(
                path.file_stem()
                    .and_then(|value| value.to_str())
                    .unwrap_or_default()
                    .as_bytes(),
            );
        }
        UsageSource::Claude => {
            hasher.update(b"physical-relative-path-v1");
            let identity = source_root
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(&path);
            for component in identity.components() {
                let component = component.as_os_str().to_string_lossy();
                hasher.update((component.len() as u64).to_le_bytes());
                hasher.update(component.as_bytes());
            }
        }
        UsageSource::Pi => {
            hasher.update(b"pi-relative-path-v1");
            let identity = source_root
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(&path);
            for component in identity.components() {
                let component = component.as_os_str().to_string_lossy();
                hasher.update((component.len() as u64).to_le_bytes());
                hasher.update(component.as_bytes());
            }
        }
        UsageSource::Opencode => {
            hasher.update(b"opencode-sqlite-v1");
            hasher.update(path.as_os_str().as_encoded_bytes());
        }
        // DSH 的会话身份由会话目录与版本号决定（批次 3 实装）；
        // Cursor 没有文件级扫描，file_key 只在间接调用时出现。
        UsageSource::Dsh | UsageSource::Cursor => {
            hasher.update(b"relative-path-v1");
            let identity = source_root
                .and_then(|root| path.strip_prefix(root).ok())
                .unwrap_or(&path);
            for component in identity.components() {
                let component = component.as_os_str().to_string_lossy();
                hasher.update((component.len() as u64).to_le_bytes());
                hasher.update(component.as_bytes());
            }
        }
    }
    SourceFile {
        source,
        path,
        file_key: format!("{:x}", hasher.finalize()),
    }
}

fn prefix_fingerprint(path: &Path, length: u64) -> io::Result<String> {
    let mut handle = fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(length as usize);
    handle.by_ref().take(length).read_to_end(&mut bytes)?;
    Ok(format!("{:x}", Sha256::digest(&bytes)))
}

fn path_mtime(path: &Path) -> i64 {
    fs::metadata(path)
        .map(|metadata| modified_ms(&metadata))
        .unwrap_or(0)
}

fn modified_ms(metadata: &fs::Metadata) -> i64 {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

enum LineRead {
    Eof,
    Partial,
    Complete { bytes: u64, oversized: bool },
}

fn read_complete_line<R: BufRead>(reader: &mut R, output: &mut Vec<u8>) -> io::Result<LineRead> {
    let mut total = 0_u64;
    let mut oversized = false;
    loop {
        let buffer = reader.fill_buf()?;
        if buffer.is_empty() {
            return Ok(if total == 0 {
                LineRead::Eof
            } else {
                LineRead::Partial
            });
        }
        let newline = buffer.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(buffer.len(), |index| index + 1);
        total = total.saturating_add(consumed as u64);

        if !oversized {
            let remaining = MAX_LINE_BYTES.saturating_sub(output.len());
            let copy = consumed.min(remaining);
            output.extend_from_slice(&buffer[..copy]);
            if copy < consumed {
                oversized = true;
            }
        }
        reader.consume(consumed);

        if newline.is_some() {
            return Ok(LineRead::Complete {
                bytes: total,
                oversized,
            });
        }
    }
}

fn encode_cursor<T: Serialize>(cursor: &T) -> Result<String, serde_json::Error> {
    serde_json::to_string(cursor)
}

fn decode_cursor<T: serde::de::DeserializeOwned>(value: Option<&str>) -> Option<T> {
    value.and_then(|value| serde_json::from_str(value).ok())
}

fn normalize_filter(filter: &mut UsageFilter) -> Result<(), UsageError> {
    filter.from = normalize_time(filter.from.as_deref())?;
    filter.to = normalize_time(filter.to.as_deref())?;
    filter.model = normalize_optional(&filter.model)?;
    filter.project = normalize_optional(&filter.project)?;
    // 可见服务集合由设置决定，前端负责传下来，Rust 不猜默认值：
    // `None` 表示不过滤，空数组表示一个服务都不可见（结果为空）。
    if let Some(sources) = &mut filter.sources {
        if sources.len() > UsageSource::ORDER.len() {
            return Err(UsageError::InvalidQuery);
        }
        sources.sort();
        sources.dedup();
    }
    if let (Some(from), Some(to)) = (&filter.from, &filter.to)
        && from >= to
    {
        return Err(UsageError::InvalidQuery);
    }
    Ok(())
}

fn normalize_time(value: Option<&str>) -> Result<Option<String>, UsageError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = DateTime::parse_from_rfc3339(value).map_err(|_| UsageError::InvalidQuery)?;
    Ok(Some(
        parsed
            .with_timezone(&Utc)
            .to_rfc3339_opts(SecondsFormat::Millis, true),
    ))
}

fn normalize_optional(value: &Option<String>) -> Result<Option<String>, UsageError> {
    let Some(value) = value.as_deref().map(str::trim) else {
        return Ok(None);
    };
    if value.chars().count() > MAX_FILTER_LENGTH {
        return Err(UsageError::InvalidQuery);
    }
    Ok((!value.is_empty()).then(|| value.to_owned()))
}

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Cursor;
    use std::io::Write;

    use super::*;

    const CODEX_FIXTURE: &[u8] = include_bytes!("../../../fixtures/usage/codex/session.jsonl");
    const CODEX_INDEX_FIXTURE: &[u8] =
        include_bytes!("../../../fixtures/usage/codex/session_index.jsonl");
    const CLAUDE_FIXTURE: &[u8] = include_bytes!("../../../fixtures/usage/claude/project.jsonl");
    const CLAUDE_HISTORY_FIXTURE: &[u8] =
        include_bytes!("../../../fixtures/usage/claude/history.jsonl");
    const PI_FIXTURE: &[u8] = include_bytes!("../../../fixtures/usage/pi/session.jsonl");

    fn fixture_roots(base: &Path) -> ScanRoots {
        ScanRoots {
            codex_sessions: Some(base.join("codex/sessions")),
            codex_archived: Some(base.join("codex/archived")),
            claude_projects: Some(base.join("claude/projects")),
            pi_sessions: Some(base.join("pi/sessions")),
            dsh_sessions: Some(base.join("dsh/sessions")),
            opencode_db: Some(base.join("opencode/opencode.db")),
            codex_title_index: Some(base.join("codex/session_index.jsonl")),
            claude_history: Some(base.join("claude/history.jsonl")),
        }
    }

    fn seed_fixture_roots(base: &Path) {
        let roots = fixture_roots(base);
        let codex = roots.codex_sessions.as_ref().expect("codex root");
        let claude = roots.claude_projects.as_ref().expect("claude root");
        fs::create_dir_all(codex).expect("codex dir");
        fs::create_dir_all(claude).expect("claude dir");
        fs::write(codex.join("fixture-codex-session.jsonl"), CODEX_FIXTURE).expect("codex fixture");
        fs::write(claude.join("fixture-claude-session.jsonl"), CLAUDE_FIXTURE)
            .expect("claude fixture");
        fs::write(
            roots.codex_title_index.as_ref().expect("codex index"),
            CODEX_INDEX_FIXTURE,
        )
        .expect("codex index fixture");
        fs::write(
            roots.claude_history.as_ref().expect("claude history"),
            CLAUDE_HISTORY_FIXTURE,
        )
        .expect("claude history fixture");
    }

    fn seed_pi_root(base: &Path) {
        let pi = fixture_roots(base).pi_sessions.expect("pi root");
        fs::create_dir_all(&pi).expect("pi dir");
        fs::write(pi.join("fixture-pi-session.jsonl"), PI_FIXTURE).expect("pi fixture");
    }

    #[test]
    fn complete_line_reader_does_not_consume_a_partial_tail_in_the_watermark() {
        let mut reader = Cursor::new(b"one\npartial".to_vec());
        let mut line = Vec::new();
        assert!(matches!(
            read_complete_line(&mut reader, &mut line),
            Ok(LineRead::Complete { bytes: 4, .. })
        ));
        line.clear();
        assert!(matches!(
            read_complete_line(&mut reader, &mut line),
            Ok(LineRead::Partial)
        ));
    }

    #[test]
    fn invalid_time_range_is_rejected_before_sql() {
        let mut filter = UsageFilter {
            from: Some("2026-07-31T00:00:00Z".to_owned()),
            to: Some("2026-07-30T00:00:00Z".to_owned()),
            ..UsageFilter::default()
        };
        assert_eq!(normalize_filter(&mut filter), Err(UsageError::InvalidQuery));
    }

    #[test]
    fn cancel_transitions_running_scan_without_a_second_state_source() {
        let config = tempfile::tempdir().expect("config");
        let service = UsageService::new(config.path().to_path_buf());
        service.status.lock().expect("status").state = UsageScanState::Running;

        let status = service.cancel_scan();

        assert_eq!(status.state, UsageScanState::Cancelling);
        assert!(service.cancel.load(Ordering::SeqCst));
    }

    #[test]
    fn file_keys_are_hashes_not_paths() {
        let file = source_file(
            UsageSource::Codex,
            PathBuf::from("/private/secret/project.jsonl"),
            None,
        );
        assert_eq!(file.file_key.len(), 64);
        assert!(!file.file_key.contains("private"));
        assert_eq!(
            file.file_key,
            source_file(
                UsageSource::Codex,
                PathBuf::from("/another/location/project.jsonl"),
                None,
            )
            .file_key,
            "moving a session from active to archive must keep its watermark"
        );
    }

    #[test]
    fn claude_same_stem_in_different_directories_keeps_both_files() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        let roots = fixture_roots(sources.path());
        let claude = roots.claude_projects.as_ref().expect("claude root");
        let first_dir = claude.join("project-a");
        let second_dir = claude.join("project-b");
        fs::create_dir_all(&first_dir).expect("first dir");
        fs::create_dir_all(&second_dir).expect("second dir");

        let first = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"shared-session\",\"timestamp\":\"2026-07-30T01:00:00Z\",",
            "\"message\":{\"id\":\"message-a\",\"model\":\"claude-sonnet-5\",\"stop_reason\":\"end_turn\",",
            "\"usage\":{\"input_tokens\":1,\"output_tokens\":1,\"speed\":\"standard\"}}}\n"
        );
        let second = concat!(
            "{\"type\":\"assistant\",\"sessionId\":\"shared-session\",\"timestamp\":\"2026-07-30T01:01:00Z\",",
            "\"message\":{\"id\":\"message-b\",\"model\":\"claude-sonnet-5\",\"stop_reason\":\"end_turn\",",
            "\"usage\":{\"input_tokens\":2,\"output_tokens\":1,\"speed\":\"standard\"}}}\n"
        );
        let first_path = first_dir.join("shared-session.jsonl");
        let second_path = second_dir.join("shared-session.jsonl");
        fs::write(&first_path, first).expect("first fixture");
        fs::write(&second_path, second).expect("second fixture");

        assert_ne!(
            source_file(UsageSource::Claude, first_path, Some(claude)).file_key,
            source_file(UsageSource::Claude, second_path, Some(claude)).file_key
        );

        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("initial scan");
        let summary = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 2);
        assert_eq!(summary.tokens.total_tokens, 5);

        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("unchanged rescan");
        assert_eq!(
            service
                .summary(UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: crate::contracts::UsageGroupBy::Source,
                })
                .expect("summary after rescan")
                .entry_count,
            2
        );
    }

    /// 在 DSH 日志根下写一个会话目录：`<项目段>/<会话段>/session.jsonl`。
    fn seed_dsh_session(
        base: &Path,
        project: &str,
        session: &str,
        lines: &[serde_json::Value],
    ) -> PathBuf {
        let root = fixture_roots(base).dsh_sessions.expect("dsh root");
        let directory = root.join(project).join(session);
        fs::create_dir_all(&directory).expect("dsh dir");
        let path = directory.join("session.jsonl");
        let mut body = String::new();
        for line in lines {
            body.push_str(&line.to_string());
            body.push('\n');
        }
        fs::write(&path, body).expect("dsh fixture");
        path
    }

    fn dsh_session_line(id: &str, cwd: &str, parent: Option<&str>) -> serde_json::Value {
        match parent {
            Some(parent) => serde_json::json!({
                "type": "session", "id": id, "cwd": cwd, "parentSession": parent
            }),
            None => serde_json::json!({ "type": "session", "id": id, "cwd": cwd }),
        }
    }

    fn dsh_assistant_line(turn: i64, step: i64, input: i64, output: i64) -> serde_json::Value {
        serde_json::json!({
            "type": "assistant/message",
            "time": 1_790_000_000_000_i64 + turn * 1000 + step,
            "data": {
                "turn": turn,
                "step": step,
                "usage": { "inputTokens": input, "outputTokens": output },
                "message": { "source": { "provider": "anthropic", "model": "claude-sonnet-4-5" } }
            }
        })
    }

    #[test]
    fn dsh_participates_in_the_pricing_catalog() {
        // DSH 日志没有账单费用，只有 token 用量；按 cc-bar 的做法由价格表估算，
        // 因此它必须出现在定价参与者集合里，而不是像 Pi／OpenCode 那样自带费用。
        assert!(UsageSource::ALL.contains(&UsageSource::Dsh));
        assert!(!UsageSource::Dsh.carries_own_cost());
        assert!(UsageSource::Pi.carries_own_cost());
        assert!(UsageSource::Opencode.carries_own_cost());
        assert!(UsageSource::Cursor.carries_own_cost());
    }

    #[test]
    fn dsh_sessions_are_scanned_priced_and_grouped_by_project() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        seed_dsh_session(
            sources.path(),
            "project-a",
            "session-1",
            &[
                dsh_session_line("s-1", "/work/demo", None),
                dsh_assistant_line(1, 1, 100, 50),
            ],
        );
        seed_dsh_session(
            sources.path(),
            "project-b",
            "session-2",
            &[
                dsh_session_line("s-2", "/work/other", None),
                dsh_assistant_line(1, 1, 20, 10),
            ],
        );

        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("dsh scan");

        let summary = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 2);
        assert_eq!(summary.tokens.uncached_input_tokens, 120);
        assert_eq!(summary.tokens.output_tokens, 60);

        // DSH 只有 token 用量、没有账单费用，因此必须走价格表：
        // 要么命中目录（priced），要么明确记成缺价（unpriced）。测试环境没有在线目录，
        // 所以这里断言的是「两条都进入了定价判定」，而不是「一定有价格」。
        assert_eq!(
            summary.cost.priced_entries + summary.cost.unpriced_entries,
            2,
            "DSH 的条目必须进入定价判定，不能跳过价格层"
        );

        let conversations = service
            .conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                ..UsageConversationQuery::default()
            })
            .expect("conversations");
        assert_eq!(conversations.total, 2);
        let projects: Vec<&str> = conversations
            .items
            .iter()
            .filter_map(|item| item.project_key.as_deref())
            .collect();
        assert!(projects.contains(&"/work/demo"));
        assert!(projects.contains(&"/work/other"));
    }

    #[test]
    fn dsh_child_sessions_merge_into_the_root_conversation() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        seed_dsh_session(
            sources.path(),
            "project-a",
            "root-session",
            &[
                dsh_session_line("root", "/work/demo", None),
                dsh_assistant_line(1, 1, 100, 50),
            ],
        );
        seed_dsh_session(
            sources.path(),
            "project-a",
            "child-session",
            &[
                dsh_session_line("child", "/work/demo", Some("root")),
                dsh_assistant_line(1, 1, 10, 5),
            ],
        );

        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("dsh scan");

        let conversations = service
            .conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                ..UsageConversationQuery::default()
            })
            .expect("conversations");
        assert_eq!(conversations.total, 1, "子代理归到根会话，只剩一行");
        let root = &conversations.items[0];
        assert_eq!(root.conversation_key, "dsh:root");
        assert_eq!(root.entry_count, 2);
        assert_eq!(root.tokens.uncached_input_tokens, 110);
    }

    #[test]
    fn dsh_history_survives_losing_the_log_file() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        let path = seed_dsh_session(
            sources.path(),
            "project-a",
            "session-1",
            &[
                dsh_session_line("s-1", "/work/demo", None),
                dsh_assistant_line(1, 1, 100, 50),
            ],
        );

        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("first scan");
        let before = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");

        // 日志被清理（cc-bar 的「源日志被删也不倒扣历史」），然后重建用量。
        fs::remove_file(&path).expect("remove log");
        service
            .rebuild_with_roots(None, fixture_roots(sources.path()))
            .expect("rebuild");
        // 重建是异步起线程的：不等它结束就读汇总会拿到中间状态。
        wait_for_idle(&service);

        let after = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary after rebuild");
        assert_eq!(
            after.tokens.uncached_input_tokens, before.tokens.uncached_input_tokens,
            "重建后历史不能倒扣"
        );
        assert_eq!(after.entry_count, 1, "按天粒度物化回一条");
    }

    #[test]
    fn fixture_scan_deduplicates_and_returns_aggregate_contracts() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        seed_fixture_roots(sources.path());
        let service = UsageService::new(config.path().to_path_buf());

        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("scan fixtures");
        let summary = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");

        assert_eq!(summary.entry_count, 3);
        assert_eq!(summary.tokens.uncached_input_tokens, 120);
        assert_eq!(summary.tokens.cache_read_input_tokens, 100);
        assert_eq!(summary.tokens.cache_write_5m_input_tokens, 17);
        assert_eq!(summary.tokens.cache_write_1h_input_tokens, 4);
        assert_eq!(summary.tokens.output_tokens, 60);
        assert_eq!(summary.tokens.total_tokens, 301);
        assert_eq!(summary.fast.raw_tokens, 100);
        assert_eq!(summary.fast.billing_equivalent_tokens, "250");
        assert_eq!(summary.fast.minimum_multiplier.as_deref(), Some("2.5"));
        assert_eq!(summary.cost.priced_entries, 3);
        let bytes_after_initial_scan = service.scan_status().bytes_read;

        let page = service
            .conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                search: None,
                limit: Some(10),
                offset: Some(0),
                ..UsageConversationQuery::default()
            })
            .expect("conversations");
        assert_eq!(page.total, 2);
        assert_eq!(page.items.len(), 2);
        assert!(
            page.items
                .iter()
                .all(|item| item.conversation_key.len() == 64)
        );
        let codex_conversation = page
            .items
            .iter()
            .find(|item| item.source == UsageSource::Codex)
            .expect("codex conversation");
        assert_eq!(
            codex_conversation.title.as_deref(),
            Some("索引标题：修复登录流程"),
            "标题索引优先于 user_message 兜底"
        );
        assert_eq!(
            codex_conversation.source_id.as_deref(),
            Some("fixture-codex-session")
        );
        assert_eq!(codex_conversation.branch, None);
        assert_eq!(
            codex_conversation.models.as_slice(),
            ["gpt-5.6-sol"],
            "会话模型列表去重排序"
        );
        let claude_conversation = page
            .items
            .iter()
            .find(|item| item.source == UsageSource::Claude)
            .expect("claude conversation");
        assert_eq!(
            claude_conversation.title.as_deref(),
            Some("索引标题：重构主窗口布局"),
            "history.jsonl 的 display 优先于 user 行兜底"
        );
        assert_eq!(
            claude_conversation.source_id.as_deref(),
            Some("fixture-claude-session")
        );
        assert_eq!(
            claude_conversation.branch.as_deref(),
            Some("fix/login-flow")
        );
        assert_eq!(
            claude_conversation.models.as_slice(),
            ["claude-opus-5"],
            "会话模型列表去重排序"
        );

        let projects = service
            .conversation_projects(UsageConversationQuery {
                filter: UsageFilter::default(),
                ..UsageConversationQuery::default()
            })
            .expect("projects");
        assert_eq!(projects.len(), 2);
        assert_eq!(projects[0].name, "project-beta");
        assert_eq!(projects[0].conversation_count, 1);
        assert_eq!(projects[1].name, "project-alpha");
        assert_eq!(projects[1].conversation_count, 1);
        let first = service
            .conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                search: None,
                limit: Some(1),
                offset: Some(0),
                ..UsageConversationQuery::default()
            })
            .expect("first page");
        let second = service
            .conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                search: None,
                limit: Some(1),
                offset: Some(1),
                ..UsageConversationQuery::default()
            })
            .expect("second page");
        assert_eq!(first.total, 2);
        assert_eq!(second.total, 2);
        assert_ne!(
            first.items[0].conversation_key,
            second.items[0].conversation_key
        );

        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("unchanged rescan");
        assert_eq!(
            service.scan_status().bytes_read,
            bytes_after_initial_scan,
            "unchanged files must not reparse content"
        );
        assert_eq!(
            service
                .summary(UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: crate::contracts::UsageGroupBy::Source,
                })
                .expect("summary after rescan")
                .entry_count,
            3
        );
    }

    #[test]
    fn partial_tail_is_not_consumed_and_is_parsed_after_newline_arrives() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        let roots = fixture_roots(sources.path());
        let codex = roots.codex_sessions.as_ref().expect("codex root");
        fs::create_dir_all(codex).expect("codex dir");
        let path = codex.join("partial-session.jsonl");
        let session = r#"{"timestamp":"2026-07-30T01:00:00Z","type":"session_meta","payload":{"id":"partial-session"}}"#;
        let token = r#"{"timestamp":"2026-07-30T01:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2},"total_token_usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}}"#;
        fs::write(&path, format!("{session}\n{token}")).expect("partial fixture");
        let service = UsageService::new(config.path().to_path_buf());

        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("first scan");
        assert_eq!(
            service
                .summary(UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: crate::contracts::UsageGroupBy::Source,
                })
                .expect("empty summary")
                .entry_count,
            0
        );

        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open append")
            .write_all(b"\n")
            .expect("finish line");
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("resume scan");
        assert_eq!(
            service
                .summary(UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: crate::contracts::UsageGroupBy::Source,
                })
                .expect("resumed summary")
                .entry_count,
            1
        );
    }

    #[test]
    fn reset_replaces_facts_owned_by_the_rewritten_file() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        let roots = fixture_roots(sources.path());
        let codex = roots.codex_sessions.as_ref().expect("codex root");
        fs::create_dir_all(codex).expect("codex dir");
        let path = codex.join("rewrite-session.jsonl");
        fs::write(&path, CODEX_FIXTURE).expect("initial fixture");
        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("initial scan");

        let replacement = concat!(
            "{\"timestamp\":\"2026-07-30T03:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"replacement-session\"}}\n",
            "{\"timestamp\":\"2026-07-30T03:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3},\"total_token_usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}}\n"
        );
        fs::write(&path, replacement).expect("rewrite");
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("scan replacement");

        let summary = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 1);
        assert_eq!(summary.tokens.total_tokens, 3);
    }

    #[test]
    fn same_size_rewrite_resets_the_file_instead_of_trusting_its_offset() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        let roots = fixture_roots(sources.path());
        let codex = roots.codex_sessions.as_ref().expect("codex root");
        fs::create_dir_all(codex).expect("codex dir");
        let path = codex.join("same-size-session.jsonl");
        let initial = concat!(
            "{\"timestamp\":\"2026-07-30T04:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"same-a\"}}\n",
            "{\"timestamp\":\"2026-07-30T04:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2},\"total_token_usage\":{\"input_tokens\":1,\"output_tokens\":1,\"total_tokens\":2}}}}\n"
        );
        let replacement = concat!(
            "{\"timestamp\":\"2026-07-30T05:00:00Z\",\"type\":\"session_meta\",\"payload\":{\"id\":\"same-b\"}}\n",
            "{\"timestamp\":\"2026-07-30T05:01:00Z\",\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"last_token_usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3},\"total_token_usage\":{\"input_tokens\":2,\"output_tokens\":1,\"total_tokens\":3}}}}\n"
        );
        assert_eq!(initial.len(), replacement.len());
        fs::write(&path, initial).expect("initial fixture");
        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("initial scan");

        fs::write(&path, replacement).expect("same-size rewrite");
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("replacement scan");

        let summary = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 1);
        assert_eq!(summary.tokens.total_tokens, 3);
    }

    #[test]
    fn offsets_above_sqlite_integer_range_are_rejected_as_invalid_queries() {
        let config = tempfile::tempdir().expect("config");
        let service = UsageService::new(config.path().to_path_buf());

        assert!(matches!(
            service.conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                search: None,
                limit: Some(1),
                offset: Some(u64::MAX),
                ..UsageConversationQuery::default()
            }),
            Err(UsageError::InvalidQuery)
        ));
    }

    #[test]
    fn pi_fixture_scan_counts_own_cost_and_deduplicates_entry_keys() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        seed_pi_root(sources.path());
        let service = UsageService::new(config.path().to_path_buf());

        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("scan pi fixture");

        let summary = service
            .summary(UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        // assistant×3 + compaction×1；重复 assistant-msg-1 被全局去重键拦截。
        assert_eq!(summary.entry_count, 4);
        assert_eq!(summary.tokens.total_tokens, 485);
        assert_eq!(summary.tokens.reasoning_output_tokens, 8);
        // 19600 + 8540 + 2800 + 42000
        assert_eq!(summary.cost.api_equivalent_cost_nanos, 72_940);
        assert_eq!(summary.cost.priced_entries, 4);
        assert_eq!(summary.fast.raw_tokens, 0);
        assert_eq!(summary.tokens.cache_write_5m_input_tokens, 50);

        let pi_row = summary
            .rows
            .iter()
            .find(|row| row.key == "pi")
            .expect("pi row");
        assert_eq!(pi_row.cost.api_equivalent_cost_nanos, 72_940);

        let page = service
            .conversations(UsageConversationQuery {
                filter: UsageFilter::default(),
                search: None,
                limit: Some(10),
                offset: Some(0),
                ..UsageConversationQuery::default()
            })
            .expect("conversations");
        assert_eq!(page.total, 1);
        let pi_conversation = &page.items[0];
        assert_eq!(pi_conversation.source, UsageSource::Pi);
        assert_eq!(pi_conversation.project_hint.as_deref(), Some("cc-trace"));
        assert_eq!(
            pi_conversation.title.as_deref(),
            Some("system>说明 实现一个函数")
        );
        assert_eq!(pi_conversation.entry_count, 4);
    }

    fn wait_for_idle(service: &UsageService) {
        for _ in 0..200 {
            if service.scan_status().state == UsageScanState::Idle {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("scan did not finish in time");
    }

    #[test]
    fn rebuild_rescans_fixtures_to_an_identical_summary() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        seed_fixture_roots(sources.path());
        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_scan_inner(fixture_roots(sources.path()))
            .expect("initial scan");
        let query = UsageSummaryQuery {
            filter: UsageFilter::default(),
            group_by: crate::contracts::UsageGroupBy::Source,
        };
        let before = service.summary(query.clone()).expect("before summary");

        let status = service
            .rebuild_with_roots(None, fixture_roots(sources.path()))
            .expect("rebuild");
        assert_eq!(status.state, UsageScanState::Running);
        wait_for_idle(&service);

        let after = service.summary(query).expect("after summary");
        assert_eq!(before.entry_count, after.entry_count);
        assert_eq!(before.tokens.total_tokens, after.tokens.total_tokens);
        assert_eq!(
            before.cost.api_equivalent_cost_nanos,
            after.cost.api_equivalent_cost_nanos
        );
    }

    #[test]
    fn rebuild_while_scanning_is_rejected_as_busy() {
        let config = tempfile::tempdir().expect("config");
        let service = UsageService::new(config.path().to_path_buf());
        service.status.lock().expect("status").state = UsageScanState::Running;

        assert_eq!(service.rebuild(None), Err(UsageError::ScanBusy));
    }

    #[cfg(feature = "perf-baseline")]
    #[test]
    fn perf_baseline_scan_checks_once_per_logical_write_not_per_batch() {
        let config = tempfile::tempdir().expect("config");
        let sources = tempfile::tempdir().expect("sources");
        seed_fixture_roots(sources.path());
        seed_pi_root(sources.path());
        let roots = fixture_roots(sources.path());
        let service = UsageService::new(config.path().to_path_buf());
        service
            .run_benchmark_scan(BenchmarkRoots {
                codex_sessions: roots.codex_sessions.clone(),
                codex_archived: roots.codex_archived.clone(),
                claude_projects: roots.claude_projects.clone(),
                pi_sessions: roots.pi_sessions.clone(),
                opencode_db: roots.opencode_db.clone(),
                codex_title_index: roots.codex_title_index.clone(),
                claude_history: roots.claude_history.clone(),
            })
            .expect("scan");
        let first = service.perf_stats();
        assert_eq!(
            first.quick_checks, 1,
            "全新库：初始化不检查，仅扫描结束边界检查 1 次"
        );
        assert!(first.batch_commits >= 1, "扫描至少产生一个批次");

        service
            .run_benchmark_scan(BenchmarkRoots {
                codex_sessions: roots.codex_sessions,
                codex_archived: roots.codex_archived,
                claude_projects: roots.claude_projects,
                pi_sessions: roots.pi_sessions,
                opencode_db: roots.opencode_db,
                codex_title_index: roots.codex_title_index,
                claude_history: roots.claude_history,
            })
            .expect("unchanged rescan");
        let second = service.perf_stats();
        assert_eq!(
            second.quick_checks, 2,
            "已有库：初始化只 1 次（幂等），第二次扫描仍只有 1 次边界检查"
        );
    }
}
