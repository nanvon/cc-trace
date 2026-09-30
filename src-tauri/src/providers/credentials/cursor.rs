//! Cursor 凭据：只读采用 Cursor 桌面端自己的登录态。
//!
//! 与 cc-bar v1.1.1 的做法一致，这条路径**只读**：
//!
//! - 只读 `state.vscdb` 里的 `cursorAuth/accessToken`；
//! - 不读 refresh token、不调用 OAuth、不写回 Cursor 的库，也不把令牌复制到
//!   CC Trace 的秘密存储里。Cursor 的登录态属于 Cursor，我们只是借用一次会话。
//!
//! 数据库路径按平台取（见 [ADR-0035]）：
//!
//! - macOS：`~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
//! - Windows：`%APPDATA%\Cursor\User\globalStorage\state.vscdb`
//!
//! Windows 路径证据等级：**中**——Cursor 未公开该路径的官方文档，取值来自 VS Code
//! 分叉的既定布局与多个第三方工具的读取实现（community 证据）。Windows 实机未验证。
//!
//! [ADR-0035]: ../../../../docs/决策/ADR-0035-服务矩阵与Windows凭据存储.md

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use rusqlite::{Connection, OpenFlags};

use super::{Discovery, Secret, jwt, non_empty};

/// Cursor 存访问令牌的键。
const ACCESS_TOKEN_KEY: &str = "cursorAuth/accessToken";
/// 令牌剩余寿命不足这个值就当已过期：读出来也发不出请求，早点告诉用户去重新登录。
const EXPIRATION_LEEWAY_SECS: i64 = 60;
const BUSY_TIMEOUT_MILLIS: u32 = 250;

#[derive(Clone, PartialEq, Eq)]
pub struct CursorCredentials {
    access_token: Secret,
    user_id: Secret,
    pub subject: String,
    pub email: Option<String>,
    pub expires_at: DateTime<Utc>,
}

/// 手动实现：只暴露可展示的字段，令牌与 user id 都不进门。
impl std::fmt::Debug for CursorCredentials {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `subject` 与 user id 都是账号身份，不进门：日志里只留是否拿到会话与到期时刻。
        formatter
            .debug_struct("CursorCredentials")
            .field("has_subject", &!self.subject.is_empty())
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl CursorCredentials {
    /// 请求 Cursor Dashboard 用的 Cookie。
    ///
    /// 值里的 `::` 必须百分号编码成 `%3A%3A`：Cursor 的会话 Cookie 解析按已编码形式
    /// 处理，直接塞原始冒号会被判成无效会话。
    pub fn cookie_header(&self) -> Secret {
        Secret::new(format!(
            "WorkosCursorSessionToken={}%3A%3A{}",
            self.user_id.expose(),
            self.access_token.expose()
        ))
    }

    /// 账号身份键：user id 优先，缺失时退回邮箱。
    pub fn account_key(&self) -> Option<Secret> {
        if !self.user_id.expose().trim().is_empty() {
            return Some(self.user_id.clone());
        }
        non_empty(self.email.as_deref()).map(Secret::new)
    }
}

/// Cursor 数据库路径。平台差异只在这里。
pub fn default_database_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        Some(home.join("Library/Application Support/Cursor/User/globalStorage/state.vscdb"))
    }

    #[cfg(windows)]
    {
        let roaming = std::env::var_os("APPDATA").map(PathBuf::from)?;
        Some(
            roaming
                .join("Cursor")
                .join("User")
                .join("globalStorage")
                .join("state.vscdb"),
        )
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    {
        None
    }
}

pub fn discover() -> Discovery<CursorCredentials> {
    let Some(path) = default_database_path() else {
        return Discovery::Unsupported;
    };
    discover_at(&path, Utc::now())
}

