//! OpenCode 本地 SQLite 会话库的只读增量扫描。
//!
//! 协议按 `docs/OpenCode数据源.md`：只读打开 `~/.local/share/opencode/opencode.db`，
//! `message.time_created` 做增量水位、`message.id` 去重（由 `usage_entries` 唯一索引兜底），
//! 官方 `cost` 为费用总额真值、不走价格表。库缺失、打开失败或表结构不符一律 no-op，
//! 返回原状态不报错。

use std::collections::{HashMap, HashSet};
use std::path::Path;

use chrono::{DateTime, Local, SecondsFormat};
use rusqlite::{Connection, OpenFlags};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::contracts::{UsageSource, UsageSpeed};
use crate::storage::{UsageDb, UsageDbError};

use super::model::{ConversationFact, OpencodeScanState, ScanBatch, TokenFacts, UsageEntry};
use super::parser::project_hint;

const SEEN_CAP: usize = 20_000;
const OPENDCODE_FILE_KEY: &str = "opencode-sqlite-v1";

/// 一次 OpenCode 扫描的结果。扫描器内部吞掉库级错误（no-op），只报 commit 失败。
pub struct OpencodeScanOutcome {
    pub inserted: u64,
    pub duplicates: u64,
}

pub fn scan_opencode(db: &UsageDb, db_path: &Path) -> Result<OpencodeScanOutcome, UsageDbError> {
    let state = db.opencode_state()?;
    let mut seen: HashSet<String> = state.seen_ids.iter().cloned().collect();
    let mut seen_order: Vec<String> = state.seen_ids;

    let Ok(connection) = Connection::open_with_flags(
        db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Ok(OpencodeScanOutcome {
            inserted: 0,
            duplicates: 0,
        });
    };
    let Ok(mut statement) = connection.prepare(
        "SELECT m.id, m.session_id, m.time_created, m.data,
                COALESCE(s.title, ''), COALESCE(s.directory, '')
           FROM message m
           JOIN session s ON s.id = m.session_id
          WHERE m.time_created >= ?1
          ORDER BY m.time_created, m.id",
    ) else {
        return Ok(OpencodeScanOutcome {
            inserted: 0,
            duplicates: 0,
        });
    };
    let Ok(rows) = statement.query_map([state.watermark_ms], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    }) else {
        return Ok(OpencodeScanOutcome {
            inserted: 0,
            duplicates: 0,
        });
    };

    let part_titles = load_part_titles(&connection);

    let mut batch = ScanBatch::default();
    let mut watermark_ms = state.watermark_ms;
    // 会话内最近一条 user 消息继承的模型标签。
    let mut inherited_model: HashMap<String, Option<String>> = HashMap::new();

    for row in rows.flatten() {
        let (id, session_id, time_created, data_json, session_title, directory) = row;
        watermark_ms = watermark_ms.max(time_created);
        if seen.contains(&id) {
            continue;
        }
        seen.insert(id.clone());
        seen_order.push(id.clone());

        let Ok(data) = serde_json::from_str::<Value>(&data_json) else {
            continue;
        };
        let role = data.get("role").and_then(Value::as_str);
        match role {
            Some("user") => {
                inherited_model.insert(
                    session_id.clone(),
                    opencode_model_label(data.get("model"))
                        .or_else(|| inherited_model.get(&session_id).cloned().flatten()),
                );
            }
            Some("assistant") => {
                let model = top_level_model_label(&data)
                    .or_else(|| inherited_model.get(&session_id).cloned().flatten());
                let Some(tokens) = opencode_tokens(data.get("tokens")) else {
                    continue;
                };
                let cost_nanos = data.get("cost").and_then(decimal_nanos);
                if tokens.total_tokens() <= 0 && cost_nanos.is_none_or(|value| value <= 0) {
                    continue;
                }
                let Some((occurred_at, day_local)) = millis_time(time_created) else {
                    continue;
                };
                let conversation_key = opaque_key("opencode-conversation", &session_id);
                let title = if session_title.trim().is_empty() {
                    part_titles.get(&session_id).cloned().flatten()
                } else {
                    Some(session_title.trim().to_owned())
                };
                let project = project_hint(&directory);

                batch.entries.push(UsageEntry {
                    source: UsageSource::Opencode,
                    dedup_key: hash_parts(&["opencode-entry", &id]),
                    conversation_key: conversation_key.clone(),
                    model,
                    speed: UsageSpeed::Standard,
                    inference_geo: super::model::InferenceGeo::Global,
                    occurred_at: occurred_at.clone(),
                    day_local,
                    tokens,
                    api_equivalent_cost_nanos: cost_nanos,
                    billing_equivalent_tokens_nanos: None,
                    fast_multiplier_nanos: None,
                    pricing_fingerprint: None,
                });
                batch.conversations.push(ConversationFact {
                    conversation_key,
                    source: UsageSource::Opencode,
                    title,
                    project_hint: project,
                    is_sidechain: false,
                    occurred_at,
                });
            }
            _ => {}
        }
    }

    let mut inserted = 0_u64;
    let mut duplicates = 0_u64;
    if !batch.entries.is_empty() {
        let result = db.commit_scan_batch(
            OPENDCODE_FILE_KEY,
            UsageSource::Opencode,
            1,
            1,
            0,
            "opencode",
            Some("{}"),
            false,
            &batch,
        )?;
        inserted = result.inserted;
        duplicates = result.duplicates;
    }

    // seen 集合按最近插入顺序保留上限。
    while seen_order.len() > SEEN_CAP {
        let removed = seen_order.remove(0);
        seen.remove(&removed);
    }
    db.save_opencode_state(&OpencodeScanState {
        watermark_ms,
        seen_ids: seen_order,
    })?;

    Ok(OpencodeScanOutcome {
        inserted,
        duplicates,
    })
}

