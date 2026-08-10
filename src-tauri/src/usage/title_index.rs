//! Codex `session_index.jsonl` 与 Claude `history.jsonl` 的对话标题索引。
//!
//! 只读两个官方索引文件，构建 `raw session id → 清理后标题` 的映射；文件缺失、
//! 损坏或单行失败时跳过对应项，不把索引读取失败升级为扫描错误。标题正文只进入
//! `conversations` 表（与 cc-bar 的 conversation rollup 一致），不进入日志。

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::Value;

use super::parser::clean_title;

#[derive(Default)]
pub struct TitleIndex {
    pub codex: HashMap<String, String>,
    pub claude: HashMap<String, String>,
}

impl TitleIndex {
    pub fn load(codex_index: Option<&Path>, claude_history: Option<&Path>) -> Self {
        Self {
            codex: load_codex(codex_index),
            claude: load_claude(claude_history),
        }
    }
}

/// `id → thread_name`；会话 id 同时存在于 JSONL `session_meta` 与文件名。
fn load_codex(path: Option<&Path>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(path) = path else {
        return result;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return result;
    };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = value.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(title) = value
            .get("thread_name")
            .and_then(Value::as_str)
            .and_then(clean_title)
        else {
            continue;
        };
        result.insert(id.to_owned(), title);
    }
    result
}

/// `sessionId → display`；sessionId 与 JSONL 的 `sessionId` 同值。
fn load_claude(path: Option<&Path>) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let Some(path) = path else {
        return result;
    };
    let Ok(text) = fs::read_to_string(path) else {
        return result;
    };
    for line in text.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(id) = value.get("sessionId").and_then(Value::as_str) else {
            continue;
        };
        let Some(title) = value
            .get("display")
            .and_then(Value::as_str)
            .and_then(clean_title)
        else {
            continue;
        };
        result.insert(id.to_owned(), title);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_index(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        fs::write(&path, content).expect("write index fixture");
        path
    }

    #[test]
    fn codex_index_maps_id_to_cleaned_thread_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_index(
            dir.path(),
            "session_index.jsonl",
            concat!(
                r#"{"id":"id-a","thread_name":"  修复登录 模块   ","updated_at":"2026-08-01T00:00:00Z"}"#,
                "\n",
                r#"{"id":"id-b","thread_name":"<可疑前缀标题"}"#,
                "\n",
                r#"{"id":"id-c","thread_name":"  "}"#,
                "\n",
            ),
        );
        let index = TitleIndex::load(Some(path.as_path()), None);
        assert_eq!(
            index.codex.get("id-a").map(String::as_str),
            Some("修复登录 模块")
        );
        assert!(index.codex.contains_key("id-b"), "带 < 前缀的标题仍入索引");
        assert!(!index.codex.contains_key("id-c"), "空白标题不入索引");
        assert!(index.claude.is_empty());
    }

    #[test]
    fn claude_index_maps_session_to_display() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_index(
            dir.path(),
            "history.jsonl",
            concat!(
                r#"{"sessionId":"session-a","display":"重构主窗口布局","timestamp":1786000000000}"#,
                "\n",
                r#"{"sessionId":"session-b","display":"[Pasted text #1 +7 lines]","timestamp":1786000000001}"#,
                "\n",
            ),
        );
        let index = TitleIndex::load(None, Some(path.as_path()));
        assert_eq!(
            index.claude.get("session-a").map(String::as_str),
            Some("重构主窗口布局")
        );
        assert_eq!(
            index.claude.get("session-b").map(String::as_str),
            Some("[Pasted text #1 +7 lines]")
        );
        assert!(index.codex.is_empty());
    }

    #[test]
    fn missing_or_invalid_index_files_yield_empty_maps() {
        let index = TitleIndex::load(None, None);
        assert!(index.codex.is_empty());
        assert!(index.claude.is_empty());

        let dir = tempfile::tempdir().expect("tempdir");
        let broken = write_index(dir.path(), "broken.jsonl", "not json\n{}");
        let index = TitleIndex::load(Some(broken.as_path()), Some(broken.as_path()));
        assert!(index.codex.is_empty());
        assert!(index.claude.is_empty());
    }
}
