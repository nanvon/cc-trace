//! DSH（DeepSeek Harness）会话日志扫描。
//!
//! 日志根默认 `~/.dsh/sessions`，目录结构是 `<项目段>/<会话段>/<日志文件>`；
//! 每个会话目录里可能有多个版本的日志，只取版本号最高的一个：
//! `session.jsonl`（v0）、`session.vN.jsonl`（N 为正整数），以及各自的 `.zstd` 变体
//! （多帧 zstd 容器，见 [`super::dsh_zstd`]）。同版本两种编码并存属于异常，
//! 固定取 zstd 并把次数记进扫描结果。
//!
//! 一个会话可能在多轮扫描间**追加**写入，因此水位按「最后一个完整帧（zstd）／
//! 最后一个换行（明文）」推进；尾部半截数据留到下一轮，从水位重扫，不重复入账，
//! 也不丢数据。文件被截断或前缀指纹变化时整份重扫。
//!
//! 会话内记录的语义（槽位替换、种子边界、标题覆盖、路由模型）逐条对齐 cc-bar
//! v1.1.1 的 `DshSessionScanner`：
//!
//! - `session` 给会话 id、cwd 与父会话，以及种子长度；
//! - `session/end-seed` 的 `inherited` 标记是**本会话**的种子边界，清掉此前条目；
//! - `request/context` 与 `request/header` 决定后续条目的模型（`provider/model`）；
//! - `assistant/message` / `assistant/attempt` 是唯一产出用量的记录；
//!   同一 `(turn, step)` 的连续采样是「替换」而不是「累加」，`llm/retry-started`
//!   关闭该槽位，之后的采样重新追加；
//! - `session/title` 最后一个非空标题胜出。
//!
//! 子代理归并（子会话计入根会话）不在本模块做：这里只产出「文件 → 条目」与
//! 父会话链接，归根由扫描编排层按父链解析后统一重挂（`usage::dsh_roots`）。

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use chrono::{Local, SecondsFormat, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::dsh_zstd::{self, DEFAULT_OUTPUT_LIMIT};
use super::model::{Granularity, TokenFacts, UsageEntry};
use super::parser::project_identity;
use crate::contracts::{UsageSource, UsageSpeed};

/// zstd 单次读盘块大小：一兆足够吞下常见帧，又不会让坏文件的首次读取撑爆内存。
const CHUNK_SIZE: usize = 1 << 20;
/// 单个明文日志单轮读取上限。超出则本轮只处理前一段，水位照样推进，下一轮继续。
const PLAIN_READ_LIMIT: u64 = 64 * 1024 * 1024;

/// 一个被选中的日志文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshLog {
    pub path: PathBuf,
    /// 跨轮稳定的文件标识（相对日志根的路径派生），用作 `scan_files.file_key`。
    pub file_key: String,
    pub is_zstd: bool,
    pub size: u64,
    pub mtime_ms: i64,
    /// 同版本两种编码并存（异常）。
    pub duplicate_encoding: bool,
}

/// 日志挑选结果。`failed_directories` 是枚举失败或文件身份读不出来的目录数：
/// 「本轮没扫到」不等于「文件消失了」，调用方不能据此删除历史。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DshLogSelection {
    pub logs: Vec<DshLog>,
    pub failed_directories: usize,
}

/// 日志文件名解析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DshLogName {
    pub version: u32,
    pub is_zstd: bool,
}

/// 解析规范日志名。非规范命名（含锁文件、临时文件、大写、前导零）一律返回 `None`。
pub fn canonical_log_name(file_name: &str) -> Option<DshLogName> {
    let (stem, is_zstd) = match file_name.strip_suffix(".zstd") {
        Some(stem) => (stem, true),
        None => (file_name, false),
    };
    let stem = stem.strip_suffix(".jsonl")?;

    if stem == "session" {
        return Some(DshLogName {
            version: 0,
            is_zstd,
        });
    }
    let version = stem.strip_prefix("session.v")?;
    // 空、前导零、非数字都排除：`session.v01.jsonl` 与 `session.v1.jsonl` 是同一个版本，
    // 认成两个会让「取版本最高」失去意义。
    if version.is_empty()
        || version.starts_with('0')
        || !version.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let version: u32 = version.parse().ok()?;
    Some(DshLogName { version, is_zstd })
}

/// 枚举日志根并挑出每个会话目录的规范日志。
///
/// 目录读取失败按 `failed_directories` 计数并继续：一个项目段读不出来不该让整轮失败。
pub fn select_logs(root: &Path, had_previous_logs: bool) -> DshLogSelection {
    let mut selection = DshLogSelection::default();

    let Ok(projects) = std::fs::read_dir(root) else {
        // 默认根从未创建是正常的；存在但读不出来才算失败。
        if had_previous_logs || root.exists() {
            selection.failed_directories += 1;
        }
        return selection;
    };

    for project in projects.flatten() {
        let project_path = project.path();
        if !is_directory(&project_path) {
            continue;
        }
        let Ok(sessions) = std::fs::read_dir(&project_path) else {
            selection.failed_directories += 1;
            continue;
        };
        for session in sessions.flatten() {
            let session_path = session.path();
            if !is_directory(&session_path) {
                continue;
            }
            let Ok(files) = std::fs::read_dir(&session_path) else {
                selection.failed_directories += 1;
                continue;
            };
            if let Some(log) =
                select_log_in(&session_path, files.flatten().map(|entry| entry.path()))
            {
                selection.logs.push(log);
            }
        }
    }

    selection
        .logs
        .sort_by(|left, right| left.path.cmp(&right.path));
    selection
}

fn is_directory(path: &Path) -> bool {
    std::fs::metadata(path)
        .map(|metadata| metadata.is_dir())
        .unwrap_or(false)
}

/// 一个会话目录里挑日志：版本最高者胜；同版本优先 zstd 并记异常。
fn select_log_in(session_path: &Path, paths: impl Iterator<Item = PathBuf>) -> Option<DshLog> {
    // 用 BTreeMap 排序，保证同一目录两次挑出的结果一致（与文件系统的枚举顺序无关）。
    let mut candidates: BTreeMap<(u32, bool), PathBuf> = BTreeMap::new();
    let mut versions: BTreeMap<u32, usize> = BTreeMap::new();

    for path in paths {
        let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let Some(name) = canonical_log_name(file_name) else {
            continue;
        };
        *versions.entry(name.version).or_default() += 1;
        candidates.insert((name.version, name.is_zstd), path);
    }

    let (&(version, is_zstd), path) = candidates.iter().next_back()?;
    let duplicate_encoding = versions.get(&version).copied().unwrap_or(0) > 1;

    let metadata = std::fs::metadata(path).ok()?;
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0);
    let relative = path.strip_prefix(session_path).unwrap_or(path);

    Some(DshLog {
        path: path.clone(),
        file_key: format!("dsh:{}:{}", version, relative.to_string_lossy()),
        is_zstd,
        size: metadata.len(),
        mtime_ms,
        duplicate_encoding,
    })
}

