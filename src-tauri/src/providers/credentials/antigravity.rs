//! Antigravity（Google Antigravity / Gemini Code Assist）凭据。
//!
//! 与 cc-bar v1.1.1 的 OpenUsage 模式一致：
//!
//! - 真源是 `~/.gemini/jetski-standalone-oauth-token`（agy CLI 与 IDE 插件登录即写），
//!   `~/.gemini/oauth_creds.json`（旧 Gemini CLI 遗留）仅作兜底；
//! - 到期后用 Google OAuth 静默续期，并把结果按 [ADR-0014] 原子回写**同一文件**；
//! - **client secret 不硬编码**：开源仓库与分发的二进制不该携带官方客户端密钥。
//!   续期时从本机已安装的官方组件（Gemini CLI 的 npm bundle、`~/.gemini/bin/agy`）
//!   里提取 `GOCSPX-` 密钥，并按就近原则与 client id 配对。读的是用户自己机器上的
//!   官方组件，因此永远与官方客户端同步，不受 Google 轮换影响。
//!
//! 路径全部按家目录拼接（Windows 用 `%USERPROFILE%`）：`.gemini` 是这三家 CLI 约定的
//! 家目录相对位置，不随平台变化。Windows 侧未实机验证。
//!
//! [ADR-0014]: ../../../../docs/决策/ADR-0014-token刷新结果回写外部凭据.md

use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use super::{Discovery, Secret, home_dir, jwt, non_empty, replace_json_atomically};

/// Gemini CLI 的公开 client id（不是密钥，是公开标识）。
const GEMINI_CLIENT_ID: &str =
    "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
/// Antigravity 原生 client id（部分新版 jetski 使用）。
const ANTIGRAVITY_CLIENT_ID: &str =
    "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
/// 到期前多久就开始续期。
const REFRESH_SKEW_SECS: i64 = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntigravitySource {
    /// `~/.gemini/oauth_creds.json`。
    OAuthCreds,
    /// `~/.gemini/jetski-standalone-oauth-token`。
    Jetski,
}