/// 从指定数据库读取并校验会话。测试与集成路径都用这一个入口。
pub fn discover_at(path: &std::path::Path, now: DateTime<Utc>) -> Discovery<CursorCredentials> {
    if !path.exists() {
        return Discovery::Missing;
    }
    let token = match read_access_token(path) {
        ReadOutcome::Found(token) => token,
        ReadOutcome::Missing => return Discovery::Missing,
        ReadOutcome::Unreadable => return Discovery::Unreadable,
    };

    match session(&token, now) {
        Ok(credentials) => Discovery::Found(credentials),
        Err(SessionError::Expired) => Discovery::Expired,
        // 令牌不是 JWT 或缺少用户信息：有登录态但这套凭据不可用。
        Err(SessionError::Malformed) => Discovery::Unreadable,
    }
}

enum ReadOutcome {
    Found(String),
    /// 库里没有这个键：Cursor 还没登录。
    Missing,
    Unreadable,
}

/// 只读打开 Cursor 的 SQLite。Cursor 正在运行时库处于 WAL 状态，
/// 我们不加写锁、只读一行，并用短 busy timeout 避免和前台卡在一起。
fn read_access_token(path: &std::path::Path) -> ReadOutcome {
    match read_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY) {
        Ok(outcome) => outcome,
        Err(_) => ReadOutcome::Unreadable,
    }
}

fn read_with_flags(
    path: &std::path::Path,
    flags: OpenFlags,
) -> Result<ReadOutcome, rusqlite::Error> {
    let connection = match Connection::open_with_flags(path, flags) {
        Ok(connection) => connection,
        Err(error) => {
            // 库处于 WAL 状态且 sidecar 不可写时，只读模式会打不开。
            // 退到 `immutable=1`：明确声明「把文件当成不会变的快照读」，
            // 代价是可能读到稍旧的状态，收益是不去动 Cursor 的数据文件。
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::CannotOpen) {
                let uri = format!(
                    "file:{}?immutable=1",
                    path.to_string_lossy().replace('?', "%3F")
                );
                return read_with_flags(
                    std::path::Path::new(&uri),
                    OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
                );
            }
            return Err(error);
        }
    };
    connection.busy_timeout(std::time::Duration::from_millis(u64::from(
        BUSY_TIMEOUT_MILLIS,
    )))?;

    let mut statement =
        connection.prepare("SELECT value FROM ItemTable WHERE key = ?1 LIMIT 1;")?;
    let mut rows = statement.query([ACCESS_TOKEN_KEY])?;
    let Some(row) = rows.next()? else {
        return Ok(ReadOutcome::Missing);
    };
    let value: String = row.get(0)?;
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(ReadOutcome::Missing);
    }
    Ok(ReadOutcome::Found(trimmed.to_owned()))
}

enum SessionError {
    /// 令牌已经过期。
    Expired,
    /// 不是 JWT、缺 `sub`、缺 `exp`，或 user id 含不安全字符。
    Malformed,
}

/// 从访问令牌构造会话。`sub` 形如 `auth0|user_xxx`，user id 取最后一段。
fn session(access_token: &str, now: DateTime<Utc>) -> Result<CursorCredentials, SessionError> {
    let payload = jwt::decode_payload(access_token).ok_or(SessionError::Malformed)?;
    let subject = non_empty(payload.get("sub").and_then(serde_json::Value::as_str))
        .ok_or(SessionError::Malformed)?;
    let user_id = subject
        .split('|')
        .rfind(|part| !part.is_empty())
        .ok_or(SessionError::Malformed)?;
    // user id 会进 Cookie 头，只允许字母数字与 `._-`：任何其他字符都可能让请求头
    // 被外部输入改写。
    if user_id.is_empty()
        || !user_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
    {
        return Err(SessionError::Malformed);
    }

    let expires_at = jwt::expires_at(access_token).ok_or(SessionError::Malformed)?;
    if (expires_at - now).num_seconds() <= EXPIRATION_LEEWAY_SECS {
        return Err(SessionError::Expired);
    }

    Ok(CursorCredentials {
        access_token: Secret::new(access_token),
        user_id: Secret::new(user_id),
        subject: subject.to_owned(),
        email: non_empty(payload.get("email").and_then(serde_json::Value::as_str)),
        expires_at,
    })
}

#[cfg(test)]
mod tests {
    use super::super::jwt::base64url_encode;
    use super::*;