/// 跨轮持久化的会话扫描状态。存在 `scan_files.cursor_json` 里。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DshScanState {
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    pub parent_session: Option<String>,
    pub title: Option<String>,
    pub seed_cut: Option<i64>,
    /// 缺省表示「还没写过」，按 [`Self::resolved_is_seeded`] 推导。
    pub is_seeded: Option<bool>,
    pub seed_complete: Option<bool>,
    pub route_provider: Option<String>,
    pub route_model: Option<String>,
    pub last_turn: Option<i64>,
    pub last_step: Option<i64>,
    pub last_slot_open: Option<bool>,
    /// 同一个 `(turn, step)` 的第几笔账。`llm/retry-started` 关闭槽位后，
    /// 下一次采样是新的一笔，必须与上一笔分开计数（两次调用的用量都算数）。
    pub slot_generation: Option<u32>,
}

impl DshScanState {
    /// 没有显式记录时按「有种子长度或种子未完成」推导，与 cc-bar 的判据一致。
    pub fn resolved_is_seeded(&self) -> bool {
        self.is_seeded
            .unwrap_or(!self.seed_complete.unwrap_or(true) || self.seed_cut.is_some())
    }

    pub fn resolved_seed_complete(&self) -> bool {
        self.seed_complete.unwrap_or(true)
    }
}

/// 本条日志这一轮的结局。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DshFileOutcome {
    Success,
    /// 读不了、损坏或结构不符：本轮结果作废，水位保持上一轮的值。
    Failed,
    /// 枚举之后文件消失了。
    Missing,
}

/// 一条待入账的用量条目（费用由价格层在提交时估算）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshEntryDraft {
    pub dedup_key: String,
    pub conversation_key: String,
    pub model: String,
    pub occurred_at: String,
    pub day_local: String,
    pub tokens: TokenFacts,
}

/// 会话信息，用于写 `conversations` 与子代理归根。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshConversationDraft {
    pub conversation_key: String,
    pub session_id: String,
    pub parent_session: Option<String>,
    pub title: Option<String>,
    pub project_hint: Option<String>,
    pub project_key: Option<String>,
    pub worktree_path: Option<String>,
    pub first_at: String,
    pub last_at: String,
}

/// 单文件扫描的完整产物。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshScanOutput {
    pub outcome: DshFileOutcome,
    pub state: DshScanState,
    /// 新的水位（字节）。失败时等于传入的旧水位。
    pub offset: u64,
    pub entries: Vec<DshEntryDraft>,
    pub conversation: Option<DshConversationDraft>,
    /// 因为槽位替换或嵌套种子而整份重扫过。
    pub restarted: bool,
    pub lines_parsed: u64,
    pub total_tokens_mismatches: u64,
}

/// 扫描一个日志文件。
///
/// `previous_offset` 是本轮起点；`reset` 为真（文件变小或前缀指纹变化）时从 0 重扫。
/// 单个文件最多内部重扫两次：一次用于「槽位替换需要整份重读」，一次不会无限递归。
pub fn scan_log(
    log: &DshLog,
    previous: Option<&DshScanState>,
    previous_offset: u64,
    reset: bool,
) -> DshScanOutput {
    let offset = if reset {
        0
    } else {
        previous_offset.min(log.size)
    };
    let mut output = scan_once(log, previous, offset);
    if !output.restarted {
        return output;
    }

    // 槽位替换发生在上一轮已入账的 (turn, step) 上：整份重扫，让替换后的读数成为唯一事实。
    let mut retry = scan_once(log, None, 0);
    if retry.outcome == DshFileOutcome::Success {
        retry.restarted = true;
        return retry;
    }
    // 重扫失败时退回第一次的结果：它至少与本轮新字节一致。
    output.restarted = false;
    output
}