/// 空标题会话的标题兜底：每个会话 `(time_created, id)` 最早的一条 part，`type == "text"`
/// 时取 `text` 字段（非 text 即跳过，不继续找下一条）。
fn load_part_titles(connection: &Connection) -> HashMap<String, Option<String>> {
    let mut titles = HashMap::new();
    let Ok(mut statement) = connection.prepare(
        "SELECT session_id, data
           FROM (
             SELECT session_id, data,
                    ROW_NUMBER() OVER (
                      PARTITION BY session_id
                      ORDER BY time_created, id
                    ) AS row_num
               FROM part
           )
          WHERE row_num = 1",
    ) else {
        return titles;
    };
    let Ok(rows) = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    }) else {
        return titles;
    };
    for row in rows.flatten() {
        let Ok(data) = serde_json::from_str::<Value>(&row.1) else {
            continue;
        };
        if data.get("type").and_then(Value::as_str) != Some("text") {
            continue;
        }
        let text = data
            .get("text")
            .and_then(Value::as_str)
            .map(|value| {
                let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
                let without_prefix = collapsed.strip_prefix('<').unwrap_or(&collapsed).trim();
                without_prefix.chars().take(80).collect::<String>()
            })
            .filter(|value| !value.is_empty());
        titles.entry(row.0).or_insert(text);
    }
    titles
}

/// assistant 消息顶层 `providerID`／`modelID` 组成标签（variant 不拼入）。
fn top_level_model_label(data: &Value) -> Option<String> {
    let provider = data
        .get("providerID")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model = data
        .get("modelID")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (provider, model) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (Some(provider), None) => Some(provider.to_owned()),
        (None, Some(model)) => Some(model.to_owned()),
        (None, None) => None,
    }
}

/// 模型标签 `providerID/modelID`（variant 不拼入）；任一侧缺失则只有存在的一侧；都缺失返回 None。
fn opencode_model_label(value: Option<&Value>) -> Option<String> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let provider = value
        .get("providerID")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model = value
        .get("modelID")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    match (provider, model) {
        (Some(provider), Some(model)) => Some(format!("{provider}/{model}")),
        (Some(provider), None) => Some(provider.to_owned()),
        (None, Some(model)) => Some(model.to_owned()),
        (None, None) => None,
    }
}

