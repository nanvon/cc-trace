//! 阶段 0 性能基线：B1/B2/B5 场景测量（docs/性能与功耗优化方案.md 阶段 0）。
//!
//! 只使用脱敏数据集与显式临时目录，不触碰真实用户目录。必须带 perf-baseline feature
//! 编译以获得 quick_check／批次计数：
//!   cargo run --release --example usage_bench --features perf-baseline --
//!     --dataset-dir /tmp/cc-trace-datasets/M --scenario B1 --work-dir /tmp/cc-trace-bench
//!
//! 输出每轮 JSON；汇总给出中位数与范围。B0/B7 的能耗指标不在本工具范围（见文档）。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use cc_trace_lib::storage::PerfStats;
use cc_trace_lib::usage::{BenchmarkRoots, UsageService};

const DEFAULT_REPEAT: usize = 5;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let Some((dataset_dir, scenario, work_dir, repeat)) = parse_args(&args) else {
        eprintln!(
            "usage: usage_bench --dataset-dir <dir> --scenario B1|B2|B5 \
             --work-dir <dir> [--repeat N]"
        );
        return ExitCode::FAILURE;
    };
    match run(&dataset_dir, &scenario, &work_dir, repeat) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("benchmark failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: &[String]) -> Option<(PathBuf, String, PathBuf, usize)> {
    let mut dataset_dir = None;
    let mut scenario = None;
    let mut work_dir = None;
    let mut repeat = DEFAULT_REPEAT;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--dataset-dir" => {
                index += 1;
                dataset_dir = args.get(index).map(PathBuf::from);
            }
            "--scenario" => {
                index += 1;
                scenario = args.get(index).map(String::as_str);
            }
            "--work-dir" => {
                index += 1;
                work_dir = args.get(index).map(PathBuf::from);
            }
            "--repeat" => {
                index += 1;
                repeat = args.get(index).and_then(|value| value.parse().ok())?;
            }
            _ => return None,
        }
        index += 1;
    }
    Some((dataset_dir?, scenario?.to_owned(), work_dir?, repeat))
}

fn run(dataset_dir: &Path, scenario: &str, work_dir: &Path, repeat: usize) -> Result<(), String> {
    fs::create_dir_all(work_dir).map_err(|error| error.to_string())?;
    let roots = roots_from_dataset(dataset_dir);
    match scenario {
        "B1" => run_b1(roots, work_dir, repeat),
        "B2" => run_b2(roots, work_dir, repeat),
        "B5" => run_b5(work_dir),
        other => Err(format!("unknown scenario: {other}")),
    }
}

fn roots_from_dataset(dir: &Path) -> BenchmarkRoots {
    BenchmarkRoots {
        codex_sessions: Some(dir.join("codex/sessions")),
        codex_archived: Some(dir.join("codex/archived")),
        claude_projects: Some(dir.join("claude/projects")),
        pi_sessions: Some(dir.join("pi/sessions")),
        opencode_db: Some(dir.join("opencode/opencode.db")),
        codex_title_index: Some(dir.join("codex/session_index.jsonl")),
        claude_history: Some(dir.join("claude/history.jsonl")),
    }
}

/// B1：完成一次全量重建（每次迭代全新 config 目录）。
fn run_b1(roots: BenchmarkRoots, work_dir: &Path, repeat: usize) -> Result<(), String> {
    let mut rounds = Vec::new();
    for index in 0..repeat {
        let config = work_dir.join(format!("b1-config-{index}"));
        if config.exists() {
            fs::remove_dir_all(&config).map_err(|error| error.to_string())?;
        }
        let service = UsageService::new(config);
        let started = Instant::now();
        service
            .run_benchmark_scan(roots.clone())
            .map_err(|error| format!("{error:?}"))?;
        let elapsed_ms = started.elapsed().as_millis();
        let stats = service.perf_stats();
        let status = service.scan_status();
        rounds.push(Round::from_scan(
            index,
            elapsed_ms,
            stats,
            status.bytes_read,
            status.completed_files,
            status.inserted_entries,
        ));
    }
    report("B1", &rounds);
    Ok(())
}