fn scan_once(log: &DshLog, previous: Option<&DshScanState>, offset: u64) -> DshScanOutput {
    let mut scan = FileScan::from_state(previous, offset > 0);

    let mut lines_parsed = 0_u64;
    let mut mismatches = 0_u64;

    let read = if log.is_zstd {
        read_zstd(log, offset, &mut scan, &mut lines_parsed, &mut mismatches)
    } else {
        read_plain(log, offset, &mut scan, &mut lines_parsed, &mut mismatches)
    };

    let (outcome, new_offset) = match read {
        Ok(new_offset) => (DshFileOutcome::Success, new_offset),
        Err(ReadFailure::Missing) => (DshFileOutcome::Missing, offset),
        Err(ReadFailure::Failed) => (DshFileOutcome::Failed, offset),
    };

    // 会话 id 缺失时条目没有稳定会话键，这份文件本轮不能用。
    // 种子边界的问题在逐行处理里已经处理过（种子未收口就不产条目）。
    let usable = outcome == DshFileOutcome::Success && scan.session_id.is_some();

    if !usable {
        return DshScanOutput {
            outcome: if outcome == DshFileOutcome::Success {
                DshFileOutcome::Failed
            } else {
                outcome
            },
            state: previous.cloned().unwrap_or_default(),
            offset,
            entries: Vec::new(),
            conversation: None,
            // 「需要整份重扫」不是失败：调用方据此从 0 重来一次。
            restarted: scan.restart_needed,
            lines_parsed,
            total_tokens_mismatches: 0,
        };
    }

    let conversation = scan.conversation_draft();
    DshScanOutput {
        outcome,
        state: scan.persisted_state(),
        offset: new_offset,
        entries: scan.entries,
        conversation,
        restarted: false,
        lines_parsed,
        total_tokens_mismatches: mismatches,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadFailure {
    Failed,
    Missing,
}

/// 明文日志：读到最后一个完整换行为止，最后一行是半截就留给下一轮。
fn read_plain(
    log: &DshLog,
    offset: u64,
    scan: &mut FileScan,
    lines_parsed: &mut u64,
    mismatches: &mut u64,
) -> Result<u64, ReadFailure> {
    let Ok(mut file) = std::fs::File::open(&log.path) else {
        return Err(if log.path.exists() {
            ReadFailure::Failed
        } else {
            ReadFailure::Missing
        });
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return Err(ReadFailure::Failed);
    }

    let mut buffer = Vec::new();
    // 一次最多读 PLAIN_READ_LIMIT 字节，剩下的下一轮继续。
    let read = file
        .take(PLAIN_READ_LIMIT)
        .read_to_end(&mut buffer)
        .map_err(|_| ReadFailure::Failed)?;
    let _ = read;

    let mut consumed = 0_usize;
    let mut line_start = 0_usize;
    while let Some(position) = buffer[line_start..].iter().position(|byte| *byte == b'\n') {
        let line_end = line_start + position;
        let line = &buffer[line_start..line_end];
        if !line.is_empty() {
            *lines_parsed += 1;
            let text = String::from_utf8_lossy(line);
            if !scan.ingest(
                &text,
                mismatches,
                offset + u64::try_from(consumed).unwrap_or(u64::MAX),
            ) {
                return Err(ReadFailure::Failed);
            }
        }
        consumed = line_end + 1;
        line_start = consumed;
    }

    Ok(offset + u64::try_from(consumed).unwrap_or(u64::MAX))
}

/// 多帧 zstd 日志：分块读，只解压完整帧，尾部未完成帧留在下一轮。
fn read_zstd(
    log: &DshLog,
    offset: u64,
    scan: &mut FileScan,
    lines_parsed: &mut u64,
    mismatches: &mut u64,
) -> Result<u64, ReadFailure> {
    let Ok(mut file) = std::fs::File::open(&log.path) else {
        return Err(if log.path.exists() {
            ReadFailure::Failed
        } else {
            ReadFailure::Missing
        });
    };
    if file.seek(SeekFrom::Start(offset)).is_err() {
        return Err(ReadFailure::Failed);
    }

    let mut pending: Vec<u8> = Vec::new();
    let mut consumed = 0_u64;
    let mut chunk = vec![0_u8; CHUNK_SIZE];

    loop {
        let read = file.read(&mut chunk).map_err(|_| ReadFailure::Failed)?;
        if read == 0 {
            break;
        }
        pending.extend_from_slice(&chunk[..read]);

        let outcome = dsh_zstd::scan(&pending, None);
        let result = match outcome {
            dsh_zstd::ScanOutcome::Corrupt(_) => return Err(ReadFailure::Failed),
            dsh_zstd::ScanOutcome::Scanned(result) => result,
        };

        for frame in &result.frames {
            let payload = &pending[frame.start..frame.end];
            let decoded = dsh_zstd::decode_frame(payload, DEFAULT_OUTPUT_LIMIT)
                .map_err(|_| ReadFailure::Failed)?;
            let text = String::from_utf8_lossy(&decoded);
            for line in text.split('\n') {
                if line.is_empty() {
                    continue;
                }
                *lines_parsed += 1;
                if !scan.ingest(line, mismatches, offset + consumed) {
                    return Err(ReadFailure::Failed);
                }
            }
        }

        consumed += u64::try_from(dsh_zstd::consumed_bytes(&dsh_zstd::ScanOutcome::Scanned(
            result.clone(),
        )))
        .unwrap_or(0);

        match result.torn_start {
            Some(torn_start) => {
                pending.drain(..torn_start);
            }
            None => pending.clear(),
        }
    }

    Ok(offset + consumed)
}

/// 单文件本轮的暂存状态；整份文件成功才提交。
#[derive(Default)]
struct FileScan {
    session_id: Option<String>,
    cwd: Option<String>,
    parent_session: Option<String>,
    title: Option<String>,
    is_seeded: bool,
    seed_cut: Option<i64>,
    seed_complete: bool,
    /// 本轮从既有水位继续，且上一轮已经过了种子边界。
    resumed_seed: bool,
    /// 需要整份重扫：替换目标落在上一轮已入账的条目上，或续写段里又出现嵌套种子标记。
    restart_needed: bool,
    /// 上一个采样的条目是不是**本轮**产出的。跨轮时无法在内存里替换，只能重扫。
    last_entry_in_round: bool,
    route_provider: Option<String>,
    route_model: Option<String>,
    last_turn: Option<i64>,
    last_step: Option<i64>,
    last_slot_open: bool,
    slot_generation: u32,
    first_at: Option<String>,
    last_at: Option<String>,
    entries: Vec<DshEntryDraft>,
}

impl FileScan {
    fn from_state(state: Option<&DshScanState>, resumed: bool) -> Self {
        let Some(state) = state else {
            return Self {
                seed_complete: true,
                ..Self::default()
            };
        };
        let is_seeded = state.resolved_is_seeded();
        let seed_complete = state.resolved_seed_complete();
        Self {
            session_id: state.session_id.clone(),
            cwd: state.cwd.clone(),
            parent_session: state.parent_session.clone(),
            title: state.title.clone(),
            is_seeded,
            seed_cut: state.seed_cut,
            seed_complete,
            // 从既有水位继续且种子已经收口：这一段是「接续」，不是种子本身。
            resumed_seed: resumed && is_seeded && seed_complete,
            restart_needed: false,
            last_entry_in_round: false,
            route_provider: state.route_provider.clone(),
            route_model: state.route_model.clone(),
            last_turn: state.last_turn,
            last_step: state.last_step,
            last_slot_open: state.last_slot_open.unwrap_or(false),
            slot_generation: state.slot_generation.unwrap_or(0),
            first_at: None,
            last_at: None,
            entries: Vec::new(),
        }
    }

    fn persisted_state(&self) -> DshScanState {
        DshScanState {
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            parent_session: self.parent_session.clone(),
            title: self.title.clone(),
            seed_cut: self.seed_cut,
            is_seeded: Some(self.is_seeded),
            seed_complete: Some(self.seed_complete),
            route_provider: self.route_provider.clone(),
            route_model: self.route_model.clone(),
            last_turn: self.last_turn,
            last_step: self.last_step,
            last_slot_open: Some(self.last_slot_open),
            slot_generation: Some(self.slot_generation),
        }
    }

    fn conversation_draft(&self) -> Option<DshConversationDraft> {
        let session_id = self.session_id.clone()?;
        let identity = self
            .cwd
            .as_deref()
            .map(project_identity)
            .unwrap_or_else(|| project_identity(""));
        Some(DshConversationDraft {
            conversation_key: format!("dsh:{session_id}"),
            session_id,
            parent_session: self.parent_session.clone(),
            title: self.title.clone(),
            project_hint: identity.hint,
            project_key: identity.path.clone(),
            worktree_path: identity.worktree,
            first_at: self
                .first_at
                .clone()
                .unwrap_or_else(|| self.last_at.clone().unwrap_or_default()),
            last_at: self
                .last_at
                .clone()
                .unwrap_or_else(|| self.first_at.clone().unwrap_or_default()),
        })
    }

    /// 处理一行记录。返回 `false` 表示整份文件本轮作废。
    fn ingest(&mut self, line: &str, mismatches: &mut u64, offset: u64) -> bool {
        let Ok(root) = serde_json::from_str::<Value>(line) else {
            return false;
        };
        if !root.is_object() {
            return false;
        }
        let Some(kind) = root.get("type").and_then(Value::as_str) else {
            return true;
        };

        // 种子切点：seq 越过 cut 之后才算种子收口。
        if kind != "session"
            && let (Some(cut), Some(seq)) = (self.seed_cut, integer(&root, "seq"))
            && seq >= cut
        {
            self.seed_complete = true;
        }

        match kind {
            "session" => {
                if let Some(id) = text(&root, "id") {
                    self.session_id = Some(id);
                }
                if let Some(cwd) = text(&root, "cwd") {
                    self.cwd = Some(cwd);
                }
                if let Some(parent) = text(&root, "parentSession") {
                    self.parent_session = Some(parent);
                }
                self.is_seeded = root.get("isSeeded").and_then(Value::as_bool) == Some(true)
                    || root.get("seedLength").is_some();
                if root.get("seedLength").is_some() {
                    let Some(cut) = integer(&root, "seedLength").filter(|value| *value >= 0) else {
                        return false;
                    };
                    self.seed_cut = Some(cut);
                } else {
                    self.seed_cut = None;
                }
                self.seed_complete = !self.is_seeded || self.seed_cut == Some(0);
                true
            }
            "session/end-seed" => {
                let inherited = root
                    .get("data")
                    .and_then(|data| data.get("inherited"))
                    .and_then(Value::as_bool)
                    == Some(true);
                if !inherited {
                    return true;
                }
                if !self.is_seeded {
                    return false;
                }
                if self.resumed_seed {
                    // 上一轮已经把这个标记算进种子了，不能再清一次。
                    self.restart_needed = true;
                    return false;
                }
                // 嵌套 fork 的种子中可能已有父会话的标记；最后一个标记才是本会话的 cut。
                self.entries.clear();
                self.title = None;
                self.last_turn = None;
                self.last_step = None;
                self.last_slot_open = false;
                self.last_entry_in_round = false;
                self.slot_generation = 0;
                self.first_at = None;
                self.last_at = None;
                self.seed_complete = true;
                true
            }
            "request/context" => {
                if let Some(data) = root.get("data") {
                    self.route_provider = data
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    self.route_model = data.get("model").and_then(Value::as_str).map(str::to_owned);
                }
                true
            }
            "request/header" => {
                if let Some(config) = root
                    .get("data")
                    .and_then(|data| data.get("header"))
                    .and_then(|header| header.get("config"))
                {
                    self.route_provider = config
                        .get("provider")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    self.route_model = config
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                true
            }
            "llm/retry-started" => {
                let turn = root.get("data").and_then(|data| integer(data, "turn"));
                let step = root.get("data").and_then(|data| integer(data, "step"));
                if self.last_turn == turn && self.last_step == step {
                    // 关闭当前槽位：同一 (turn, step) 的后续采样重新成为一笔新账。
                    self.last_slot_open = false;
                    self.last_entry_in_round = false;
                }
                true
            }
            "session/title" => {
                if !self.seed_complete {
                    return true;
                }
                let title = root
                    .get("data")
                    .and_then(|data| data.get("title"))
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_owned);
                if title.is_some() {
                    // 最后一个有效标题胜出；空标题不覆盖已有标题。
                    self.title = title;
                }
                true
            }
            "assistant/message" | "assistant/attempt" => {
                if !self.seed_complete {
                    return true;
                }
                self.ingest_assistant(kind, &root, mismatches, offset)
            }
            _ => true,
        }
    }

    fn ingest_assistant(
        &mut self,
        kind: &str,
        root: &Value,
        mismatches: &mut u64,
        offset: u64,
    ) -> bool {
        let Some(data) = root.get("data") else {
            return true;
        };
        let Some(usage) = token_usage(data) else {
            return true;
        };
        let Some(session_id) = self.session_id.clone() else {
            return true;
        };
        let Some(time) = number(root, "time").filter(|value| *value > 0.0) else {
            return true;
        };

        let turn = integer(data, "turn");
        let step = integer(data, "step");

        // 同一 (turn, step) 的连续采样是「替换」：槽位还开着且指向同一个结算点。
        let replaces_previous = self.last_slot_open
            && turn.is_some()
            && step.is_some()
            && self.last_turn == turn
            && self.last_step == step;
        if replaces_previous && !self.last_entry_in_round {
            // 要替换的那条是上一轮入账的，本轮内存里没有它：整份重扫才能让替换成立。
            // 只改新字节会同时留下旧读数与新读数，是两笔账。
            self.restart_needed = true;
            return false;
        }

        let occurred_at = Utc
            .timestamp_millis_opt(time as i64)
            .single()
            .unwrap_or_else(Utc::now);
        if let Some(reported) = integer(usage, "totalTokens") {
            let sum = int_value(usage, "inputTokens")
                + int_value(usage, "outputTokens")
                + int_value(usage, "cacheReadTokens")
                + int_value(usage, "cacheWriteTokens");
            if reported != sum {
                *mismatches += 1;
            }
        }

        let model = if kind == "assistant/message" {
            let source = data
                .get("message")
                .and_then(|message| message.get("source"));
            model_label(
                source
                    .and_then(|source| source.get("provider"))
                    .and_then(Value::as_str)
                    .or(self.route_provider.as_deref()),
                source
                    .and_then(|source| source.get("model"))
                    .and_then(Value::as_str)
                    .or(self.route_model.as_deref()),
            )
        } else {
            model_label(self.route_provider.as_deref(), self.route_model.as_deref())
        };

        let tokens = TokenFacts {
            uncached_input_tokens: int_value(usage, "inputTokens").max(0),
            output_tokens: int_value(usage, "outputTokens").max(0),
            // reasoningTokens 是 outputTokens 的子集，不重复计数。
            reasoning_output_tokens: 0,
            cache_read_input_tokens: int_value(usage, "cacheReadTokens").max(0),
            // DSH 不区分 5 分钟与 1 小时缓存写入：全部记在 5 分钟一档。
            cache_write_5m_input_tokens: int_value(usage, "cacheWriteTokens").max(0),
            cache_write_1h_input_tokens: 0,
        };

        let local = occurred_at.with_timezone(&Local);
        let day_local = local.format("%Y-%m-%d").to_string();
        // 代次：同一结算点的第几笔账。槽位还开着指向同一个结算点就是替换（代次不变），
        // 槽位被 `llm/retry-started` 关过就是新账（代次 +1）。
        let same_settlement =
            turn.is_some() && step.is_some() && self.last_turn == turn && self.last_step == step;
        if turn.is_some() && step.is_some() {
            if replaces_previous {
                // 代次保持不变。
            } else if same_settlement {
                self.slot_generation = self.slot_generation.saturating_add(1);
            } else {
                self.slot_generation = 0;
            }
        }

        let dedup_key = match (turn, step) {
            (Some(turn), Some(step)) => format!(
                "dsh-entry:{session_id}:{turn}:{step}:{}",
                self.slot_generation
            ),
            // 没有结算槽的记录用「会话 + 文件内偏移」做键：重扫同一内容得到同一个键，
            // 不会重复入账；文件被替换后重扫也会得到同一批键，由 ON CONFLICT 更新。
            _ => format!("dsh-entry:{session_id}:offset:{offset}"),
        };

        let entry = DshEntryDraft {
            dedup_key,
            conversation_key: format!("dsh:{session_id}"),
            model,
            occurred_at: occurred_at.to_rfc3339_opts(SecondsFormat::Secs, true),
            day_local,
            tokens,
        };

        self.last_turn = turn;
        self.last_step = step;
        self.last_slot_open = turn.is_some() && step.is_some();
        self.last_entry_in_round = !replaces_previous;
        self.first_at = Some(
            self.first_at
                .clone()
                .unwrap_or_else(|| entry.occurred_at.clone()),
        );
        self.last_at = Some(entry.occurred_at.clone());

        if replaces_previous {
            // 替换：旧读数不再存在，只有最后一次采样算数。
            if let Some(last) = self.entries.last_mut() {
                *last = entry;
            } else {
                self.entries.push(entry);
            }
        } else {
            self.entries.push(entry);
        }
        true
    }
}

/// 用量优先取 `data.usage`，缺失时取 `data.stream` 里最后一个 usage chunk；两处不可相加。
fn token_usage(data: &Value) -> Option<&Value> {
    if let Some(usage) = data.get("usage")
        && has_required_fields(usage)
    {
        return Some(usage);
    }
    let stream = data.get("stream")?.as_array()?;
    stream.iter().rev().find_map(|record| {
        let candidate = record
            .get("chunk")
            .and_then(|chunk| chunk.get("usage"))
            .or_else(|| record.get("usage"))?;
        has_required_fields(candidate).then_some(candidate)
    })
}

/// DSH 的 tokenUsageValue 要求 `inputTokens` / `outputTokens` 必填。
fn has_required_fields(usage: &Value) -> bool {
    usage.get("inputTokens").is_some() && usage.get("outputTokens").is_some()
}

/// 模型标签 `provider/model`；provider 缺失时只用模型名。
fn model_label(provider: Option<&str>, model: Option<&str>) -> String {
    let model = model
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("unknown");
    match provider.map(str::trim).filter(|value| !value.is_empty()) {
        Some(provider) => format!("{provider}/{model}"),
        None => model.to_owned(),
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    let raw = value.get(key)?;
    match raw {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|v| v as i64)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn number(value: &Value, key: &str) -> Option<f64> {
    let raw = value.get(key)?;
    match raw {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn int_value(value: &Value, key: &str) -> i64 {
    integer(value, key).unwrap_or(0)
}

/// 把待入账条目转成通用条目。费用留空：DSH 日志没有可当作账单的费用，
/// 由价格层按模型估算（与 Codex 同一套）。
pub fn into_usage_entry(draft: &DshEntryDraft) -> UsageEntry {
    UsageEntry {
        source: UsageSource::Dsh,
        dedup_key: draft.dedup_key.clone(),
        conversation_key: draft.conversation_key.clone(),
        model: Some(draft.model.clone()),
        speed: UsageSpeed::Standard,
        inference_geo: super::model::InferenceGeo::Unknown,
        occurred_at: draft.occurred_at.clone(),
        day_local: draft.day_local.clone(),
        tokens: draft.tokens,
        api_equivalent_cost_nanos: None,
        billing_equivalent_tokens_nanos: None,
        fast_multiplier_nanos: None,
        pricing_fingerprint: None,
        request_count: 1,
        granularity: Granularity::Request,
    }
}

/// 一个会话的归属信息，供 `dsh_sessions` 表与归根使用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DshSessionRow {
    pub session_id: String,
    pub parent_session: Option<String>,
    pub cwd: Option<String>,
    pub title: Option<String>,
    pub project_key: Option<String>,
}

impl DshSessionRow {
    pub fn from_conversation(draft: &DshConversationDraft) -> Self {
        Self {
            session_id: draft.session_id.clone(),
            parent_session: draft.parent_session.clone(),
            cwd: draft
                .worktree_path
                .clone()
                .or_else(|| draft.project_key.clone()),
            title: draft.title.clone(),
            project_key: draft.project_key.clone(),
        }
    }
}

/// 父链解析：`session_id → 根 session_id`。
///
/// 规则与 cc-bar 的归根一致：有父会话且父链可达根就挂在根上；父链成环、超出深度上限
/// 或父会话不存在时，把自己当根。**不做「大概是一家人」的猜测**，宁可少归一条。
pub fn resolve_roots(
    sessions: &[(String, Option<String>)],
    max_depth: usize,
) -> BTreeMap<String, String> {
    let parents: BTreeMap<&str, &str> = sessions
        .iter()
        .filter_map(|(id, parent)| parent.as_deref().map(|parent| (id.as_str(), parent)))
        .collect();
    let known: BTreeSet<&str> = sessions.iter().map(|(id, _)| id.as_str()).collect();

    let mut resolved = BTreeMap::new();
    for (session_id, _) in sessions {
        let mut cursor = session_id.as_str();
        let mut seen: Vec<&str> = vec![cursor];
        let mut root = cursor;

        while let Some(parent) = parents.get(cursor).copied() {
            // 父会话不在已知集合里：我们没看过那份日志，不能凭空造一条根会话，
            // 也不能把子会话挂到一个不存在的会话上——那样会得到一条没有标题与项目的空根。
            if !known.contains(parent) {
                break;
            }
            // 环或超深：把起点当根，不继续往上走。
            if seen.contains(&parent) || seen.len() > max_depth {
                root = session_id.as_str();
                break;
            }
            seen.push(parent);
            cursor = parent;
            root = parent;
        }

        resolved.insert(session_id.clone(), root.to_owned());
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn log_path(dir: &Path) -> PathBuf {
        dir.join("session.jsonl")
    }

    fn plain_log(dir: &Path, lines: &[Value]) -> DshLog {
        let path = log_path(dir);
        let mut file = std::fs::File::create(&path).expect("create");
        for line in lines {
            writeln!(file, "{line}").expect("write");
        }
        file.sync_all().expect("sync");
        DshLog {
            path,
            file_key: "dsh".to_owned(),
            is_zstd: false,
            size: std::fs::metadata(log_path(dir)).expect("meta").len(),
            mtime_ms: 0,
            duplicate_encoding: false,
        }
    }

    fn zstd_log(dir: &Path, frames: &[&str]) -> DshLog {
        let path = dir.join("session.jsonl.zstd");
        let mut bytes = Vec::new();
        for frame in frames {
            bytes.extend(zstd::encode_all(frame.as_bytes(), 3).expect("compress"));
        }
        std::fs::write(&path, &bytes).expect("write");
        DshLog {
            path,
            file_key: "dsh-zstd".to_owned(),
            is_zstd: true,
            size: bytes.len() as u64,
            mtime_ms: 0,
            duplicate_encoding: false,
        }
    }

    fn session_line(id: &str) -> Value {
        serde_json::json!({ "type": "session", "id": id, "cwd": "/work/demo" })
    }

    fn assistant_line(turn: i64, step: i64, input: i64, output: i64, time_ms: i64) -> Value {
        serde_json::json!({
            "type": "assistant/message",
            "time": time_ms,
            "data": {
                "turn": turn,
                "step": step,
                "usage": { "inputTokens": input, "outputTokens": output },
                "message": { "source": { "provider": "anthropic", "model": "claude-sonnet-4-5" } }
            }
        })
    }

    fn scan_plain(dir: &Path, lines: &[Value]) -> DshScanOutput {
        let log = plain_log(dir, lines);
        scan_log(&log, None, 0, false)
    }

    #[test]
    fn canonical_log_names_are_recognized_and_others_rejected() {
        assert_eq!(
            canonical_log_name("session.jsonl"),
            Some(DshLogName {
                version: 0,
                is_zstd: false
            })
        );
        assert_eq!(
            canonical_log_name("session.v12.jsonl.zstd"),
            Some(DshLogName {
                version: 12,
                is_zstd: true
            })
        );
        for rejected in [
            "session.v01.jsonl",
            "session.v.jsonl",
            "SESSION.jsonl",
            "session.jsonl.lock",
            "session.jsonl.tmp",
            "other.jsonl",
            "session.v1.json",
        ] {
            assert!(canonical_log_name(rejected).is_none(), "{rejected}");
        }
    }

    #[test]
    fn the_highest_version_wins_and_zstd_breaks_ties() {
        let dir = tempfile::tempdir().expect("temp dir");
        let session = dir.path().join("project").join("session");
        std::fs::create_dir_all(&session).expect("create dirs");
        for name in ["session.jsonl", "session.v2.jsonl", "session.v2.jsonl.zstd"] {
            std::fs::write(session.join(name), b"x").expect("write");
        }

        let selection = select_logs(dir.path(), false);
        assert_eq!(selection.logs.len(), 1);
        let log = &selection.logs[0];
        assert!(log.is_zstd, "同版本并存时固定取 zstd");
        assert!(log.duplicate_encoding, "并存要记一次异常");
        assert!(log.file_key.contains(":2:"), "{}", log.file_key);
    }

    #[test]
    fn a_missing_root_is_not_a_failure_but_an_unreadable_one_is() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("sessions");
        assert_eq!(select_logs(&missing, false).failed_directories, 0);

        // 存在但不是目录 → 枚举失败。
        std::fs::write(&missing, b"not a directory").expect("write");
        assert_eq!(select_logs(&missing, false).failed_directories, 1);

        // 曾经扫到过日志、现在整根读不出来：算失败，不能当成「历史被清空」。
        assert_eq!(select_logs(&missing, true).failed_directories, 1);
    }

    #[test]
    fn entries_carry_provider_qualified_models_and_token_splits() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-1"),
                serde_json::json!({
                    "type": "assistant/message",
                    "time": 1_790_000_000_000_i64,
                    "data": {
                        "turn": 1,
                        "step": 1,
                        "usage": {
                            "inputTokens": 100,
                            "outputTokens": 40,
                            "cacheReadTokens": 10,
                            "cacheWriteTokens": 5,
                            "totalTokens": 155
                        }
                    }
                }),
            ],
        );

        assert_eq!(output.outcome, DshFileOutcome::Success);
        assert_eq!(output.entries.len(), 1);
        let entry = &output.entries[0];
        assert_eq!(entry.model, "unknown");
        assert_eq!(entry.dedup_key, "dsh-entry:s-1:1:1:0");
        assert_eq!(entry.conversation_key, "dsh:s-1");
        assert_eq!(
            entry.tokens,
            TokenFacts {
                uncached_input_tokens: 100,
                output_tokens: 40,
                reasoning_output_tokens: 0,
                cache_read_input_tokens: 10,
                cache_write_5m_input_tokens: 5,
                cache_write_1h_input_tokens: 0,
            }
        );
        assert_eq!(output.total_tokens_mismatches, 0);

        let conversation = output.conversation.expect("conversation");
        assert_eq!(conversation.conversation_key, "dsh:s-1");
        assert_eq!(conversation.project_hint.as_deref(), Some("demo"));
        assert_eq!(conversation.project_key.as_deref(), Some("/work/demo"));
    }

    #[test]
    fn the_routed_model_is_used_and_the_message_source_wins() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-2"),
                serde_json::json!({
                    "type": "request/context",
                    "data": { "provider": "deepseek", "model": "deepseek-chat" }
                }),
                assistant_line(1, 1, 10, 5, 1_790_000_000_000),
                serde_json::json!({
                    "type": "assistant/attempt",
                    "time": 1_790_000_010_000_i64,
                    "data": { "turn": 1, "step": 2, "usage": { "inputTokens": 7, "outputTokens": 3 } }
                }),
            ],
        );

        assert_eq!(output.entries[0].model, "anthropic/claude-sonnet-4-5");
        assert_eq!(output.entries[1].model, "deepseek/deepseek-chat");
    }

    #[test]
    fn stream_usage_is_used_when_the_top_level_usage_is_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-3"),
                serde_json::json!({
                    "type": "assistant/attempt",
                    "time": 1_790_000_000_000_i64,
                    "data": {
                        "turn": 1,
                        "step": 1,
                        "stream": [
                            { "chunk": { "usage": { "inputTokens": 1, "outputTokens": 1 } } },
                            { "usage": { "inputTokens": 9, "outputTokens": 4 } }
                        ]
                    }
                }),
            ],
        );

        assert_eq!(output.entries[0].tokens.output_tokens, 4);
        assert_eq!(output.entries[0].tokens.uncached_input_tokens, 9);
    }

    #[test]
    fn the_same_turn_and_step_collapse_into_one_reading() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-4"),
                assistant_line(3, 1, 100, 50, 1_790_000_000_000),
                assistant_line(3, 1, 120, 60, 1_790_000_001_000),
            ],
        );

        // 同一结算点的连续采样是替换：内存里就只剩一条，且是最后一次读数。
        assert_eq!(output.entries.len(), 1);
        assert_eq!(output.entries[0].tokens.uncached_input_tokens, 120);
    }

    #[test]
    fn a_cross_round_replacement_asks_for_a_full_rescan() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = plain_log(
            dir.path(),
            &[
                session_line("s-20"),
                assistant_line(1, 1, 100, 50, 1_790_000_000_000),
            ],
        );
        let first = scan_log(&log, None, 0, false);
        assert_eq!(first.entries.len(), 1);

        // 追一条同 (turn, step) 的替换读数：上一轮那条已经在库里，只接新字节会留下两笔账。
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&log.path)
            .expect("open");
        writeln!(file, "{}", assistant_line(1, 1, 130, 70, 1_790_000_002_000)).expect("append");
        file.sync_all().expect("sync");

        let grown = DshLog {
            size: std::fs::metadata(&log.path).expect("meta").len(),
            ..log.clone()
        };
        let second = scan_log(&grown, Some(&first.state), first.offset, false);

        assert!(second.restarted, "跨轮替换必须整份重扫");
        assert_eq!(second.entries.len(), 1, "重扫后只剩最后一次读数");
        assert_eq!(second.entries[0].tokens.uncached_input_tokens, 130);
        assert_eq!(second.offset, grown.size);
    }

    #[test]
    fn retry_started_starts_a_second_charge_for_the_same_settlement() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-5"),
                assistant_line(4, 2, 100, 50, 1_790_000_000_000),
                serde_json::json!({
                    "type": "llm/retry-started",
                    "data": { "turn": 4, "step": 2 }
                }),
                assistant_line(4, 2, 60, 30, 1_790_000_002_000),
            ],
        );

        // 关闭槽位后同 (turn, step) 的新采样是**新的一笔账**：两次调用的用量都算数，
        // 因此两条记录的键必须不同（代次 +1），否则存储层会把前一次读数顶掉。
        assert_eq!(output.entries.len(), 2);
        assert_ne!(output.entries[0].dedup_key, output.entries[1].dedup_key);
        assert_eq!(output.entries[0].tokens.uncached_input_tokens, 100);
        assert_eq!(output.entries[1].tokens.uncached_input_tokens, 60);
        assert_eq!(output.entries[0].dedup_key, "dsh-entry:s-5:4:2:0");
        assert_eq!(output.entries[1].dedup_key, "dsh-entry:s-5:4:2:1");
    }

    #[test]
    fn a_new_step_appends_a_new_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-6"),
                assistant_line(4, 2, 100, 50, 1_790_000_000_000),
                assistant_line(4, 3, 10, 5, 1_790_000_001_000),
            ],
        );

        assert_eq!(output.entries.len(), 2);
        assert_ne!(output.entries[0].dedup_key, output.entries[1].dedup_key);
    }

    #[test]
    fn the_last_non_empty_title_wins() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                session_line("s-7"),
                serde_json::json!({ "type": "session/title", "data": { "title": "  第一次  " } }),
                serde_json::json!({ "type": "session/title", "data": { "title": "   " } }),
                serde_json::json!({ "type": "session/title", "data": { "title": "第二次" } }),
            ],
        );

        assert_eq!(output.state.title.as_deref(), Some("第二次"));
        assert_eq!(
            output.conversation.expect("conversation").title.as_deref(),
            Some("第二次")
        );
    }

    #[test]
    fn a_seeded_session_discards_inherited_records_until_the_seed_ends() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                serde_json::json!({
                    "type": "session",
                    "id": "s-8",
                    "cwd": "/work/demo",
                    "isSeeded": true,
                    "seedLength": 50
                }),
                serde_json::json!({ "type": "session/title", "seq": 10, "data": { "title": "继承的标题" } }),
                assistant_line(1, 1, 100, 50, 1_790_000_000_000),
                serde_json::json!({ "type": "session/end-seed", "data": { "inherited": true } }),
                assistant_line(2, 1, 20, 10, 1_790_000_100_000),
            ],
        );

        assert_eq!(output.entries.len(), 1, "种子里的记录不入账");
        assert_eq!(output.entries[0].tokens.uncached_input_tokens, 20);
        assert_eq!(output.state.title, None, "种子里的标题被清掉");
        assert_eq!(output.state.seed_complete, Some(true));
    }

    #[test]
    fn a_sequence_beyond_the_seed_cut_ends_the_seed_without_a_marker() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                serde_json::json!({
                    "type": "session",
                    "id": "s-9",
                    "cwd": "/work/demo",
                    "isSeeded": true,
                    "seedLength": 5
                }),
                assistant_line(1, 1, 100, 50, 1_790_000_000_000),
                serde_json::json!({ "type": "request/context", "seq": 40, "data": {} }),
                assistant_line(2, 1, 30, 15, 1_790_000_100_000),
            ],
        );

        assert_eq!(output.entries.len(), 1);
        assert_eq!(output.entries[0].tokens.uncached_input_tokens, 30);
    }

    #[test]
    fn a_session_without_an_id_cannot_be_accounted_for() {
        let dir = tempfile::tempdir().expect("temp dir");
        let output = scan_plain(
            dir.path(),
            &[
                serde_json::json!({ "type": "request/context", "data": {} }),
                assistant_line(1, 1, 10, 5, 1_790_000_000_000),
            ],
        );

        assert_eq!(output.outcome, DshFileOutcome::Failed);
        assert!(output.entries.is_empty());
    }

    #[test]
    fn a_broken_line_abandons_the_file_and_keeps_the_watermark() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = log_path(dir.path());
        let mut file = std::fs::File::create(&path).expect("create");
        writeln!(file, "{}", session_line("s-10")).expect("write");
        writeln!(file, "not json").expect("write");
        file.sync_all().expect("sync");

        let log = DshLog {
            path: path.clone(),
            file_key: "dsh".to_owned(),
            is_zstd: false,
            size: std::fs::metadata(&path).expect("meta").len(),
            mtime_ms: 0,
            duplicate_encoding: false,
        };
        let output = scan_log(&log, None, 0, false);

        assert_eq!(output.outcome, DshFileOutcome::Failed);
        assert_eq!(output.offset, 0, "失败时不推进水位");
        assert!(output.entries.is_empty());
    }

    #[test]
    fn appended_lines_resume_from_the_watermark() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = plain_log(
            dir.path(),
            &[
                session_line("s-11"),
                assistant_line(1, 1, 100, 50, 1_790_000_000_000),
            ],
        );
        let first = scan_log(&log, None, 0, false);
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.offset, log.size);

        // 追加一行（带完整换行）后再扫：只处理新字节。
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&log.path)
            .expect("open");
        writeln!(file, "{}", assistant_line(2, 1, 10, 5, 1_790_000_100_000)).expect("append");
        file.sync_all().expect("sync");

        let grown = DshLog {
            size: std::fs::metadata(&log.path).expect("meta").len(),
            ..log.clone()
        };
        let second = scan_log(&grown, Some(&first.state), first.offset, false);
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].tokens.uncached_input_tokens, 10);
        assert_eq!(second.offset, grown.size);
    }

    #[test]
    fn a_partial_trailing_line_is_left_for_the_next_round() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = log_path(dir.path());
        let mut file = std::fs::File::create(&path).expect("create");
        writeln!(file, "{}", session_line("s-12")).expect("write");
        write!(file, "{{\"type\":\"assistant/message\"").expect("partial");
        file.sync_all().expect("sync");

        let log = DshLog {
            path: path.clone(),
            file_key: "dsh".to_owned(),
            is_zstd: false,
            size: std::fs::metadata(&path).expect("meta").len(),
            mtime_ms: 0,
            duplicate_encoding: false,
        };
        let output = scan_log(&log, None, 0, false);

        assert_eq!(output.outcome, DshFileOutcome::Success);
        assert!(output.entries.is_empty());
        assert!(output.offset < log.size, "半截行不能推进水位");
    }

    #[test]
    fn zstd_frames_are_decoded_and_a_torn_tail_is_deferred() {
        let dir = tempfile::tempdir().expect("temp dir");
        let payload = format!(
            "{}\n{}\n",
            session_line("s-13"),
            assistant_line(1, 1, 42, 21, 1_790_000_000_000)
        );
        let log = zstd_log(dir.path(), &[payload.as_str()]);

        let first = scan_log(&log, None, 0, false);
        assert_eq!(first.outcome, DshFileOutcome::Success);
        assert_eq!(first.entries.len(), 1);
        assert_eq!(first.entries[0].tokens.uncached_input_tokens, 42);
        assert_eq!(first.offset, log.size);

        // 追加一个完整帧与一个半截帧：只有完整帧入账。
        let mut bytes = std::fs::read(&log.path).expect("read");
        let appended = format!("{}\n", assistant_line(2, 1, 7, 3, 1_790_000_100_000));
        bytes.extend(zstd::encode_all(appended.as_bytes(), 3).expect("compress"));
        let torn =
            zstd::encode_all(&b"{\"type\":\"assistant/attempt\"}\nmore"[..], 3).expect("compress");
        bytes.extend(&torn[..10]);
        std::fs::write(&log.path, &bytes).expect("write");

        let grown = DshLog {
            size: bytes.len() as u64,
            ..log.clone()
        };
        let second = scan_log(&grown, Some(&first.state), first.offset, false);
        assert_eq!(second.entries.len(), 1);
        assert_eq!(second.entries[0].tokens.uncached_input_tokens, 7);
        assert!(second.offset < grown.size, "撕裂尾帧不能推进水位");
        assert!(second.offset > first.offset);
    }

    #[test]
    fn a_corrupt_frame_discards_the_file_for_this_round() {
        let dir = tempfile::tempdir().expect("temp dir");
        let payload = format!("{}\n", session_line("s-14"));
        let mut bytes = zstd::encode_all(payload.as_bytes(), 3).expect("compress");
        bytes[0] ^= 0xFF;
        let path = dir.path().join("session.jsonl.zstd");
        std::fs::write(&path, &bytes).expect("write");

        let log = DshLog {
            path,
            file_key: "dsh".to_owned(),
            is_zstd: true,
            size: bytes.len() as u64,
            mtime_ms: 0,
            duplicate_encoding: false,
        };
        let output = scan_log(&log, None, 0, false);

        assert_eq!(output.outcome, DshFileOutcome::Failed);
        assert_eq!(output.offset, 0);
    }

    #[test]
    fn a_reset_rereads_the_whole_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let log = plain_log(
            dir.path(),
            &[
                session_line("s-15"),
                assistant_line(1, 1, 100, 50, 1_790_000_000_000),
            ],
        );
        let first = scan_log(&log, None, 0, false);

        let reset = scan_log(&log, Some(&first.state), first.offset, true);
        assert_eq!(reset.entries.len(), 1);
        assert_eq!(reset.offset, log.size);
    }

    #[test]
    fn usage_entries_are_built_without_a_reported_cost() {
        let draft = DshEntryDraft {
            dedup_key: "dsh-entry:s:1:1".to_owned(),
            conversation_key: "dsh:s".to_owned(),
            model: "anthropic/claude-sonnet-4-5".to_owned(),
            occurred_at: "2026-10-01T12:00:00Z".to_owned(),
            day_local: "2026-10-01".to_owned(),
            tokens: TokenFacts::default(),
        };

        let entry = into_usage_entry(&draft);
        assert_eq!(entry.source, UsageSource::Dsh);
        assert_eq!(entry.speed, UsageSpeed::Standard);
        assert!(
            entry.api_equivalent_cost_nanos.is_none(),
            "费用留给价格层估算"
        );
        assert_eq!(entry.request_count, 1);
        assert_eq!(entry.granularity, Granularity::Request);
    }

    #[test]
    fn child_sessions_resolve_to_their_root() {
        let sessions = vec![
            ("root".to_owned(), None),
            ("child".to_owned(), Some("root".to_owned())),
            ("grandchild".to_owned(), Some("child".to_owned())),
            ("orphan".to_owned(), Some("missing".to_owned())),
        ];

        let roots = resolve_roots(&sessions, 64);
        assert_eq!(roots.get("root").map(String::as_str), Some("root"));
        assert_eq!(roots.get("child").map(String::as_str), Some("root"));
        assert_eq!(roots.get("grandchild").map(String::as_str), Some("root"));
        // 父会话不在集合里：自己就是根，不猜它属于谁。
        assert_eq!(roots.get("orphan").map(String::as_str), Some("orphan"));
    }

    #[test]
    fn a_parent_cycle_falls_back_to_self() {
        let sessions = vec![
            ("a".to_owned(), Some("b".to_owned())),
            ("b".to_owned(), Some("a".to_owned())),
            ("c".to_owned(), Some("c".to_owned())),
        ];

        let roots = resolve_roots(&sessions, 64);
        assert_eq!(roots.get("a").map(String::as_str), Some("a"));
        assert_eq!(roots.get("b").map(String::as_str), Some("b"));
        assert_eq!(roots.get("c").map(String::as_str), Some("c"));
    }

    #[test]
    fn a_chain_longer_than_the_depth_limit_falls_back_to_self() {
        let mut sessions = vec![("s0".to_owned(), None)];
        for index in 1..10 {
            sessions.push((format!("s{index}"), Some(format!("s{}", index - 1))));
        }

        let roots = resolve_roots(&sessions, 3);
        assert_eq!(roots.get("s2").map(String::as_str), Some("s0"));
        assert_eq!(
            roots.get("s5").map(String::as_str),
            Some("s5"),
            "超出上限即自认根"
        );
    }

    #[test]
    fn scan_state_round_trips_through_json() {
        let state = DshScanState {
            session_id: Some("s-16".to_owned()),
            cwd: Some("/work/demo".to_owned()),
            parent_session: Some("root".to_owned()),
            title: Some("标题".to_owned()),
            seed_cut: Some(12),
            is_seeded: Some(true),
            seed_complete: Some(false),
            route_provider: Some("anthropic".to_owned()),
            route_model: Some("claude-sonnet-4-5".to_owned()),
            last_turn: Some(3),
            last_step: Some(2),
            last_slot_open: Some(true),
            slot_generation: Some(1),
        };

        let json = serde_json::to_string(&state).expect("serialize");
        let back: DshScanState = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, state);
        assert!(back.resolved_is_seeded());
        assert!(!back.resolved_seed_complete());
    }

    #[test]
    fn a_state_without_explicit_flags_derives_the_seeded_shape() {
        let fresh = DshScanState::default();
        assert!(!fresh.resolved_is_seeded());
        assert!(fresh.resolved_seed_complete());

        let seeded = DshScanState {
            seed_cut: Some(3),
            ..DshScanState::default()
        };
        assert!(seeded.resolved_is_seeded());
    }

    #[test]
    fn a_seeded_resume_that_meets_another_seed_marker_asks_for_a_restart() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = log_path(dir.path());
        let mut file = std::fs::File::create(&path).expect("create");
        writeln!(
            file,
            "{}",
            serde_json::json!({
                "type": "session",
                "id": "s-17",
                "cwd": "/work/demo",
                "isSeeded": true,
                "seedLength": 0
            })
        )
        .expect("write");
        file.sync_all().expect("sync");

        let log = DshLog {
            path: path.clone(),
            file_key: "dsh".to_owned(),
            is_zstd: false,
            size: std::fs::metadata(&path).expect("meta").len(),
            mtime_ms: 0,
            duplicate_encoding: false,
        };
        let first = scan_log(&log, None, 0, false);
        assert_eq!(first.outcome, DshFileOutcome::Success);

        // 续写一段嵌套 fork 的种子标记：必须整份重扫，不能只往后接。
        let mut handle = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open");
        writeln!(
            handle,
            "{}",
            serde_json::json!({ "type": "session/end-seed", "data": { "inherited": true } })
        )
        .expect("append");
        handle.sync_all().expect("sync");

        let grown = DshLog {
            size: std::fs::metadata(&path).expect("meta").len(),
            ..log.clone()
        };
        let state = DshScanState {
            seed_complete: Some(true),
            is_seeded: Some(true),
            ..first.state.clone()
        };
        let second = scan_log(&grown, Some(&state), first.offset, false);

        assert!(second.restarted, "应当整份重扫");
        assert_eq!(second.outcome, DshFileOutcome::Success);
        assert_eq!(second.offset, grown.size);
    }
}
