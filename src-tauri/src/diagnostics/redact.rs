//! 诊断输出的脱敏。规则来自 `docs/日志与诊断.md` 第 5 节，与 cc-bar `Redact.swift` 同口径：
//! 邮箱、令牌、绝对路径、长十六进制与长随机串一律替换成分类占位符。
//!
//! 实现按「词」处理，不引入正则依赖；宁可多遮，不可漏。

/// 词的分隔符：空白与常见标点。分隔符原样保留，保证行结构不变。
fn is_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | ',' | ';' | '(' | ')' | '<' | '>' | '[' | ']' | '{' | '}'
        )
}

const SECRET_KEY_HINTS: [&str; 9] = [
    "token",
    "secret",
    "password",
    "passwd",
    "authorization",
    "cookie",
    "apikey",
    "api_key",
    "api-key",
];

const SECRET_PREFIXES: [&str; 6] = ["sk-", "GOCSPX-", "ghp_", "github_pat_", "xoxb-", "AIza"];

fn looks_like_email(word: &str) -> bool {
    match word.find('@') {
        Some(at) if at > 0 => word[at + 1..].contains('.'),
        _ => false,
    }
}

fn looks_like_path(word: &str) -> bool {
    if word.starts_with("file://") || word.starts_with("~/") || word.starts_with("\\\\") {
        return true;
    }
    let bytes = word.as_bytes();
    // `C:\...` 或 `C:/...`
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'\\' || bytes[2] == b'/')
    {
        return true;
    }
    // `/Users/x/...`；只有一段的 `/foo` 不当作路径，避免误伤 `/` 分隔的短语。
    word.starts_with('/') && word[1..].contains('/')
}

fn looks_like_hex(word: &str) -> bool {
    word.len() >= 32 && word.chars().all(|c| c.is_ascii_hexdigit())
}

fn looks_like_long_secret(word: &str) -> bool {
    word.len() >= 40
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '/' | '=' | '.'))
}

fn redact_word(word: &str) -> String {
    if word.is_empty() {
        return String::new();
    }
    if let Some((key, value)) = word.split_once('=') {
        let lower = key.to_ascii_lowercase();
        if SECRET_KEY_HINTS.iter().any(|hint| lower.contains(hint)) {
            return format!("{key}=<redacted>");
        }
        // `key=value` 里的值单独判断，前缀 `path=`、`account=` 不能遮住路径与邮箱。
        return format!("{key}={}", redact_word(value));
    }
    if word.starts_with("eyJ") && word.matches('.').count() >= 2 {
        return "<jwt>".to_owned();
    }
    if SECRET_PREFIXES
        .iter()
        .any(|prefix| word.starts_with(prefix) && word.len() >= 12)
    {
        return "<secret>".to_owned();
    }
    if looks_like_email(word) {
        return "<email>".to_owned();
    }
    if looks_like_path(word) {
        return "<path>".to_owned();
    }
    if looks_like_hex(word) {
        return "<hex>".to_owned();
    }
    if looks_like_long_secret(word) {
        return "<secret>".to_owned();
    }
    word.to_owned()
}

/// 脱敏一段文本（可多行）。
pub fn redact(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut word = String::new();
    // 上一个词若是认证方案或 `Authorization:`，下一个词整体视为秘密。
    let mut redact_next = false;

    let flush = |word: &mut String, output: &mut String, redact_next: &mut bool| {
        if word.is_empty() {
            return;
        }
        let lower = word.to_ascii_lowercase();
        if *redact_next {
            output.push_str("<redacted>");
            *redact_next = false;
        } else {
            output.push_str(&redact_word(word));
        }
        if matches!(
            lower.as_str(),
            "bearer" | "basic" | "authorization:" | "cookie:"
        ) {
            *redact_next = true;
        }
        word.clear();
    };

    for c in input.chars() {
        if is_delimiter(c) {
            flush(&mut word, &mut output, &mut redact_next);
            output.push(c);
        } else {
            word.push(c);
        }
    }
    flush(&mut word, &mut output, &mut redact_next);
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_email_and_paths() {
        let out = redact("account=alice@example.com path=/Users/alice/.codex/auth.json ok");
        assert!(!out.contains("alice"));
        assert!(out.contains("<email>") || out.contains("account="));
        assert!(out.contains("<path>"));
        assert!(out.ends_with("ok"));
    }

    #[test]
    fn redacts_windows_paths() {
        let out = redact(r"read C:\Users\bob\.claude\.credentials.json failed");
        assert!(!out.contains("bob"));
        assert!(out.contains("<path>"));
    }

    #[test]
    fn redacts_tokens() {
        let jwt = "eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxIn0.sig";
        let out = redact(&format!(
            "Authorization: Bearer abc123 jwt {jwt} sk-abcdefghijklmnop"
        ));
        assert!(!out.contains("abc123"));
        assert!(!out.contains("eyJ"));
        assert!(!out.contains("sk-abcdef"));
    }

    #[test]
    fn redacts_secret_key_values() {
        let out = redact("access_token=xyz refresh_token=qq status=200");
        assert!(!out.contains("xyz"));
        assert!(!out.contains("qq"));
        assert!(out.contains("status=200"));
    }

    #[test]
    fn redacts_long_hex_and_random() {
        let hex = "a".repeat(40);
        assert_eq!(redact(&hex), "<hex>");
        let random = "Ab1-".repeat(12);
        assert_eq!(redact(&random), "<secret>");
    }

    #[test]
    fn keeps_plain_diagnostics() {
        let line = "2026-10-01T00:00:00Z info scheduler refresh_end provider=codex status=200";
        assert_eq!(redact(line), line);
    }
}
