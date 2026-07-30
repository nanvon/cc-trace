//! 本地用量 SQLite。
//!
//! 路径只用于打开 CC Trace 自己的数据库，从不进入错误、日志或 command 载荷。
//! 外部 JSONL 路径只以 SHA-256 `file_key` 出现在表中。

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::Utc;
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::contracts::{
    ProviderId, QuotaSnapshot, QuotaWindowKind, UsageConversation, UsageConversationPage,
    UsageConversationQuery, UsageCostTotals, UsageGroupBy, UsageRepriceResult, UsageSource,
    UsageSpeed, UsageSummary, UsageSummaryQuery, UsageSummaryRow, UsageTokenTotals,
};
use crate::usage::model::{InferenceGeo, RepriceRow, ScanBatch, ScanFileState, TokenFacts};
use crate::usage::pricing::PricingCatalog;

const DATABASE_FILE: &str = "usage.db";
const SCHEMA_VERSION: i64 = 1;

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

struct QuotaEventBackup {
    provider: String,
    identity_key: String,
    window_kind: String,
    window_id: Option<String>,
    remaining_percent: i64,
    observed_at: String,
}

pub struct UsageDb {
    directory: PathBuf,
    write_lock: Mutex<()>,
    initialized: AtomicBool,
}

impl UsageDb {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            write_lock: Mutex::new(()),
            initialized: AtomicBool::new(false),
        }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(DATABASE_FILE)
    }

    pub fn initialize(&self) -> Result<(), UsageDbError> {
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        let _guard = self.write_lock.lock().expect("usage db write lock");
        if self.initialized.load(Ordering::Acquire) {
            return Ok(());
        }
        self.open_write()?;
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
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write()?;
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
                   api_equivalent_cost_nanos, pricing_fingerprint
                 ) VALUES (
                   ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17
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
                   pricing_fingerprint = excluded.pricing_fingerprint
                 WHERE (
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
                    entry.pricing_fingerprint,
                ])?)
                .unwrap_or(0);
            }
        }

        {
            let mut statement = transaction.prepare_cached(
                "INSERT INTO conversations (
                   conversation_key, source, title, project_hint, is_sidechain, first_at, last_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)
                 ON CONFLICT(conversation_key) DO UPDATE SET
                   title = COALESCE(excluded.title, conversations.title),
                   project_hint = COALESCE(excluded.project_hint, conversations.project_hint),
                   is_sidechain = MAX(conversations.is_sidechain, excluded.is_sidechain),
                   first_at = MIN(conversations.first_at, excluded.first_at),
                   last_at = MAX(conversations.last_at, excluded.last_at)",
            )?;
            for conversation in &batch.conversations {
                statement.execute(params![
                    conversation.conversation_key,
                    conversation.source.as_db(),
                    conversation.title,
                    conversation.project_hint,
                    i64::from(conversation.is_sidechain),
                    conversation.occurred_at,
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

        Ok(CommitResult {
            inserted,
            duplicates: batch.entries.len() as u64 - inserted,
        })
    }

    pub fn summary(&self, query: &UsageSummaryQuery) -> Result<UsageSummary, UsageDbError> {
        let connection = self.open_read()?;
        let group = match query.group_by {
            UsageGroupBy::Day => "day_local",
            UsageGroupBy::Source => "source",
            UsageGroupBy::Model => "COALESCE(model, '')",
            UsageGroupBy::Speed => "speed",
        };
        let sql = format!(
            "SELECT {group}, COUNT(*),
                    COALESCE(SUM(uncached_input_tokens), 0),
                    COALESCE(SUM(output_tokens), 0),
                    COALESCE(SUM(reasoning_output_tokens), 0),
                    COALESCE(SUM(cache_read_input_tokens), 0),
                    COALESCE(SUM(cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(api_equivalent_cost_nanos), 0),
                    SUM(CASE WHEN api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN source = 'claude' AND inference_geo = 'unknown'
                             AND api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    CASE WHEN COUNT(DISTINCT pricing_fingerprint) = 1
                         THEN MAX(pricing_fingerprint) END
               FROM usage_entries
              WHERE (?1 IS NULL OR occurred_at >= ?1)
                AND (?2 IS NULL OR occurred_at < ?2)
                AND (?3 IS NULL OR source = ?3)
                AND (?4 IS NULL OR model = ?4)
                AND (?5 IS NULL OR speed = ?5)
              GROUP BY {group}
              ORDER BY {group}"
        );

        let mut statement = connection.prepare(&sql)?;
        let source = query.filter.source.map(UsageSource::as_db);
        let speed = query.filter.speed.map(UsageSpeed::as_db);
        let mut rows = statement.query(params![
            query.filter.from.as_deref(),
            query.filter.to.as_deref(),
            source,
            query.filter.model.as_deref(),
            speed,
        ])?;
        let mut output = Vec::new();
        while let Some(row) = rows.next()? {
            output.push(summary_row(row)?);
        }

        let mut total_tokens = UsageTokenTotals::default();
        let mut total_cost = UsageCostTotals::default();
        let mut entry_count = 0_i64;
        let mut fingerprints = HashSet::new();
        let mut mixed_pricing_versions = false;
        for row in &output {
            entry_count += row.entry_count;
            total_tokens.add_assign(&row.tokens);
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
            tokens: total_tokens,
            cost: total_cost,
        })
    }

    pub fn conversations(
        &self,
        query: &UsageConversationQuery,
        limit: u32,
        offset: u64,
        search: Option<&str>,
    ) -> Result<UsageConversationPage, UsageDbError> {
        let connection = self.open_read()?;
        let filter = &query.filter;
        let source = filter.source.map(UsageSource::as_db);
        let speed = filter.speed.map(UsageSpeed::as_db);
        let escaped_search = search.map(escape_like);

        let count = connection.query_row(
            "SELECT COUNT(DISTINCT c.conversation_key)
               FROM conversations c
               JOIN usage_entries e ON e.conversation_key = c.conversation_key
              WHERE (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.source = ?3)
                AND (?4 IS NULL OR e.model = ?4)
                AND (?5 IS NULL OR e.speed = ?5)
                AND (?6 IS NULL
                     OR COALESCE(c.title, '') LIKE '%' || ?6 || '%' ESCAPE '\'
                     OR COALESCE(c.project_hint, '') LIKE '%' || ?6 || '%' ESCAPE '\')",
            params![
                filter.from.as_deref(),
                filter.to.as_deref(),
                source,
                filter.model.as_deref(),
                speed,
                escaped_search.as_deref(),
            ],
            |row| row.get(0),
        )?;

        let mut statement = connection.prepare(
            "SELECT c.conversation_key, c.source, c.title, c.project_hint,
                    c.is_sidechain, c.first_at, c.last_at, COUNT(*),
                    COALESCE(SUM(e.uncached_input_tokens), 0),
                    COALESCE(SUM(e.output_tokens), 0),
                    COALESCE(SUM(e.reasoning_output_tokens), 0),
                    COALESCE(SUM(e.cache_read_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_5m_input_tokens), 0),
                    COALESCE(SUM(e.cache_write_1h_input_tokens), 0),
                    COALESCE(SUM(e.api_equivalent_cost_nanos), 0),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                    SUM(CASE WHEN e.source = 'claude' AND e.inference_geo = 'unknown'
                             AND e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                    CASE WHEN COUNT(DISTINCT e.pricing_fingerprint) = 1
                         THEN MAX(e.pricing_fingerprint) END
               FROM conversations c
               JOIN usage_entries e ON e.conversation_key = c.conversation_key
              WHERE (?1 IS NULL OR e.occurred_at >= ?1)
                AND (?2 IS NULL OR e.occurred_at < ?2)
                AND (?3 IS NULL OR e.source = ?3)
                AND (?4 IS NULL OR e.model = ?4)
                AND (?5 IS NULL OR e.speed = ?5)
                AND (?6 IS NULL
                     OR COALESCE(c.title, '') LIKE '%' || ?6 || '%' ESCAPE '\'
                     OR COALESCE(c.project_hint, '') LIKE '%' || ?6 || '%' ESCAPE '\')
              GROUP BY c.conversation_key
              ORDER BY c.last_at DESC, c.conversation_key ASC
              LIMIT ?7 OFFSET ?8",
        )?;
        let mapped = statement.query_map(
            params![
                filter.from.as_deref(),
                filter.to.as_deref(),
                source,
                filter.model.as_deref(),
                speed,
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

    pub fn conversation(
        &self,
        conversation_key: &str,
    ) -> Result<Option<UsageConversation>, UsageDbError> {
        let connection = self.open_read()?;
        connection
            .query_row(
                "SELECT c.conversation_key, c.source, c.title, c.project_hint,
                        c.is_sidechain, c.first_at, c.last_at, COUNT(e.id),
                        COALESCE(SUM(e.uncached_input_tokens), 0),
                        COALESCE(SUM(e.output_tokens), 0),
                        COALESCE(SUM(e.reasoning_output_tokens), 0),
                        COALESCE(SUM(e.cache_read_input_tokens), 0),
                        COALESCE(SUM(e.cache_write_5m_input_tokens), 0),
                        COALESCE(SUM(e.cache_write_1h_input_tokens), 0),
                        COALESCE(SUM(e.api_equivalent_cost_nanos), 0),
                        SUM(CASE WHEN e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN e.api_equivalent_cost_nanos IS NULL THEN 1 ELSE 0 END),
                        SUM(CASE WHEN e.source = 'claude' AND e.inference_geo = 'unknown'
                                 AND e.api_equivalent_cost_nanos IS NOT NULL THEN 1 ELSE 0 END),
                        CASE WHEN COUNT(DISTINCT e.pricing_fingerprint) = 1
                             THEN MAX(e.pricing_fingerprint) END
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
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write()?;
        let transaction = connection.transaction()?;
        let entries = {
            let mut statement = transaction.prepare(
                "SELECT id, source, model, speed, inference_geo, occurred_at,
                        uncached_input_tokens, output_tokens, reasoning_output_tokens,
                        cache_read_input_tokens, cache_write_5m_input_tokens,
                        cache_write_1h_input_tokens
                   FROM usage_entries",
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
                        pricing_fingerprint = ?2
                  WHERE id = ?3
                    AND (api_equivalent_cost_nanos IS NOT ?1
                         OR pricing_fingerprint IS NOT ?2)",
            )?;
            for entry in &entries {
                let estimate = catalog.estimate_row(entry);
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
                    fingerprint,
                    entry.id
                ])?)
                .unwrap_or(0);
            }
        }
        transaction.commit()?;

        Ok(UsageRepriceResult {
            updated_entries: updated,
            priced_entries: priced,
            unpriced_entries: unpriced,
            pricing_fingerprint: catalog.fingerprint().to_owned(),
        })
    }

    pub fn record_quota_snapshot(
        &self,
        provider: ProviderId,
        identity_key: &str,
        snapshot: &QuotaSnapshot,
    ) -> Result<(), UsageDbError> {
        let _guard = self.write_lock.lock().expect("usage db write lock");
        let mut connection = self.open_write()?;
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
                   remaining_percent, observed_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    provider_db(provider),
                    identity_key,
                    kind,
                    window.id,
                    remaining,
                    snapshot.captured_at,
                ],
            )?;
        }
        transaction.commit()?;
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

    fn open_write(&self) -> Result<Connection, UsageDbError> {
        fs::create_dir_all(&self.directory)?;
        let path = self.path();
        let existed = path.exists();
        let mut connection = Connection::open(&path)?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;

        if existed {
            let check = connection
                .query_row("PRAGMA quick_check", [], |row| row.get::<_, String>(0))
                .unwrap_or_else(|_| "failed".to_owned());
            if check != "ok" {
                drop(connection);
                self.recover_corrupt_database()?;
                connection = Connection::open(&path)?;
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
        Ok(connection)
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
                       remaining_percent, observed_at
                     ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        event.provider,
                        event.identity_key,
                        event.window_kind,
                        event.window_id,
                        event.remaining_percent,
                        event.observed_at,
                    ],
                )?;
            }
            transaction.commit()?;
        }
        Ok(())
    }
}

fn migrate(connection: &mut Connection, from: i64) -> Result<(), UsageDbError> {
    if from != 0 {
        return Err(UsageDbError::UnsupportedSchema);
    }
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE scan_files (
           file_key TEXT PRIMARY KEY,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude')),
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
         CREATE TABLE conversations (
           conversation_key TEXT PRIMARY KEY,
           source TEXT NOT NULL CHECK(source IN ('codex', 'claude')),
           title TEXT,
           project_hint TEXT,
           is_sidechain INTEGER NOT NULL DEFAULT 0 CHECK(is_sidechain IN (0, 1)),
           first_at TEXT NOT NULL,
           last_at TEXT NOT NULL
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
           observed_at TEXT NOT NULL
         );
         CREATE INDEX ix_quota_series
           ON quota_events(provider, identity_key, window_kind, window_id, observed_at);
         PRAGMA user_version = 1;",
    )?;
    transaction.commit()?;
    Ok(())
}

fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageSummaryRow> {
    let tokens = token_totals(row, 2)?;
    Ok(UsageSummaryRow {
        key: row.get(0)?,
        entry_count: row.get(1)?,
        tokens,
        cost: UsageCostTotals {
            api_equivalent_cost_nanos: row.get(8)?,
            priced_entries: row.get(9)?,
            unpriced_entries: row.get(10)?,
            assumed_geo_entries: row.get(11)?,
            pricing_fingerprint: row.get(12)?,
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

fn conversation_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<UsageConversation> {
    let source_value: String = row.get(1)?;
    let source = source_from_db(&source_value)?;
    Ok(UsageConversation {
        conversation_key: row.get(0)?,
        source,
        title: row.get(2)?,
        project_hint: row.get(3)?,
        is_sidechain: row.get::<_, i64>(4)? != 0,
        first_at: row.get(5)?,
        last_at: row.get(6)?,
        entry_count: row.get(7)?,
        tokens: token_totals(row, 8)?,
        cost: UsageCostTotals {
            api_equivalent_cost_nanos: row.get(14)?,
            priced_entries: row.get(15)?,
            unpriced_entries: row.get(16)?,
            assumed_geo_entries: row.get(17)?,
            pricing_fingerprint: row.get(18)?,
        },
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

fn provider_db(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => "codex",
        ProviderId::Claude => "claude",
    }
}

fn quota_kind(kind: QuotaWindowKind) -> &'static str {
    match kind {
        QuotaWindowKind::FiveHour => "five_hour",
        QuotaWindowKind::Weekly => "weekly",
        QuotaWindowKind::ModelWeekly => "model_weekly",
        QuotaWindowKind::Unknown => "unknown",
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
                remaining_percent, observed_at
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{QuotaWindow, UsageFilter};
    use crate::usage::model::{ConversationFact, InferenceGeo, ScanBatch, TokenFacts, UsageEntry};
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
            pricing_fingerprint: Some("price".to_owned()),
        };
        ScanBatch {
            entries: vec![entry],
            conversations: vec![ConversationFact {
                conversation_key: "conversation".to_owned(),
                source: UsageSource::Codex,
                title: None,
                project_hint: None,
                is_sidechain: false,
                occurred_at: "2026-07-30T00:00:00Z".to_owned(),
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
                    "pricing_fingerprint",
                ][..],
            ),
            (
                "conversations",
                &[
                    "conversation_key",
                    "source",
                    "title",
                    "project_hint",
                    "is_sidechain",
                    "first_at",
                    "last_at",
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
                resets_at: None,
                window_seconds: Some(18_000),
                is_active: true,
                is_primary: true,
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
        snapshot.captured_at = "2026-07-30T00:10:00Z".to_owned();
        database
            .record_quota_snapshot(ProviderId::Codex, "identity", &snapshot)
            .expect("changed");

        let connection = database.open_read().expect("read");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM quota_events", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 2);
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
}