/// OpenCode `tokens` → 六维事实：reasoning 已包含在 output 中，只取 output（与 cc-bar 口径一致）。
fn opencode_tokens(tokens: Option<&Value>) -> Option<TokenFacts> {
    let tokens = tokens?;
    let input = non_negative(tokens.get("input"))?;
    let output = non_negative(tokens.get("output"))?;
    let cache_read = tokens
        .get("cache")
        .and_then(|cache| non_negative(cache.get("read")).or(Some(0)))
        .unwrap_or(0);
    let cache_write = tokens
        .get("cache")
        .and_then(|cache| non_negative(cache.get("write")).or(Some(0)))
        .unwrap_or(0);
    let facts = TokenFacts {
        uncached_input_tokens: input,
        output_tokens: output,
        reasoning_output_tokens: 0,
        cache_read_input_tokens: cache_read,
        cache_write_5m_input_tokens: cache_write,
        cache_write_1h_input_tokens: 0,
    };
    facts.is_valid().then_some(facts)
}

fn non_negative(value: Option<&Value>) -> Option<i64> {
    value?.as_u64().and_then(|value| i64::try_from(value).ok())
}

/// 美元小数 → 整数 USD nanos；接受 JSON number 与 string 两种形态。
fn decimal_nanos(value: &Value) -> Option<i64> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_f64().map(|value| value.to_string()))?;
    let parsed: f64 = text.parse().ok()?;
    Some((parsed * 1_000_000_000.0).round() as i64)
}

fn millis_time(ms: i64) -> Option<(String, String)> {
    let utc = DateTime::from_timestamp_millis(ms)?;
    let local = utc.with_timezone(&Local);
    Some((
        utc.to_rfc3339_opts(SecondsFormat::Millis, true),
        local.format("%Y-%m-%d").to_string(),
    ))
}

fn opaque_key(namespace: &str, value: &str) -> String {
    hash_parts(&[namespace, value])
}

