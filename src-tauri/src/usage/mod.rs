//! Codex／Claude Code 本地 JSONL 的只读索引。
//!
//! 扫描只读外部文件，所有派生数据写入 CC Trace 自己的 SQLite。command 只接收固定
//! 查询参数，不接收路径；测试通过显式临时根目录覆盖，避免触碰真实用户数据。

pub(crate) mod model;
mod parser;
pub mod pricing;
mod pricing_remote;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, SecondsFormat, Utc};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::contracts::{
    PricingCatalogRefreshStatus, ProviderId, QuotaHistory, QuotaHistoryQuery, QuotaSnapshot,
    UsageConversation, UsageConversationBreakdown, UsageConversationPage, UsageConversationQuery,
    UsageFilter, UsageRepriceResult, UsageScanState, UsageScanStatus, UsageSource, UsageSummary,
    UsageSummaryQuery,
};
use crate::storage::{UsageDb, UsageDbError};

use model::{ClaudeCursor, CodexCursor, ParsedLine, PiCursor, ScanBatch};
use parser::{parse_claude_line, parse_codex_line, parse_pi_line};
use pricing::{
    PricingCatalog, PricingCatalogStore, PricingRefreshMode, PricingRefreshOutcome, PricingUsageKey,
};

const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
const BATCH_LINES: u64 = 2_000;
const BATCH_BYTES: u64 = 8 * 1024 * 1024;
const PREFIX_BYTES: u64 = 4_096;
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
    /// 串行化“开始扫描”与“提交价格 + 数据库重计价”，保证两者不会越过安全边界。
    lifecycle: Mutex<()>,
    reprice_pending: AtomicBool,
    cancel: AtomicBool,
}

impl UsageService {
    pub fn new(config_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            db: UsageDb::new(config_dir.clone()),
            pricing: PricingCatalogStore::new(config_dir),
            status: Mutex::new(UsageScanStatus::default()),
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
            if self.scan_file(&file, &catalog, previous).is_err() {
                let mut status = self.status.lock().expect("usage scan status");
                status.failed_files = status.failed_files.saturating_add(1);
                status.partial_failure = true;
            }
            self.status
                .lock()
                .expect("usage scan status")
                .completed_files += 1;
        }
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

    fn scan_file(
        &self,
        file: &SourceFile,
        catalog: &PricingCatalog,
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
                    UsageSource::Codex => parse_codex_line(&line, &mut codex_cursor, catalog),
                    UsageSource::Claude => parse_claude_line(&line, &mut claude_cursor, catalog),
                    UsageSource::Pi => parse_pi_line(&line, &mut pi_cursor, &pi_filename_key),
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

struct ScanRoots {
    codex_sessions: Option<PathBuf>,
    codex_archived: Option<PathBuf>,
    claude_projects: Option<PathBuf>,
    pi_sessions: Option<PathBuf>,
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
    const CLAUDE_FIXTURE: &[u8] = include_bytes!("../../../fixtures/usage/claude/project.jsonl");
    const PI_FIXTURE: &[u8] = include_bytes!("../../../fixtures/usage/pi/session.jsonl");

    fn fixture_roots(base: &Path) -> ScanRoots {
        ScanRoots {
            codex_sessions: Some(base.join("codex/sessions")),
            codex_archived: Some(base.join("codex/archived")),
            claude_projects: Some(base.join("claude/projects")),
            pi_sessions: Some(base.join("pi/sessions")),
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
}
