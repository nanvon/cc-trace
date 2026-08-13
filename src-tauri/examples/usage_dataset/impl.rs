//! 阶段 0 性能基线：从脱敏 Fixture 机械派生 M/L/R 数据集。
//!
//! 见 docs/性能与功耗优化方案.md 阶段 0：M/L 只机械复制并改写脱敏 ID 与时间，
//! 不引入真实标题、路径、账号或消息正文；输出目录不入仓库。
//!
//! 用法：
//!   cargo run --release --example usage_dataset -- --kind M --out /tmp/cc-trace-datasets/M
//!   cargo run --release --example usage_dataset -- --kind L --out /tmp/cc-trace-datasets/L
//!   cargo run --release --example usage_dataset -- --kind R --out /tmp/cc-trace-datasets/R

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{Value, json};

const CODEX_SESSION: &[u8] = include_bytes!("../../fixtures/usage/codex/session.jsonl");
const CODEX_INDEX: &[u8] = include_bytes!("../../fixtures/usage/codex/session_index.jsonl");
const CLAUDE_HISTORY: &[u8] = include_bytes!("../../fixtures/usage/claude/history.jsonl");
const CLAUDE_PROJECT: &[u8] = include_bytes!("../../fixtures/usage/claude/project.jsonl");
const PI_SESSION: &[u8] = include_bytes!("../../fixtures/usage/pi/session.jsonl");

