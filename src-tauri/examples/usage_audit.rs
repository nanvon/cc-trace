//! 手动真实数据审计入口。
//!
//! 只读取 Codex／Claude Code 默认 JSONL 根目录，并把派生数据写入调用方显式提供的
//! 临时目录。它不属于产品入口，也不输出路径、消息正文或对话元数据。

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::thread;
use std::time::{Duration, Instant};

use cc_trace_lib::contracts::UsageScanState;
use cc_trace_lib::usage::UsageService;

const AUDIT_DIR_MARKER: &str = "cc-trace-usage-audit";
const DEFAULT_TIMEOUT_SECONDS: u64 = 1_800;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("usage audit failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let config_dir = parse_config_dir()?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("cannot create audit directory: {error}"))?;

    let service = UsageService::new(config_dir);
    service
        .start_default_scan()
        .map_err(|error| format!("cannot start scan: {error:?}"))?;

    let started = Instant::now();
    let mut last_progress_at = Instant::now();
    loop {
        let status = service.scan_status();
        if status.state == UsageScanState::Idle && status.finished_at.is_some() {
            println!(
                "{}",
                serde_json::to_string_pretty(&status)
                    .map_err(|error| format!("cannot serialize scan result: {error}"))?
            );
            if status.cancelled {
                return Err("scan was cancelled".to_owned());
            }
            return Ok(());
        }

        if started.elapsed() > Duration::from_secs(DEFAULT_TIMEOUT_SECONDS) {
            service.cancel_scan();
            return Err(format!(
                "scan exceeded {DEFAULT_TIMEOUT_SECONDS} seconds and cancellation was requested"
            ));
        }

        if last_progress_at.elapsed() >= Duration::from_secs(5) {
            eprintln!(
                "usage audit progress: completed={}/{} bytes={} inserted={} invalid={} failed={}",
                status.completed_files,
                status.discovered_files,
                status.bytes_read,
                status.inserted_entries,
                status.invalid_lines,
                status.failed_files
            );
            last_progress_at = Instant::now();
        }
        thread::sleep(Duration::from_millis(200));
    }
}

fn parse_config_dir() -> Result<PathBuf, String> {
    let mut args = env::args_os();
    let _program = args.next();
    let Some(flag) = args.next() else {
        return Err(format!(
            "usage: usage_audit --config-dir <absolute path containing {AUDIT_DIR_MARKER}>"
        ));
    };
    if flag != "--config-dir" {
        return Err("expected --config-dir".to_owned());
    }
    let Some(raw_path) = args.next() else {
        return Err("missing --config-dir value".to_owned());
    };
    if args.next().is_some() {
        return Err("unexpected extra arguments".to_owned());
    }

    let path = PathBuf::from(raw_path);
    if !path.is_absolute() {
        return Err("audit directory must be absolute".to_owned());
    }
    if !path
        .file_name()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.starts_with(AUDIT_DIR_MARKER))
    {
        return Err(format!(
            "audit directory name must start with {AUDIT_DIR_MARKER}"
        ));
    }
    Ok(path)
}