    fn token(subject: &str, expires_in_secs: i64) -> String {
        let header = base64url_encode(br#"{"alg":"none"}"#);
        let exp = Utc::now().timestamp() + expires_in_secs;
        let payload = base64url_encode(
            serde_json::json!({
                "sub": subject,
                "exp": exp,
                "email": "cursor@example.com"
            })
            .to_string()
            .as_bytes(),
        );
        format!("{header}.{payload}.signature")
    }

    fn database_with_token(path: &std::path::Path, token: Option<&str>) {
        let connection = Connection::open(path).expect("create db");
        connection
            .execute_batch("CREATE TABLE ItemTable (key TEXT PRIMARY KEY, value BLOB);")
            .expect("schema");
        if let Some(token) = token {
            connection
                .execute(
                    "INSERT INTO ItemTable (key, value) VALUES (?1, ?2);",
                    rusqlite::params![ACCESS_TOKEN_KEY, token],
                )
                .expect("insert");
        }
    }

    #[test]
    fn a_missing_database_is_no_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let outcome = discover_at(&dir.path().join("state.vscdb"), Utc::now());
        assert!(matches!(outcome, Discovery::Missing));
    }

    #[test]
    fn a_database_without_the_key_is_no_credentials() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        database_with_token(&path, None);

        assert!(matches!(discover_at(&path, Utc::now()), Discovery::Missing));
    }

    #[test]
    fn a_valid_token_yields_a_session_and_an_encoded_cookie() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        let raw = token("auth0|user_abc123", 3600);
        database_with_token(&path, Some(&raw));

        let Discovery::Found(credentials) = discover_at(&path, Utc::now()) else {
            panic!("expected a session");
        };
        assert_eq!(credentials.email.as_deref(), Some("cursor@example.com"));
        assert_eq!(credentials.subject, "auth0|user_abc123");
        assert_eq!(
            credentials.cookie_header().expose(),
            format!("WorkosCursorSessionToken=user_abc123%3A%3A{raw}")
        );
        assert_eq!(
            credentials.account_key().map(|key| key.expose().to_owned()),
            Some("user_abc123".to_owned())
        );
    }

    #[test]
    fn an_expired_token_is_reported_as_expired_not_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        database_with_token(&path, Some(&token("auth0|user_abc123", -10)));

        assert!(matches!(discover_at(&path, Utc::now()), Discovery::Expired));
    }

    #[test]
    fn a_token_about_to_expire_is_treated_as_expired() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        database_with_token(&path, Some(&token("auth0|user_abc123", 30)));

        assert!(matches!(discover_at(&path, Utc::now()), Discovery::Expired));
    }

    #[test]
    fn an_unsafe_user_id_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        database_with_token(&path, Some(&token("auth0|user bad;id", 3600)));

        assert!(matches!(
            discover_at(&path, Utc::now()),
            Discovery::Unreadable
        ));
    }

    #[test]
    fn a_non_jwt_token_is_unreadable_rather_than_missing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        database_with_token(&path, Some("not-a-jwt"));

        assert!(matches!(
            discover_at(&path, Utc::now()),
            Discovery::Unreadable
        ));
    }

    #[test]
    fn credentials_never_print_the_token() {
        let credentials = session(&token("auth0|user_abc123", 3600), Utc::now())
            .ok()
            .expect("session");
        let printed = format!("{credentials:?}");
        assert!(!printed.contains("user_abc123"));
        assert!(!printed.contains("auth0|"));
    }

    #[test]
    fn the_immutable_fallback_reads_a_wal_database() {
        // 只读打开一个带 WAL 的库时，如果没有写权限会走 immutable 分支。
        // 这里验证那条分支本身能读出令牌。
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("state.vscdb");
        let raw = token("auth0|user_wal", 3600);
        database_with_token(&path, Some(&raw));

        let uri = format!("file:{}?immutable=1", path.to_string_lossy());
        let outcome = read_with_flags(
            std::path::Path::new(&uri),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )
        .expect("read");
        match outcome {
            ReadOutcome::Found(value) => assert_eq!(value, raw),
            _ => panic!("expected the token"),
        }
    }
}