impl AntigravitySource {
    pub fn key(self) -> &'static str {
        match self {
            Self::OAuthCreds => "oauthCreds",
            Self::Jetski => "jetski",
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct AntigravityAccount {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub plan: Option<String>,
    pub access_token: Option<Secret>,
    pub refresh_token: Option<Secret>,
    pub expires_at: Option<DateTime<Utc>>,
    pub id_token: Option<Secret>,
    pub source: AntigravitySource,
}

/// 手动实现：只暴露来源与是否拿到各字段，令牌与 id token 都不进门。
impl std::fmt::Debug for AntigravityAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AntigravityAccount")
            .field("source", &self.source.key())
            .field("has_email", &self.email.is_some())
            .field("has_access_token", &self.access_token.is_some())
            .field("has_refresh_token", &self.refresh_token.is_some())
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl AntigravityAccount {
    /// 是否需要在本次刷新前续期。没有到期时刻时按「需要续期」处理：
    /// jetski 的老版本可能不写 `expiry`，宁可多续一次也不要拿着过期令牌发请求。
    pub fn is_expired(&self, now: DateTime<Utc>) -> bool {
        match self.expires_at {
            Some(expires_at) => (expires_at - now).num_seconds() < REFRESH_SKEW_SECS,
            None => true,
        }
    }

    /// 有凭据可用：access token 或能续期的 refresh token 至少有一个。
    fn is_present(&self) -> bool {
        self.access_token.is_some() || self.refresh_token.is_some()
    }
}

pub fn credentials_paths() -> Option<(PathBuf, PathBuf)> {
    let home = home_dir()?;
    let gemini = home.join(".gemini");
    Some((
        gemini.join("oauth_creds.json"),
        gemini.join("jetski-standalone-oauth-token"),
    ))
}

/// 按「jetski 优先、oauth_creds 兜底」读取账号。
pub fn discover() -> Discovery<AntigravityAccount> {
    let Some((oauth_path, jetski_path)) = credentials_paths() else {
        return Discovery::Unsupported;
    };

    let jetski = read_source(&jetski_path, AntigravitySource::Jetski);
    let oauth = read_source(&oauth_path, AntigravitySource::OAuthCreds);

    // 读取失败（有文件但解不开）优先上报：那说明凭据可能还在，只是我们读不懂，
    // 直接说「没有登录」会把用户引向错误的重登操作。
    if jetski == ReadOutcome::Unreadable || oauth == ReadOutcome::Unreadable {
        return Discovery::Unreadable;
    }

    match (jetski, oauth) {
        (ReadOutcome::Found(account), _) => Discovery::Found(account),
        (_, ReadOutcome::Found(account)) => Discovery::Found(account),
        _ => Discovery::Missing,
    }
}

#[derive(PartialEq, Eq)]
enum ReadOutcome {
    Found(AntigravityAccount),
    Missing,
    Unreadable,
}

fn read_source(path: &Path, source: AntigravitySource) -> ReadOutcome {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return ReadOutcome::Missing;
    };
    if raw.trim().is_empty() {
        return ReadOutcome::Missing;
    }
    let Ok(root) = serde_json::from_str::<Value>(&raw) else {
        return ReadOutcome::Unreadable;
    };

    let account = match source {
        AntigravitySource::Jetski => parse_jetski(&root),
        AntigravitySource::OAuthCreds => parse_oauth_creds(&root),
    };
    match account {
        Some(account) if account.is_present() => ReadOutcome::Found(account),
        Some(_) => ReadOutcome::Missing,
        None => ReadOutcome::Unreadable,
    }
}

/// jetski 结构：`{ "token": { "access_token", "refresh_token", "expiry" } }`，也可能是平铺。
fn parse_jetski(root: &Value) -> Option<AntigravityAccount> {
    let token = root.get("token").unwrap_or(root);
    if !token.is_object() {
        return None;
    }

    let access_token = field(token, &["access_token", "accessToken"]).map(Secret::new);
    let refresh_token = field(token, &["refresh_token", "refreshToken"]).map(Secret::new);
    let expires_at = field(token, &["expiry"])
        .and_then(|value| DateTime::parse_from_rfc3339(value.trim()).ok())
        .map(|time| time.with_timezone(&Utc));

    // jetski 的 access token 是不透明字符串（不是 JWT），本地解不出邮箱；
    // 账号展示依赖 loadCodeAssist 回填。
    let email = field(root, &["email"]);

    Some(AntigravityAccount {
        email,
        display_name: field(root, &["displayName", "name"]),
        plan: field(root, &["planType", "plan"]),
        access_token,
        refresh_token,
        expires_at,
        id_token: None,
        source: AntigravitySource::Jetski,
    })
}

/// `oauth_creds.json`：`{ access_token, refresh_token, id_token, expiry_date(毫秒) }`。
fn parse_oauth_creds(root: &Value) -> Option<AntigravityAccount> {
    if !root.is_object() {
        return None;
    }

    let id_token = field(root, &["id_token"]).map(Secret::new);
    let claims = id_token
        .as_ref()
        .and_then(|token| jwt::decode_payload(token.expose()));

    Some(AntigravityAccount {
        email: claims.as_ref().and_then(|claims| claim(claims, "email")),
        display_name: claims
            .as_ref()
            .and_then(|claims| claim(claims, "name").or_else(|| claim(claims, "given_name"))),
        plan: None,
        access_token: field(root, &["access_token"]).map(Secret::new),
        refresh_token: field(root, &["refresh_token"]).map(Secret::new),
        expires_at: root.get("expiry_date").and_then(as_millis),
        id_token,
        source: AntigravitySource::OAuthCreds,
    })
}

fn claim(claims: &Value, name: &str) -> Option<String> {
    non_empty(claims.get(name).and_then(Value::as_str))
}

fn field(value: &Value, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| non_empty(value.get(*name).and_then(Value::as_str)))
}

