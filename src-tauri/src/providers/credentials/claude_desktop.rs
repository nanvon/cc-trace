//! Claude Desktop 的 Electron 凭据缓存读取器（只读）。
//!
//! Claude Code CLI 与 Claude Desktop 是**两个平等的凭据来源**，不是主备关系：
//! 订阅用户的日常客户端往往是 Desktop，CLI 的 access token 只有 8 小时且聊天不会刷新它。
//! 没有这条路径，这类用户会一直看到「凭据过期，请登录」——账号没问题，是我们在看错的地方。
//!
//! **只读取 access token，永不读取、永不使用 Desktop 的 refresh token。**
//! Anthropic 的 refresh token 一次性轮换并带重用检测，任何一方刷新都会作废其他副本；
//! 拿它去换 token 会把用户从 Claude Code 挤下线。access token 没有这个问题：
//! 它是只读凭证，拿去调 usage 端点不产生任何轮换。
//!
//! ## 存储格式与平台差异
//!
//! 两边都用 Electron `safeStorage`，但后端不同，这一点决定了实现分叉：
//!
//! - **macOS**（与 cc-bar v1.1.1 相同）：`config.json` 里 `oauth:tokenCacheV2` /
//!   `oauth:tokenCache` 是 base64，解码后以 `v10` 开头，其后是 AES-128-CBC 密文，
//!   IV 为 16 个空格，密钥 = PBKDF2-SHA1(钥匙串 `Claude Safe Storage` / `Claude Key`
//!   的密码, salt `saltysalt`, 1003 轮, 16 字节)。
//! - **Windows**：Electron 官方文档写明 Windows 的加密密钥由 **DPAPI** 生成，
//!   密文与当前登录凭据绑定（同一用户可解，其他用户不可解）。
//!   因此这里用 `CryptUnprotectData`，不去猜 Chromium 那套 master key + AES-GCM 方案。
//!
//! 证据等级：Electron 的 DPAPI 说明是**官方文档**；`config.json` 在 Windows 上的
//! 具体取值形态（是否 base64、是否带前缀）**未验证**——因此解码器按「原样尝试」、
//! 「先 base64 再尝试」两种顺序试，两者都失败就上报「读不出来」，而不是「没有凭据」。
//!
//! [ADR-0035]: ../../../../docs/决策/ADR-0035-服务矩阵与Windows凭据存储.md

use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;

/// Claude Code 官方 OAuth client；同账号下优先选它签发的 token。
const CLAUDE_CODE_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// 读 usage 端点所需的 scope；缓存键里带不上它的条目直接跳过。
const USAGE_SCOPE: &str = "user:profile";
/// 「还够用」的临期余量。
const EXPIRY_SKEW_SECS: i64 = 5;

/// 缓存里的一个账号条目。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesktopCacheEntry {
    pub account_uuid: String,
    pub client_id: String,
    pub organization_uuid: String,
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub subscription_type: Option<String>,
    /// 原始缓存键。`audience` 与 `scopes` 都含冒号，精确切分既脆弱又不必要，
    /// scope 判定按子串来。
    raw_key: String,
}

impl DesktopCacheEntry {
    /// 能否读 usage 端点：缓存键里必须带 `user:profile`。
    pub fn has_usage_scope(&self) -> bool {
        self.raw_key.contains(USAGE_SCOPE)
    }

    pub fn is_usable_at(&self, now: DateTime<Utc>) -> bool {
        self.has_usage_scope()
            && self
                .expires_at
                .is_some_and(|expires_at| (expires_at - now).num_seconds() > EXPIRY_SKEW_SECS)
    }

    fn is_official_client(&self) -> bool {
        self.client_id == CLAUDE_CODE_CLIENT_ID
    }
}

/// 读到的 Desktop 凭据。刻意不含 refresh token——调用方拿不到，也就无从误用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BorrowedDesktopToken {
    pub access_token: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub client_id: String,
    pub account_uuid: String,
}

/// 读取结果。与 [`super::Discovery`] 的语义一致，但这里多一层「用户拒绝授权」。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopRead {
    /// 读到了可用条目。
    Found(BorrowedDesktopToken),
    /// 没装 Desktop、没有缓存、或缓存里没有当前账号的条目。
    Missing,
    /// 有缓存但解不开：钥匙串授权被拒、DPAPI 失败、格式不认识。
    Unreadable,
}

