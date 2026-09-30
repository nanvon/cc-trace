//! 本地用量 SQLite。
//!
//! 路径只用于打开 CC Trace 自己的数据库，从不进入错误、日志或 command 载荷。
//! 外部 JSONL 路径只以 SHA-256 `file_key` 出现在表中。

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(feature = "perf-baseline")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::contracts::{
    ProviderId, QuotaHistoryEvent, QuotaSnapshot, QuotaWindowKind, UsageConversation,
    UsageConversationBreakdown, UsageConversationPage, UsageConversationProjectOption,
    UsageConversationQuery, UsageConversationSort, UsageCostTotals, UsageFastTotals, UsageGroupBy,
    UsageProjectPage, UsageProjectQuery, UsageProjectSort, UsageProjectSummary, UsageRepriceResult,
    UsageSource, UsageSpeed, UsageSummary, UsageSummaryQuery, UsageSummaryRow, UsageTokenTotals,
    decimal_nanos_string,
};
use crate::usage::model::{
    InferenceGeo, OpencodeScanState, RepriceRow, ScanBatch, ScanFileState, TokenFacts,
};
use crate::usage::pricing::{PricingCatalog, PricingUsageKey};

const DATABASE_FILE: &str = "usage.db";
const SCHEMA_VERSION: i64 = 9;

#[derive(Debug)]
pub enum UsageDbError {
    Io,
    Sql,
    UnsupportedSchema,
    Recovery,
}

impl From<std::io::Error> for UsageDbError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}

impl From<rusqlite::Error> for UsageDbError {
    fn from(_: rusqlite::Error) -> Self {
        Self::Sql
    }
}

pub struct CommitResult {
    pub inserted: u64,
    pub duplicates: u64,
}

pub struct RebuildResult {
    pub entries_removed: u64,
    pub conversations_removed: u64,
    pub files_removed: u64,
}

struct QuotaEventBackup {
    provider: String,
    identity_key: String,
    window_kind: String,
    window_id: Option<String>,
    remaining_percent: i64,
    observed_at: String,
    resets_at: Option<String>,
}

pub struct UsageDb {
    directory: PathBuf,
    write_lock: Mutex<()>,
    initialized: AtomicBool,
    #[cfg(feature = "perf-baseline")]
    perf: PerfCounters,
}

/// 性能基线统计快照（`perf-baseline` feature 门控，见 docs/性能与功耗优化方案.md 阶段 0）。
#[cfg(feature = "perf-baseline")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PerfStats {
    pub write_opens: u64,
    pub quick_checks: u64,
    pub batch_commits: u64,
    pub batch_commit_nanos: u64,
    pub quota_snapshots: u64,
}

#[cfg(feature = "perf-baseline")]
#[derive(Default)]
struct PerfCounters {
    write_opens: AtomicU64,
    quick_checks: AtomicU64,
    batch_commits: AtomicU64,
    batch_commit_nanos: AtomicU64,
    quota_snapshots: AtomicU64,
}