/// `expiry_date` 是毫秒时间戳（网络值可能带小数，`Value` 里会变成浮点数）。
fn as_millis(value: &Value) -> Option<DateTime<Utc>> {
    let raw = match value {
        Value::Number(number) => number.as_f64()?,
        Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !raw.is_finite() || raw <= 0.0 {
        return None;
    }
    chrono::TimeZone::timestamp_millis_opt(&Utc, raw as i64).single()
}

/// 一次续期尝试的候选 `(client_id, client_secret?)`。
///
/// 顺序：两个官方 client id 各自搭配从本机提取到的密钥；提取到的每个未配对密钥
/// 再各试一遍两个 client id。没有密钥的候选也保留——部分环境改用 PKCE、
/// 不带 secret 也能续期。
pub fn refresh_candidates() -> Vec<(String, Option<String>)> {
    refresh_candidates_with(&extract_local_client_secrets())
}

/// 在阻塞线程上提取并构造候选：扫本机 bundle 可能耗时数百毫秒到数秒，
/// 不能占着异步执行器。
pub async fn refresh_candidates_blocking() -> Vec<(String, Option<String>)> {
    tokio::task::spawn_blocking(refresh_candidates)
        .await
        .unwrap_or_else(|_| {
            // 阻塞任务 panic（磁盘异常等）：退回「没有密钥」的两个官方 client 候选。
            refresh_candidates_with(&[])
        })
}

/// 候选构造的纯函数形态：便于用注入的密钥断言顺序与配对，不去扫本机文件。
pub fn refresh_candidates_with(secrets: &[(String, String)]) -> Vec<(String, Option<String>)> {
    let paired_gemini = secrets
        .iter()
        .find(|(client_id, _)| client_id == GEMINI_CLIENT_ID)
        .map(|(_, secret)| secret.clone());
    let paired_antigravity = secrets
        .iter()
        .find(|(client_id, _)| client_id == ANTIGRAVITY_CLIENT_ID)
        .map(|(_, secret)| secret.clone());

    let mut candidates = vec![
        (GEMINI_CLIENT_ID.to_owned(), paired_gemini),
        (ANTIGRAVITY_CLIENT_ID.to_owned(), paired_antigravity),
    ];
    for (_, secret) in secrets {
        for client_id in [GEMINI_CLIENT_ID, ANTIGRAVITY_CLIENT_ID] {
            let candidate = (client_id.to_owned(), Some(secret.clone()));
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

/// 未配对的密钥集合（供诊断说明「本机找到了几份官方密钥」用，不含密钥内容）。
pub fn local_secret_count() -> usize {
    extract_local_client_secrets().len()
}

/// 从本机官方组件里提取 `(client_id, client_secret)` 对。
///
/// 只扫一遍：官方 bundle 有几十 MB，凭据不到期就不会走到这里，但同一进程内
/// 第二次续期不该再扫一次（`OnceLock` 缓存）。
fn extract_local_client_secrets() -> Vec<(String, String)> {
    static CACHE: std::sync::OnceLock<Vec<(String, String)>> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let mut pairs: Vec<(String, String)> = Vec::new();
            for text in local_tool_texts() {
                for (client_id, secret) in pair_secrets(&text) {
                    if !pairs.iter().any(|(existing_id, existing_secret)| {
                        existing_id == &client_id && existing_secret == &secret
                    }) {
                        pairs.push((client_id, secret));
                    }
                }
                // 两个官方 client 都配上了就不必再读下去。
                if pairs.iter().any(|(id, _)| id == GEMINI_CLIENT_ID)
                    && pairs.iter().any(|(id, _)| id == ANTIGRAVITY_CLIENT_ID)
                {
                    break;
                }
            }
            pairs
        })
        .clone()
}

/// 单次提取允许读取的总字节上限。官方 bundle 可能很大，但两份密钥总是相邻的，
/// 读满上限仍找不到就按「没有密钥」处理，不让续期卡在一次磁盘全扫上。
const SCAN_BYTE_BUDGET: u64 = 48 * 1024 * 1024;
/// 单个文件的大小上限。
const SCAN_FILE_LIMIT: u64 = 32 * 1024 * 1024;

/// 官方组件里的候选文本。读取失败一律跳过：提取密钥只是续期的准备工作，
/// 拿不到就走「没有密钥」的候选，不影响额度本身的读取。
fn local_tool_texts() -> Vec<String> {
    let mut texts = Vec::new();
    let mut budget = SCAN_BYTE_BUDGET;
    let Some(home) = home_dir() else {
        return texts;
    };

    let mut directories = vec![
        home.join(".gemini/node_modules/@google/gemini-cli/bundle"),
        PathBuf::from("/opt/homebrew/lib/node_modules/@google/gemini-cli/bundle"),
        PathBuf::from("/usr/local/lib/node_modules/@google/gemini-cli/bundle"),
        PathBuf::from("/opt/homebrew/Cellar/gemini-cli"),
    ];
    // Windows 上 npm 全局包通常装在 `%APPDATA%\npm\node_modules`。
    if let Some(roaming) = std::env::var_os("APPDATA") {
        directories.push(PathBuf::from(roaming).join("npm/node_modules/@google/gemini-cli/bundle"));
    }

    for directory in directories {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("js") {
                continue;
            }
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            let size = metadata.len();
            if size > SCAN_FILE_LIMIT || size > budget {
                continue;
            }
            budget = budget.saturating_sub(size);
            if let Ok(raw) = std::fs::read(&path) {
                texts.push(latin1(&raw));
            }
        }
    }

    for path in [
        home.join(".gemini/bin/agy"),
        home.join(".gemini/antigravity/bin/agy"),
    ] {
        let Ok(metadata) = std::fs::metadata(&path) else {
            continue;
        };
        if metadata.len() > SCAN_FILE_LIMIT || metadata.len() > budget {
            continue;
        }
        budget = budget.saturating_sub(metadata.len());
        if let Ok(raw) = std::fs::read(&path) {
            texts.push(latin1(&raw));
        }
    }

    texts
}

/// 按 Latin-1 转文本：保留全部字节，二进制里跨块相邻的 client id 与密钥才能就近配对。
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| char::from(*byte)).collect()
}