const BASE_EPOCH: i64 = 1_752_912_000; // 2026-07-20T00:00:00Z 附近

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let (kind, out) = match parse_args(&args) {
        Some(value) => value,
        None => {
            eprintln!("usage: usage_dataset --kind M|L|R --out <dir> [--files N]");
            return ExitCode::FAILURE;
        }
    };
    match run(&kind, &out) {
        Ok(()) => {
            println!("dataset {kind} written to {}", out.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("dataset generation failed: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(args: &[String]) -> Option<(String, PathBuf)> {
    let mut kind = None;
    let mut out = None;
    let mut index = 1;
    while index < args.len() {
        match args[index].as_str() {
            "--kind" => {
                index += 1;
                kind = args.get(index).map(String::as_str);
            }
            "--out" => {
                index += 1;
                out = args.get(index).map(PathBuf::from);
            }
            _ => return None,
        }
        index += 1;
    }
    Some((kind?.to_owned(), out?))
}

fn run(kind: &str, out: &Path) -> Result<(), String> {
    match kind {
        "M" => derive_scale(out, 300, 150, 0),
        "L" => derive_scale(out, 3_000, 150, 0),
        "R" => derive_adversarial(out),
        other => Err(format!("unknown dataset kind: {other}")),
    }
}

fn ts(epoch_secs: i64) -> String {
    DateTime::<Utc>::from_timestamp(epoch_secs, 0)
        .expect("epoch in range")
        .to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn parse_lines(raw: &[u8]) -> Result<Vec<Value>, String> {
    std::str::from_utf8(raw)
        .map_err(|error| error.to_string())?
        .lines()
        .map(|line| serde_json::from_str(line).map_err(|error| error.to_string()))
        .collect()
}

fn set_time(line: &mut Value, epoch_secs: i64) {
    line["timestamp"] = json!(ts(epoch_secs));
}

fn set_payload_id(line: &mut Value, id: &str) {
    line["payload"]["id"] = json!(id);
}

/// 派生一个 Codex 会话文件：固定结构行 + 复制模板 token_count 事件直到行数达标，
/// 时间戳严格递增，只改写脱敏 ID 与时间。
fn codex_session_file(lines: &[Value], file_index: u64, line_count: usize) -> String {
    let mut output = Vec::new();
    let mut tick = 0_i64;
    let session_id = format!("bench-codex-session-{file_index}");
    let mut remaining = line_count;
    for template in lines {
        let kind = template["type"].as_str().unwrap_or("");
        let is_token_count =
            kind == "event_msg" && template["payload"]["type"].as_str() == Some("token_count");
        if is_token_count {
            while remaining > 0 {
                let mut line = template.clone();
                set_time(&mut line, BASE_EPOCH + file_index as i64 * 3_600 + tick);
                tick += 1;
                output.push(serde_json::to_string(&line).expect("serialize codex line"));
                remaining -= 1;
            }
            continue;
        }
        if remaining == 0 {
            break;
        }
        let mut line = template.clone();
        set_time(&mut line, BASE_EPOCH + file_index as i64 * 3_600 + tick);
        tick += 1;
        if kind == "session_meta" {
            set_payload_id(&mut line, &session_id);
            line["payload"]["cwd"] = json!(format!("/fixture/bench-project-{}", file_index % 20));
        }
        output.push(serde_json::to_string(&line).expect("serialize codex line"));
        remaining -= 1;
    }
    output.join("\n")
}

fn derive_scale(out: &Path, files: u64, lines_per_file: usize, _seed: u64) -> Result<(), String> {
    let codex_template = parse_lines(CODEX_SESSION)?;
    let codex_index_template = parse_lines(CODEX_INDEX)?;
    let claude_history_template = parse_lines(CLAUDE_HISTORY)?;
    let claude_project_template = parse_lines(CLAUDE_PROJECT)?;
    let pi_template = parse_lines(PI_SESSION)?;

    let codex_dir = out.join("codex/sessions");
    let archived_dir = out.join("codex/archived");
    let claude_dir = out.join("claude/projects");
    let pi_dir = out.join("pi/sessions");
    let opencode_dir = out.join("opencode");
    for dir in [
        codex_dir.as_path(),
        archived_dir.as_path(),
        claude_dir.as_path(),
        pi_dir.as_path(),
        opencode_dir.as_path(),
    ] {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }

    let mut codex_index_lines = Vec::new();
    let mut claude_history_lines = Vec::new();
    let codex_files = files * 45 / 100;
    let claude_files = files * 45 / 100;
    let pi_files = files - codex_files - claude_files;

    for file_index in 0..codex_files {
        let session_id = format!("bench-codex-session-{file_index}");
        let content = codex_session_file(&codex_template, file_index, lines_per_file);
        fs::write(codex_dir.join(format!("{session_id}.jsonl")), &content)
            .map_err(|error| error.to_string())?;
        let mut index_line = codex_index_template[0].clone();
        index_line["id"] = json!(session_id);
        index_line["thread_name"] = json!(format!("索引标题：派生会话 {file_index}"));
        index_line["updated_at"] = json!(ts(BASE_EPOCH + file_index as i64 * 3_600));
        codex_index_lines.push(serde_json::to_string(&index_line).expect("serialize index"));
        if file_index % 10 == 0 {
            fs::write(
                archived_dir.join(format!("{session_id}.jsonl")),
                &content[..content.len() / 2],
            )
            .map_err(|error| error.to_string())?;
        }
    }

    for file_index in 0..claude_files {
        let session_id = format!("bench-claude-session-{file_index}");
        let mut history_line = claude_history_template[0].clone();
        history_line["sessionId"] = json!(session_id);
        history_line["display"] = json!(format!("索引标题：派生会话 {file_index}"));
        history_line["project"] = json!(format!("/fixture/bench-project-{}", file_index % 20));
        history_line["timestamp"] = json!(BASE_EPOCH * 1000 + file_index as i64 * 3_600_000);
        claude_history_lines.push(serde_json::to_string(&history_line).expect("serialize"));

        let mut rows = Vec::new();
        let mut tick = 0_i64;
        for (i, template) in claude_project_template.iter().enumerate() {
            let mut line = template.clone();
            set_time(&mut line, BASE_EPOCH + file_index as i64 * 3_600 + tick);
            tick += 1;
            line["sessionId"] = json!(session_id);
            line["cwd"] = json!(format!("/fixture/bench-project-{}", file_index % 20));
            if i > 0 {
                line["message"]["id"] = json!(format!("bench-m-{file_index}-{tick}"));
            }
            rows.push(serde_json::to_string(&line).expect("serialize claude line"));
            if rows.len() >= lines_per_file {
                break;
            }
        }
        fs::write(
            claude_dir.join(format!("{session_id}.jsonl")),
            rows.join("\n"),
        )
        .map_err(|error| error.to_string())?;
    }

    for file_index in 0..pi_files {
        let session_id = format!("bench-pi-session-{file_index}");
        let mut rows = Vec::new();
        let mut tick = 0_i64;
        for template in &pi_template {
            let mut line = template.clone();
            set_time(&mut line, BASE_EPOCH + file_index as i64 * 3_600 + tick);
            tick += 1;
            line["id"] = json!(format!("bench-pi-{file_index}-{tick}"));
            if line["type"] == "session" {
                line["cwd"] = json!(format!("/fixture/bench-project-{}", file_index % 20));
            }
            if let Some(value) = line
                .get_mut("message")
                .and_then(|message| message.get_mut("model"))
                .and_then(|model| model.as_str())
            {
                line["message"]["model"] = json!(format!("{value}-bench"));
            }
            rows.push(serde_json::to_string(&line).expect("serialize pi line"));
        }
        fs::write(pi_dir.join(format!("{session_id}.jsonl")), rows.join("\n"))
            .map_err(|error| error.to_string())?;
    }

    fs::write(
        out.join("codex/session_index.jsonl"),
        format!("{}\n", codex_index_lines.join("\n")),
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        out.join("claude/history.jsonl"),
        format!("{}\n", claude_history_lines.join("\n")),
    )
    .map_err(|error| error.to_string())?;

    seed_opencode_db(&opencode_dir.join("opencode.db"), files / 10, 10)
        .map_err(|error| error.to_string())?;

    println!(
        "scaled dataset: codex={codex_files} claude={claude_files} pi={pi_files} lines_per_file={lines_per_file}"
    );
    Ok(())
}

/// 与 usage/opencode.rs 测试同构的脱敏 OpenCode 数据库：session/message/part 全脱敏。
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

/// 异常数据集：截断、同尺寸改写、归档重复、半行、无效 JSON、超长行、OpenCode 首 part 非 text。
fn derive_adversarial(out: &Path) -> Result<(), String> {
    let codex_template = parse_lines(CODEX_SESSION)?;
    let codex_dir = out.join("codex/sessions");
    let archived_dir = out.join("codex/archived");
    let claude_dir = out.join("claude/projects");
    let pi_dir = out.join("pi/sessions");
    let opencode_dir = out.join("opencode");
    for dir in [
        codex_dir.as_path(),
        archived_dir.as_path(),
        claude_dir.as_path(),
        pi_dir.as_path(),
        opencode_dir.as_path(),
    ] {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }

    // 对照正常文件
    let normal = codex_session_file(&codex_template, 0, 40);
    fs::write(codex_dir.join("r-normal.jsonl"), &normal).map_err(|error| error.to_string())?;

    // 截断：删掉尾部一段（不含完整换行）
    let cut = normal.len() / 2;
    fs::write(codex_dir.join("r-truncated.jsonl"), &normal[..cut])
        .map_err(|error| error.to_string())?;

    // 同尺寸改写：同字节数、不同内容
    let rewrite_a = codex_session_file(&codex_template, 1, 40);
    let padded = format!("{rewrite_a}\n// pad {}", "x".repeat(128));
    let target_len = padded.len();
    let mut rewrite_b = String::new();
    for (i, template) in codex_template.iter().enumerate() {
        let mut line = template.clone();
        set_time(&mut line, BASE_EPOCH + 3_600 + i as i64);
        if i > 0
            && let Some(info) = line["payload"].get_mut("info")
        {
            info["last_token_usage"]["input_tokens"] = json!(7);
        }
        rewrite_b.push_str(&serde_json::to_string(&line).expect("serialize"));
        rewrite_b.push('\n');
    }
    rewrite_b.push_str(&"y".repeat(target_len.saturating_sub(rewrite_b.len() + 1)));
    rewrite_b.push('\n');
    fs::write(codex_dir.join("r-same-size-rewrite-a.jsonl"), &padded)
        .map_err(|error| error.to_string())?;
    fs::write(codex_dir.join("r-same-size-rewrite-b.jsonl"), &rewrite_b)
        .map_err(|error| error.to_string())?;

    // 半行：末尾无换行
    let partial = normal.trim_end().to_string();
    fs::write(codex_dir.join("r-partial-line.jsonl"), partial)
        .map_err(|error| error.to_string())?;

    // 无效 JSON 行夹在正常行之间
    let mut invalid = normal.clone();
    invalid.push_str("{not-valid-json}\n");
    invalid.push_str(&normal);
    fs::write(codex_dir.join("r-invalid-json.jsonl"), invalid)
        .map_err(|error| error.to_string())?;

    // 超长行：单行超过 16 MiB（parser MAX_LINE_BYTES 上限）
    let oversize = format!("{}\n", "A".repeat(17 * 1024 * 1024));
    fs::write(codex_dir.join("r-oversize-line.jsonl"), oversize)
        .map_err(|error| error.to_string())?;

    // 归档重复：active 与 archived 同名同 stem
    fs::write(archived_dir.join("r-normal.jsonl"), &normal).map_err(|error| error.to_string())?;

    // Claude / Pi 正常对照
    let claude_template = parse_lines(CLAUDE_PROJECT)?;
    let mut claude_rows = Vec::new();
    for (tick, template) in claude_template.iter().enumerate() {
        let mut line = template.clone();
        set_time(&mut line, BASE_EPOCH + tick as i64);
        claude_rows.push(serde_json::to_string(&line).expect("serialize"));
    }
    fs::write(claude_dir.join("r-project.jsonl"), claude_rows.join("\n"))
        .map_err(|error| error.to_string())?;
    fs::write(
        out.join("claude/history.jsonl"),
        "{\"display\":\"索引标题：异常对照\",\"pastedContents\":{},\"timestamp\":1785952800000,\"project\":\"/fixture/r-project\",\"sessionId\":\"r-claude-session\"}\n",
    )
    .map_err(|error| error.to_string())?;
    fs::write(
        out.join("codex/session_index.jsonl"),
        "{\"id\":\"r-normal\",\"thread_name\":\"索引标题：异常对照\",\"updated_at\":\"2026-07-30T01:00:00.000000Z\"}\n",
    )
    .map_err(|error| error.to_string())?;

    // OpenCode：首 part 非 text，标题兜底走 message 的 user 行
    let connection = rusqlite::Connection::open(opencode_dir.join("opencode.db"))
        .map_err(|error| error.to_string())?;
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
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO session (id, title, directory, workspace_id) VALUES ('r-oc', '', '/fixture/r', 'r-ws')",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO message (id, session_id, time_created, data) VALUES ('r-m1', 'r-oc', 1750000000000, '{\"role\":\"user\",\"model\":{\"providerID\":\"opencode-go\",\"modelID\":\"deepseek-v4-flash\",\"variant\":\"default\"}}')",
            [],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "INSERT INTO part (session_id, time_created, id, data) VALUES ('r-oc', 1750000000500, 'r-p1', '{\"type\":\"screenshot\",\"text\":\"\"}')",
            [],
        )
        .map_err(|error| error.to_string())?;
    let _ = connection.close().map_err(|(_, error)| error);
    Ok(())
}