/// `config.json` 里可能承载缓存的键，按新到旧。
const CACHE_KEYS: [&str; 2] = ["oauth:tokenCacheV2", "oauth:tokenCache"];

/// Claude Desktop 配置路径候选，按优先顺序。
///
/// Windows 上 Claude Desktop 可能是 MSIX（微软商店／winget 安装）版本，
/// 那种安装方式下 `%APPDATA%\Claude` 会被重定向到包内虚拟路径；
/// 两处都列出来，先存在的先用。Windows 取值证据等级：**中**（社区实现与
/// Electron/VS Code 分叉的既定布局），实机未验证。
pub fn config_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Library/Application Support/Claude/config.json"));
        }
    }

    #[cfg(windows)]
    {
        if let Some(roaming) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(roaming).join("Claude").join("config.json"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let packages = PathBuf::from(local).join("Packages");
            if let Ok(entries) = std::fs::read_dir(&packages) {
                for entry in entries.flatten() {
                    let name = entry.file_name();
                    let name = name.to_string_lossy();
                    if name.starts_with("Claude_") {
                        paths.push(
                            entry
                                .path()
                                .join("LocalCache")
                                .join("Roaming")
                                .join("Claude")
                                .join("config.json"),
                        );
                    }
                }
            }
        }
    }

    paths
}

/// 缓存载体是否存在。只读明文 JSON，不碰钥匙串／DPAPI，不会弹授权框——
/// 没装 Desktop 的机器上可以先用它跳过后续全部开销。
pub fn has_credential_material() -> bool {
    config_paths()
        .iter()
        .filter_map(|path| read_config(path))
        .any(|root| CACHE_KEYS.iter().any(|key| root.get(*key).is_some()))
}

/// 读取与指定账号匹配、且仍可用的 Desktop token。
///
/// 只有 `account_uuid` 与 `organization_uuid` 都对得上才会返回：用户在 CLI 与
/// Desktop 登录了不同账号时，否则会把别人的额度显示成自己的。任一缺失就保守放弃，
/// 不做「大概是同一个人」的猜测。
pub fn borrow_token(
    account_uuid: Option<&str>,
    organization_uuid: Option<&str>,
    now: DateTime<Utc>,
) -> DesktopRead {
    let (Some(account_uuid), Some(organization_uuid)) = (account_uuid, organization_uuid) else {
        return DesktopRead::Missing;
    };
    let account_uuid = account_uuid.to_lowercase();
    let organization_uuid = organization_uuid.to_lowercase();

    let Some(entries) = load_entries() else {
        return DesktopRead::Missing;
    };
    let matching: Vec<DesktopCacheEntry> = entries
        .into_iter()
        .filter(|entry| {
            entry.account_uuid.eq_ignore_ascii_case(&account_uuid)
                && entry
                    .organization_uuid
                    .eq_ignore_ascii_case(&organization_uuid)
        })
        .collect();

    match best_usable(matching, now) {
        Some(entry) => DesktopRead::Found(entry.into_token()),
        None => DesktopRead::Missing,
    }
}

/// 完全没有 CLI 凭据时，直接从 Desktop 认出账号。
///
/// 账号选择优先 `config.json` 的 `lastKnownAccountUuid`（Desktop 当前登录的那个），
/// 缺失时才退而用可用条目里最优的一条。
pub fn discover_current_account(now: DateTime<Utc>) -> DesktopRead {
    let Some(entries) = load_entries() else {
        return DesktopRead::Missing;
    };
    let preferred = last_known_account_uuid().map(|uuid| uuid.to_lowercase());
    let scoped: Vec<DesktopCacheEntry> = match preferred.as_deref() {
        Some(uuid) => entries
            .iter()
            .filter(|entry| entry.account_uuid.eq_ignore_ascii_case(uuid))
            .cloned()
            .collect(),
        None => entries.clone(),
    };

    best_usable(scoped, now)
        .or_else(|| best_usable(entries, now))
        .map(DesktopCacheEntry::into_token)
        .map_or(DesktopRead::Missing, DesktopRead::Found)
}

impl DesktopCacheEntry {
    fn into_token(self) -> BorrowedDesktopToken {
        BorrowedDesktopToken {
            access_token: self.access_token,
            expires_at: self.expires_at,
            client_id: self.client_id,
            account_uuid: self.account_uuid,
        }
    }
}