/// B2：同一 config 首次全量 + 连续 repeat 次无变化扫描。
fn run_b2(roots: BenchmarkRoots, work_dir: &Path, repeat: usize) -> Result<(), String> {
    let config = work_dir.join("b2-config");
    if config.exists() {
        fs::remove_dir_all(&config).map_err(|error| error.to_string())?;
    }
    let service = UsageService::new(config);
    service
        .run_benchmark_scan(roots.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut previous = service.perf_stats();
    let mut previous_bytes = service.scan_status().bytes_read;
    let mut rounds = Vec::new();
    for index in 0..repeat {
        let started = Instant::now();
        service
            .run_benchmark_scan(roots.clone())
            .map_err(|error| format!("{error:?}"))?;
        let elapsed_ms = started.elapsed().as_millis();
        let stats = service.perf_stats();
        let status = service.scan_status();
        let delta = stats_delta(&stats, &previous);
        let bytes_delta = status.bytes_read.saturating_sub(previous_bytes);
        previous = stats;
        previous_bytes = status.bytes_read;
        rounds.push(Round {
            label: index,
            elapsed_ms,
            write_opens: delta.write_opens,
            quick_checks: delta.quick_checks,
            batch_commits: delta.batch_commits,
            batch_commit_nanos: delta.batch_commit_nanos,
            quota_snapshots: delta.quota_snapshots,
            bytes_read: bytes_delta,
            files: status.completed_files,
            inserted: status.inserted_entries,
        });
    }
    report("B2", &rounds);
    Ok(())
}

/// B5：OpenCode 无新增／少量新增／全量重建。
fn run_b5(work_dir: &Path) -> Result<(), String> {
    let db_dir = work_dir.join("b5-opencode");
    fs::create_dir_all(&db_dir).map_err(|error| error.to_string())?;
    let db_path = db_dir.join("opencode.db");
    if db_path.exists() {
        fs::remove_file(&db_path).map_err(|error| error.to_string())?;
    }
    seed_opencode_db(&db_path, 200, 20).map_err(|error| error.to_string())?;
    let roots = BenchmarkRoots {
        opencode_db: Some(db_path.clone()),
        ..BenchmarkRoots::default()
    };
    let config = work_dir.join("b5-config");
    if config.exists() {
        fs::remove_dir_all(&config).map_err(|error| error.to_string())?;
    }
    let service = UsageService::new(config);

    // 全量重建：新 db 首次扫描
    let mut rounds = Vec::new();
    let started = Instant::now();
    service
        .run_benchmark_scan(roots.clone())
        .map_err(|error| format!("{error:?}"))?;
    let mut previous = service.perf_stats();
    rounds.push(Round::from_scan(
        0,
        started.elapsed().as_millis(),
        previous,
        service.scan_status().bytes_read,
        service.scan_status().completed_files,
        service.scan_status().inserted_entries,
    ));

    // 无新增：同一 db 立即重扫
    let started = Instant::now();
    service
        .run_benchmark_scan(roots.clone())
        .map_err(|error| format!("{error:?}"))?;
    let stats = service.perf_stats();
    let delta = stats_delta(&stats, &previous);
    previous = stats;
    rounds.push(Round {
        label: 1,
        elapsed_ms: started.elapsed().as_millis(),
        write_opens: delta.write_opens,
        quick_checks: delta.quick_checks,
        batch_commits: delta.batch_commits,
        batch_commit_nanos: delta.batch_commit_nanos,
        quota_snapshots: delta.quota_snapshots,
        bytes_read: 0,
        files: 0,
        inserted: 0,
    });

    // 少量新增：追加一条 assistant message + 2 parts
    append_opencode_message(&db_path, "bench-oc-session-0", "m-append-1")
        .map_err(|error| error.to_string())?;
    let started = Instant::now();
    service
        .run_benchmark_scan(roots.clone())
        .map_err(|error| format!("{error:?}"))?;
    let stats = service.perf_stats();
    let delta = stats_delta(&stats, &previous);
    rounds.push(Round {
        label: 2,
        elapsed_ms: started.elapsed().as_millis(),
        write_opens: delta.write_opens,
        quick_checks: delta.quick_checks,
        batch_commits: delta.batch_commits,
        batch_commit_nanos: delta.batch_commit_nanos,
        quota_snapshots: delta.quota_snapshots,
        bytes_read: 0,
        files: 0,
        inserted: 0,
    });
    report("B5", &rounds);
    Ok(())
}

#[derive(Debug, Clone)]
struct Round {
    label: usize,
    elapsed_ms: u128,
    write_opens: u64,
    quick_checks: u64,
    batch_commits: u64,
    batch_commit_nanos: u64,
    quota_snapshots: u64,
    bytes_read: u64,
    files: u64,
    inserted: u64,
}

impl Round {
    fn from_scan(
        label: usize,
        elapsed_ms: u128,
        stats: PerfStats,
        bytes_read: u64,
        files: u64,
        inserted: u64,
    ) -> Self {
        Self {
            label,
            elapsed_ms,
            write_opens: stats.write_opens,
            quick_checks: stats.quick_checks,
            batch_commits: stats.batch_commits,
            batch_commit_nanos: stats.batch_commit_nanos,
            quota_snapshots: stats.quota_snapshots,
            bytes_read,
            files,
            inserted,
        }
    }
}

fn stats_delta(current: &PerfStats, previous: &PerfStats) -> PerfStats {
    PerfStats {
        write_opens: current.write_opens.saturating_sub(previous.write_opens),
        quick_checks: current.quick_checks.saturating_sub(previous.quick_checks),
        batch_commits: current.batch_commits.saturating_sub(previous.batch_commits),
        batch_commit_nanos: current
            .batch_commit_nanos
            .saturating_sub(previous.batch_commit_nanos),
        quota_snapshots: current
            .quota_snapshots
            .saturating_sub(previous.quota_snapshots),
    }
}

fn report(scenario: &str, rounds: &[Round]) {
    for round in rounds {
        println!(
            "{}",
            serde_json::json!({
                "scenario": scenario,
                "round": round.label,
                "elapsed_ms": round.elapsed_ms,
                "write_opens": round.write_opens,
                "quick_checks": round.quick_checks,
                "batch_commits": round.batch_commits,
                "batch_commit_nanos": round.batch_commit_nanos,
                "quota_snapshots": round.quota_snapshots,
                "bytes_read": round.bytes_read,
                "files": round.files,
                "inserted": round.inserted,
            })
        );
    }
    let mut times: Vec<u128> = rounds.iter().map(|round| round.elapsed_ms).collect();
    times.sort_unstable();
    let median = times[times.len() / 2];
    println!(
        "{} summary: elapsed_ms min={} median={} max={}",
        scenario,
        times.first().copied().unwrap_or(0),
        median,
        times.last().copied().unwrap_or(0)
    );
}

/// 与 usage/opencode.rs 测试同构的脱敏 OpenCode 数据库。
fn seed_opencode_db(path: &Path, sessions: u64, messages_per_session: u64) -> rusqlite::Result<()> {
    let connection = rusqlite::Connection::open(path)?;
    connection.execute_batch(
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
    )?;
    for session in 0..sessions {
        let session_id = format!("bench-oc-session-{session}");
        connection.execute(
            "INSERT INTO session (id, title, directory, workspace_id) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![
                session_id,
                format!("派生会话 {session}"),
                format!("/fixture/bench-project-{}", session % 20),
                format!("ws-{session}"),
            ],
        )?;
        connection.execute(
            "INSERT INTO workspace (id, branch) VALUES (?1, 'main')",
            rusqlite::params![format!("ws-{session}")],
        )?;
        for message in 0..messages_per_session {
            let base = 1_752_912_000_i64 + session as i64 * 3_600 + message as i64 * 60;
            let user = message % 2 == 0;
            let id = format!("bench-oc-m-{session}-{message}");
            let data = if user {
                r#"{"role":"user","model":{"providerID":"opencode-go","modelID":"deepseek-v4-flash","variant":"default"}}"#
            } else {
                r#"{"role":"assistant","providerID":"opencode-go","modelID":"deepseek-v4-flash","tokens":{"input":100,"output":20,"reasoning":5,"cache":{"read":10,"write":2},"total":137},"cost":"0.000456"}"#
            };
            connection.execute(
                "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![id, session_id, base * 1000, data],
            )?;
            connection.execute(
                "INSERT INTO part (session_id, time_created, id, data) VALUES (?1, ?2, ?3, ?4)",
                rusqlite::params![
                    session_id,
                    base * 1000,
                    format!("bench-oc-p-{session}-{message}"),
                    r#"{"type":"text","text":"<plan>派生占位计划"}"#
                ],
            )?;
        }
    }
    let _ = connection.close();
    Ok(())
}

fn append_opencode_message(
    path: &Path,
    session_id: &str,
    message_id: &str,
) -> rusqlite::Result<()> {
    let connection = rusqlite::Connection::open(path)?;
    let base: i64 = connection.query_row("SELECT MAX(time_created) FROM message", [], |row| {
        row.get(0)
    })?;
    connection.execute(
        "INSERT INTO message (id, session_id, time_created, data) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            message_id,
            session_id,
            base + 60_000,
            r#"{"role":"assistant","providerID":"opencode-go","modelID":"deepseek-v4-flash","tokens":{"input":10,"output":5,"reasoning":0,"cache":{"read":0,"write":0},"total":15},"cost":"0.000001"}"#
        ],
    )?;
    connection.execute(
        "INSERT INTO part (session_id, time_created, id, data) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            session_id,
            base + 60_000,
            "bench-oc-p-append-1",
            r#"{"type":"text","text":"<plan>追加占位"}"#
        ],
    )?;
    connection.execute(
        "INSERT INTO part (session_id, time_created, id, data) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            session_id,
            base + 60_001,
            "bench-oc-p-append-2",
            r#"{"type":"text","text":"<plan>追加占位二"}"#
        ],
    )?;
    let _ = connection.close();
    Ok(())
}
