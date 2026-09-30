//! 本地文件日志：格式、轮转与级别见 `docs/日志与诊断.md` 第 2～4 节。
//!
//! 写日志失败一律静默降级，不得阻断业务，也不得重试。字段值写入前统一脱敏。

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::{SecondsFormat, Utc};

use super::redact::redact;

pub const LOG_FILE_NAME: &str = "cc-trace.log";
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const KEPT_FILES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warn,
    Info,
    Debug,
}

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warn => "warn",
            Self::Info => "info",
            Self::Debug => "debug",
        }
    }
}

struct FileLog {
    dir: PathBuf,
    max_bytes: u64,
}

impl FileLog {
    fn path(&self) -> PathBuf {
        self.dir.join(LOG_FILE_NAME)
    }

    fn rotated(&self, index: usize) -> PathBuf {
        self.dir.join(format!("{LOG_FILE_NAME}.{index}"))
    }

    /// `cc-trace.log` → `.1` → `.2`，最旧的被覆盖；总共保留 `KEPT_FILES` 个。
    fn rotate(&self) {
        let oldest = KEPT_FILES - 1;
        let _ = fs::remove_file(self.rotated(oldest));
        for index in (1..oldest).rev() {
            let _ = fs::rename(self.rotated(index), self.rotated(index + 1));
        }
        let _ = fs::rename(self.path(), self.rotated(1));
    }

    fn append(&self, line: &str) {
        if fs::create_dir_all(&self.dir).is_err() {
            return;
        }
        let size = fs::metadata(self.path())
            .map(|meta| meta.len())
            .unwrap_or(0);
        if size + line.len() as u64 > self.max_bytes {
            self.rotate();
        }
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.path())
        {
            let _ = file.write_all(line.as_bytes());
        }
    }
}

static LOGGER: OnceLock<FileLog> = OnceLock::new();
static VERBOSE: AtomicBool = AtomicBool::new(false);
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// 在启动时调用一次。目录创建失败不影响启动，之后的写入静默丢弃。
pub fn init(dir: PathBuf, verbose: bool) {
    let _ = LOGGER.set(FileLog {
        dir,
        max_bytes: MAX_FILE_BYTES,
    });
    set_verbose(verbose);
}

/// 「详细日志」开关：开启后才写 `debug`。
pub fn set_verbose(verbose: bool) {
    VERBOSE.store(verbose, Ordering::Relaxed);
}

pub fn log_dir() -> Option<PathBuf> {
    LOGGER.get().map(|logger| logger.dir.clone())
}

fn format_line(level: Level, module: &str, event: &str, fields: &[(&str, &str)]) -> String {
    let mut line = format!(
        "{} {} {} {}",
        Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
        level.as_str(),
        module,
        event
    );
    for (key, value) in fields {
        let value = redact(value);
        if value.contains(char::is_whitespace) || value.is_empty() {
            line.push_str(&format!(" {key}=\"{}\"", value.replace('"', "'")));
        } else {
            line.push_str(&format!(" {key}={value}"));
        }
    }
    line.push('\n');
    line
}

pub fn write(level: Level, module: &str, event: &str, fields: &[(&str, &str)]) {
    if level == Level::Debug && !VERBOSE.load(Ordering::Relaxed) {
        return;
    }
    let Some(logger) = LOGGER.get() else {
        return;
    };
    let line = format_line(level, module, event, fields);
    if let Ok(_guard) = WRITE_LOCK.lock() {
        logger.append(&line);
    }
}

pub fn info(module: &str, event: &str, fields: &[(&str, &str)]) {
    write(Level::Info, module, event, fields);
}

pub fn warn(module: &str, event: &str, fields: &[(&str, &str)]) {
    write(Level::Warn, module, event, fields);
}

pub fn error(module: &str, event: &str, fields: &[(&str, &str)]) {
    write(Level::Error, module, event, fields);
}

/// 日志尾部若干行（当前文件优先，不够再补上一个轮转文件）。
pub fn tail(max_lines: usize) -> Vec<String> {
    let Some(logger) = LOGGER.get() else {
        return Vec::new();
    };
    tail_from(logger, max_lines)
}

fn tail_from(logger: &FileLog, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for path in [logger.rotated(1), logger.path()] {
        if let Ok(text) = fs::read_to_string(&path) {
            lines.extend(text.lines().map(str::to_owned));
        }
    }
    let skip = lines.len().saturating_sub(max_lines);
    lines.split_off(skip)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("cc-trace-log-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn rotation_keeps_three_files() {
        let dir = temp_dir("rotate");
        let logger = FileLog {
            dir: dir.clone(),
            max_bytes: 64,
        };
        for index in 0..20 {
            logger.append(&format!(
                "line-{index:02}-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\n"
            ));
        }
        assert!(logger.path().exists());
        assert!(logger.rotated(1).exists());
        assert!(logger.rotated(2).exists());
        assert!(!logger.rotated(3).exists());
        let tail = tail_from(&logger, 100);
        assert!(tail.last().is_some_and(|line| line.starts_with("line-19")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn append_to_unwritable_dir_does_not_panic() {
        let logger = FileLog {
            dir: PathBuf::from("/proc/definitely/not/writable"),
            max_bytes: 64,
        };
        logger.append("x\n");
    }

    #[test]
    fn line_format_redacts_field_values() {
        let line = format_line(
            Level::Info,
            "commands",
            "demo",
            &[("path", "/Users/alice/x/y"), ("note", "two words")],
        );
        assert!(line.contains("info commands demo"));
        assert!(line.contains("path=<path>"));
        assert!(line.contains("note=\"two words\""));
        assert!(!line.contains("alice"));
    }
}