impl UsageDb {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            write_lock: Mutex::new(()),
            initialized: AtomicBool::new(false),
            #[cfg(feature = "perf-baseline")]
            perf: PerfCounters::default(),
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(DATABASE_FILE)
    }

    /// 一次性初始化：已有库执行一次 `quick_check`（失败走损坏恢复）、校验并迁移 schema、
    /// 设置 WAL／synchronous／foreign_keys。幂等；之后的写连接不再重复检查与迁移。
    pub fn initialize(&self) -> Result<(), UsageDbError> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.write_lock.lock().expect("usage db write lock");
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        fs::create_dir_all(&self.directory)?;
        #[cfg(feature = "perf-baseline")]
        self.perf.write_opens.fetch_add(1, Ordering::Relaxed);
        let existed = self.path().exists();
        let mut connection = Connection::open(self.path())?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        if existed {
            #[cfg(feature = "perf-baseline")]
            self.perf.quick_checks.fetch_add(1, Ordering::Relaxed);
            let check = connection
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "failed".to_owned());
            if check != "ok" {
                drop(connection);
                self.recover_corrupt_database()?;
                connection = Connection::open(self.path())?;
                connection.busy_timeout(std::time::Duration::from_secs(5))?;
            }
        }

        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(UsageDbError::UnsupportedSchema);
        }
        if version < SCHEMA_VERSION {
            migrate(&mut connection, version)?;
        }
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        self.initialized.store(true, Ordering::Release);
        Ok(())
    }

    pub fn scan_file_state(&self, file_key: &str) -> Result<Option<ScanFileState>, UsageDbError> {
        let connection = self.open_read()?;
        connection
            .query_row(
                "SELECT mtime_ms, size_bytes, offset_bytes, prefix_fingerprint, cursor_json
                   FROM scan_files WHERE file_key = ?1",
                [file_key],
                |row| {
                    Ok(ScanFileState {
                        mtime_ms: row.get(0)?,
                        size_bytes: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                        offset_bytes: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                        prefix_fingerprint: row.get(3)?,
                        cursor_json: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(Into::into)
    }

    /// 一次读取全部文件水位，避免扫描上千个文件时为每个文件反复打开 SQLite 连接。
    pub fn scan_file_states(&self) -> Result<HashMap<String, ScanFileState>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement = connection.prepare(
            "SELECT file_key, mtime_ms, size_bytes, offset_bytes, prefix_fingerprint, cursor_json
               FROM scan_files",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ScanFileState {
                    mtime_ms: row.get(1)?,
                    size_bytes: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                    offset_bytes: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    prefix_fingerprint: row.get(4)?,
                    cursor_json: row.get(5)?,
                },
            ))
        })?;
        let mut output = HashMap::new();
        for row in rows {
            let (file_key, state) = row?;
            output.insert(file_key, state);
        }
        Ok(output)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_scan_batch(
        &self,
        file_key: &str,
        source: UsageSource,
        mtime_ms: i64,
        size_bytes: u64,
        offset_bytes: u64,
        prefix_fingerprint: &str,
        cursor_json: Option<&str>,
        reset_file: bool,
        batch: &ScanBatch,
    ) -> Result<CommitResult, UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        #[cfg(feature = "perf-baseline")]
        let batch_started = std::time::Instant::now();
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;
        let mut inserted = 0_u64;

        if reset_file {
            transaction.execute("DELETE FROM usage_entries WHERE file_key = ?1", [file_key])?;
            transaction.execute(
                "DELETE FROM conversations
                  WHERE NOT EXISTS (
                    SELECT 1 FROM usage_entries
                     WHERE usage_entries.conversation_key = conversations.conversation_key
                  )",
                [],
            )?;
        }

        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO usage_entries (
                   file_key, source, dedup_key, conversation_key, model, speed, inference_geo,
                   occurred_at, day_local, uncached_input_tokens, output_tokens,
                   reasoning_output_tokens, cache_read_input_tokens,
                   cache_write_5m_input_tokens, cache_write_1h_input_tokens,
                   api_equivalent_cost_nanos, billing_equivalent_tokens_nanos,
                   fast_multiplier_nanos, pricing_fingerprint, request_count, granularity
                 ) VALUES (
                   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                   ?16, ?17, ?18, ?19, ?20, ?21
                 )
                 ON CONFLICT(source, dedup_key) DO UPDATE SET
                   file_key = excluded.file_key,
                   conversation_key = excluded.conversation_key,
                   model = excluded.model,
                   speed = excluded.speed,
                   inference_geo = excluded.inference_geo,
                   occurred_at = excluded.occurred_at,
                   day_local = excluded.day_local,
                   uncached_input_tokens = excluded.uncached_input_tokens,
                   output_tokens = excluded.output_tokens,
                   reasoning_output_tokens = excluded.reasoning_output_tokens,
                   cache_read_input_tokens = excluded.cache_read_input_tokens,
                   cache_write_5m_input_tokens = excluded.cache_write_5m_input_tokens,
                   cache_write_1h_input_tokens = excluded.cache_write_1h_input_tokens,
                   api_equivalent_cost_nanos = excluded.api_equivalent_cost_nanos,
                   billing_equivalent_tokens_nanos = excluded.billing_equivalent_tokens_nanos,
                   fast_multiplier_nanos = excluded.fast_multiplier_nanos,
                   pricing_fingerprint = excluded.pricing_fingerprint,
                   request_count = excluded.request_count,
                   granularity = excluded.granularity
                 WHERE (
                   excluded.source NOT IN ('codex', 'claude')
                   OR (
                     excluded.uncached_input_tokens
                     + excluded.cache_read_input_tokens
                     + excluded.cache_write_5m_input_tokens
                     + excluded.cache_write_1h_input_tokens
                     + excluded.output_tokens
                   ) > (
                     usage_entries.uncached_input_tokens
                     + usage_entries.cache_read_input_tokens
                     + usage_entries.cache_write_5m_input_tokens
                     + usage_entries.cache_write_1h_input_tokens
                     + usage_entries.output_tokens
                   )
                 )",
            )?;
            for entry in &batch.entries {
                inserted += u64::try_from(statement.execute(params![
                    file_key,
                    entry.source.as_db(),
                    entry.dedup_key,
                    entry.conversation_key,
                    entry.model,
                    entry.speed.as_db(),
                    entry.inference_geo.as_db(),
                    entry.occurred_at,
                    entry.day_local,
                    entry.tokens.uncached_input_tokens,
                    entry.tokens.output_tokens,
                    entry.tokens.reasoning_output_tokens,
                    entry.tokens.cache_read_input_tokens,
                    entry.tokens.cache_write_5m_input_tokens,
                    entry.tokens.cache_write_1h_input_tokens,
                    entry.api_equivalent_cost_nanos,
                    entry.billing_equivalent_tokens_nanos,
                    entry.fast_multiplier_nanos,
                    entry.pricing_fingerprint,
                    entry.request_count,
                    entry.granularity.as_db(),
                ])?)
                .unwrap_or(0);
            }
        }

        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO conversations (
                   conversation_key, source, title, project_hint, project_key, worktree_path,
                   is_sidechain, unattributed, first_at, last_at, source_id, branch
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11)
                 ON CONFLICT(conversation_key) DO UPDATE SET
                   title = COALESCE(excluded.title, conversations.title),
                   project_hint = COALESCE(excluded.project_hint, conversations.project_hint),
                   project_key = COALESCE(excluded.project_key, conversations.project_key),
                   worktree_path = COALESCE(excluded.worktree_path, conversations.worktree_path),
                   is_sidechain = MAX(conversations.is_sidechain, excluded.is_sidechain),
                   unattributed = MAX(conversations.unattributed, excluded.unattributed),
                   first_at = MIN(conversations.first_at, excluded.first_at),
                   last_at = MAX(conversations.last_at, excluded.last_at),
                   source_id = COALESCE(excluded.source_id, conversations.source_id),
                   branch = COALESCE(excluded.branch, conversations.branch)",
            )?;
            for conversation in &batch.conversations {
                statement.execute(params![
                    conversation.conversation_key,
                    conversation.source.as_db(),
                    conversation.title,
                    conversation.project_hint,
                    conversation.project_key,
                    conversation.worktree_path,
                    i64::from(conversation.is_sidechain),
                    i64::from(conversation.unattributed),
                    conversation.occurred_at,
                    conversation.source_id,
                    conversation.branch,
                ])?;
            }
        }

        transaction.execute(
            "INSERT INTO scan_files (
               file_key, source, mtime_ms, size_bytes, offset_bytes,
               prefix_fingerprint, cursor_json, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(file_key) DO UPDATE SET
               source = excluded.source,
               mtime_ms = excluded.mtime_ms,
               size_bytes = excluded.size_bytes,
               offset_bytes = excluded.offset_bytes,
               prefix_fingerprint = excluded.prefix_fingerprint,
               cursor_json = excluded.cursor_json,
               updated_at = excluded.updated_at",
            params![
                file_key,
                source.as_db(),
                mtime_ms,
                i64::try_from(size_bytes).map_err(|_| UsageDbError::Sql)?,
                i64::try_from(offset_bytes).map_err(|_| UsageDbError::Sql)?,
                prefix_fingerprint,
                cursor_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        transaction.commit()?;

        #[cfg(feature = "perf-baseline")]
        {
            self.perf.batch_commits.fetch_add(1, Ordering::Relaxed);
            let elapsed = u64::try_from(batch_started.elapsed().as_nanos()).unwrap_or(0);
            self.perf
                .batch_commit_nanos
                .fetch_add(elapsed, Ordering::Relaxed);
        }

        Ok(CommitResult {
            inserted,
            duplicates: batch.entries.len() as u64 - inserted,
        })
    }

    /// 重建指定数据源：删除用量条目、无引用的对话与扫描水位，调用方随后全量重扫。
    /// `sources` 为 `None` 或空时重建全部数据源；含 OpenCode 时一并重置其增量水位。
    pub fn rebuild_data(
        &self,
        sources: Option<&[UsageSource]>,
    ) -> Result<RebuildResult, UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;
        // source 值为静态枚举常量，不来自 command 输入，拼接无注入风险。
        let condition = match sources.filter(|list| !list.is_empty()) {
            Some(list) => {
                let values = list
                    .iter()
                    .map(|source| source.as_db())
                    .collect::<Vec<_>>()
                    .join("', '");
                format!("source IN ('{values}')")
            }
            None => "1 = 1".to_owned(),
        };
        let entries_removed =
            transaction.execute(&format!("DELETE FROM usage_entries WHERE {condition}"), [])?;
        let conversations_removed = transaction.execute(
            "DELETE FROM conversations
              WHERE NOT EXISTS (
                SELECT 1 FROM usage_entries
                 WHERE usage_entries.conversation_key = conversations.conversation_key
              )",
            [],
        )?;
        let files_removed =
            transaction.execute(&format!("DELETE FROM scan_files WHERE {condition}"), [])?;
        if sources.is_none_or(|list| list.contains(&UsageSource::Opencode)) {
            transaction.execute("DELETE FROM opencode_state", [])?;
        }
        transaction.commit()?;
        self.verify_after_logical_write_locked()?;
        Ok(RebuildResult {
            entries_removed: u64::try_from(entries_removed).map_err(|_| UsageDbError::Sql)?,
            conversations_removed: u64::try_from(conversations_removed)
                .map_err(|_| UsageDbError::Sql)?,
            files_removed: u64::try_from(files_removed).map_err(|_| UsageDbError::Sql)?,
        })
    }

    // ---- DSH 会话归属与贡献汇总（schema v8） ----

    /// 写入或更新会话归属信息。
    ///
    /// 父会话与标题都按「新值非空才覆盖」合并：同一会话在后续轮次里可能只写出部分字段，
    /// 用空值覆盖会把已有的父子关系抹掉，子代理就再也归不到根上。
    pub fn dsh_upsert_sessions(&self, rows: &[DshSessionUpsert]) -> Result<(), UsageDbError> {
        if rows.is_empty() {
            return Ok(());
        }
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO dsh_sessions (
                   session_id, parent_session, cwd, title, project_key, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(session_id) DO UPDATE SET
                   parent_session = COALESCE(excluded.parent_session, dsh_sessions.parent_session),
                   cwd = COALESCE(excluded.cwd, dsh_sessions.cwd),
                   title = COALESCE(excluded.title, dsh_sessions.title),
                   project_key = COALESCE(excluded.project_key, dsh_sessions.project_key),
                   updated_at = excluded.updated_at",
            )?;
            for row in rows {
                statement.execute(params![
                    row.session_id,
                    row.parent_session,
                    row.cwd,
                    row.title,
                    row.project_key,
                    row.updated_at,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// 全部会话的父子关系，供父链解析使用。
    pub fn dsh_session_parents(&self) -> Result<Vec<(String, Option<String>)>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement =
            connection.prepare("SELECT session_id, parent_session FROM dsh_sessions")?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 写入解析出的根会话。`None` 表示「缺失」，此时保留旧值。
    pub fn dsh_set_roots(&self, roots: &[(String, Option<String>)]) -> Result<(), UsageDbError> {
        if roots.is_empty() {
            return Ok(());
        }
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;
        {
            let mut statement = transaction.prepare(
                "UPDATE dsh_sessions SET root_session = COALESCE(?2, root_session)
                  WHERE session_id = ?1",
            )?;
            for (session_id, root) in roots {
                statement.execute(params![session_id, root])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// 把一个子会话的条目与对话行重挂到根会话。返回是否发生了移动。
    ///
    /// 对话行按「根的信息优先、子会话只补空」合并：根自己的标题与项目不能被
    /// 子会话的取值覆盖，否则合并后列表上会出现子会话的标题。
    pub fn dsh_rekey_session(&self, from_key: &str, to_key: &str) -> Result<bool, UsageDbError> {
        if from_key == to_key {
            return Ok(false);
        }
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;

        let moved_entries = transaction.execute(
            "UPDATE usage_entries SET conversation_key = ?2 WHERE conversation_key = ?1",
            params![from_key, to_key],
        )?;

        let exists: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM conversations WHERE conversation_key = ?1",
            params![from_key],
            |row| row.get(0),
        )?;
        if exists > 0 {
            transaction.execute(
                "INSERT INTO conversations (
                   conversation_key, source, title, project_hint, project_key, worktree_path,
                   is_sidechain, unattributed, first_at, last_at, source_id, branch
                 )
                 SELECT ?2, source, title, project_hint, project_key, worktree_path,
                        1, unattributed, first_at, last_at, source_id, branch
                   FROM conversations WHERE conversation_key = ?1
                 ON CONFLICT(conversation_key) DO UPDATE SET
                   title = COALESCE(conversations.title, excluded.title),
                   project_hint = COALESCE(conversations.project_hint, excluded.project_hint),
                   project_key = COALESCE(conversations.project_key, excluded.project_key),
                   worktree_path = COALESCE(conversations.worktree_path, excluded.worktree_path),
                   is_sidechain = MAX(conversations.is_sidechain, excluded.is_sidechain),
                   unattributed = MAX(conversations.unattributed, excluded.unattributed),
                   first_at = MIN(conversations.first_at, excluded.first_at),
                   last_at = MAX(conversations.last_at, excluded.last_at),
                   source_id = COALESCE(conversations.source_id, excluded.source_id),
                   branch = COALESCE(conversations.branch, excluded.branch)",
                params![from_key, to_key],
            )?;
            transaction.execute(
                "DELETE FROM conversations WHERE conversation_key = ?1",
                params![from_key],
            )?;
        }

        transaction.commit()?;
        Ok(moved_entries > 0)
    }

    /// 重算一个会话的贡献汇总（按自然日 × 模型 × 速度）。
    ///
    /// 汇总来自 `usage_entries`，是「重建后仍能还原历史」的唯一依据；
    /// 因此重建前必须已经把汇总写好。
    pub fn dsh_recompute_rollup(&self, session_id: &str) -> Result<(), UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let conversation_key = format!("dsh:{session_id}");
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM dsh_session_usage WHERE session_id = ?1",
            params![session_id],
        )?;
        transaction.execute(
            "INSERT INTO dsh_session_usage (
               session_id, day_local, model, speed, request_count,
               uncached_input_tokens, output_tokens, reasoning_output_tokens,
               cache_read_input_tokens, cache_write_5m_input_tokens,
               cache_write_1h_input_tokens, api_equivalent_cost_nanos, updated_at
             )
             SELECT ?1, day_local, COALESCE(model, ''), speed,
                    COALESCE(SUM(request_count), 0),
                    COALESCE(SUM(uncached_input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(reasoning_output_tokens), 0),
                    COALESCE(SUM(cache_read_input_tokens), 0),
                    COALESCE(SUM(cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(api_equivalent_cost_nanos), 0),
                    strftime('%Y-%m-%dT%H:%M:%SZ', 'now')
               FROM usage_entries
              WHERE conversation_key = ?2
              GROUP BY day_local, COALESCE(model, ''), speed",
            params![session_id, conversation_key],
        )?;
        transaction.commit()?;
        Ok(())
    }

    /// 删掉某个会话的「汇总物化」条目。日志回来了就用逐请求条目，两者不能共存。
    pub fn dsh_drop_contribution_entries(&self, session_id: &str) -> Result<u64, UsageDbError> {
        let prefix = contribution_dedup_prefix(session_id);
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let connection = self.open_write_unchecked()?;
        let removed = connection.execute(
            "DELETE FROM usage_entries
              WHERE source = 'dsh' AND substr(dedup_key, 1, ?1) = ?2",
            params![prefix.len() as i64, prefix],
        )?;
        u64::try_from(removed).map_err(|_| UsageDbError::Sql)
    }

    /// 源日志已经不在、但汇总还在的会话，按天粒度把历史物化回 `usage_entries`。
    ///
    /// 只在「这个会话一条条目都没有」时物化：日志还在时扫描会写出逐请求条目，
    /// 再补一份按天的就是双份。返回物化的行数。
    ///
    /// `occurred_at` 取「本地日零点换算成的 UTC 时刻」：按天粒度的事实只有自然日一个
    /// 时间锚点，直接写 `T00:00:00Z` 会让西半球时区的行落到当天的查询窗口之外。
    /// SQLite 的 `'utc'` 修饰符按当前时区换算，跨夏令时的历史日期可能差一小时——
    /// 对按天的账不影响总量，只影响极端时区下的边界归日。
    pub fn dsh_materialize_missing(&self) -> Result<u64, UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;

        let sessions: Vec<String> = {
            let mut statement = transaction.prepare(
                "SELECT r.session_id
                   FROM dsh_session_usage r
                  WHERE r.request_count > 0
                    AND NOT EXISTS (
                          SELECT 1 FROM usage_entries e
                           WHERE e.conversation_key = 'dsh:' || r.session_id
                        )
                  GROUP BY r.session_id",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut inserted = 0_u64;
        for session_id in sessions {
            let file_key = format!("dsh:contribution:{session_id}");
            let dedup_prefix = contribution_dedup_prefix(&session_id);
            let conversation_key = format!("dsh:{session_id}");
            inserted += transaction.execute(
                "INSERT OR REPLACE INTO usage_entries (
                   file_key, source, dedup_key, conversation_key, model, speed,
                   inference_geo, occurred_at, day_local, uncached_input_tokens,
                   output_tokens, reasoning_output_tokens, cache_read_input_tokens,
                   cache_write_5m_input_tokens, cache_write_1h_input_tokens,
                   api_equivalent_cost_nanos, billing_equivalent_tokens_nanos,
                   fast_multiplier_nanos, pricing_fingerprint, request_count,
                   granularity
                 )
                 SELECT ?1, 'dsh',
                        ?2 || r.day_local || ':' || r.model || ':' || r.speed,
                        ?3, r.model, r.speed, 'unknown',
                        strftime('%Y-%m-%dT%H:%M:%SZ', r.day_local || ' 00:00:00', 'utc'),
                        r.day_local,
                        r.uncached_input_tokens, r.output_tokens,
                        r.reasoning_output_tokens, r.cache_read_input_tokens,
                        r.cache_write_5m_input_tokens, r.cache_write_1h_input_tokens,
                        r.api_equivalent_cost_nanos, NULL, NULL, NULL,
                        r.request_count, 'day'
                   FROM dsh_session_usage r
                  WHERE r.session_id = ?4 AND r.request_count > 0",
                params![file_key, dedup_prefix, conversation_key, session_id],
            )? as u64;
        }

        transaction.commit()?;
        Ok(inserted)
    }

    // ---- Cursor 远端计量（schema v9） ----

    /// 某个账号已经拉全的自然日。
    pub fn cursor_coverage(&self, account_key: &str) -> Result<Vec<String>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement = connection.prepare(
            "SELECT day_local FROM cursor_usage_coverage
              WHERE account_key = ?1 ORDER BY day_local",
        )?;
        let rows = statement.query_map(params![account_key], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 覆盖表里的全部账号键。账号换了要整体重拉，所以需要知道之前记的是谁。
    pub fn cursor_coverage_accounts(&self) -> Result<Vec<String>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT account_key FROM cursor_usage_coverage ORDER BY account_key",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 按自然日原子替换一个账号的远端计费用量。
    ///
    /// 替换单位是「天」而不是「事件」：服务端不提供稳定事件 id，只有整天替换才能
    /// 保证重复拉取不会把同一天算两遍。调用方必须传**一整天的桶**（`day_local`
    /// 落在 `[from_day, to_day]` 闭区间内），函数只删这些天，不动其他天。
    ///
    /// 同时更新覆盖表：一个自然日只有真的拉全了才会被调用方传进来。
    pub fn cursor_replace_days(
        &self,
        account_key: &str,
        from_day: &str,
        to_day: &str,
        conversation_key: &str,
        buckets: &[CursorRemoteBucket],
    ) -> Result<CursorReplaceResult, UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;

        let removed = transaction.execute(
            "DELETE FROM usage_entries
              WHERE source = 'cursor' AND day_local >= ?1 AND day_local <= ?2",
            params![from_day, to_day],
        )?;

        let mut inserted = 0_u64;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO usage_entries (
                   file_key, source, dedup_key, conversation_key, model, speed,
                   inference_geo, occurred_at, day_local, uncached_input_tokens,
                   output_tokens, reasoning_output_tokens, cache_read_input_tokens,
                   cache_write_5m_input_tokens, cache_write_1h_input_tokens,
                   api_equivalent_cost_nanos, billing_equivalent_tokens_nanos,
                   fast_multiplier_nanos, pricing_fingerprint, request_count,
                   granularity
                 ) VALUES (
                   ?1, 'cursor', ?2, ?3, ?4, 'standard', 'unknown', ?5, ?6, ?7, ?8, 0, ?9,
                   ?10, 0, ?11, NULL, NULL, NULL, ?12, 'day'
                 )",
            )?;
            let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            let _ = now;
            for bucket in buckets {
                let occurred_at = local_day_start_utc(&bucket.day_local);
                inserted += statement.execute(params![
                    format!("cursor:remote:{account_key}"),
                    format!(
                        "cursor-day:{account_key}:{}:{}",
                        bucket.day_local, bucket.model
                    ),
                    conversation_key,
                    bucket.model,
                    occurred_at,
                    bucket.day_local,
                    bucket.input_tokens,
                    bucket.output_tokens,
                    bucket.cache_read_tokens,
                    bucket.cache_write_tokens,
                    bucket.charged_nanos,
                    bucket.request_count,
                ])? as u64;
            }
        }

        transaction.execute(
            "DELETE FROM cursor_usage_coverage
              WHERE account_key = ?1 AND day_local >= ?2 AND day_local <= ?3",
            params![account_key, from_day, to_day],
        )?;
        {
            let mut statement = transaction.prepare(
                "INSERT OR REPLACE INTO cursor_usage_coverage (account_key, day_local, updated_at)
                 VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            )?;
            let mut day = from_day.to_owned();
            loop {
                statement.execute(params![account_key, day])?;
                if day.as_str() >= to_day {
                    break;
                }
                day = next_day(&day).ok_or(UsageDbError::Sql)?;
            }
        }

        transaction.commit()?;
        Ok(CursorReplaceResult {
            removed: u64::try_from(removed).map_err(|_| UsageDbError::Sql)?,
            inserted,
        })
    }

    /// 换账号时清掉旧账号的远端计量与覆盖状态：两个账号的用量不能混在一张账上。
    pub fn cursor_reset_account(&self, account_key: &str) -> Result<(), UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM usage_entries
              WHERE source = 'cursor' AND file_key = ?1",
            params![format!("cursor:remote:{account_key}")],
        )?;
        transaction.execute(
            "DELETE FROM cursor_usage_coverage WHERE account_key = ?1",
            params![account_key],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn summary(&self, query: &UsageSummaryQuery) -> Result<UsageSummary, UsageDbError> {
        let connection = self.open_read()?;
        let filter = &query.filter;
        // 项目维度与项目过滤都需要对话表的项目身份；其余维度不必为聚合多付一次 join。
        let needs_conversations =
            matches!(query.group_by, UsageGroupBy::Project) || filter.project.is_some();
        let group = match query.group_by {
            UsageGroupBy::Day => "e.day_local".to_owned(),
            // SQLite 的 `%W` 以周一为周首日，与 cc-bar 的周起点口径一致。
            UsageGroupBy::Week => "strftime('%Y-W%W', e.day_local)".to_owned(),
            UsageGroupBy::Month => "substr(e.day_local, 1, 7)".to_owned(),
            UsageGroupBy::Source => "e.source".to_owned(),
            UsageGroupBy::Model => "COALESCE(e.model, '')".to_owned(),
            UsageGroupBy::Speed => "e.speed".to_owned(),
            UsageGroupBy::Project => "COALESCE(c.project_key, '')".to_owned(),
            // 提供商归属是模型名的派生，不在 SQL 里重写一套前缀规则，
            // 按模型聚合后由 `UsageService` 在 Rust 侧归并。
            UsageGroupBy::Provider => "COALESCE(e.model, '')".to_owned(),
        };
        let join = if needs_conversations {
            "LEFT JOIN conversations c
               ON c.conversation_key = e.conversation_key"
        } else {
            ""
        };
        // 没有 join 时不能引用对话表；参数位号保持不变，用永假条件占位。
        let project_predicate = if needs_conversations {
            "AND (?6 IS NULL OR COALESCE(c.project_key, '') = ?6)"
        } else {
            "AND (?6 IS NULL)"
        };
        let sql = format!(
            "SELECT {group}, COUNT(*), COALESCE(SUM(e.request_count), 0),
                    COALESCE(SUM(e.uncached_input_tokens), 0),
                    COALESCE(SUM(e.output_tokens), 0),
                    COALESCE(SUM(e.reasoning_output_tokens), 0),
                    COALESCE(SUM(e.cache_read_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(CASE WHEN e.speed = 'fast' THEN
                        e.uncached_input_tokens + e.output_tokens + e.cache_read_input_tokens
                        + e.cache_write_5m_input_tokens + e.cache_write_1h_input_tokens
                    ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.speed = 'fast'
                        THEN e.billing_equivalent_tokens_nanos ELSE 0 END), 0),
                    MIN(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                    MAX(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                    SUM(CASE WHEN e.speed = 'fast'
                             AND e.billing_equivalent_tokens_nanos IS NULL THEN 1 ELSE 0 END),
                    COALESCE(SUM(e.api_equivalent_cost_nanos), 0),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.source = 'claude' AND e.inference_geo = 'unknown'
                             AND e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    CASE WHEN COUNT(DISTINCT e.pricing_fingerprint) = 1
                         THEN MAX(e.pricing_fingerprint) END
               FROM usage_entries e
               {join}
              WHERE (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.model = ?3)
                AND (?4 IS NULL OR e.speed = ?4)
                AND (?5 IS NULL OR e.source IN (SELECT value FROM json_each(?5)))
                {project_predicate}
              GROUP BY {group}
              ORDER BY {group}"
        );

        let mut statement = connection.prepare(&sql)?;
        let speed = filter.speed.map(UsageSpeed::as_db);
        let sources_json = sources_json(filter.sources.as_deref());
        let mut rows = statement.query(params![
            filter.from.as_deref(),
            filter.to.as_deref(),
            filter.model.as_deref(),
            speed,
            sources_json,
            filter.project.as_deref(),
        ])?;
        let mut output = Vec::new();
        while let Some(row) = rows.next()? {
            output.push(summary_row(row)?);
        }

        let mut total_tokens = UsageTokenTotals::default();
        let mut total_fast = UsageFastTotals::default();
        let mut total_cost = UsageCostTotals::default();
        let mut entry_count = 0_i64;
        let mut request_count = 0_i64;
        let mut fingerprints = HashSet::new();
        let mut mixed_pricing_versions = false;
        for row in &output {
            entry_count += row.entry_count;
            request_count += row.request_count;
            total_tokens.add_assign(&row.tokens);
            total_fast.add_assign(&row.fast);
            total_cost.api_equivalent_cost_nanos += row.cost.api_equivalent_cost_nanos;
            total_cost.priced_entries += row.cost.priced_entries;
            total_cost.unpriced_entries += row.cost.unpriced_entries;
            total_cost.assumed_geo_entries += row.cost.assumed_geo_entries;
            if row.cost.priced_entries > 0 {
                if let Some(fingerprint) = &row.cost.pricing_fingerprint {
                    fingerprints.insert(fingerprint.clone());
                } else {
                    mixed_pricing_versions = true;
                }
            }
        }
        total_cost.pricing_fingerprint = (!mixed_pricing_versions && fingerprints.len() == 1)
            .then(|| fingerprints.into_iter().next())
            .flatten();

        Ok(UsageSummary {
            rows: output,
            entry_count,
            request_count,
            tokens: total_tokens,
            fast: total_fast,
            cost: total_cost,
        })
    }

    /// 项目聚合：按项目身份汇总 Tokens、费用、对话数与活跃天数。
    ///
    /// 未归属项目（Cursor 远端计量、补录与早期按天汇总）的 `project_key` 为 NULL，
    /// 单独作为最后一行返回，不分摊到任何项目。
    pub fn projects(
        &self,
        query: &UsageProjectQuery,
        limit: u32,
        offset: u64,
    ) -> Result<UsageProjectPage, UsageDbError> {
        let connection = self.open_read()?;
        let filter = &query.filter;
        let sources_json = sources_json(filter.sources.as_deref());
        let escaped_search = query.search.as_deref().map(escape_like);
        let order = match query.sort.unwrap_or(UsageProjectSort::Recent) {
            UsageProjectSort::Recent => "MAX(e.occurred_at) DESC, project_key ASC",
            UsageProjectSort::Tokens => {
                "(COALESCE(SUM(e.uncached_input_tokens), 0)
                  + COALESCE(SUM(e.output_tokens), 0)
                  + COALESCE(SUM(e.cache_read_input_tokens), 0)
                  + COALESCE(SUM(e.cache_write_5m_input_tokens), 0)
                  + COALESCE(SUM(e.cache_write_1h_input_tokens), 0)) DESC, project_key ASC"
            }
            UsageProjectSort::Cost => {
                "COALESCE(SUM(e.api_equivalent_cost_nanos), 0) DESC, project_key ASC"
            }
        };

        let count = connection.query_row(
            "SELECT COUNT(*) FROM (
               SELECT COALESCE(c.project_key, '') AS project_key
                 FROM usage_entries e
                 LEFT JOIN conversations c ON c.conversation_key = e.conversation_key
                WHERE (?1 IS NULL OR e.occurred_at >= ?1)
                  AND (?2 IS NULL OR e.occurred_at < ?2)
                  AND (?3 IS NULL OR e.source IN (SELECT value FROM json_each(?3)))
                  AND (?4 IS NULL
                       OR COALESCE(c.project_key, '') LIKE '%' || ?4 || '%' ESCAPE '\\'
                       OR COALESCE(c.project_hint, '') LIKE '%' || ?4 || '%' ESCAPE '\\')
                GROUP BY COALESCE(c.project_key, '')
             )",
            params![
                filter.from.as_deref(),
                filter.to.as_deref(),
                sources_json,
                escaped_search.as_deref(),
            ],
            |row| row.get::<_, i64>(0),
        )?;

        let mut statement = connection.prepare(&format!(
            "SELECT COALESCE(c.project_key, '') AS project_key,
                    COALESCE(MAX(c.project_hint), '') AS project_hint,
                    COUNT(DISTINCT c.conversation_key),
                    COUNT(DISTINCT e.day_local),
                    MIN(e.occurred_at), MAX(e.occurred_at),
                    COUNT(*), COALESCE(SUM(e.request_count), 0),
                    COALESCE(SUM(e.uncached_input_tokens), 0),
                    COALESCE(SUM(e.output_tokens), 0),
                    COALESCE(SUM(e.reasoning_output_tokens), 0),
                    COALESCE(SUM(e.cache_read_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(CASE WHEN e.speed = 'fast' THEN
                        e.uncached_input_tokens + e.output_tokens + e.cache_read_input_tokens
                        + e.cache_write_5m_input_tokens + e.cache_write_1h_input_tokens
                    ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.speed = 'fast'
                        THEN e.billing_equivalent_tokens_nanos ELSE 0 END), 0),
                    MIN(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                    MAX(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                    SUM(CASE WHEN e.speed = 'fast'
                             AND e.billing_equivalent_tokens_nanos IS NULL THEN 1 ELSE 0 END),
                    COALESCE(SUM(e.api_equivalent_cost_nanos), 0),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.source = 'claude' AND e.inference_geo = 'unknown'
                             AND e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    CASE WHEN COUNT(DISTINCT e.pricing_fingerprint) = 1
                         THEN MAX(e.pricing_fingerprint) END,
                    MAX(COALESCE(c.unattributed, 0)),
                    MIN(COALESCE(c.worktree_path, ''))
               FROM usage_entries e
               LEFT JOIN conversations c ON c.conversation_key = e.conversation_key
              WHERE (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.source IN (SELECT value FROM json_each(?3)))
                AND (?4 IS NULL
                     OR COALESCE(c.project_key, '') LIKE '%' || ?4 || '%' ESCAPE '\\'
                     OR COALESCE(c.project_hint, '') LIKE '%' || ?4 || '%' ESCAPE '\\')
              GROUP BY COALESCE(c.project_key, '')
              ORDER BY {order}
              LIMIT ?5 OFFSET ?6"
        ))?;
        let mapped = statement.query_map(
            params![
                filter.from.as_deref(),
                filter.to.as_deref(),
                sources_json,
                escaped_search.as_deref(),
                i64::from(limit),
                i64::try_from(offset).map_err(|_| UsageDbError::Sql)?,
            ],
            project_row,
        )?;
        let items = mapped.collect::<Result<Vec<_>, _>>()?;

        Ok(UsageProjectPage {
            items,
            total: count,
            limit,
            offset,
        })
    }

    pub fn conversations(
        &self,
        query: &UsageConversationQuery,
        limit: u32,
        offset: u64,
        search: Option<&str>,
        project: Option<&str>,
    ) -> Result<UsageConversationPage, UsageDbError> {
        let connection = self.open_read()?;
        let filter = &query.filter;
        let speed = filter.speed.map(UsageSpeed::as_db);
        let escaped_search = search.map(escape_like);
        let sources_json = sources_json(filter.sources.as_deref());
        let order = match query.sort.unwrap_or(UsageConversationSort::Recent) {
            UsageConversationSort::Recent => {
                "ORDER BY c.last_at DESC, c.conversation_key ASC".to_owned()
            }
            UsageConversationSort::Tokens => "ORDER BY (COALESCE(SUM(e.uncached_input_tokens), 0)
                     + COALESCE(SUM(e.output_tokens), 0)
                     + COALESCE(SUM(e.cache_read_input_tokens), 0)
                     + COALESCE(SUM(e.cache_write_5m_input_tokens), 0)
                     + COALESCE(SUM(e.cache_write_1h_input_tokens), 0)) DESC,
                     c.conversation_key ASC"
                .to_owned(),
            UsageConversationSort::Cost => {
                "ORDER BY COALESCE(SUM(e.api_equivalent_cost_nanos), 0) DESC,
                     c.conversation_key ASC"
                    .to_owned()
            }
        };

        // 未归属条目（Cursor 远端计量与补录历史）没有对话身份，不进入对话列表；
        // 它们计入概览与项目页的「未归属」分组。
        let count = connection.query_row(
            "SELECT COUNT(DISTINCT c.conversation_key)
               FROM conversations c
               JOIN usage_entries e ON e.conversation_key = c.conversation_key
              WHERE c.unattributed = 0
                AND (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.model = ?3)
                AND (?4 IS NULL OR e.speed = ?4)
                AND (?6 IS NULL OR c.project_key = ?6)
                AND (?5 IS NULL OR e.source IN (SELECT value FROM json_each(?5)))
                AND (?7 IS NULL
                     OR COALESCE(c.title, '') LIKE '%' || ?7 || '%' ESCAPE '\\'
                     OR COALESCE(c.project_hint, '') LIKE '%' || ?7 || '%' ESCAPE '\\')",
            params![
                filter.from.as_deref(),
                filter.to.as_deref(),
                filter.model.as_deref(),
                speed,
                sources_json,
                project,
                escaped_search.as_deref(),
            ],
            |row| row.get(0),
        )?;

        let mut statement = connection.prepare(&format!(
            "SELECT c.conversation_key, c.source, c.title, c.project_hint, c.project_key,
                    c.worktree_path, c.unattributed,
                    c.is_sidechain, c.first_at, c.last_at, COUNT(*),
                    COALESCE(SUM(e.request_count), 0),
                    COALESCE(SUM(e.uncached_input_tokens), 0),
                    COALESCE(SUM(e.output_tokens), 0),
                    COALESCE(SUM(e.reasoning_output_tokens), 0),
                    COALESCE(SUM(e.cache_read_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(CASE WHEN e.speed = 'fast' THEN
                        e.uncached_input_tokens + e.output_tokens + e.cache_read_input_tokens
                        + e.cache_write_5m_input_tokens + e.cache_write_1h_input_tokens
                    ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.speed = 'fast'
                        THEN e.billing_equivalent_tokens_nanos ELSE 0 END), 0),
                    MIN(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                    MAX(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                    SUM(CASE WHEN e.speed = 'fast'
                             AND e.billing_equivalent_tokens_nanos IS NULL THEN 1 ELSE 0 END),
                    COALESCE(SUM(e.api_equivalent_cost_nanos), 0),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.source = 'claude' AND e.inference_geo = 'unknown'
                             AND e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    CASE WHEN COUNT(DISTINCT e.pricing_fingerprint) = 1
                         THEN MAX(e.pricing_fingerprint) END,
                    c.source_id, c.branch,
                    (SELECT json_group_array(model) FROM (
                        SELECT DISTINCT model FROM usage_entries
                         WHERE conversation_key = c.conversation_key
                           AND model IS NOT NULL AND model != ''
                         ORDER BY model
                    ))
               FROM conversations c
               JOIN usage_entries e ON e.conversation_key = c.conversation_key
              WHERE c.unattributed = 0
                AND (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.model = ?3)
                AND (?4 IS NULL OR e.speed = ?4)
                AND (?6 IS NULL OR c.project_key = ?6)
                AND (?5 IS NULL OR e.source IN (SELECT value FROM json_each(?5)))
                AND (?7 IS NULL
                     OR COALESCE(c.title, '') LIKE '%' || ?7 || '%' ESCAPE '\\'
                     OR COALESCE(c.project_hint, '') LIKE '%' || ?7 || '%' ESCAPE '\\')
              GROUP BY c.conversation_key
              {order}
              LIMIT ?8 OFFSET ?9"
        ))?;
        let mapped = statement.query_map(
            params![
                filter.from.as_deref(),
                filter.to.as_deref(),
                filter.model.as_deref(),
                speed,
                sources_json,
                project,
                escaped_search.as_deref(),
                i64::from(limit),
                i64::try_from(offset).map_err(|_| UsageDbError::Sql)?,
            ],
            conversation_row,
        )?;
        let items = mapped.collect::<Result<Vec<_>, _>>()?;

        Ok(UsageConversationPage {
            items,
            total: count,
            limit,
            offset,
        })
    }

    pub fn conversation_breakdown(
        &self,
        conversation_key: &str,
    ) -> Result<UsageConversationBreakdown, UsageDbError> {
        let connection = self.open_read()?;
        let select = "SELECT COALESCE({group}, ''), COUNT(*),
                    COALESCE(SUM(request_count), 0),
                    COALESCE(SUM(uncached_input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(reasoning_output_tokens), 0),
                    COALESCE(SUM(cache_read_input_tokens), 0),
                    COALESCE(SUM(cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(CASE WHEN speed = 'fast' THEN
                        uncached_input_tokens + output_tokens + cache_read_input_tokens
                        + cache_write_5m_input_tokens + cache_write_1h_input_tokens
                    ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN speed = 'fast'
                        THEN billing_equivalent_tokens_nanos ELSE 0 END), 0),
                    MIN(CASE WHEN speed = 'fast' THEN fast_multiplier_nanos END),
                    MAX(CASE WHEN speed = 'fast' THEN fast_multiplier_nanos END),
                    SUM(CASE WHEN speed = 'fast'
                             AND billing_equivalent_tokens_nanos IS NULL THEN 1 ELSE 0 END),
                    COALESCE(SUM(api_equivalent_cost_nanos), 0),
                    SUM(CASE WHEN api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN source = 'claude' AND inference_geo = 'unknown'
                             AND api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    CASE WHEN COUNT(DISTINCT pricing_fingerprint) = 1
                         THEN MAX(pricing_fingerprint) END
               FROM usage_entries
              WHERE conversation_key = ?1
              GROUP BY {group}
              ORDER BY {group}";

        let models_sql = select.replace("{group}", "COALESCE(model, '')");
        let mut models_statement = connection.prepare(&models_sql)?;
        let models = models_statement
            .query_map([conversation_key], summary_row)?
            .collect::<Result<Vec<_>, _>>()?;

        let speeds_sql = select.replace("{group}", "speed");
        let mut speeds_statement = connection.prepare(&speeds_sql)?;
        let speeds = speeds_statement
            .query_map([conversation_key], summary_row)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(UsageConversationBreakdown { models, speeds })
    }

    pub fn conversation(
        &self,
        conversation_key: &str,
    ) -> Result<Option<UsageConversation>, UsageDbError> {
        let connection = self.open_read()?;
        connection
            .query_row(
                "SELECT c.conversation_key, c.source, c.title, c.project_hint, c.project_key,
                        c.worktree_path, c.unattributed,
                        c.is_sidechain, c.first_at, c.last_at, COUNT(e.id),
                        COALESCE(SUM(e.request_count), 0),
                        COALESCE(SUM(e.uncached_input_tokens), 0),
                        COALESCE(SUM(e.output_tokens), 0),
                        COALESCE(SUM(e.reasoning_output_tokens), 0),
                        COALESCE(SUM(e.cache_read_input_tokens), 0),
                        COALESCE(SUM(e.cache_write_5m_input_tokens), 0),
                        COALESCE(SUM(e.cache_write_1h_input_tokens), 0),
                        COALESCE(SUM(CASE WHEN e.speed = 'fast' THEN
                            e.uncached_input_tokens + e.output_tokens + e.cache_read_input_tokens
                            + e.cache_write_5m_input_tokens + e.cache_write_1h_input_tokens
                        ELSE 0 END), 0),
                        COALESCE(SUM(CASE WHEN e.speed = 'fast'
                            THEN e.billing_equivalent_tokens_nanos ELSE 0 END), 0),
                        MIN(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                        MAX(CASE WHEN e.speed = 'fast' THEN e.fast_multiplier_nanos END),
                        SUM(CASE WHEN e.speed = 'fast'
                                 AND e.billing_equivalent_tokens_nanos IS NULL THEN 1 ELSE 0 END),
                        COALESCE(SUM(e.api_equivalent_cost_nanos), 0),
                        SUM(CASE WHEN e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN e.api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN e.source = 'claude' AND e.inference_geo = 'unknown'
                                 AND e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                        CASE WHEN COUNT(DISTINCT e.pricing_fingerprint) = 1
                             THEN MAX(e.pricing_fingerprint) END,
                        c.source_id, c.branch,
                        (SELECT json_group_array(model) FROM (
                            SELECT DISTINCT model FROM usage_entries
                             WHERE conversation_key = c.conversation_key
                               AND model IS NOT NULL AND model != ''
                             ORDER BY model
                        ))
                   FROM conversations c
                   JOIN usage_entries e ON e.conversation_key = c.conversation_key
                  WHERE c.conversation_key = ?1
                  GROUP BY c.conversation_key",
                [conversation_key],
                conversation_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn reprice(&self, catalog: &PricingCatalog) -> Result<UsageRepriceResult, UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;
        let entries = {
            let mut statement = transaction.prepare(
                "SELECT id, source, model, speed, inference_geo, occurred_at,
                        uncached_input_tokens, output_tokens, reasoning_output_tokens,
                        cache_read_input_tokens, cache_write_5m_input_tokens,
                        cache_write_1h_input_tokens
                   FROM usage_entries
                  WHERE source IN ('codex', 'claude')",
            )?;
            let rows = statement.query_map([], reprice_row)?;
            rows.collect::<Result<Vec<_>, _>>()?
        };

        let mut updated = 0_u64;
        let mut priced = 0_u64;
        let mut unpriced = 0_u64;
        {
            let mut statement = transaction.prepare_cached(
                "UPDATE usage_entries
                    SET api_equivalent_cost_nanos = ?1,
                        billing_equivalent_tokens_nanos = ?2,
                        fast_multiplier_nanos = ?3,
                        pricing_fingerprint = ?4
                  WHERE id = ?5
                    AND (api_equivalent_cost_nanos IS NOT ?1
                         OR billing_equivalent_tokens_nanos IS NOT ?2
                         OR fast_multiplier_nanos IS NOT ?3
                         OR pricing_fingerprint IS NOT ?4)",
            )?;
            for entry in &entries {
                let estimate = catalog.estimate_row(entry);
                let (billing_equivalent, multiplier) = catalog.fast_billing_equivalent(
                    entry.source,
                    entry.model.as_deref(),
                    entry.speed,
                    entry.tokens.total_tokens(),
                );
                let fingerprint = estimate
                    .cost_nanos
                    .map(|_| catalog.fingerprint().to_owned());
                if estimate.cost_nanos.is_some() {
                    priced += 1;
                } else {
                    unpriced += 1;
                }
                updated += u64::try_from(statement.execute(params![
                    estimate.cost_nanos,
                    billing_equivalent,
                    multiplier,
                    fingerprint,
                    entry.id
                ])?)
                .unwrap_or(0);
            }
        }
        transaction.execute(
            "INSERT INTO pricing_state (id, fingerprint)
             VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET fingerprint = excluded.fingerprint",
            [catalog.fingerprint()],
        )?;
        transaction.commit()?;
        self.verify_after_logical_write_locked()?;

        Ok(UsageRepriceResult {
            updated_entries: updated,
            priced_entries: priced,
            unpriced_entries: unpriced,
            pricing_fingerprint: catalog.fingerprint().to_owned(),
        })
    }

    pub(crate) fn pricing_fingerprint(&self) -> Result<Option<String>, UsageDbError> {
        let connection = self.open_read()?;
        connection
            .query_row(
                "SELECT fingerprint FROM pricing_state WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(Into::into)
    }

    /// 只返回可能由远端目录补齐的价格身份；Unknown 与模型缺失在 SQL 层排除，
    /// 长上下文不支持等政策性未定价由 `PricingCatalog::needs_remote_refresh` 再过滤。
    pub(crate) fn unpriced_usage_keys(&self) -> Result<Vec<PricingUsageKey>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT source, model, speed
               FROM usage_entries
              WHERE source IN ('codex', 'claude')
                AND api_equivalent_cost_nanos IS NULL
                AND model IS NOT NULL
                AND TRIM(model) <> ''
                AND speed <> 'unknown'",
        )?;
        let rows = statement.query_map([], |row| {
            let source: String = row.get(0)?;
            let model: String = row.get(1)?;
            let speed: String = row.get(2)?;
            Ok(PricingUsageKey::new(
                source_from_db(&source)?,
                &model,
                speed_from_db(&speed)?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub(crate) fn known_usage_keys(&self) -> Result<HashSet<PricingUsageKey>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT source, model, speed
               FROM usage_entries
              WHERE source IN ('codex', 'claude')
                AND model IS NOT NULL
                AND TRIM(model) <> ''
                AND speed <> 'unknown'",
        )?;
        let rows = statement.query_map([], |row| {
            let source: String = row.get(0)?;
            let model: String = row.get(1)?;
            let speed: String = row.get(2)?;
            Ok(PricingUsageKey::new(
                source_from_db(&source)?,
                &model,
                speed_from_db(&speed)?,
            ))
        })?;
        rows.collect::<Result<HashSet<_>, _>>().map_err(Into::into)
    }

    /// 额度历史查询：返回去重后的事件点，最近优先截取 `limit` 条，再按时间升序返回。
    /// `identity_key` 用于前端按账号归组；主账号镜像去重由「只显示每个 Provider 当前
    /// 身份的活动序列」在前端完成，这里不做跨指纹的账号合并。
    /// 项目筛选选项：按当前过滤范围聚合项目身份、展示名、对话数与最近活动时间。
    /// 未归属条目没有项目身份，不进入筛选菜单。
    pub fn conversation_projects(
        &self,
        query: &UsageConversationQuery,
    ) -> Result<Vec<UsageConversationProjectOption>, UsageDbError> {
        let connection = self.open_read()?;
        let filter = &query.filter;
        let sources_json = sources_json(filter.sources.as_deref());
        let mut statement = connection.prepare(
            "SELECT c.project_key, COALESCE(MAX(c.project_hint), ''),
                    COUNT(DISTINCT c.conversation_key), MAX(c.last_at)
               FROM conversations c
               JOIN usage_entries e ON e.conversation_key = c.conversation_key
              WHERE c.project_key IS NOT NULL
                AND c.unattributed = 0
                AND (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.source IN (SELECT value FROM json_each(?3)))
              GROUP BY c.project_key
              ORDER BY MAX(c.last_at) DESC, c.project_key ASC",
        )?;
        let mapped = statement.query_map(
            params![filter.from.as_deref(), filter.to.as_deref(), sources_json,],
            |row| {
                let key: String = row.get(0)?;
                let hint: String = row.get(1)?;
                Ok(UsageConversationProjectOption {
                    name: if hint.is_empty() {
                        tail_segment(&key)
                    } else {
                        hint
                    },
                    key: Some(key),
                    conversation_count: row.get(2)?,
                    last_at: row.get(3)?,
                })
            },
        )?;
        mapped.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn quota_history(
        &self,
        provider: Option<ProviderId>,
        from: Option<&str>,
        to: Option<&str>,
        limit: u32,
    ) -> Result<Vec<QuotaHistoryEvent>, UsageDbError> {
        let connection = self.open_read()?;
        let mut statement = connection.prepare(
            "SELECT provider, identity_key, window_kind, window_id,
                    remaining_percent, observed_at, resets_at
               FROM (
                 SELECT id, provider, identity_key, window_kind, window_id,
                        remaining_percent, observed_at, resets_at
                   FROM quota_events
                  WHERE (?1 IS NULL OR provider = ?1)
                    AND (?2 IS NULL OR observed_at >= ?2)
                    AND (?3 IS NULL OR observed_at < ?3)
                  ORDER BY observed_at DESC, id DESC
                  LIMIT ?4
               )
              ORDER BY observed_at ASC, id ASC",
        )?;
        let rows = statement.query_map(
            params![provider.map(provider_db), from, to, i64::from(limit)],
            |row| {
                Ok(QuotaHistoryEvent {
                    provider: provider_from_db(&row.get::<_, String>(0)?)?,
                    identity_key: row.get(1)?,
                    window_kind: window_kind_from_db(&row.get::<_, String>(2)?)?,
                    window_id: row.get(3)?,
                    remaining_percent: row.get(4)?,
                    observed_at: row.get(5)?,
                    resets_at: row.get(6)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 读取 OpenCode 增量扫描状态；无记录时返回默认值。
    pub fn opencode_state(&self) -> Result<OpencodeScanState, UsageDbError> {
        self.initialize()?;
        let connection = self.open_read()?;
        connection
            .query_row(
                "SELECT watermark_ms, seen_ids FROM opencode_state WHERE id = 1",
                [],
                |row| {
                    Ok(OpencodeScanState {
                        watermark_ms: row.get(0)?,
                        seen_ids: serde_json::from_str(&row.get::<_, String>(1)?)
                            .unwrap_or_default(),
                    })
                },
            )
            .optional()
            .map(|value| value.unwrap_or_default())
            .map_err(Into::into)
    }

    pub fn save_opencode_state(&self, state: &OpencodeScanState) -> Result<(), UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let connection = self.open_write_unchecked()?;
        connection.execute(
            "INSERT INTO opencode_state (id, watermark_ms, seen_ids) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET watermark_ms = excluded.watermark_ms,
                                            seen_ids = excluded.seen_ids",
            params![
                state.watermark_ms,
                serde_json::to_string(&state.seen_ids).unwrap_or_else(|_| "[]".to_owned())
            ],
        )?;
        Ok(())
    }

    pub fn record_quota_snapshot(
        &self,
        provider: ProviderId,
        identity_key: &str,
        snapshot: &QuotaSnapshot,
    ) -> Result<(), UsageDbError> {
        self.initialize()?;
        let _guard = self.write_lock.lock().expect("usage db write lock");
        #[cfg(feature = "perf-baseline")]
        self.perf.quota_snapshots.fetch_add(1, Ordering::Relaxed);
        let mut connection = self.open_write_unchecked()?;
        let transaction = connection.transaction()?;

        for window in &snapshot.windows {
            let remaining = window.remaining_percent.round().clamp(0.0, 100.0) as i64;
            let kind = quota_kind(window.kind);
            let previous: Option<i64> = transaction
                .query_row(
                    "SELECT remaining_percent
                       FROM quota_events
                      WHERE provider = ?1 AND identity_key = ?2 AND window_kind = ?3
                        AND window_id IS ?4
                      ORDER BY id DESC LIMIT 1",
                    params![provider_db(provider), identity_key, kind, window.id],
                    |row| row.get(0),
                )
                .optional()?;
            if previous == Some(remaining) {
                continue;
            }
            transaction.execute(
                "INSERT INTO quota_events (
                   provider, identity_key, window_kind, window_id,
                   remaining_percent, observed_at, resets_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    provider_db(provider),
                    identity_key,
                    kind,
                    window.id,
                    remaining,
                    snapshot.captured_at,
                    window.resets_at,
                ],
            )?;
        }
        transaction.commit()?;
        self.verify_after_logical_write_locked()?;
        Ok(())
    }

    fn open_read(&self) -> Result<Connection, UsageDbError> {
        self.initialize()?;
        let connection = Connection::open_with_flags(
            self.path(),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        Ok(connection)
    }

    /// 返回 perf-baseline 累积计数快照（feature 门控，生产构建不存在）。
    #[cfg(feature = "perf-baseline")]
    pub fn perf_stats(&self) -> PerfStats {
        PerfStats {
            write_opens: self.perf.write_opens.load(Ordering::Relaxed),
            quick_checks: self.perf.quick_checks.load(Ordering::Relaxed),
            batch_commits: self.perf.batch_commits.load(Ordering::Relaxed),
            batch_commit_nanos: self.perf.batch_commit_nanos.load(Ordering::Relaxed),
            quota_snapshots: self.perf.quota_snapshots.load(Ordering::Relaxed),
        }
    }

    /// 打开写连接：不重复 `quick_check` 与迁移（`initialize` 已一次性完成）。
    /// 调用方必须已持有 `write_lock`，或保证无并发写者。
    fn open_write_unchecked(&self) -> Result<Connection, UsageDbError> {
        #[cfg(feature = "perf-baseline")]
        self.perf.write_opens.fetch_add(1, Ordering::Relaxed);
        fs::create_dir_all(&self.directory)?;
        let connection = Connection::open(self.path())?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(UsageDbError::UnsupportedSchema);
        }

        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "NORMAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }

    /// 逻辑写操作完成后的健康检查：一次完整扫描、一次重计价、一次重建或一次额度历史写入
    /// 的边界执行 `quick_check`。扫描的 2000 行／8 MiB 批次只是同一逻辑操作的事务边界，
    /// 不逐批重复全库检查。
    pub fn verify_after_logical_write(&self) -> Result<(), UsageDbError> {
        let _guard = self.write_lock.lock().expect("usage db write lock");
        self.verify_after_logical_write_locked()
    }

    /// 调用方已持有 `write_lock` 时使用（各逻辑写路径的事务边界）。
    /// 只执行 `quick_check`：先检查后取版本号，与旧 `open_write` 的检查顺序一致，
    /// 保证非数据库文件在版本查询前先被识别为损坏并恢复。
    fn verify_after_logical_write_locked(&self) -> Result<(), UsageDbError> {
        #[cfg(feature = "perf-baseline")]
        self.perf.write_opens.fetch_add(1, Ordering::Relaxed);
        let connection = Connection::open(self.path())?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        #[cfg(feature = "perf-baseline")]
        self.perf.quick_checks.fetch_add(1, Ordering::Relaxed);
        let check = connection
            .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
            .unwrap_or_else(|_| "failed".to_owned());
        if check != "ok" {
            drop(connection);
            self.recover_corrupt_database()?;
        }
        Ok(())
    }

    fn recover_corrupt_database(&self) -> Result<(), UsageDbError> {
        let path = self.path();
        let backup = export_quota_events(&path);
        let suffix = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();

        for member in database_family(&path) {
            if member.exists() {
                let file_name = member
                    .file_name()
                    .and_then(|value| value.to_str())
                    .ok_or(UsageDbError::Recovery)?;
                let target = self.directory.join(format!("{file_name}.corrupt-{suffix}"));
                fs::rename(&member, target)?;
            }
        }

        let mut connection = Connection::open(&path)?;
        migrate(&mut connection, 0)?;
        if !backup.is_empty() {
            let transaction = connection.transaction()?;
            for event in backup {
                if !valid_quota_backup(&event) {
                    continue;
                }
                transaction.execute(
                    "INSERT INTO quota_events (
                       provider, identity_key, window_kind, window_id,
                       remaining_percent, observed_at, resets_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        event.provider,
                        event.identity_key,
                        event.window_kind,
                        event.window_id,
                        event.remaining_percent,
                        event.observed_at,
                        event.resets_at,
                    ],
                )?;
            }
            transaction.commit()?;
        }
        Ok(())
    }
}

fn migrate(connection: &mut Connection, from: i64) -> Result<(), UsageDbError> {
    if !matches!(from, 0..=8) {
        return Err(UsageDbError::UnsupportedSchema);
    }
    let transaction = connection.transaction()?;
    if from == 0 {
        transaction.execute_batch(
            "CREATE TABLE scan_files (
           file_key TEXT PRIMARY KEY,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
           mtime_ms INTEGER NOT NULL,
           size_bytes INTEGER NOT NULL,
           offset_bytes INTEGER NOT NULL,
           prefix_fingerprint TEXT NOT NULL,
           cursor_json TEXT,
           updated_at TEXT NOT NULL
         );
         CREATE TABLE usage_entries (
           id INTEGER PRIMARY KEY,
           file_key TEXT NOT NULL,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
           dedup_key TEXT NOT NULL,
           conversation_key TEXT NOT NULL,
           model TEXT,
           speed TEXT NOT NULL CHECK(speed IN ('standard', 'fast', 'unknown')),
           inference_geo TEXT NOT NULL CHECK(inference_geo IN ('global', 'us', 'unknown')),
           occurred_at TEXT NOT NULL,
           day_local TEXT NOT NULL,
           uncached_input_tokens INTEGER NOT NULL CHECK(uncached_input_tokens >= 0),
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
           reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0),
           cache_read_input_tokens INTEGER NOT NULL CHECK(cache_read_input_tokens >= 0),
           cache_write_5m_input_tokens INTEGER NOT NULL CHECK(cache_write_5m_input_tokens >= 0),
           cache_write_1h_input_tokens INTEGER NOT NULL CHECK(cache_write_1h_input_tokens >= 0),
           api_equivalent_cost_nanos INTEGER,
           billing_equivalent_tokens_nanos INTEGER,
           fast_multiplier_nanos INTEGER,
           pricing_fingerprint TEXT,
           CHECK(reasoning_output_tokens <= output_tokens)
         );
         CREATE UNIQUE INDEX ux_entries_dedup
           ON usage_entries(source, dedup_key);
         CREATE INDEX ix_entries_file
           ON usage_entries(file_key);
         CREATE INDEX ix_entries_time
           ON usage_entries(occurred_at, source);
         CREATE INDEX ix_entries_day
           ON usage_entries(day_local, source, model, speed);
         CREATE INDEX ix_entries_conversation
           ON usage_entries(conversation_key, occurred_at);
         CREATE INDEX ix_entries_repricing
           ON usage_entries(pricing_fingerprint);
         CREATE TABLE pricing_state (
           id INTEGER PRIMARY KEY CHECK(id = 1),
           fingerprint TEXT NOT NULL
         );
          CREATE TABLE conversations (
            conversation_key TEXT PRIMARY KEY,
            source TEXT NOT NULL CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
            title TEXT,
            project_hint TEXT,
            is_sidechain INTEGER NOT NULL DEFAULT 0 CHECK(is_sidechain IN (0, 1)),
            first_at TEXT NOT NULL,
            last_at TEXT NOT NULL,
            source_id TEXT,
            branch TEXT
          );
          CREATE INDEX ix_conversations_recent
            ON conversations(last_at DESC);
          CREATE INDEX ix_conversations_source_recent
            ON conversations(source, last_at DESC);
          CREATE TABLE quota_events (
           id INTEGER PRIMARY KEY,
           provider TEXT NOT NULL CHECK(provider IN ('codex', 'claude')),
           identity_key TEXT NOT NULL,
           window_kind TEXT NOT NULL,
           window_id TEXT,
           remaining_percent INTEGER NOT NULL CHECK(remaining_percent BETWEEN 0 AND 100),
           observed_at TEXT NOT NULL,
           resets_at TEXT
         );
          CREATE INDEX ix_quota_series
            ON quota_events(provider, identity_key, window_kind, window_id, observed_at);
           CREATE TABLE opencode_state (
             id INTEGER PRIMARY KEY CHECK(id = 1),
             watermark_ms INTEGER NOT NULL DEFAULT 0,
             seen_ids TEXT NOT NULL DEFAULT '[]'
           );
           PRAGMA user_version = 6;",
        )?;
    } else {
        if from == 1 {
            transaction.execute_batch(
                "ALTER TABLE usage_entries
                   ADD COLUMN billing_equivalent_tokens_nanos INTEGER;
                 ALTER TABLE usage_entries
                   ADD COLUMN fast_multiplier_nanos INTEGER;
                 CREATE TABLE pricing_state (
                   id INTEGER PRIMARY KEY CHECK(id = 1),
                   fingerprint TEXT NOT NULL
                 );",
            )?;
        }
        if from < 4 {
            rebuild_source_constraints(&transaction)?;
        }
        // v5：conversations 新增 source_id 与 branch 列。
        if from < 5 {
            transaction.execute_batch(
                "ALTER TABLE conversations ADD COLUMN source_id TEXT;
                 ALTER TABLE conversations ADD COLUMN branch TEXT;
                 PRAGMA user_version = 5;",
            )?;
        }
        // v6：quota_events 记录事件时点的窗口重置时间（重置时间列，旧行保持 NULL）。
        if from < 6 {
            transaction.execute_batch(
                "ALTER TABLE quota_events ADD COLUMN resets_at TEXT;
                 PRAGMA user_version = 6;",
            )?;
        }
    }
    // v7：数据源扩到六个（含 DSH 与 Cursor 远端计量）、对话补项目归属、新增周期表。
    // 与 `from` 无关地统一从这里升，新建库也走同一条路径，避免两套 schema 文本分叉。
    if from < 7 {
        upgrade_to_v7(&transaction)?;
    }
    // v8：DSH 的会话归属与「源日志被清理后仍保留历史」所需的贡献汇总。
    if from < 8 {
        upgrade_to_v8(&transaction)?;
    }
    // v9：Cursor 远端计量的覆盖表。按自然日记「已经拉全」，重复拉取不会重复入账。
    if from < 9 {
        upgrade_to_v9(&transaction)?;
    }
    transaction.commit()?;
    Ok(())
}

/// v6 → v7。
///
/// - `usage_entries`：`source` CHECK 扩到六个，新增 `request_count`（远端计量日桶的
///   请求数为 0，不能再拿行数当请求数）与 `granularity`（`request` / `day`）。
/// - `conversations`：新增 `project_key`（项目身份）、`worktree_path`（对话自身工作目录，
///   用于 worktree 明细）与 `unattributed`（Cursor 远端计量与补录历史的未归属标记）。
/// - 新增 `quota_cycles` / `quota_allowance_segments` / `quota_account_segments`。
///
/// SQLite 无法直接改 CHECK，`usage_entries`、`conversations`、`scan_files` 均整表重建；
/// 重建保持既有行不变。
fn upgrade_to_v7(transaction: &rusqlite::Transaction) -> Result<(), UsageDbError> {
    transaction.execute_batch(
        "ALTER TABLE usage_entries RENAME TO usage_entries_v6;
         CREATE TABLE usage_entries (
           id INTEGER PRIMARY KEY,
           file_key TEXT NOT NULL,
           source TEXT NOT NULL
             CHECK(source IN ('codex', 'claude', 'pi', 'opencode', 'dsh', 'cursor')),
           dedup_key TEXT NOT NULL,
           conversation_key TEXT NOT NULL,
           model TEXT,
           speed TEXT NOT NULL CHECK(speed IN ('standard', 'fast', 'unknown')),
           inference_geo TEXT NOT NULL CHECK(inference_geo IN ('global', 'us', 'unknown')),
           occurred_at TEXT NOT NULL,
           day_local TEXT NOT NULL,
           uncached_input_tokens INTEGER NOT NULL CHECK(uncached_input_tokens >= 0),
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
           reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0),
           cache_read_input_tokens INTEGER NOT NULL CHECK(cache_read_input_tokens >= 0),
           cache_write_5m_input_tokens INTEGER NOT NULL CHECK(cache_write_5m_input_tokens >= 0),
           cache_write_1h_input_tokens INTEGER NOT NULL CHECK(cache_write_1h_input_tokens >= 0),
           api_equivalent_cost_nanos INTEGER,
           billing_equivalent_tokens_nanos INTEGER,
           fast_multiplier_nanos INTEGER,
           pricing_fingerprint TEXT,
           request_count INTEGER NOT NULL DEFAULT 1 CHECK(request_count >= 0),
           granularity TEXT NOT NULL DEFAULT 'request'
             CHECK(granularity IN ('request', 'day')),
           CHECK(reasoning_output_tokens <= output_tokens)
         );
         INSERT INTO usage_entries (
           id, file_key, source, dedup_key, conversation_key, model, speed, inference_geo,
           occurred_at, day_local, uncached_input_tokens, output_tokens, reasoning_output_tokens,
           cache_read_input_tokens, cache_write_5m_input_tokens, cache_write_1h_input_tokens,
           api_equivalent_cost_nanos, billing_equivalent_tokens_nanos, fast_multiplier_nanos,
           pricing_fingerprint, request_count, granularity
         )
         SELECT id, file_key, source, dedup_key, conversation_key, model, speed, inference_geo,
                occurred_at, day_local, uncached_input_tokens, output_tokens,
                reasoning_output_tokens, cache_read_input_tokens, cache_write_5m_input_tokens,
                cache_write_1h_input_tokens, api_equivalent_cost_nanos,
                billing_equivalent_tokens_nanos, fast_multiplier_nanos, pricing_fingerprint,
                1, 'request'
           FROM usage_entries_v6;
         DROP TABLE usage_entries_v6;
         CREATE UNIQUE INDEX ux_entries_dedup
           ON usage_entries(source, dedup_key);
         CREATE INDEX ix_entries_file
           ON usage_entries(file_key);
         CREATE INDEX ix_entries_time
           ON usage_entries(occurred_at, source);
         CREATE INDEX ix_entries_day
           ON usage_entries(day_local, source, model, speed);
         CREATE INDEX ix_entries_conversation
           ON usage_entries(conversation_key, occurred_at);
         CREATE INDEX ix_entries_repricing
           ON usage_entries(pricing_fingerprint);
         ALTER TABLE conversations RENAME TO conversations_v6;
         CREATE TABLE conversations (
           conversation_key TEXT PRIMARY KEY,
           source TEXT NOT NULL
             CHECK(source IN ('codex', 'claude', 'pi', 'opencode', 'dsh', 'cursor')),
           title TEXT,
           project_hint TEXT,
           project_key TEXT,
           worktree_path TEXT,
           is_sidechain INTEGER NOT NULL DEFAULT 0 CHECK(is_sidechain IN (0, 1)),
           unattributed INTEGER NOT NULL DEFAULT 0 CHECK(unattributed IN (0, 1)),
           first_at TEXT NOT NULL,
           last_at TEXT NOT NULL,
           source_id TEXT,
           branch TEXT
         );
         INSERT INTO conversations (
           conversation_key, source, title, project_hint, project_key, worktree_path,
           is_sidechain, unattributed, first_at, last_at, source_id, branch
         )
         SELECT conversation_key, source, title, project_hint, NULL, NULL,
                is_sidechain, 0, first_at, last_at, source_id, branch
           FROM conversations_v6;
         DROP TABLE conversations_v6;
         CREATE INDEX ix_conversations_recent
           ON conversations(last_at DESC);
         CREATE INDEX ix_conversations_source_recent
           ON conversations(source, last_at DESC);
         CREATE INDEX ix_conversations_project
           ON conversations(project_key, last_at DESC);
         ALTER TABLE scan_files RENAME TO scan_files_v6;
         CREATE TABLE scan_files (
           file_key TEXT PRIMARY KEY,
           source TEXT NOT NULL
             CHECK(source IN ('codex', 'claude', 'pi', 'opencode', 'dsh', 'cursor')),
           mtime_ms INTEGER NOT NULL,
           size_bytes INTEGER NOT NULL,
           offset_bytes INTEGER NOT NULL,
           prefix_fingerprint TEXT NOT NULL,
           cursor_json TEXT,
           updated_at TEXT NOT NULL
         );
         INSERT INTO scan_files (
           file_key, source, mtime_ms, size_bytes, offset_bytes, prefix_fingerprint,
           cursor_json, updated_at
         )
         SELECT file_key, source, mtime_ms, size_bytes, offset_bytes, prefix_fingerprint,
                cursor_json, updated_at
           FROM scan_files_v6;
         DROP TABLE scan_files_v6;
         CREATE TABLE quota_cycles (
           id TEXT PRIMARY KEY,
           provider TEXT NOT NULL,
           identity_key TEXT NOT NULL,
           window_kind TEXT NOT NULL,
           window_id TEXT NOT NULL,
           start_at TEXT NOT NULL,
           end_at TEXT NOT NULL,
           scheduled_end_at TEXT NOT NULL,
           first_sample_at TEXT,
           last_sample_at TEXT,
           latest_used_percent REAL NOT NULL,
           boundary_quality TEXT NOT NULL
             CHECK(boundary_quality IN ('observed', 'inferred')),
           source TEXT NOT NULL CHECK(source IN ('live', 'cache')),
           updated_at TEXT NOT NULL
         );
         CREATE INDEX ix_cycles_lookup
           ON quota_cycles(provider, identity_key, window_id, end_at DESC);
         CREATE TABLE quota_allowance_segments (
           id TEXT PRIMARY KEY,
           cycle_id TEXT NOT NULL REFERENCES quota_cycles(id) ON DELETE CASCADE,
           start_at TEXT NOT NULL,
           end_at TEXT,
           baseline_used_percent REAL NOT NULL,
           latest_used_percent REAL NOT NULL,
           maximum_used_percent REAL NOT NULL,
           first_sample_at TEXT NOT NULL,
           last_sample_at TEXT NOT NULL,
           start_reason TEXT NOT NULL CHECK(start_reason IN ('initial', 'extraReset'))
         );
         CREATE INDEX ix_segments_cycle
           ON quota_allowance_segments(cycle_id, start_at);
         CREATE TABLE quota_account_segments (
           id TEXT PRIMARY KEY,
           provider TEXT NOT NULL,
           identity_key TEXT NOT NULL,
           start_at TEXT NOT NULL,
           end_at TEXT
         );
         CREATE INDEX ix_account_segments_lookup
           ON quota_account_segments(provider, identity_key, start_at DESC);
         PRAGMA user_version = 7;",
    )?;
    Ok(())
}

/// v2 → v4（一次到位）：把 `usage_entries`、`conversations`、`scan_files` 的 `source` CHECK
/// 扩展为含 `pi` 与 `opencode`，并新增 `opencode_state`。SQLite 无法直接改 CHECK，
/// 采用整表重建；重建保持既有行不变。
fn rebuild_source_constraints(transaction: &rusqlite::Transaction) -> Result<(), UsageDbError> {
    transaction.execute_batch(
        "ALTER TABLE usage_entries RENAME TO usage_entries_v2;
         CREATE TABLE usage_entries (
           id INTEGER PRIMARY KEY,
           file_key TEXT NOT NULL,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
           dedup_key TEXT NOT NULL,
           conversation_key TEXT NOT NULL,
           model TEXT,
           speed TEXT NOT NULL CHECK(speed IN ('standard', 'fast', 'unknown')),
           inference_geo TEXT NOT NULL CHECK(inference_geo IN ('global', 'us', 'unknown')),
           occurred_at TEXT NOT NULL,
           day_local TEXT NOT NULL,
           uncached_input_tokens INTEGER NOT NULL CHECK(uncached_input_tokens >= 0),
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
           reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0),
           cache_read_input_tokens INTEGER NOT NULL CHECK(cache_read_input_tokens >= 0),
           cache_write_5m_input_tokens INTEGER NOT NULL CHECK(cache_write_5m_input_tokens >= 0),
           cache_write_1h_input_tokens INTEGER NOT NULL CHECK(cache_write_1h_input_tokens >= 0),
           api_equivalent_cost_nanos INTEGER,
           billing_equivalent_tokens_nanos INTEGER,
           fast_multiplier_nanos INTEGER,
           pricing_fingerprint TEXT,
           CHECK(reasoning_output_tokens <= output_tokens)
         );
         INSERT INTO usage_entries (
           id, file_key, source, dedup_key, conversation_key, model, speed, inference_geo,
           occurred_at, day_local, uncached_input_tokens, output_tokens, reasoning_output_tokens,
           cache_read_input_tokens, cache_write_5m_input_tokens, cache_write_1h_input_tokens,
           api_equivalent_cost_nanos, billing_equivalent_tokens_nanos, fast_multiplier_nanos,
           pricing_fingerprint
         )
         SELECT id, file_key, source, dedup_key, conversation_key, model, speed, inference_geo,
                occurred_at, day_local, uncached_input_tokens, output_tokens,
                reasoning_output_tokens, cache_read_input_tokens, cache_write_5m_input_tokens,
                cache_write_1h_input_tokens, api_equivalent_cost_nanos,
                billing_equivalent_tokens_nanos, fast_multiplier_nanos, pricing_fingerprint
           FROM usage_entries_v2;
         DROP TABLE usage_entries_v2;
         CREATE UNIQUE INDEX ux_entries_dedup
           ON usage_entries(source, dedup_key);
         CREATE INDEX ix_entries_file
           ON usage_entries(file_key);
         CREATE INDEX ix_entries_time
           ON usage_entries(occurred_at, source);
         CREATE INDEX ix_entries_day
           ON usage_entries(day_local, source, model, speed);
         CREATE INDEX ix_entries_conversation
           ON usage_entries(conversation_key, occurred_at);
         CREATE INDEX ix_entries_repricing
           ON usage_entries(pricing_fingerprint);
         ALTER TABLE conversations RENAME TO conversations_v2;
         CREATE TABLE conversations (
           conversation_key TEXT PRIMARY KEY,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
           title TEXT,
           project_hint TEXT,
           is_sidechain INTEGER NOT NULL DEFAULT 0 CHECK(is_sidechain IN (0, 1)),
           first_at TEXT NOT NULL,
           last_at TEXT NOT NULL
         );
         INSERT INTO conversations (
           conversation_key, source, title, project_hint, is_sidechain, first_at, last_at
         )
         SELECT conversation_key, source, title, project_hint, is_sidechain, first_at, last_at
           FROM conversations_v2;
         DROP TABLE conversations_v2;
         CREATE INDEX ix_conversations_recent
           ON conversations(last_at DESC);
         CREATE INDEX ix_conversations_source_recent
           ON conversations(source, last_at DESC);
         ALTER TABLE scan_files RENAME TO scan_files_v2;
         CREATE TABLE scan_files (
           file_key TEXT PRIMARY KEY,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
           mtime_ms INTEGER NOT NULL,
           size_bytes INTEGER NOT NULL,
           offset_bytes INTEGER NOT NULL,
           prefix_fingerprint TEXT NOT NULL,
           cursor_json TEXT,
           updated_at TEXT NOT NULL
         );
         INSERT INTO scan_files (
           file_key, source, mtime_ms, size_bytes, offset_bytes, prefix_fingerprint,
           cursor_json, updated_at
         )
         SELECT file_key, source, mtime_ms, size_bytes, offset_bytes, prefix_fingerprint,
                cursor_json, updated_at
           FROM scan_files_v2;
          DROP TABLE scan_files_v2;
         CREATE TABLE opencode_state (
           id INTEGER PRIMARY KEY CHECK(id = 1),
           watermark_ms INTEGER NOT NULL DEFAULT 0,
           seen_ids TEXT NOT NULL DEFAULT '[]'
         );
         PRAGMA user_version = 4;",
    )?;
    Ok(())
}

fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSummaryRow> {
    let tokens = token_totals(row, 3)?;
    Ok(UsageSummaryRow {
        key: row.get(0)?,
        entry_count: row.get(1)?,
        request_count: row.get(2)?,
        tokens,
        fast: fast_totals(row, 9)?,
        cost: UsageCostTotals {
            api_equivalent_cost_nanos: row.get(14)?,
            priced_entries: row.get(15)?,
            unpriced_entries: row.get(16)?,
            assumed_geo_entries: row.get(17)?,
            pricing_fingerprint: row.get(18)?,
        },
    })
}

fn token_totals(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<UsageTokenTotals> {
    let uncached: i64 = row.get(start)?;
    let output: i64 = row.get(start + 1)?;
    let reasoning: i64 = row.get(start + 2)?;
    let cache_read: i64 = row.get(start + 3)?;
    let write_5m: i64 = row.get(start + 4)?;
    let write_1h: i64 = row.get(start + 5)?;
    let input = uncached + cache_read + write_5m + write_1h;
    Ok(UsageTokenTotals {
        uncached_input_tokens: uncached,
        output_tokens: output,
        reasoning_output_tokens: reasoning,
        cache_read_input_tokens: cache_read,
        cache_write_5m_input_tokens: write_5m,
        cache_write_1h_input_tokens: write_1h,
        input_tokens: input,
        total_tokens: input + output,
    })
}

fn fast_totals(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<UsageFastTotals> {
    let equivalent_nanos: i64 = row.get(start + 1)?;
    let minimum_nanos: Option<i64> = row.get(start + 2)?;
    let maximum_nanos: Option<i64> = row.get(start + 3)?;
    Ok(UsageFastTotals {
        raw_tokens: row.get(start)?,
        billing_equivalent_tokens: decimal_nanos_string(equivalent_nanos),
        minimum_multiplier: minimum_nanos.map(decimal_nanos_string),
        maximum_multiplier: maximum_nanos.map(decimal_nanos_string),
        has_unpriced_equivalent: row.get::<_, i64>(start + 4)? > 0,
    })
}

fn project_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageProjectSummary> {
    let key: String = row.get(0)?;
    let hint: String = row.get(1)?;
    let unattributed = row.get::<_, i64>(24)? != 0 || key.is_empty();
    let name = if key.is_empty() {
        String::new()
    } else if hint.is_empty() {
        tail_segment(&key)
    } else {
        hint
    };
    Ok(UsageProjectSummary {
        path: (!key.is_empty()).then(|| key.clone()),
        key,
        name,
        unattributed,
        conversation_count: row.get(2)?,
        active_days: row.get(3)?,
        first_at: row.get(4)?,
        last_at: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
        entry_count: row.get(6)?,
        request_count: row.get(7)?,
        tokens: token_totals(row, 8)?,
        fast: fast_totals(row, 14)?,
        cost: UsageCostTotals {
            api_equivalent_cost_nanos: row.get(19)?,
            priced_entries: row.get(20)?,
            unpriced_entries: row.get(21)?,
            assumed_geo_entries: row.get(22)?,
            pricing_fingerprint: row.get(23)?,
        },
    })
}

/// 路径尾段，用作项目展示名；分隔符同时兼容 Windows 反斜杠与 POSIX 斜杠。
fn tail_segment(path: &str) -> String {
    path.rsplit(['/', '\\'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// 可见服务集合的 SQLite JSON 参数。`None` 保持 `None`（不过滤），
/// 空数组保持空数组（一个服务都不可见，结果为空）。
fn sources_json(sources: Option<&[UsageSource]>) -> Option<String> {
    sources.map(|sources| {
        serde_json::to_string(&sources.iter().map(|s| s.as_db()).collect::<Vec<_>>())
            .unwrap_or_else(|_| "[]".to_owned())
    })
}

fn conversation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageConversation> {
    let source_value: String = row.get(1)?;
    let source = source_from_db(&source_value)?;
    let models_json: Option<String> = row.get(30)?;
    let models = models_json
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default();
    Ok(UsageConversation {
        conversation_key: row.get(0)?,
        source,
        title: row.get(2)?,
        project_hint: row.get(3)?,
        project_key: row.get(4)?,
        worktree_path: row.get(5)?,
        unattributed: row.get::<_, i64>(6)? != 0,
        is_sidechain: row.get::<_, i64>(7)? != 0,
        first_at: row.get(8)?,
        last_at: row.get(9)?,
        entry_count: row.get(10)?,
        request_count: row.get(11)?,
        tokens: token_totals(row, 12)?,
        fast: fast_totals(row, 18)?,
        cost: UsageCostTotals {
            api_equivalent_cost_nanos: row.get(23)?,
            priced_entries: row.get(24)?,
            unpriced_entries: row.get(25)?,
            assumed_geo_entries: row.get(26)?,
            pricing_fingerprint: row.get(27)?,
        },
        source_id: row.get(28)?,
        branch: row.get(29)?,
        models,
    })
}

fn reprice_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepriceRow> {
    let source: String = row.get(1)?;
    let speed: String = row.get(3)?;
    let geo: String = row.get(4)?;
    Ok(RepriceRow {
        id: row.get(0)?,
        source: source_from_db(&source)?,
        model: row.get(2)?,
        speed: speed_from_db(&speed)?,
        inference_geo: InferenceGeo::from_db(&geo),
        occurred_at: row.get(5)?,
        tokens: TokenFacts {
            uncached_input_tokens: row.get(6)?,
            output_tokens: row.get(7)?,
            reasoning_output_tokens: row.get(8)?,
            cache_read_input_tokens: row.get(9)?,
            cache_write_5m_input_tokens: row.get(10)?,
            cache_write_1h_input_tokens: row.get(11)?,
        },
    })
}

fn source_from_db(value: &str) -> rusqlite::Result<UsageSource> {
    match value {
        "codex" => Ok(UsageSource::Codex),
        "claude" => Ok(UsageSource::Claude),
        "pi" => Ok(UsageSource::Pi),
        "opencode" => Ok(UsageSource::Opencode),
        "dsh" => Ok(UsageSource::Dsh),
        "cursor" => Ok(UsageSource::Cursor),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn speed_from_db(value: &str) -> rusqlite::Result<UsageSpeed> {
    match value {
        "standard" => Ok(UsageSpeed::Standard),
        "fast" => Ok(UsageSpeed::Fast),
        "unknown" => Ok(UsageSpeed::Unknown),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

/// 一个按天粒度的 Cursor 远端计量桶（来自 `usage::cursor_remote`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CursorRemoteBucket {
    pub day_local: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    /// 服务端计费金额，纳秒单位。
    pub charged_nanos: i64,
    pub request_count: i64,
}

/// 一次按日替换的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CursorReplaceResult {
    pub removed: u64,
    pub inserted: u64,
}

/// 本地自然日零点对应的 UTC 时刻（`YYYY-MM-DD` → ISO 8601）。
///
/// 按天粒度的事实只有自然日一个时间锚点；写 UTC 零点会让西半球时区的行落到当天窗口外，
/// 因此按本地时区换算。夏令时下零点不存在时取当天第一个有效时刻。
fn local_day_start_utc(day_local: &str) -> String {
    use chrono::{Local, NaiveDate, TimeZone};

    let Ok(day) = NaiveDate::parse_from_str(day_local, "%Y-%m-%d") else {
        return format!("{day_local}T00:00:00Z");
    };
    let Some(naive) = day.and_hms_opt(0, 0, 0) else {
        return format!("{day_local}T00:00:00Z");
    };
    match Local.from_local_datetime(&naive) {
        chrono::LocalResult::Single(time) => time
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        chrono::LocalResult::Ambiguous(first, _) => first
            .with_timezone(&Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        chrono::LocalResult::None => day
            .and_hms_opt(1, 0, 0)
            .and_then(|naive| Local.from_local_datetime(&naive).earliest())
            .map(|time| {
                time.with_timezone(&Utc)
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
            })
            .unwrap_or_else(|| format!("{day_local}T00:00:00Z")),
    }
}

/// `YYYY-MM-DD` 的次日。
fn next_day(day_local: &str) -> Option<String> {
    use chrono::{Days, NaiveDate};
    NaiveDate::parse_from_str(day_local, "%Y-%m-%d")
        .ok()?
        .checked_add_days(Days::new(1))
        .map(|day| day.format("%Y-%m-%d").to_string())
}

/// 写入 `dsh_sessions` 的一行。父会话与标题按「新值非空才覆盖」合并。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshSessionUpsert {
    pub session_id: String,
    pub parent_session: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub project_key: Option<String>,
    pub updated_at: String,
}

/// 会话汇总物化条目在 `dedup_key` 上的前缀。
fn contribution_dedup_prefix(session_id: &str) -> String {
    format!("dsh-contribution:{session_id}:")
}

fn provider_db(provider: ProviderId) -> &'static str {
    provider.key()
}

fn quota_kind(kind: QuotaWindowKind) -> &'static str {
    match kind {
        QuotaWindowKind::FiveHour => "five_hour",
        QuotaWindowKind::Weekly => "weekly",
        QuotaWindowKind::ModelWeekly => "model_weekly",
        QuotaWindowKind::Monthly => "monthly",
        QuotaWindowKind::Total => "total",
        QuotaWindowKind::Auto => "auto",
        QuotaWindowKind::Api => "api",
        QuotaWindowKind::Unknown => "unknown",
    }
}

fn provider_from_db(value: &str) -> rusqlite::Result<ProviderId> {
    match value {
        "codex" => Ok(ProviderId::Codex),
        "claude" => Ok(ProviderId::Claude),
        "antigravity" => Ok(ProviderId::Antigravity),
        "cursor" => Ok(ProviderId::Cursor),
        "commandCode" => Ok(ProviderId::CommandCode),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown provider {value}").into(),
        )),
    }
}

fn window_kind_from_db(value: &str) -> rusqlite::Result<QuotaWindowKind> {
    match value {
        "five_hour" => Ok(QuotaWindowKind::FiveHour),
        "weekly" => Ok(QuotaWindowKind::Weekly),
        "model_weekly" => Ok(QuotaWindowKind::ModelWeekly),
        "monthly" => Ok(QuotaWindowKind::Monthly),
        "total" => Ok(QuotaWindowKind::Total),
        "auto" => Ok(QuotaWindowKind::Auto),
        "api" => Ok(QuotaWindowKind::Api),
        "unknown" => Ok(QuotaWindowKind::Unknown),
        _ => Err(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            format!("unknown window kind {value}").into(),
        )),
    }
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn database_family(path: &Path) -> [PathBuf; 3] {
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    let mut shm = path.as_os_str().to_os_string();
    shm.push("-shm");
    [path.to_path_buf(), PathBuf::from(wal), PathBuf::from(shm)]
}

fn export_quota_events(path: &Path) -> Vec<QuotaEventBackup> {
    let Ok(connection) = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let exists = connection
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'quota_events'",
            [],
            |_| Ok(()),
        )
        .optional()
        .ok()
        .flatten()
        .is_some();
    if !exists {
        return Vec::new();
    }
    let Ok(mut statement) = connection.prepare(
        "SELECT provider, identity_key, window_kind, window_id,
                remaining_percent, observed_at, resets_at
           FROM quota_events",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok(QuotaEventBackup {
            provider: row.get(0)?,
            identity_key: row.get(1)?,
            window_kind: row.get(2)?,
            window_id: row.get(3)?,
            remaining_percent: row.get(4)?,
            observed_at: row.get(5)?,
            resets_at: row.get(6)?,
        })
    }) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

fn valid_quota_backup(event: &QuotaEventBackup) -> bool {
    matches!(event.provider.as_str(), "codex" | "claude")
        && !event.identity_key.is_empty()
        && !event.window_kind.is_empty()
        && (0..=100).contains(&event.remaining_percent)
        && DateTimeValidator::is_rfc3339(&event.observed_at)
}

struct DateTimeValidator;

impl DateTimeValidator {
    fn is_rfc3339(value: &str) -> bool {
        chrono::DateTime::parse_from_rfc3339(value).is_ok()
    }
}

/// v7 → v8。
///
/// - `dsh_sessions`：会话 → 父会话 → 根会话。父链在每轮扫描后重算，子会话的条目与
///   对话行重挂到根上（子代理归并，对齐 cc-bar 的 `includesSubtasks`）。
/// - `dsh_session_usage`：按会话 × 自然日 × 模型 × 速度的贡献汇总。
///   `usage_entries` 被重建（「重新计算用量」）或源日志被清理后，用它把历史重新物化，
///   避免「清一次日志就倒扣历史」。
fn upgrade_to_v8(transaction: &rusqlite::Transaction) -> Result<(), UsageDbError> {
    transaction.execute_batch(
        "CREATE TABLE dsh_sessions (
           session_id TEXT PRIMARY KEY,
           parent_session TEXT,
           root_session TEXT,
           cwd TEXT,
           title TEXT,
           project_key TEXT,
           updated_at TEXT NOT NULL
         );
         CREATE INDEX ix_dsh_sessions_root ON dsh_sessions(root_session);
         CREATE TABLE dsh_session_usage (
           session_id TEXT NOT NULL,
           day_local TEXT NOT NULL,
           model TEXT NOT NULL,
           speed TEXT NOT NULL,
           request_count INTEGER NOT NULL CHECK(request_count >= 0),
           uncached_input_tokens INTEGER NOT NULL CHECK(uncached_input_tokens >= 0),
           output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
           reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0),
           cache_read_input_tokens INTEGER NOT NULL CHECK(cache_read_input_tokens >= 0),
           cache_write_5m_input_tokens INTEGER NOT NULL
             CHECK(cache_write_5m_input_tokens >= 0),
           cache_write_1h_input_tokens INTEGER NOT NULL
             CHECK(cache_write_1h_input_tokens >= 0),
           api_equivalent_cost_nanos INTEGER,
           updated_at TEXT NOT NULL,
           PRIMARY KEY (session_id, day_local, model, speed)
         );
         PRAGMA user_version = 8;",
    )?;
    Ok(())
}

/// v8 → v9：Cursor 远端计量的按日覆盖表。
///
/// 远端计量没有事件 id，去重只能靠「按自然日原子替换」：某天拉全了就整天替换，
/// 因此需要逐日记下覆盖状态，而不是记一个范围——范围在部分失败时无法表达空洞。
fn upgrade_to_v9(transaction: &rusqlite::Transaction) -> Result<(), UsageDbError> {
    transaction.execute_batch(
        "CREATE TABLE cursor_usage_coverage (
           account_key TEXT NOT NULL,
           day_local TEXT NOT NULL,
           updated_at TEXT NOT NULL,
           PRIMARY KEY (account_key, day_local)
         );
         CREATE INDEX ix_cursor_coverage_account ON cursor_usage_coverage(account_key);
         PRAGMA user_version = 9;",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{QuotaWindow, UsageFilter};
    use crate::usage::model::{
        ConversationFact, Granularity, InferenceGeo, ScanBatch, TokenFacts, UsageEntry,
    };
    use crate::usage::pricing::PricingCatalog;

    fn database() -> (tempfile::TempDir, UsageDb) {
        let dir = tempfile::tempdir().expect("temp dir");
        let database = UsageDb::new(dir.path().to_path_buf());
        database.initialize().expect("initialize");
        (dir, database)
    }

    fn sample_batch(dedup: &str) -> ScanBatch {
        let entry = UsageEntry {
            source: UsageSource::Codex,
            dedup_key: dedup.to_owned(),
            conversation_key: "conversation".to_owned(),
            model: Some("gpt-5.6-sol".to_owned()),
            speed: UsageSpeed::Standard,
            inference_geo: InferenceGeo::Global,
            occurred_at: "2026-07-30T00:00:00Z".to_owned(),
            day_local: "2026-07-30".to_owned(),
            tokens: TokenFacts {
                uncached_input_tokens: 10,
                output_tokens: 5,
                ..TokenFacts::default()
            },
            api_equivalent_cost_nanos: Some(200),
            billing_equivalent_tokens_nanos: None,
            fast_multiplier_nanos: None,
            pricing_fingerprint: Some("price".to_owned()),
            request_count: 1,
            granularity: Granularity::Request,
        };
        ScanBatch {
            entries: vec![entry],
            conversations: vec![ConversationFact {
                conversation_key: "conversation".to_owned(),
                source: UsageSource::Codex,
                title: None,
                project_hint: None,
                project_key: None,
                worktree_path: None,
                unattributed: false,
                is_sidechain: false,
                occurred_at: "2026-07-30T00:00:00Z".to_owned(),
                source_id: None,
                branch: None,
            }],
            ..ScanBatch::default()
        }
    }

    #[test]
    fn batch_commits_entries_and_watermark_together() {
        let (_dir, database) = database();
        let result = database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                1,
                100,
                80,
                "prefix",
                Some("{}"),
                false,
                &sample_batch("entry"),
            )
            .expect("commit");

        assert_eq!(result.inserted, 1);
        assert_eq!(
            database
                .scan_file_state("file")
                .expect("state")
                .expect("present")
                .offset_bytes,
            80
        );
    }

    #[test]
    fn schema_v1_migrates_to_latest_with_fast_pricing_pi_opencode_and_quota_resets() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(DATABASE_FILE);
        let connection = Connection::open(&path).expect("open legacy");
        connection
            .execute_batch(
                "CREATE TABLE usage_entries (
                   id INTEGER PRIMARY KEY,
                   file_key TEXT NOT NULL,
                   source TEXT NOT NULL CHECK(source IN ('codex', 'claude')),
                   dedup_key TEXT NOT NULL,
                   conversation_key TEXT NOT NULL,
                   model TEXT,
                   speed TEXT NOT NULL CHECK(speed IN ('standard', 'fast', 'unknown')),
                   inference_geo TEXT NOT NULL CHECK(inference_geo IN ('global', 'us', 'unknown')),
                   occurred_at TEXT NOT NULL,
                   day_local TEXT NOT NULL,
                   uncached_input_tokens INTEGER NOT NULL CHECK(uncached_input_tokens >= 0),
                   output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
                   reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0),
                   cache_read_input_tokens INTEGER NOT NULL CHECK(cache_read_input_tokens >= 0),
                   cache_write_5m_input_tokens INTEGER NOT NULL CHECK(cache_write_5m_input_tokens >= 0),
                   cache_write_1h_input_tokens INTEGER NOT NULL CHECK(cache_write_1h_input_tokens >= 0),
                   api_equivalent_cost_nanos INTEGER,
                   pricing_fingerprint TEXT
                 );
                 CREATE TABLE scan_files (
                   file_key TEXT PRIMARY KEY,
                   source TEXT NOT NULL CHECK(source IN ('codex', 'claude')),
                   mtime_ms INTEGER NOT NULL,
                   size_bytes INTEGER NOT NULL,
                   offset_bytes INTEGER NOT NULL,
                   prefix_fingerprint TEXT NOT NULL,
                   cursor_json TEXT,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE conversations (
                   conversation_key TEXT PRIMARY KEY,
                   source TEXT NOT NULL CHECK(source IN ('codex', 'claude')),
                   title TEXT,
                   project_hint TEXT,
                   is_sidechain INTEGER NOT NULL DEFAULT 0 CHECK(is_sidechain IN (0, 1)),
                   first_at TEXT NOT NULL,
                   last_at TEXT NOT NULL
                 );
                 CREATE TABLE quota_events (
                   id INTEGER PRIMARY KEY,
                   provider TEXT NOT NULL CHECK(provider IN ('codex', 'claude')),
                   identity_key TEXT NOT NULL,
                   window_kind TEXT NOT NULL,
                   window_id TEXT,
                   remaining_percent INTEGER NOT NULL CHECK(remaining_percent BETWEEN 0 AND 100),
                   observed_at TEXT NOT NULL
                 );
                 PRAGMA user_version = 1;",
            )
            .expect("seed v1");
        drop(connection);

        let database = UsageDb::new(dir.path().to_path_buf());
        database.initialize().expect("migrate");
        let connection = Connection::open(&path).expect("open migrated");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        let mut statement = connection
            .prepare("PRAGMA table_info(usage_entries)")
            .expect("columns");
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .expect("query columns")
            .collect::<Result<HashSet<_>, _>>()
            .expect("collect columns");

        assert_eq!(version, SCHEMA_VERSION);
        assert!(columns.contains("billing_equivalent_tokens_nanos"));
        assert!(columns.contains("fast_multiplier_nanos"));
        let conversation_columns = {
            let mut statement = connection
                .prepare("PRAGMA table_info(conversations)")
                .expect("columns");
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query columns")
                .collect::<Result<HashSet<_>, _>>()
                .expect("collect columns")
        };
        assert!(conversation_columns.contains("source_id"));
        assert!(conversation_columns.contains("branch"));
        let quota_columns = {
            let mut statement = connection
                .prepare("PRAGMA table_info(quota_events)")
                .expect("columns");
            statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("query columns")
                .collect::<Result<HashSet<_>, _>>()
                .expect("collect columns")
        };
        assert!(quota_columns.contains("resets_at"));
        connection
            .execute(
                "INSERT INTO usage_entries (
                   file_key, source, dedup_key, conversation_key, model, speed, inference_geo,
                   occurred_at, day_local, uncached_input_tokens, output_tokens,
                   reasoning_output_tokens, cache_read_input_tokens, cache_write_5m_input_tokens,
                   cache_write_1h_input_tokens, api_equivalent_cost_nanos,
                   billing_equivalent_tokens_nanos, fast_multiplier_nanos, pricing_fingerprint
                 ) VALUES (
                   'f', 'pi', 'd', 'c', NULL, 'standard', 'global', '2026-07-30T00:00:00Z',
                   '2026-07-30', 1, 1, 0, 0, 0, 0, 0, NULL, NULL, NULL
                 )",
                [],
            )
            .expect("pi source accepted after migration");
    }

    #[test]
    fn summary_keeps_raw_and_billing_equivalent_fast_tokens_separate() {
        let (_dir, database) = database();
        let mut batch = sample_batch("fast");
        batch.entries[0].speed = UsageSpeed::Fast;
        batch.entries[0].billing_equivalent_tokens_nanos = Some(37_500_000_000);
        batch.entries[0].fast_multiplier_nanos = Some(2_500_000_000);
        database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                1,
                100,
                80,
                "prefix",
                None,
                false,
                &batch,
            )
            .expect("commit fast");

        let summary = database
            .summary(&UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: UsageGroupBy::Source,
            })
            .expect("summary");

        assert_eq!(summary.fast.raw_tokens, 15);
        assert_eq!(summary.fast.billing_equivalent_tokens, "37.5");
        assert_eq!(summary.fast.minimum_multiplier.as_deref(), Some("2.5"));
        assert!(!summary.fast.has_unpriced_equivalent);
    }

    #[test]
    fn failed_batch_rolls_back_entries_and_watermark_together() {
        let (_dir, database) = database();
        let mut invalid = sample_batch("invalid");
        invalid.entries[0].tokens.uncached_input_tokens = -1;

        assert!(
            database
                .commit_scan_batch(
                    "file",
                    UsageSource::Codex,
                    1,
                    100,
                    80,
                    "prefix",
                    Some("{}"),
                    false,
                    &invalid,
                )
                .is_err()
        );
        assert!(database.scan_file_state("file").expect("state").is_none());
        assert_eq!(
            database
                .summary(&UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: UsageGroupBy::Source,
                })
                .expect("summary")
                .entry_count,
            0
        );
    }

    #[test]
    fn unique_index_deduplicates_without_an_in_memory_limit() {
        let (_dir, database) = database();
        for expected in [1, 0] {
            let result = database
                .commit_scan_batch(
                    "file",
                    UsageSource::Codex,
                    1,
                    100,
                    100,
                    "prefix",
                    None,
                    false,
                    &sample_batch("same"),
                )
                .expect("commit");
            assert_eq!(result.inserted, expected);
        }
    }

    #[test]
    fn later_complete_duplicate_can_replace_a_smaller_streaming_fact() {
        let (_dir, database) = database();
        let mut smaller = sample_batch("same-message");
        smaller.entries[0].source = UsageSource::Claude;
        smaller.conversations[0].source = UsageSource::Claude;
        database
            .commit_scan_batch(
                "file",
                UsageSource::Claude,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &smaller,
            )
            .expect("smaller");
        let mut larger = sample_batch("same-message");
        larger.entries[0].source = UsageSource::Claude;
        larger.conversations[0].source = UsageSource::Claude;
        larger.entries[0].tokens.output_tokens = 25;
        let outcome = database
            .commit_scan_batch(
                "file",
                UsageSource::Claude,
                2,
                2,
                2,
                "prefix",
                None,
                false,
                &larger,
            )
            .expect("larger");

        assert_eq!(outcome.inserted, 1);
        let summary = database
            .summary(&UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 1);
        assert_eq!(summary.tokens.output_tokens, 25);
    }

    #[test]
    fn schema_columns_are_allowlisted_and_never_include_raw_paths_or_secrets() {
        let (_dir, database) = database();
        let connection = database.open_read().expect("read");
        let expected = [
            (
                "scan_files",
                &[
                    "file_key",
                    "source",
                    "mtime_ms",
                    "size_bytes",
                    "offset_bytes",
                    "prefix_fingerprint",
                    "cursor_json",
                    "updated_at",
                ][..],
            ),
            (
                "usage_entries",
                &[
                    "id",
                    "file_key",
                    "source",
                    "dedup_key",
                    "conversation_key",
                    "model",
                    "speed",
                    "inference_geo",
                    "occurred_at",
                    "day_local",
                    "uncached_input_tokens",
                    "output_tokens",
                    "reasoning_output_tokens",
                    "cache_read_input_tokens",
                    "cache_write_5m_input_tokens",
                    "cache_write_1h_input_tokens",
                    "api_equivalent_cost_nanos",
                    "billing_equivalent_tokens_nanos",
                    "fast_multiplier_nanos",
                    "pricing_fingerprint",
                    "request_count",
                    "granularity",
                ][..],
            ),
            ("pricing_state", &["id", "fingerprint"][..]),
            (
                "conversations",
                &[
                    "conversation_key",
                    "source",
                    "title",
                    "project_hint",
                    "project_key",
                    "worktree_path",
                    "is_sidechain",
                    "unattributed",
                    "first_at",
                    "last_at",
                    "source_id",
                    "branch",
                ][..],
            ),
            (
                "quota_cycles",
                &[
                    "id",
                    "provider",
                    "identity_key",
                    "window_kind",
                    "window_id",
                    "start_at",
                    "end_at",
                    "scheduled_end_at",
                    "first_sample_at",
                    "last_sample_at",
                    "latest_used_percent",
                    "boundary_quality",
                    "source",
                    "updated_at",
                ][..],
            ),
            (
                "quota_allowance_segments",
                &[
                    "id",
                    "cycle_id",
                    "start_at",
                    "end_at",
                    "baseline_used_percent",
                    "latest_used_percent",
                    "maximum_used_percent",
                    "first_sample_at",
                    "last_sample_at",
                    "start_reason",
                ][..],
            ),
            (
                "quota_account_segments",
                &["id", "provider", "identity_key", "start_at", "end_at"][..],
            ),
            (
                "dsh_sessions",
                &[
                    "session_id",
                    "parent_session",
                    "root_session",
                    "cwd",
                    "title",
                    "project_key",
                    "updated_at",
                ][..],
            ),
            (
                "cursor_usage_coverage",
                &["account_key", "day_local", "updated_at"][..],
            ),
            (
                "dsh_session_usage",
                &[
                    "session_id",
                    "day_local",
                    "model",
                    "speed",
                    "request_count",
                    "uncached_input_tokens",
                    "output_tokens",
                    "reasoning_output_tokens",
                    "cache_read_input_tokens",
                    "cache_write_5m_input_tokens",
                    "cache_write_1h_input_tokens",
                    "api_equivalent_cost_nanos",
                    "updated_at",
                ][..],
            ),
            (
                "quota_events",
                &[
                    "id",
                    "provider",
                    "identity_key",
                    "window_kind",
                    "window_id",
                    "remaining_percent",
                    "observed_at",
                    "resets_at",
                ][..],
            ),
        ];

        for (table, columns) in expected {
            let mut statement = connection
                .prepare(&format!("PRAGMA table_info({table})"))
                .expect("pragma");
            let actual = statement
                .query_map([], |row| row.get::<_, String>(1))
                .expect("columns")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect");
            assert_eq!(actual, columns);
        }
    }

    #[test]
    fn summary_totals_count_priced_entries_once_and_reject_mixed_pricing_versions() {
        let (_dir, database) = database();
        database
            .commit_scan_batch(
                "one",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &sample_batch("one"),
            )
            .expect("first");
        let mut second = sample_batch("two");
        second.entries[0].pricing_fingerprint = Some("other".to_owned());
        database
            .commit_scan_batch(
                "two",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &second,
            )
            .expect("second");
        let mut third = sample_batch("three");
        third.entries[0].source = UsageSource::Claude;
        third.entries[0].conversation_key = "claude-conversation".to_owned();
        third.conversations[0].conversation_key = "claude-conversation".to_owned();
        third.conversations[0].source = UsageSource::Claude;
        database
            .commit_scan_batch(
                "three",
                UsageSource::Claude,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &third,
            )
            .expect("third");

        let summary = database
            .summary(&UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 3);
        assert_eq!(summary.cost.priced_entries, 3);
        assert_eq!(summary.cost.pricing_fingerprint, None);
        assert_eq!(summary.tokens.total_tokens, 45);
    }

    #[test]
    fn corrupt_database_is_quarantined_before_a_fresh_schema_is_created() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(DATABASE_FILE), b"not a sqlite database").expect("seed corrupt");
        let database = UsageDb::new(dir.path().to_path_buf());

        database.initialize().expect("recover");

        let names = fs::read_dir(dir.path())
            .expect("read directory")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        assert!(names.iter().any(|name| name == DATABASE_FILE));
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("usage.db.corrupt-")),
            "the broken database must be retained for diagnosis"
        );
        assert_eq!(
            database
                .summary(&UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: UsageGroupBy::Source,
                })
                .expect("empty rebuilt database")
                .entry_count,
            0
        );
    }

    #[test]
    fn verify_after_logical_write_recovers_a_database_corrupted_after_initialization() {
        let dir = tempfile::tempdir().expect("temp dir");
        let database = UsageDb::new(dir.path().to_path_buf());
        database.initialize().expect("init");

        fs::write(dir.path().join(DATABASE_FILE), b"not a sqlite database").expect("corrupt");

        database.verify_after_logical_write().expect("recover");
        database
            .verify_after_logical_write()
            .expect("healthy after recovery");

        let names = fs::read_dir(dir.path())
            .expect("read directory")
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .collect::<Vec<_>>();
        assert!(
            names
                .iter()
                .any(|name| name.starts_with("usage.db.corrupt-")),
            "the broken database must be retained for diagnosis"
        );
        assert_eq!(
            database
                .summary(&UsageSummaryQuery {
                    filter: UsageFilter::default(),
                    group_by: UsageGroupBy::Source,
                })
                .expect("empty rebuilt database")
                .entry_count,
            0
        );
    }

    #[test]
    fn newer_schema_is_never_silently_downgraded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let connection = Connection::open(dir.path().join(DATABASE_FILE)).expect("open");
        connection
            .pragma_update(None, "user_version", SCHEMA_VERSION + 1)
            .expect("set future version");
        drop(connection);
        let database = UsageDb::new(dir.path().to_path_buf());

        assert!(matches!(
            database.initialize(),
            Err(UsageDbError::UnsupportedSchema)
        ));
        let connection = Connection::open(dir.path().join(DATABASE_FILE)).expect("reopen");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION + 1);
    }

    #[test]
    fn reset_transaction_replaces_only_the_target_file_facts() {
        let (_dir, database) = database();
        database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &sample_batch("old"),
            )
            .expect("old");
        database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                2,
                1,
                1,
                "new-prefix",
                None,
                true,
                &sample_batch("new"),
            )
            .expect("replacement");

        let summary = database
            .summary(&UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 1);
    }

    #[test]
    fn quota_history_only_appends_integer_changes() {
        let (_dir, database) = database();
        let mut snapshot = QuotaSnapshot {
            windows: vec![QuotaWindow {
                id: "window".to_owned(),
                kind: QuotaWindowKind::FiveHour,
                display_name: None,
                used_percent: 32.0,
                remaining_percent: 68.0,
                resets_at: Some("2026-07-30T06:00:00Z".to_owned()),
                window_seconds: Some(18_000),
                is_active: true,
                is_primary: true,
                unlimited: false,
            }],
            captured_at: "2026-07-30T00:00:00Z".to_owned(),
        };
        database
            .record_quota_snapshot(ProviderId::Codex, "identity", &snapshot)
            .expect("first");
        database
            .record_quota_snapshot(ProviderId::Codex, "identity", &snapshot)
            .expect("duplicate");
        snapshot.windows[0].remaining_percent = 67.0;
        snapshot.windows[0].resets_at = None;
        snapshot.captured_at = "2026-07-30T00:10:00Z".to_owned();
        database
            .record_quota_snapshot(ProviderId::Codex, "identity", &snapshot)
            .expect("changed");

        let connection = database.open_read().expect("read");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM quota_events", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 2);

        let events = database
            .quota_history(None, None, None, 100)
            .expect("history");
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].resets_at.as_deref(), Some("2026-07-30T06:00:00Z"));
        assert_eq!(events[1].resets_at, None);
    }

    #[test]
    fn cursor_remote_days_are_replaced_atomically_and_covered() {
        let (_dir, database) = database();
        let bucket = |day: &str, model: &str, input: i64| CursorRemoteBucket {
            day_local: day.to_owned(),
            model: model.to_owned(),
            input_tokens: input,
            output_tokens: 5,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            charged_nanos: 1_000_000,
            request_count: 2,
        };

        let first = database
            .cursor_replace_days(
                "account-1",
                "2026-10-01",
                "2026-10-02",
                "cursor:account-1",
                &[
                    bucket("2026-10-01", "a", 100),
                    bucket("2026-10-02", "a", 50),
                ],
            )
            .expect("replace");
        assert_eq!(first.inserted, 2);
        assert_eq!(
            database.cursor_coverage("account-1").expect("coverage"),
            vec!["2026-10-01".to_owned(), "2026-10-02".to_owned()]
        );

        // 再拉一次同一天：整天替换，不叠加。
        let second = database
            .cursor_replace_days(
                "account-1",
                "2026-10-01",
                "2026-10-01",
                "cursor:account-1",
                &[bucket("2026-10-01", "a", 7)],
            )
            .expect("replace again");
        assert_eq!(second.removed, 1);
        assert_eq!(second.inserted, 1);

        let summary = database
            .summary(&UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 2, "10-01 被替换、10-02 保留");
        // 两条按天事实：替换后的 7 与保留的 50。
        assert_eq!(summary.tokens.uncached_input_tokens, 57);
        assert_eq!(summary.request_count, 4);
        // 按天粒度的事实请求数来自事件条数，不是行数。
        assert_eq!(summary.rows[0].entry_count, 2);
    }

    #[test]
    fn cursor_remote_entries_are_unattributed_and_excluded_from_conversations() {
        let (_dir, database) = database();
        database
            .cursor_replace_days(
                "account-1",
                "2026-10-01",
                "2026-10-01",
                "cursor:account-1",
                &[CursorRemoteBucket {
                    day_local: "2026-10-01".to_owned(),
                    model: "claude-4.5-sonnet".to_owned(),
                    input_tokens: 10,
                    output_tokens: 1,
                    cache_read_tokens: 0,
                    cache_write_tokens: 0,
                    charged_nanos: 0,
                    request_count: 1,
                }],
            )
            .expect("replace");

        let page = database
            .conversations(
                &UsageConversationQuery {
                    filter: UsageFilter::default(),
                    ..UsageConversationQuery::default()
                },
                10,
                0,
                None,
                None,
            )
            .expect("conversations");
        // 远端计量没有对话身份：不进入对话列表，但计入概览与「未归属」分组。
        assert_eq!(page.total, 0);
        assert!(
            !database
                .cursor_coverage("account-1")
                .expect("coverage")
                .is_empty()
        );
    }

    #[test]
    fn switching_cursor_accounts_drops_the_previous_ledger() {
        let (_dir, database) = database();
        let bucket = |day: &str| CursorRemoteBucket {
            day_local: day.to_owned(),
            model: "a".to_owned(),
            input_tokens: 10,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            charged_nanos: 0,
            request_count: 1,
        };

        database
            .cursor_replace_days(
                "account-1",
                "2026-10-01",
                "2026-10-01",
                "cursor:account-1",
                &[bucket("2026-10-01")],
            )
            .expect("first account");
        assert_eq!(
            database.cursor_coverage_accounts().expect("accounts").len(),
            1
        );

        database.cursor_reset_account("account-1").expect("reset");
        let summary = database
            .summary(&UsageSummaryQuery {
                filter: UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        assert_eq!(summary.entry_count, 0, "换账号后旧账必须清掉");
        assert!(
            database
                .cursor_coverage("account-1")
                .expect("coverage")
                .is_empty()
        );
    }

    #[test]
    fn schema_v5_migrates_to_v6_preserving_existing_quota_rows() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(DATABASE_FILE);
        let connection = Connection::open(&path).expect("open legacy");
        connection
            .execute_batch(
                "CREATE TABLE usage_entries (
                   id INTEGER PRIMARY KEY,
                   file_key TEXT NOT NULL,
                   source TEXT NOT NULL
                     CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
                   dedup_key TEXT NOT NULL,
                   conversation_key TEXT NOT NULL,
                   model TEXT,
                   speed TEXT NOT NULL CHECK(speed IN ('standard', 'fast', 'unknown')),
                   inference_geo TEXT NOT NULL
                     CHECK(inference_geo IN ('global', 'us', 'unknown')),
                   occurred_at TEXT NOT NULL,
                   day_local TEXT NOT NULL,
                   uncached_input_tokens INTEGER NOT NULL CHECK(uncached_input_tokens >= 0),
                   output_tokens INTEGER NOT NULL CHECK(output_tokens >= 0),
                   reasoning_output_tokens INTEGER NOT NULL CHECK(reasoning_output_tokens >= 0),
                   cache_read_input_tokens INTEGER NOT NULL CHECK(cache_read_input_tokens >= 0),
                   cache_write_5m_input_tokens INTEGER NOT NULL
                     CHECK(cache_write_5m_input_tokens >= 0),
                   cache_write_1h_input_tokens INTEGER NOT NULL
                     CHECK(cache_write_1h_input_tokens >= 0),
                   api_equivalent_cost_nanos INTEGER,
                   billing_equivalent_tokens_nanos INTEGER,
                   fast_multiplier_nanos INTEGER,
                   pricing_fingerprint TEXT,
                   CHECK(reasoning_output_tokens <= output_tokens)
                 );
                 CREATE UNIQUE INDEX ux_entries_dedup ON usage_entries(source, dedup_key);
                 CREATE TABLE conversations (
                   conversation_key TEXT PRIMARY KEY,
                   source TEXT NOT NULL
                     CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
                   title TEXT,
                   project_hint TEXT,
                   is_sidechain INTEGER NOT NULL DEFAULT 0 CHECK(is_sidechain IN (0, 1)),
                   first_at TEXT NOT NULL,
                   last_at TEXT NOT NULL,
                   source_id TEXT,
                   branch TEXT
                 );
                 CREATE TABLE scan_files (
                   file_key TEXT PRIMARY KEY,
                   source TEXT NOT NULL
                     CHECK(source IN ('codex', 'claude', 'pi', 'opencode')),
                   mtime_ms INTEGER NOT NULL,
                   size_bytes INTEGER NOT NULL,
                   offset_bytes INTEGER NOT NULL,
                   prefix_fingerprint TEXT NOT NULL,
                   cursor_json TEXT,
                   updated_at TEXT NOT NULL
                 );
                 CREATE TABLE quota_events (
                   id INTEGER PRIMARY KEY,
                   provider TEXT NOT NULL CHECK(provider IN ('codex', 'claude')),
                   identity_key TEXT NOT NULL,
                   window_kind TEXT NOT NULL,
                   window_id TEXT,
                   remaining_percent INTEGER NOT NULL CHECK(remaining_percent BETWEEN 0 AND 100),
                   observed_at TEXT NOT NULL
                 );
                 CREATE INDEX ix_quota_series
                   ON quota_events(provider, identity_key, window_kind, window_id, observed_at);
                 INSERT INTO quota_events (
                   provider, identity_key, window_kind, window_id,
                   remaining_percent, observed_at
                 ) VALUES ('codex', 'identity', 'five_hour', NULL, 68, '2026-07-30T00:00:00Z');
                 PRAGMA user_version = 5;",
            )
            .expect("seed v5");
        drop(connection);

        let database = UsageDb::new(dir.path().to_path_buf());
        database.initialize().expect("migrate");
        let connection = Connection::open(&path).expect("open migrated");
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        let resets_at: Option<String> = connection
            .query_row(
                "SELECT resets_at FROM quota_events WHERE observed_at = '2026-07-30T00:00:00Z'",
                [],
                |row| row.get(0),
            )
            .expect("legacy row keeps null resets_at");
        assert_eq!(resets_at, None);

        let database = UsageDb::new(dir.path().to_path_buf());
        database.initialize().expect("reopen");
        let events = database
            .quota_history(None, None, None, 100)
            .expect("history reads migrated rows");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].remaining_percent, 68);
        assert_eq!(events[0].resets_at, None);
    }

    #[test]
    fn conversation_list_supports_project_filter_and_token_cost_sort() {
        let (_dir, database) = database();
        #[allow(clippy::too_many_arguments)]
        fn entry_batch(
            key: &str,
            title: Option<&str>,
            project: Option<&str>,
            model: &str,
            speed: UsageSpeed,
            occurred_at: &str,
            tokens: TokenFacts,
            cost: Option<i64>,
        ) -> ScanBatch {
            ScanBatch {
                entries: vec![UsageEntry {
                    source: UsageSource::Codex,
                    dedup_key: format!("{key}-{model}-{occurred_at}"),
                    conversation_key: key.to_owned(),
                    model: Some(model.to_owned()),
                    speed,
                    inference_geo: InferenceGeo::Global,
                    occurred_at: occurred_at.to_owned(),
                    day_local: occurred_at[..10].to_owned(),
                    tokens,
                    api_equivalent_cost_nanos: cost,
                    billing_equivalent_tokens_nanos: None,
                    fast_multiplier_nanos: None,
                    pricing_fingerprint: Some("price".to_owned()),
                    request_count: 1,
                    granularity: Granularity::Request,
                }],
                conversations: vec![ConversationFact {
                    conversation_key: key.to_owned(),
                    source: UsageSource::Codex,
                    title: title.map(str::to_owned),
                    project_hint: project.map(str::to_owned),
                    project_key: project.map(|value| format!("/work/{value}")),
                    worktree_path: project.map(|value| format!("/work/{value}")),
                    unattributed: false,
                    is_sidechain: false,
                    occurred_at: occurred_at.to_owned(),
                    source_id: None,
                    branch: None,
                }],
                ..ScanBatch::default()
            }
        }
        let small_tokens = TokenFacts {
            uncached_input_tokens: 100,
            output_tokens: 50,
            ..TokenFacts::default()
        };
        let big_tokens = TokenFacts {
            uncached_input_tokens: 1_000,
            output_tokens: 500,
            ..TokenFacts::default()
        };

        database
            .commit_scan_batch(
                "file-big",
                UsageSource::Codex,
                1,
                1,
                1,
                "p",
                None,
                false,
                &entry_batch(
                    "big",
                    Some("Big title"),
                    Some("proj-a"),
                    "gpt-5.6-sol",
                    UsageSpeed::Standard,
                    "2026-07-30T00:00:00Z",
                    big_tokens,
                    Some(900),
                ),
            )
            .expect("big");
        database
            .commit_scan_batch(
                "file-small",
                UsageSource::Codex,
                1,
                1,
                1,
                "p",
                None,
                false,
                &entry_batch(
                    "small",
                    Some("Small title"),
                    Some("proj-b"),
                    "gpt-5.6-sol",
                    UsageSpeed::Standard,
                    "2026-07-31T00:00:00Z",
                    small_tokens,
                    Some(100),
                ),
            )
            .expect("small");

        let list = |query: &UsageConversationQuery| {
            database
                .conversations(query, 50, 0, None, query.project.as_deref())
                .expect("list")
        };
        let by_tokens = list(&UsageConversationQuery {
            sort: Some(UsageConversationSort::Tokens),
            ..UsageConversationQuery::default()
        });
        assert_eq!(
            by_tokens.items[0].conversation_key, "big",
            "bigger conversation sorts first by tokens"
        );
        let by_cost = list(&UsageConversationQuery {
            sort: Some(UsageConversationSort::Cost),
            ..UsageConversationQuery::default()
        });
        assert_eq!(by_cost.items[0].conversation_key, "big");
        let by_recent = list(&UsageConversationQuery {
            sort: Some(UsageConversationSort::Recent),
            ..UsageConversationQuery::default()
        });
        assert_eq!(by_recent.items[0].conversation_key, "small");

        let filtered = list(&UsageConversationQuery {
            project: Some("/work/proj-a".to_owned()),
            ..UsageConversationQuery::default()
        });
        assert_eq!(filtered.total, 1);
        assert_eq!(filtered.items[0].conversation_key, "big");
    }

    #[test]
    fn conversation_breakdown_groups_by_model_and_speed() {
        let (_dir, database) = database();
        let standard = TokenFacts {
            uncached_input_tokens: 10,
            output_tokens: 5,
            ..TokenFacts::default()
        };
        let fast = TokenFacts {
            uncached_input_tokens: 20,
            output_tokens: 10,
            ..TokenFacts::default()
        };

        let batch = ScanBatch {
            entries: vec![
                UsageEntry {
                    source: UsageSource::Codex,
                    dedup_key: "a-1".to_owned(),
                    conversation_key: "conv".to_owned(),
                    model: Some("model-a".to_owned()),
                    speed: UsageSpeed::Standard,
                    inference_geo: InferenceGeo::Global,
                    occurred_at: "2026-07-30T00:00:00Z".to_owned(),
                    day_local: "2026-07-30".to_owned(),
                    tokens: standard,
                    api_equivalent_cost_nanos: Some(100),
                    billing_equivalent_tokens_nanos: None,
                    fast_multiplier_nanos: None,
                    pricing_fingerprint: Some("price".to_owned()),
                    request_count: 1,
                    granularity: Granularity::Request,
                },
                UsageEntry {
                    source: UsageSource::Codex,
                    dedup_key: "b-1".to_owned(),
                    conversation_key: "conv".to_owned(),
                    model: Some("model-b".to_owned()),
                    speed: UsageSpeed::Fast,
                    inference_geo: InferenceGeo::Global,
                    occurred_at: "2026-07-30T01:00:00Z".to_owned(),
                    day_local: "2026-07-30".to_owned(),
                    tokens: fast,
                    api_equivalent_cost_nanos: Some(300),
                    billing_equivalent_tokens_nanos: Some(2_000_000_000),
                    fast_multiplier_nanos: Some(1_500_000_000),
                    pricing_fingerprint: Some("price".to_owned()),
                    request_count: 1,
                    granularity: Granularity::Request,
                },
            ],
            conversations: vec![ConversationFact {
                conversation_key: "conv".to_owned(),
                source: UsageSource::Codex,
                title: None,
                project_hint: None,
                project_key: None,
                worktree_path: None,
                unattributed: false,
                is_sidechain: false,
                occurred_at: "2026-07-30T00:00:00Z".to_owned(),
                source_id: None,
                branch: None,
            }],
            ..ScanBatch::default()
        };
        database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                1,
                1,
                1,
                "p",
                None,
                false,
                &batch,
            )
            .expect("commit");

        let breakdown = database.conversation_breakdown("conv").expect("breakdown");
        assert_eq!(breakdown.models.len(), 2);
        let model_b = breakdown
            .models
            .iter()
            .find(|row| row.key == "model-b")
            .expect("model-b");
        assert_eq!(model_b.tokens.total_tokens, 30);
        assert_eq!(model_b.cost.api_equivalent_cost_nanos, 300);
        assert_eq!(model_b.fast.billing_equivalent_tokens, "2");
        assert_eq!(model_b.fast.raw_tokens, 30);

        let speeds = &breakdown.speeds;
        assert_eq!(speeds.len(), 2);
        let fast_speed = speeds
            .iter()
            .find(|row| row.key == "fast")
            .expect("fast speed");
        assert_eq!(fast_speed.fast.raw_tokens, 30);
        assert_eq!(fast_speed.fast.minimum_multiplier.as_deref(), Some("1.5"));
        assert_eq!(fast_speed.fast.maximum_multiplier.as_deref(), Some("1.5"));
        let standard_speed = speeds
            .iter()
            .find(|row| row.key == "standard")
            .expect("standard speed");
        assert_eq!(standard_speed.fast.raw_tokens, 0);
    }

    #[test]
    fn quota_history_returns_chronological_events_with_filtering_and_limit() {
        let (_dir, database) = database();
        fn snapshot(captured_at: &str, remaining: f64) -> QuotaSnapshot {
            QuotaSnapshot {
                windows: vec![QuotaWindow {
                    id: "window".to_owned(),
                    kind: QuotaWindowKind::FiveHour,
                    display_name: None,
                    used_percent: 100.0 - remaining,
                    remaining_percent: remaining,
                    resets_at: None,
                    window_seconds: Some(18_000),
                    is_active: true,
                    is_primary: true,
                    unlimited: false,
                }],
                captured_at: captured_at.to_owned(),
            }
        }

        database
            .record_quota_snapshot(
                ProviderId::Codex,
                "codex-identity",
                &snapshot("2026-07-30T00:00:00Z", 80.0),
            )
            .expect("codex-1");
        database
            .record_quota_snapshot(
                ProviderId::Codex,
                "codex-identity",
                &snapshot("2026-07-30T02:00:00Z", 70.0),
            )
            .expect("codex-2");
        database
            .record_quota_snapshot(
                ProviderId::Claude,
                "claude-identity",
                &snapshot("2026-07-30T01:00:00Z", 90.0),
            )
            .expect("claude-1");

        let all = database.quota_history(None, None, None, 500).expect("all");
        assert_eq!(all.len(), 3);
        assert_eq!(
            all.iter()
                .map(|event| event.observed_at.as_str())
                .collect::<Vec<_>>(),
            vec![
                "2026-07-30T00:00:00Z",
                "2026-07-30T01:00:00Z",
                "2026-07-30T02:00:00Z"
            ],
            "events must come back in chronological order"
        );
        assert_eq!(all[0].provider, ProviderId::Codex);
        assert_eq!(all[0].identity_key, "codex-identity");
        assert_eq!(all[0].window_kind, QuotaWindowKind::FiveHour);
        assert_eq!(all[0].remaining_percent, 80);

        let codex = database
            .quota_history(Some(ProviderId::Codex), None, None, 500)
            .expect("codex");
        assert_eq!(codex.len(), 2);
        assert!(
            codex
                .iter()
                .all(|event| event.provider == ProviderId::Codex)
        );

        let bounded = database
            .quota_history(
                None,
                Some("2026-07-30T00:30:00Z"),
                Some("2026-07-30T01:30:00Z"),
                500,
            )
            .expect("bounded");
        assert_eq!(bounded.len(), 1);
        assert_eq!(bounded[0].identity_key, "claude-identity");

        let limited = database
            .quota_history(None, None, None, 2)
            .expect("limited");
        assert_eq!(
            limited
                .iter()
                .map(|event| event.observed_at.as_str())
                .collect::<Vec<_>>(),
            vec!["2026-07-30T01:00:00Z", "2026-07-30T02:00:00Z"],
            "limit keeps the two most recent events, still in chronological order"
        );
    }

    #[test]
    fn recovery_rebuilds_derived_tables_and_restores_valid_quota_history() {
        let (dir, database) = database();
        let snapshot = QuotaSnapshot {
            windows: vec![QuotaWindow {
                id: "window".to_owned(),
                kind: QuotaWindowKind::Weekly,
                display_name: None,
                used_percent: 40.0,
                remaining_percent: 60.0,
                resets_at: None,
                window_seconds: Some(604_800),
                is_active: true,
                is_primary: true,
                unlimited: false,
            }],
            captured_at: "2026-07-30T00:00:00Z".to_owned(),
        };
        database
            .record_quota_snapshot(ProviderId::Claude, "identity", &snapshot)
            .expect("history");
        database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &sample_batch("derived"),
            )
            .expect("derived");

        database
            .recover_corrupt_database()
            .expect("recover database family");

        let connection = database.open_read().expect("read");
        let history_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM quota_events", [], |row| row.get(0))
            .expect("history count");
        let usage_count: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_entries", [], |row| row.get(0))
            .expect("usage count");
        assert_eq!(history_count, 1);
        assert_eq!(usage_count, 0);
        assert!(
            fs::read_dir(dir.path())
                .expect("directory")
                .filter_map(Result::ok)
                .filter_map(|entry| entry.file_name().into_string().ok())
                .any(|name| name.starts_with("usage.db.corrupt-"))
        );
    }

    #[test]
    fn repricing_only_updates_database_derived_columns() {
        let (_dir, database) = database();
        let mut batch = sample_batch("reprice");
        batch.entries[0].api_equivalent_cost_nanos = None;
        batch.entries[0].pricing_fingerprint = None;
        database
            .commit_scan_batch(
                "file",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &batch,
            )
            .expect("seed");

        let result = database
            .reprice(&PricingCatalog::bundled())
            .expect("reprice");

        assert_eq!(result.updated_entries, 1);
        assert_eq!(result.priced_entries, 1);
        assert_eq!(result.unpriced_entries, 0);
    }

    #[test]
    fn repricing_never_touches_pi_or_opencode_cost() {
        let (_dir, database) = database();

        let mut codex_batch = sample_batch("codex-reprice");
        codex_batch.entries[0].api_equivalent_cost_nanos = None;
        codex_batch.entries[0].pricing_fingerprint = None;
        database
            .commit_scan_batch(
                "file-codex",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &codex_batch,
            )
            .expect("codex");

        // Pi 行：自带真实 cost，必须原样保留。
        let mut pi_batch = sample_batch("pi-reprice");
        pi_batch.entries[0].source = UsageSource::Pi;
        pi_batch.entries[0].api_equivalent_cost_nanos = Some(42_000);
        pi_batch.entries[0].pricing_fingerprint = None;
        database
            .commit_scan_batch(
                "file-pi",
                UsageSource::Pi,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &pi_batch,
            )
            .expect("pi");

        let result = database
            .reprice(&PricingCatalog::bundled())
            .expect("reprice");

        // 只有 codex 行被重算；pi 行不参与。
        assert_eq!(result.updated_entries, 1);
        assert_eq!(result.priced_entries, 1);
        assert_eq!(result.unpriced_entries, 0);

        let pi_cost: Option<i64> = database
            .open_read()
            .expect("read")
            .query_row(
                "SELECT api_equivalent_cost_nanos FROM usage_entries WHERE source = 'pi'",
                [],
                |row| row.get(0),
            )
            .expect("pi cost");
        assert_eq!(pi_cost, Some(42_000), "pi 的真实 cost 不得被重计价覆盖");
    }

    #[test]
    fn rebuild_data_removes_only_the_selected_sources() {
        let (_dir, database) = database();
        database
            .commit_scan_batch(
                "file-codex",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &sample_batch("codex-rebuild"),
            )
            .expect("codex");
        let mut pi_batch = sample_batch("pi-rebuild");
        pi_batch.entries[0].source = UsageSource::Pi;
        pi_batch.entries[0].conversation_key = "conversation-pi".to_owned();
        pi_batch.conversations[0].source = UsageSource::Pi;
        pi_batch.conversations[0].conversation_key = "conversation-pi".to_owned();
        database
            .commit_scan_batch(
                "file-pi",
                UsageSource::Pi,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &pi_batch,
            )
            .expect("pi");
        database
            .save_opencode_state(&OpencodeScanState {
                watermark_ms: 1234,
                seen_ids: vec!["m1".to_owned()],
            })
            .expect("opencode state");

        let result = database
            .rebuild_data(Some(&[UsageSource::Pi, UsageSource::Opencode]))
            .expect("rebuild");
        assert_eq!(result.entries_removed, 1);
        assert_eq!(result.conversations_removed, 1);
        assert_eq!(result.files_removed, 1);

        let connection = database.open_read().expect("read");
        let codex_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM usage_entries WHERE source = 'codex'",
                [],
                |row| row.get(0),
            )
            .expect("codex count");
        assert_eq!(codex_count, 1, "codex 数据不得被部分重建误删");
        let removed_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM usage_entries WHERE source IN ('pi', 'opencode')",
                [],
                |row| row.get(0),
            )
            .expect("removed count");
        assert_eq!(removed_count, 0);
        let scan_count: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM scan_files WHERE source IN ('pi', 'opencode')",
                [],
                |row| row.get(0),
            )
            .expect("scan count");
        assert_eq!(scan_count, 0);

        let state = database.opencode_state().expect("state");
        assert_eq!(state.watermark_ms, 0, "opencode 水位必须重置为全量重扫起点");
        assert!(state.seen_ids.is_empty());
    }

    #[test]
    fn rebuild_all_sources_empties_entries_conversations_and_watermarks() {
        let (_dir, database) = database();
        database
            .commit_scan_batch(
                "file-codex",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &sample_batch("codex-all"),
            )
            .expect("codex");
        let mut pi_batch = sample_batch("pi-all");
        pi_batch.entries[0].source = UsageSource::Pi;
        pi_batch.entries[0].conversation_key = "conversation-pi".to_owned();
        pi_batch.conversations[0].source = UsageSource::Pi;
        pi_batch.conversations[0].conversation_key = "conversation-pi".to_owned();
        database
            .commit_scan_batch(
                "file-pi",
                UsageSource::Pi,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &pi_batch,
            )
            .expect("pi");
        database
            .save_opencode_state(&OpencodeScanState {
                watermark_ms: 999,
                seen_ids: vec!["m1".to_owned(), "m2".to_owned()],
            })
            .expect("opencode state");

        let result = database.rebuild_data(None).expect("rebuild all");
        assert_eq!(result.entries_removed, 2);
        assert_eq!(result.conversations_removed, 2);
        assert_eq!(result.files_removed, 2);

        let connection = database.open_read().expect("read");
        let total_entries: i64 = connection
            .query_row("SELECT COUNT(*) FROM usage_entries", [], |row| row.get(0))
            .expect("entries");
        let total_conversations: i64 = connection
            .query_row("SELECT COUNT(*) FROM conversations", [], |row| row.get(0))
            .expect("conversations");
        assert_eq!(total_entries, 0);
        assert_eq!(total_conversations, 0, "无引用的对话必须一并清理");
        assert_eq!(database.opencode_state().expect("state").watermark_ms, 0);
    }

    #[test]
    fn conflict_overwrites_opencode_shrunk_values_but_keeps_codex_append_only() {
        let (_dir, database) = database();

        // opencode 官方 cost 是真值：token 变小也必须采纳，否则上游补写永不生效。
        let mut first = sample_batch("opencode-shrink");
        first.entries[0].source = UsageSource::Opencode;
        first.entries[0].api_equivalent_cost_nanos = Some(1000);
        database
            .commit_scan_batch(
                "file-opencode",
                UsageSource::Opencode,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &first,
            )
            .expect("opencode first");
        let mut shrunk = sample_batch("opencode-shrink");
        shrunk.entries[0].source = UsageSource::Opencode;
        shrunk.entries[0].api_equivalent_cost_nanos = Some(500);
        shrunk.entries[0].tokens = TokenFacts {
            uncached_input_tokens: 4,
            output_tokens: 2,
            ..TokenFacts::default()
        };
        database
            .commit_scan_batch(
                "file-opencode",
                UsageSource::Opencode,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &shrunk,
            )
            .expect("opencode second");
        let opencode_row: (i64, Option<i64>) = database
            .open_read()
            .expect("read")
            .query_row(
                "SELECT uncached_input_tokens + output_tokens, api_equivalent_cost_nanos
                   FROM usage_entries WHERE source = 'opencode'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .expect("opencode row");
        assert_eq!(opencode_row, (6, Some(500)));

        // codex 由价格目录驱动：token 变小不采纳，保持只增不减的防倒算语义。
        database
            .commit_scan_batch(
                "file-codex",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &sample_batch("codex-append"),
            )
            .expect("codex first");
        let mut codex_shrunk = sample_batch("codex-append");
        codex_shrunk.entries[0].tokens = TokenFacts {
            uncached_input_tokens: 2,
            output_tokens: 1,
            ..TokenFacts::default()
        };
        database
            .commit_scan_batch(
                "file-codex",
                UsageSource::Codex,
                1,
                1,
                1,
                "prefix",
                None,
                false,
                &codex_shrunk,
            )
            .expect("codex second");
        let codex_tokens: i64 = database
            .open_read()
            .expect("read")
            .query_row(
                "SELECT uncached_input_tokens + output_tokens
                   FROM usage_entries WHERE source = 'codex'",
                [],
                |row| row.get(0),
            )
            .expect("codex row");
        assert_eq!(codex_tokens, 15);
    }
}