fn hash_parts(parts: &[&str]) -> String {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::storage::UsageDb;

    fn seed_opencode_db(path: &Path) {
        let connection = Connection::open(path).expect("open opencode db");
        connection
            .execute_batch(
                "CREATE TABLE session (
                   id TEXT PRIMARY KEY, title TEXT, directory TEXT, workspace_id TEXT
                 );
                 CREATE TABLE workspace (
                   id TEXT PRIMARY KEY, branch TEXT
                 );
                 CREATE TABLE message (
                   id TEXT PRIMARY KEY, session_id TEXT, time_created INTEGER, data TEXT
                 );
                 CREATE TABLE part (
                   session_id TEXT, time_created INTEGER, id TEXT, data TEXT
                 );",
            )
            .expect("create schema");
        connection
            .execute(
                "INSERT INTO session (id, title, directory, workspace_id) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "sess-a",
                    "Refactor parser",
                    "/Users/nanvon/Code/cc-trace",
                    "ws-1"
                ],
            )
            .expect("insert session a");
        connection
            .execute(
                "INSERT INTO session (id, title, directory, workspace_id) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params!["sess-b", "", "/tmp/other", Option::<String>::None],
            )
            .expect("insert session b");
        connection
            .execute(
                "INSERT INTO workspace (id, branch) VALUES ('ws-1', 'main')",
                [],
            )
            .expect("insert workspace");
        // session a: user + assistant
        connection
            .execute(
                "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "m1",
                    "sess-a",
                    1750000000000i64,
                    r#"{"role":"user","model":{"providerID":"opencode-go","modelID":"deepseek-v4-flash","variant":"default"}}"#
                ],
            )
            .expect("insert user");
        connection
            .execute(
                "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "m2",
                    "sess-a",
                    1750000001000i64,
                    r#"{"role":"assistant","providerID":"opencode-go","modelID":"deepseek-v4-flash","tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":10,"write":2},"total":132},"cost":"0.000456"}"#
                ],
            )
            .expect("insert assistant");
        // session b: assistant without own model (inherit user fallback missing) + zero-cost skip
        connection
            .execute(
                "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "m3",
                    "sess-b",
                    1750000002000i64,
                    r#"{"role":"assistant","tokens":{"input":0,"output":0,"cache":{"read":0,"write":0},"total":0},"cost":"0"}"#
                ],
            )
            .expect("insert zero-cost");
        connection
            .execute(
                "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "m4",
                    "sess-b",
                    1750000003000i64,
                    r#"{"role":"assistant","providerID":"deepseek","modelID":"deepseek-v4-flash","tokens":{"input":4,"output":2,"cache":{"read":0,"write":0},"total":6},"cost":0.000001}"#
                ],
            )
            .expect("insert assistant b");
        // part fallback title for session b
        connection
            .execute(
                "INSERT INTO part (session_id, time_created, id, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    "sess-b",
                    1750000000500i64,
                    "p1",
                    r#"{"type":"text","text":"<plan>Refactor the loader"}"#
                ],
            )
            .expect("insert part");
        connection.close().expect("close");
    }

    #[test]
    fn opencode_scan_counts_official_cost_and_dedups_by_message_id() {
        let config = tempfile::tempdir().expect("config");
        let db = UsageDb::new(config.path().to_path_buf());
        let db_path = config.path().join("opencode.db");
        seed_opencode_db(&db_path);

        let first = scan_opencode(&db, &db_path).expect("first scan");
        // m2 与 m4 计入；m3 零费用跳过。
        assert_eq!(first.inserted, 2);
        assert_eq!(first.duplicates, 0);

        let summary = db
            .summary(&crate::contracts::UsageSummaryQuery {
                filter: crate::contracts::UsageFilter::default(),
                group_by: crate::contracts::UsageGroupBy::Source,
            })
            .expect("summary");
        let row = summary
            .rows
            .iter()
            .find(|row| row.key == "opencode")
            .expect("row");
        assert_eq!(row.entry_count, 2);
        // m2: (100 + 10 + 2) + 20 = 132; m4: 4 + 2 = 6
        assert_eq!(row.tokens.total_tokens, 138);
        assert_eq!(row.tokens.reasoning_output_tokens, 0);
        assert_eq!(row.tokens.cache_read_input_tokens, 10);
        assert_eq!(row.tokens.cache_write_5m_input_tokens, 2);
        // 0.000456 * 1e9 + 0.000001 * 1e9
        assert_eq!(row.cost.api_equivalent_cost_nanos, 457_000);

        // 二次扫描：水位推进后不重复；同一水位窗口内 re-scan 由 seen 集合去重。
        let second = scan_opencode(&db, &db_path).expect("second scan");
        assert_eq!(second.inserted, 0);

        let conversations = db
            .conversations(
                &crate::contracts::UsageConversationQuery::default(),
                50,
                0,
                None,
                None,
            )
            .expect("conversations");
        assert_eq!(conversations.total, 2);
        let sess_a = conversations
            .items
            .iter()
            .find(|item| item.title.as_deref() == Some("Refactor parser"))
            .expect("sess a");
        assert_eq!(sess_a.project_hint.as_deref(), Some("cc-trace"));
        let sess_b = conversations
            .items
            .iter()
            .find(|item| item.project_hint.as_deref() == Some("other"))
            .expect("sess b");
        assert_eq!(sess_b.title.as_deref(), Some("plan>Refactor the loader"));
        assert_eq!(sess_b.entry_count, 1);
    }

    #[test]
    fn opencode_missing_db_is_a_noop() {
        let config = tempfile::tempdir().expect("config");
        let db = UsageDb::new(config.path().to_path_buf());
        let outcome =
            scan_opencode(&db, Path::new("/definitely/missing/opencode.db")).expect("noop");
        assert_eq!(outcome.inserted, 0);
        let state = db.opencode_state().expect("state");
        assert_eq!(state.watermark_ms, 0);
    }
}