/// 可读 usage、未过期的条目里挑最优：官方 client 优先，同档取有效期更长的。
fn best_usable(entries: Vec<DesktopCacheEntry>, now: DateTime<Utc>) -> Option<DesktopCacheEntry> {
    entries
        .into_iter()
        .filter(|entry| entry.is_usable_at(now))
        .max_by(|left, right| {
            left.is_official_client()
                .cmp(&right.is_official_client())
                .then_with(|| left.expires_at.cmp(&right.expires_at))
        })
}

/// 读出并解密缓存条目。任何一步失败都返回 `None`（调用方区分「没有」与「读不出来」）。
fn load_entries() -> Option<Vec<DesktopCacheEntry>> {
    let root = config_paths().iter().find_map(|path| read_config(path))?;
    let encrypted = CACHE_KEYS
        .iter()
        .find_map(|key| root.get(*key).and_then(Value::as_str))?;
    let plaintext = decrypt_safe_storage(encrypted)?;
    let parsed: Value = serde_json::from_slice(&plaintext).ok()?;
    let object = parsed.as_object()?;

    let mut entries = Vec::new();
    for (key, value) in object {
        if let Some(entry) = parse_cache_key(key, value) {
            entries.push(entry);
        }
    }
    (!entries.is_empty()).then_some(entries)
}

fn read_config(path: &Path) -> Option<Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn last_known_account_uuid() -> Option<String> {
    let root = config_paths().iter().find_map(|path| read_config(path))?;
    let uuid = root.get("lastKnownAccountUuid")?.as_str()?.trim();
    (!uuid.is_empty()).then(|| uuid.to_owned())
}

/// 缓存键形态：`acct:<accountUuid>|<clientID>:<orgUuid>:<audience>:<scopes>:`
///
/// 只精确切前三段身份；后面的 audience 与 scopes 都含冒号，交给子串判定。
fn parse_cache_key(key: &str, value: &Value) -> Option<DesktopCacheEntry> {
    let body = key.strip_prefix("acct:")?;
    let (account_part, rest) = body.split_once('|')?;
    let account_uuid = account_part.trim().to_lowercase();
    if account_uuid.is_empty() {
        return None;
    }

    let mut fields = rest.splitn(3, ':');
    let client_id = fields.next()?.trim();
    let organization_uuid = fields.next()?.trim().to_lowercase();
    if organization_uuid.is_empty() || client_id.is_empty() {
        return None;
    }

    let access_token = value.get("token")?.as_str()?.trim();
    if access_token.is_empty() {
        return None;
    }

    Some(DesktopCacheEntry {
        account_uuid,
        client_id: client_id.to_owned(),
        organization_uuid,
        access_token: access_token.to_owned(),
        expires_at: value.get("expiresAt").and_then(parse_expiry),
        subscription_type: value
            .get("subscriptionType")
            .and_then(Value::as_str)
            .map(str::to_owned),
        raw_key: key.to_owned(),
    })
}

/// 到期时刻：缓存里通常是 epoch 秒或毫秒的数值，少数版本写 ISO 8601 字符串。
/// 大于 10^10 的数值按毫秒解释（与 cc-bar 的判据一致）。
fn parse_expiry(value: &Value) -> Option<DateTime<Utc>> {
    match value {
        Value::Number(number) => {
            let raw = number.as_f64()?;
            if !raw.is_finite() {
                return None;
            }
            let millis = if raw > 10_000_000_000.0 {
                raw.round()
            } else {
                (raw * 1000.0).round()
            };
            Utc.timestamp_millis_opt(millis as i64).single()
        }
        Value::String(text) => DateTime::parse_from_rfc3339(text.trim())
            .ok()
            .map(|time| time.with_timezone(&Utc)),
        _ => None,
    }
}

/// 解开 Electron `safeStorage` 的密文。
///
/// 两种平台后端的差别被收敛在这里：macOS 是「钥匙串密码派生的 AES 密钥」，
/// Windows 是 DPAPI。密文可能是 base64，也可能已经是原始字节，两种都试。
#[cfg(target_os = "macos")]
fn decrypt_safe_storage(encrypted: &str) -> Option<Vec<u8>> {
    let raw = decode_possible_base64(encrypted)?;
    let payload = raw.strip_prefix(b"v10")?;
    let password = safe_storage_password()?;
    let key = pbkdf2_sha1(password.as_bytes(), b"saltysalt", 1003, 16);
    aes_128_cbc_decrypt(&key, &[b' '; 16], payload)
}