/// 扫描文本里的 client id 与 `GOCSPX-` 密钥，按就近原则配对。
///
/// 窗口与 cc-bar 一致（512 字节内才配对，**不做顺序硬配对**，配不上的密钥仍保留给
/// 调用方逐个 client 试）。唯一的差别是选择顺序：优先取**紧邻在密钥之前**的那个 id，
/// 没有才用后面最近的。真实 bundle 里 id 与 secret 是相邻的两次赋值，cc-bar 的纯距离
/// 判据在这种布局下偶尔会把下一个 client 的 id 配给上一个 secret。
fn pair_secrets(text: &str) -> Vec<(String, String)> {
    let ids = scan_client_ids(text);
    let secrets = scan_secrets(text);

    let mut pairs = Vec::new();
    let mut used_ids: Vec<usize> = Vec::new();
    for (secret_offset, secret) in &secrets {
        let mut preceding: Option<(usize, usize)> = None;
        let mut following: Option<(usize, usize)> = None;
        for (index, (id_offset, _)) in ids.iter().enumerate() {
            if used_ids.contains(&index) {
                continue;
            }
            let distance = id_offset.abs_diff(*secret_offset);
            if distance >= 512 {
                continue;
            }
            if *id_offset <= *secret_offset {
                if preceding.is_none_or(|(_, best)| distance < best) {
                    preceding = Some((index, distance));
                }
            } else if following.is_none_or(|(_, best)| distance < best) {
                following = Some((index, distance));
            }
        }
        if let Some((index, _)) = preceding.or(following) {
            used_ids.push(index);
            pairs.push((ids[index].1.clone(), secret.clone()));
        }
    }
    pairs
}

/// 扫描 `<数字>-<小写字母数字>.apps.googleusercontent.com` 形态的 client id。
fn scan_client_ids(text: &str) -> Vec<(usize, String)> {
    const SUFFIX: &str = ".apps.googleusercontent.com";
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut cursor = 0;

    while let Some(position) = text[cursor..].find(SUFFIX) {
        let suffix_start = cursor + position;
        // 向前回退到 token 边界。
        let mut start = suffix_start;
        while start > 0 {
            let byte = bytes[start - 1];
            if byte.is_ascii_alphanumeric() || byte == b'-' {
                start -= 1;
            } else {
                break;
            }
        }
        let candidate = &text[start..suffix_start];
        if let Some((digits, rest)) = candidate.split_once('-')
            && digits.len() >= 10
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && !rest.is_empty()
        {
            found.push((start, text[start..suffix_start + SUFFIX.len()].to_owned()));
        }
        cursor = suffix_start + SUFFIX.len();
    }

    found
}

/// 扫描 `GOCSPX-` 密钥，遇到 `http` 视为粘连尾缀并截断。
fn scan_secrets(text: &str) -> Vec<(usize, String)> {
    const PREFIX: &str = "GOCSPX-";
    let mut found = Vec::new();
    let mut cursor = 0;

    while let Some(position) = text[cursor..].find(PREFIX) {
        let start = cursor + position + PREFIX.len();
        let mut end = start;
        let bytes = text.as_bytes();
        while end < bytes.len() {
            let byte = bytes[end];
            if byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' {
                end += 1;
            } else {
                break;
            }
        }
        let body = &text[start..end];
        if body.len() >= 20 {
            let clean = match body.find("http") {
                Some(index) => &body[..index],
                None => body,
            };
            if clean.len() >= 20 {
                found.push((position + cursor, format!("{PREFIX}{clean}")));
            }
        }
        cursor = end.max(start);
    }

    found
}