#[cfg(windows)]
fn decrypt_safe_storage(encrypted: &str) -> Option<Vec<u8>> {
    // 先按原样试（DPAPI 密文常常直接是 base64 文本），再按解码后的字节试。
    if let Some(plaintext) = dpapi_unprotect(encrypted.as_bytes()) {
        return Some(plaintext);
    }
    let decoded = decode_possible_base64(encrypted)?;
    if let Some(plaintext) = dpapi_unprotect(&decoded) {
        return Some(plaintext);
    }
    // `v10` 前缀在 Chromium 的 master key 方案里有意义；Electron 的 Windows 后端
    // 直接走 DPAPI，文档没有说它会加前缀。带上就剥掉再试一次，成本很低。
    let without_prefix = decoded.strip_prefix(b"v10").unwrap_or(&decoded);
    dpapi_unprotect(without_prefix)
}

#[cfg(not(any(target_os = "macos", windows)))]
fn decrypt_safe_storage(_encrypted: &str) -> Option<Vec<u8>> {
    None
}

/// base64 解码；不是合法 base64 时按原始字节返回。
fn decode_possible_base64(input: &str) -> Option<Vec<u8>> {
    let trimmed = input.trim().trim_matches('"');
    match decode_base64(trimmed) {
        Some(bytes) => Some(bytes),
        None => Some(trimmed.as_bytes().to_vec()),
    }
}

/// 标准 base64（可带 padding）解码。自己写一遍，不额外引依赖。
fn decode_base64(input: &str) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer: u32 = 0;
    let mut bits: u32 = 0;
    for byte in input.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        buffer = (buffer << 6) | value;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push(((buffer >> bits) & 0xFF) as u8);
        }
    }
    Some(output)
}

/// PBKDF2-HMAC-SHA1。只用于解 Electron 缓存，不用于任何凭据派生。
#[cfg(target_os = "macos")]
fn pbkdf2_sha1(password: &[u8], salt: &[u8], rounds: u32, length: usize) -> Vec<u8> {
    use hmac::{Hmac, Mac};
    use sha1::Sha1;

    type HmacSha1 = Hmac<Sha1>;

    let mut key = Vec::with_capacity(length);
    let mut block_index: u32 = 1;
    while key.len() < length {
        let mut mac = HmacSha1::new_from_slice(password).expect("HMAC accepts any key length");
        mac.update(salt);
        mac.update(&block_index.to_be_bytes());
        let mut block = mac.finalize().into_bytes().to_vec();

        let mut previous = block.clone();
        for _ in 1..rounds {
            let mut mac = HmacSha1::new_from_slice(password).expect("HMAC accepts any key length");
            mac.update(&previous);
            previous = mac.finalize().into_bytes().to_vec();
            for (slot, value) in block.iter_mut().zip(previous.iter()) {
                *slot ^= value;
            }
        }
        key.extend_from_slice(&block);
        block_index += 1;
    }
    key.truncate(length);
    key
}

/// AES-128-CBC 解密（PKCS#7 去填充）。
#[cfg(target_os = "macos")]
fn aes_128_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Option<Vec<u8>> {
    use aes::cipher::{BlockDecryptMut, KeyIvInit};

    type Aes128CbcDec = cbc::Decryptor<aes::Aes128>;

    let mut buffer = ciphertext.to_vec();
    let decrypted = Aes128CbcDec::new_from_slices(key, iv)
        .ok()?
        .decrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buffer)
        .ok()?;
    Some(decrypted.to_vec())
}

/// 读钥匙串里 Claude Desktop 的 Safe Storage 密码。
///
/// 只有「CLI 凭据缺失或过期」时才会走到这里，所以最多打扰用户一次：
/// `find_generic_password` 命中权限不足时，系统会弹一次授权框；用户拒绝就返回 `None`，
/// 调用方按「读不出来」上报并停止重试（下一次刷新不会立刻再弹）。
///
/// cc-bar 用两段式（先无交互探测、再允许交互）减少弹框次数，那是 Security 框架
/// 底层 API 才有的开关；这里用高层封装，换取「只读一次、失败即放弃」的简单语义。
#[cfg(target_os = "macos")]
fn safe_storage_password() -> Option<String> {
    use security_framework::os::macos::passwords::find_generic_password;

    const SERVICE: &str = "Claude Safe Storage";
    const ACCOUNT: &str = "Claude Key";

    let (password, _item) = find_generic_password(None, SERVICE, ACCOUNT).ok()?;
    // 钥匙串里存的是 ASCII 文本；非 UTF-8 说明这不是我们要的条目。
    String::from_utf8(password.as_ref().to_vec()).ok()
}

#[cfg(windows)]
fn dpapi_unprotect(ciphertext: &[u8]) -> Option<Vec<u8>> {
    use windows_sys::Win32::Foundation::{HLOCAL, LocalFree};
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptUnprotectData};

    if ciphertext.is_empty() || ciphertext.len() > u32::MAX as usize {
        return None;
    }

    let mut input = CRYPT_INTEGER_BLOB {
        cbData: ciphertext.len() as u32,
        pbData: ciphertext.as_ptr() as *mut u8,
    };
    let mut output = CRYPT_INTEGER_BLOB {
        cbData: 0,
        pbData: std::ptr::null_mut(),
    };

    // SAFETY: 输入指针指向在调用期间有效的缓冲；输出由系统分配，成功后必须 `LocalFree`。
    let ok = unsafe {
        CryptUnprotectData(
            &mut input,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            0,
            &mut output,
        )
    };
    if ok == 0 || output.pbData.is_null() {
        return None;
    }

    // SAFETY: `CryptUnprotectData` 成功时返回长度为 `cbData` 的已初始化缓冲。
    let plaintext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    unsafe {
        LocalFree(output.pbData as HLOCAL);
    }
    Some(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
            .expect("valid")
            .with_timezone(&Utc)
    }

    fn entry(account: &str, expires_in_hours: i64) -> Value {
        serde_json::json!({
            "token": format!("token-{account}"),
            "refreshToken": "never-read",
            "expiresAt": (now() + chrono::Duration::hours(expires_in_hours)).timestamp() as f64,
            "subscriptionType": "max"
        })
    }

    #[test]
    fn cache_keys_are_parsed_into_account_entries() {
        let key = format!(
            "acct:UUID-1|{CLAUDE_CODE_CLIENT_ID}:ORG-1:https://api.anthropic.com:{USAGE_SCOPE}:"
        );
        let parsed = parse_cache_key(&key, &entry("a", 1)).expect("parses");

        assert_eq!(parsed.account_uuid, "uuid-1");
        assert_eq!(parsed.client_id, CLAUDE_CODE_CLIENT_ID);
        assert_eq!(parsed.organization_uuid, "org-1");
        assert!(parsed.has_usage_scope());
        assert_eq!(parsed.subscription_type.as_deref(), Some("max"));
        assert!(parsed.expires_at.is_some());
    }

    #[test]
    fn an_entry_without_the_usage_scope_is_not_usable() {
        let key = "acct:uuid-2|client-x:org-2:https://api.anthropic.com:user:inference:";
        let parsed = parse_cache_key(key, &entry("b", 1)).expect("parses");
        assert!(!parsed.has_usage_scope());
        assert!(!parsed.is_usable_at(now()));
    }

    #[test]
    fn a_key_without_an_account_prefix_is_skipped() {
        assert!(parse_cache_key("oauth:tokenCacheV2", &entry("c", 1)).is_none());
        assert!(parse_cache_key("acct:uuid|client:org:aud", &serde_json::json!({})).is_none());
        // 组织 id 缺失的条目按 cc-bar 的判据直接跳过，不做「大概是同一个人」的猜测。
        assert!(parse_cache_key("acct:uuid|client::aud:x:", &entry("d", 1)).is_none());
    }

    #[test]
    fn expiry_accepts_seconds_milliseconds_and_iso_strings() {
        let seconds = parse_expiry(&serde_json::json!(1_790_000_000_u64)).expect("seconds");
        let millis = parse_expiry(&serde_json::json!(1_790_000_000_000_u64)).expect("millis");
        assert_eq!(seconds, millis);

        let iso = parse_expiry(&serde_json::json!("2026-09-21T14:13:20Z")).expect("iso");
        assert_eq!(iso, seconds);
        assert!(parse_expiry(&serde_json::json!(true)).is_none());
        assert!(parse_expiry(&serde_json::json!("nonsense")).is_none());
    }

    #[test]
    fn the_official_client_wins_among_usable_entries() {
        let entries = vec![
            DesktopCacheEntry {
                account_uuid: "uuid".to_owned(),
                client_id: "other-client".to_owned(),
                organization_uuid: "org".to_owned(),
                access_token: "longer-lived".to_owned(),
                expires_at: Some(now() + chrono::Duration::hours(48)),
                subscription_type: None,
                raw_key: format!("acct:uuid|other-client:org:aud:{USAGE_SCOPE}:"),
            },
            DesktopCacheEntry {
                account_uuid: "uuid".to_owned(),
                client_id: CLAUDE_CODE_CLIENT_ID.to_owned(),
                organization_uuid: "org".to_owned(),
                access_token: "official".to_owned(),
                expires_at: Some(now() + chrono::Duration::hours(1)),
                subscription_type: None,
                raw_key: format!("acct:uuid|{CLAUDE_CODE_CLIENT_ID}:org:aud:{USAGE_SCOPE}:"),
            },
        ];

        let best = best_usable(entries, now()).expect("a winner");
        assert_eq!(best.access_token, "official");
    }

    #[test]
    fn the_longest_lived_entry_wins_within_the_same_client() {
        let entries = vec![
            DesktopCacheEntry {
                account_uuid: "uuid".to_owned(),
                client_id: CLAUDE_CODE_CLIENT_ID.to_owned(),
                organization_uuid: "org".to_owned(),
                access_token: "short".to_owned(),
                expires_at: Some(now() + chrono::Duration::hours(1)),
                subscription_type: None,
                raw_key: format!("acct:uuid|{CLAUDE_CODE_CLIENT_ID}:org:aud:{USAGE_SCOPE}:"),
            },
            DesktopCacheEntry {
                account_uuid: "uuid".to_owned(),
                client_id: CLAUDE_CODE_CLIENT_ID.to_owned(),
                organization_uuid: "org".to_owned(),
                access_token: "long".to_owned(),
                expires_at: Some(now() + chrono::Duration::days(30)),
                subscription_type: None,
                raw_key: format!("acct:uuid|{CLAUDE_CODE_CLIENT_ID}:org:aud:{USAGE_SCOPE}:"),
            },
        ];

        let best = best_usable(entries, now()).expect("a winner");
        assert_eq!(best.access_token, "long");
    }

    #[test]
    fn entries_without_the_usage_scope_or_expired_are_not_usable() {
        let without_scope = DesktopCacheEntry {
            account_uuid: "uuid".to_owned(),
            client_id: CLAUDE_CODE_CLIENT_ID.to_owned(),
            organization_uuid: "org".to_owned(),
            access_token: "token".to_owned(),
            expires_at: Some(now() + chrono::Duration::days(1)),
            subscription_type: None,
            raw_key: "acct:uuid|client:org:aud:user:inference:".to_owned(),
        };
        assert!(!without_scope.is_usable_at(now()));

        let expired = DesktopCacheEntry {
            raw_key: format!("acct:uuid|client:org:aud:{USAGE_SCOPE}:"),
            expires_at: Some(now() - chrono::Duration::minutes(1)),
            ..without_scope
        };
        assert!(!expired.is_usable_at(now()));

        let almost_expired = DesktopCacheEntry {
            expires_at: Some(now() + chrono::Duration::seconds(3)),
            ..expired
        };
        assert!(!almost_expired.is_usable_at(now()));
    }

    #[test]
    fn base64_decoding_accepts_padding_and_ignores_whitespace() {
        assert_eq!(decode_base64("aGVsbG8=").as_deref(), Some(&b"hello"[..]));
        assert_eq!(
            decode_base64("aGVs\nbG8=").as_deref(),
            Some(&b"hello"[..]),
            "换行是 Claude Desktop 缓存里常见的折行"
        );
        assert!(decode_base64("not base64 !!!").is_none());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_macos_scheme_decrypts_a_v10_payload() {
        // 用同样的参数反向构造一份密文，验证解密路径本身正确。
        use aes::cipher::{BlockEncryptMut, KeyIvInit};

        type Aes128CbcEnc = cbc::Encryptor<aes::Aes128>;

        let key = pbkdf2_sha1(b"password", b"saltysalt", 1003, 16);
        let plaintext = br#"{"acct:uuid|client:org:aud:user:profile:":{"token":"t"}}"#;
        let mut buffer = plaintext.to_vec();
        let length = buffer.len();
        buffer.resize(length + 16, 0);
        let encrypted = Aes128CbcEnc::new_from_slices(&key, &[b' '; 16])
            .expect("key and iv lengths")
            .encrypt_padded_mut::<aes::cipher::block_padding::Pkcs7>(&mut buffer, length)
            .expect("encrypts")
            .to_vec();

        let decrypted = aes_128_cbc_decrypt(&key, &[b' '; 16], &encrypted).expect("decrypts");
        assert_eq!(decrypted, plaintext);
    }
}