/// 续期成功后的令牌三元组。
#[derive(Clone, PartialEq, Eq)]
pub struct RefreshedAntigravityTokens {
    pub access_token: Secret,
    pub refresh_token: Option<Secret>,
    pub expires_at: DateTime<Utc>,
}

/// 续期：依次尝试候选 `(client_id, secret)`，第一个成功即返回。
///
/// 失败原因分三类，调用方据此决定退避与文案：401/403 是凭据问题，
/// 5xx 与网络问题属于服务端，其余按协议错误。
pub enum RefreshFailure {
    /// 所有候选都被拒（invalid_grant / unauthorized_client）。
    Rejected,
    /// 网络或服务端问题。
    Transport,
    /// 响应结构不认识。
    Protocol,
}

pub async fn refresh(
    refresh_token: &Secret,
    now: DateTime<Utc>,
) -> Result<RefreshedAntigravityTokens, RefreshFailure> {
    use crate::providers::http;

    let mut last = RefreshFailure::Rejected;
    for (client_id, client_secret) in refresh_candidates_blocking().await {
        let mut form = vec![
            ("client_id", client_id.clone()),
            ("grant_type", "refresh_token".to_owned()),
            ("refresh_token", refresh_token.expose().to_owned()),
        ];
        if let Some(client_secret) = client_secret {
            form.push(("client_secret", client_secret));
        }

        let response = match http::client().post(TOKEN_ENDPOINT).form(&form).send().await {
            Ok(response) => response,
            Err(_) => {
                last = RefreshFailure::Transport;
                continue;
            }
        };

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            // 400/401/403：这个候选的 client 与凭据不匹配，换下一个；
            // 429/5xx：服务端问题，继续换候选也没意义，但为了不漏掉
            // 「某个 client 刚好可用」的情况仍然继续，最后按最严重的原因上报。
            last = if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                RefreshFailure::Transport
            } else {
                last
            };
            continue;
        }

        let Ok(root) = serde_json::from_str::<Value>(&body) else {
            last = RefreshFailure::Protocol;
            continue;
        };
        let Some(access_token) = field(&root, &["access_token"]) else {
            last = RefreshFailure::Protocol;
            continue;
        };
        let expires_in = root
            .get("expires_in")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && *value > 0.0)
            .unwrap_or(3600.0);

        return Ok(RefreshedAntigravityTokens {
            access_token: Secret::new(access_token),
            refresh_token: field(&root, &["refresh_token"]).map(Secret::new),
            expires_at: now + chrono::Duration::seconds(expires_in as i64),
        });
    }

    Err(last)
}

/// 把续期结果原子回写读到的那个文件，只改 token 字段，保留文件里的其他内容。
pub fn write_back(
    account: &AntigravityAccount,
    refreshed: &RefreshedAntigravityTokens,
) -> std::io::Result<()> {
    let Some((oauth_path, jetski_path)) = credentials_paths() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "antigravity credential path is not resolvable",
        ));
    };
    let path = match account.source {
        AntigravitySource::Jetski => jetski_path,
        AntigravitySource::OAuthCreds => oauth_path,
    };

    let raw = std::fs::read_to_string(&path)?;
    let mut root: Value = serde_json::from_str(&raw)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

    match account.source {
        AntigravitySource::Jetski => {
            let target = if root.get("token").is_some_and(Value::is_object) {
                root.get_mut("token")
            } else {
                Some(&mut root)
            };
            let Some(target) = target else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "jetski token object is missing",
                ));
            };
            set_field(target, "access_token", refreshed.access_token.expose());
            if let Some(refresh_token) = refreshed.refresh_token.as_ref() {
                set_field(target, "refresh_token", refresh_token.expose());
            }
            set_field(
                target,
                "expiry",
                &refreshed
                    .expires_at
                    .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            );
        }
        AntigravitySource::OAuthCreds => {
            set_field(&mut root, "access_token", refreshed.access_token.expose());
            if let Some(refresh_token) = refreshed.refresh_token.as_ref() {
                set_field(&mut root, "refresh_token", refresh_token.expose());
            }
            if let Some(object) = root.as_object_mut() {
                object.insert(
                    "expiry_date".to_owned(),
                    Value::Number(refreshed.expires_at.timestamp_millis().into()),
                );
            }
        }
    }

    replace_json_atomically(&path, &root)
}

fn set_field(value: &mut Value, name: &str, text: &str) {
    if let Some(object) = value.as_object_mut() {
        let mut map: Map<String, Value> = Map::new();
        map.insert(name.to_owned(), Value::String(text.to_owned()));
        for (key, value) in map {
            object.insert(key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fetched_at() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .expect("valid")
            .with_timezone(&Utc)
    }

    #[test]
    fn jetski_tokens_are_read_from_the_nested_object() {
        let root = serde_json::json!({
            "token": {
                "access_token": "ya29.access",
                "refresh_token": "1//refresh",
                "expiry": "2026-10-01T13:00:00Z"
            }
        });

        let account = parse_jetski(&root).expect("parses");
        assert_eq!(account.source, AntigravitySource::Jetski);
        assert!(account.access_token.is_some());
        assert_eq!(
            account.expires_at,
            Some(
                DateTime::parse_from_rfc3339("2026-10-01T13:00:00Z")
                    .expect("valid")
                    .with_timezone(&Utc)
            )
        );
    }

    #[test]
    fn flat_jetski_payloads_are_accepted_too() {
        let root = serde_json::json!({ "access_token": "ya29.flat" });
        let account = parse_jetski(&root).expect("parses");
        assert!(account.access_token.is_some());
        assert!(account.expires_at.is_none());
        // 没有到期时刻按「需要续期」处理，避免拿着过期令牌发请求。
        assert!(account.is_expired(fetched_at()));
    }

    #[test]
    fn oauth_creds_reads_identity_from_the_id_token() {
        use super::super::jwt::base64url_encode;

        let id_token = format!(
            "{}.{}.signature",
            base64url_encode(br#"{"alg":"none"}"#),
            base64url_encode(
                serde_json::json!({"email": "user@example.com", "name": "用户"})
                    .to_string()
                    .as_bytes(),
            ),
        );
        let root = serde_json::json!({
            "access_token": "ya29.oauth",
            "refresh_token": "1//oauth",
            "id_token": id_token,
            "expiry_date": 1_790_000_000_000_u64
        });

        let account = parse_oauth_creds(&root).expect("parses");
        assert_eq!(account.email.as_deref(), Some("user@example.com"));
        assert_eq!(account.display_name.as_deref(), Some("用户"));
        assert_eq!(
            account.expires_at,
            Some(
                DateTime::parse_from_rfc3339("2026-09-21T14:13:20Z")
                    .expect("valid")
                    .with_timezone(&Utc)
            )
        );
    }

    #[test]
    fn a_token_with_two_minutes_left_needs_refreshing() {
        let account = AntigravityAccount {
            email: None,
            display_name: None,
            plan: None,
            access_token: Some(Secret::new("ya29.a")),
            refresh_token: Some(Secret::new("1//r")),
            expires_at: Some(fetched_at() + chrono::Duration::minutes(2)),
            id_token: None,
            source: AntigravitySource::Jetski,
        };
        assert!(account.is_expired(fetched_at()));

        let fresh = AntigravityAccount {
            expires_at: Some(fetched_at() + chrono::Duration::minutes(30)),
            ..account
        };
        assert!(!fresh.is_expired(fetched_at()));
    }

    #[test]
    fn client_ids_are_scanned_with_their_boundaries() {
        let text = "x=1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com;y";
        let ids = scan_client_ids(text);
        assert_eq!(ids.len(), 1);
        assert!(ids[0].1.starts_with("1071006060591-"));
        assert!(ids[0].1.ends_with(".apps.googleusercontent.com"));
    }

    #[test]
    fn secrets_are_scanned_and_truncated_at_glued_suffixes() {
        let text = "a=GOCSPX-K58Fabcdefghijklmnopqrstuvwxyz9YQWhttps://example";
        let secrets = scan_secrets(text);
        assert_eq!(secrets.len(), 1);
        assert_eq!(secrets[0].1, "GOCSPX-K58Fabcdefghijklmnopqrstuvwxyz9YQW");
    }

    #[test]
    fn short_secrets_are_not_accepted() {
        assert!(scan_secrets("GOCSPX-tooshort").is_empty());
    }

    #[test]
    fn secrets_pair_with_the_nearest_client_id_within_the_window() {
        let text = format!(
            "{}:GOCSPX-aaaaaaaaaaaaaaaaaaaaaaaaaa:x:{}:GOCSPX-bbbbbbbbbbbbbbbbbbbbbbbbbb",
            "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com",
            "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com"
        );
        let pairs = pair_secrets(&text);

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].0, GEMINI_CLIENT_ID);
        assert_eq!(pairs[0].1, "GOCSPX-aaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(pairs[1].0, ANTIGRAVITY_CLIENT_ID);
        assert_eq!(pairs[1].1, "GOCSPX-bbbbbbbbbbbbbbbbbbbbbbbbbb");
    }

    #[test]
    fn a_secret_before_any_client_id_pairs_with_the_following_one() {
        let text = format!("GOCSPX-aaaaaaaaaaaaaaaaaaaaaaaaaa:{}", GEMINI_CLIENT_ID);
        let pairs = pair_secrets(&text);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].0, GEMINI_CLIENT_ID);
    }

    #[test]
    fn a_distant_secret_is_not_paired() {
        let far = "x".repeat(4096);
        let text = format!(
            "{}:{}GOCSPX-aaaaaaaaaaaaaaaaaaaaaaaaaa",
            "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com", far
        );
        assert!(pair_secrets(&text).is_empty());
    }

    #[test]
    fn candidates_always_include_both_official_clients() {
        let candidates = refresh_candidates_with(&[]);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0], (GEMINI_CLIENT_ID.to_owned(), None));
        assert_eq!(candidates[1], (ANTIGRAVITY_CLIENT_ID.to_owned(), None));
    }

    #[test]
    fn a_paired_secret_is_used_with_its_own_client_first() {
        let candidates = refresh_candidates_with(&[(
            ANTIGRAVITY_CLIENT_ID.to_owned(),
            "GOCSPX-secret-for-antigravity1".to_owned(),
        )]);

        assert_eq!(candidates[0], (GEMINI_CLIENT_ID.to_owned(), None));
        assert_eq!(
            candidates[1],
            (
                ANTIGRAVITY_CLIENT_ID.to_owned(),
                Some("GOCSPX-secret-for-antigravity1".to_owned())
            )
        );
    }

    #[test]
    fn an_unpaired_secret_is_tried_against_both_clients() {
        let candidates = refresh_candidates_with(&[(
            "unmatched".to_owned(),
            "GOCSPX-orphan-secret-value".to_owned(),
        )]);

        assert!(candidates.contains(&(
            GEMINI_CLIENT_ID.to_owned(),
            Some("GOCSPX-orphan-secret-value".to_owned())
        )));
        assert!(candidates.contains(&(
            ANTIGRAVITY_CLIENT_ID.to_owned(),
            Some("GOCSPX-orphan-secret-value".to_owned())
        )));
    }

    #[test]
    fn scanning_the_machine_never_panics_without_official_tools() {
        // 本机没装 Gemini CLI 时结果是 0；这里只断言调用本身安全。
        let _ = local_secret_count();
    }

    #[test]
    fn writing_back_keeps_unrelated_fields_and_updates_tokens() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("jetski-standalone-oauth-token");
        std::fs::write(
            &path,
            r#"{"token":{"access_token":"old","refresh_token":"old-r","expiry":"2020-01-01T00:00:00Z"},"other":"keep"}"#,
        )
        .expect("write");

        let raw = std::fs::read_to_string(&path).expect("read");
        let mut root: Value = serde_json::from_str(&raw).expect("parse");
        let target = root.get_mut("token").expect("token");
        set_field(target, "access_token", "new");
        set_field(target, "expiry", "2026-10-01T13:00:00.000Z");
        replace_json_atomically(&path, &root).expect("write back");

        let updated = std::fs::read_to_string(&path).expect("read back");
        assert!(updated.contains("\"access_token\": \"new\""));
        assert!(updated.contains("\"other\": \"keep\""));
        assert!(updated.contains("2026-10-01T13:00:00.000Z"));
    }

    #[test]
    fn an_account_never_prints_its_tokens() {
        let account = AntigravityAccount {
            email: Some("user@example.com".to_owned()),
            display_name: None,
            plan: None,
            access_token: Some(Secret::new("ya29.secret-value")),
            refresh_token: Some(Secret::new("1//secret-refresh")),
            expires_at: None,
            id_token: None,
            source: AntigravitySource::Jetski,
        };
        let printed = format!("{account:?}");
        assert!(!printed.contains("ya29.secret-value"));
        assert!(!printed.contains("1//secret-refresh"));
    }
}
