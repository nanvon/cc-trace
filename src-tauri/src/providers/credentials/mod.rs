//! Provider 凭据的只读发现与 token 回写。
//!
//! 来源、平台差异与回写边界见 `docs/额度领域模型.md` 第 5 节：
//!
//! - 除 token 刷新结果外不修改外部凭据来源，见
//!   [ADR-0014](../../../../docs/决策/ADR-0014-token刷新结果回写外部凭据.md)。
//! - macOS 上 Claude Code 在文件缺失时回退到系统钥匙串，见
//!   [ADR-0013](../../../../docs/决策/ADR-0013-macOS读取ClaudeCode钥匙串凭据.md)。
//!
//! 本模块的所有秘密都包在 [`Secret`] 里，它的 `Debug` 永远输出占位符，因此把凭据
//! 结构整体写进日志或 panic 信息也不会泄露内容，见 `docs/日志与诊断.md` 第 5 节。

pub mod claude;
pub mod codex;
mod jwt;

use std::fmt;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// 一次凭据发现的结局。每个分支对应 `docs/状态与错误模型.md` 第 4 节的一类来源。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discovery<T> {
    Found(T),
    /// 来源不存在或不含可用凭据 → `no_credentials`，不发起请求。
    Missing,
    /// 来源存在但形式不受首版支持（例如只含 PAT）→ `unsupported`，不发起请求。
    Unsupported,
    /// 来源存在但读不出来：权限被拒、钥匙串授权被拒、IO 失败 → 凭据类 `error`。
    /// 不并入 `Missing`，否则会把「有登录态但拿不到」说成「没有登录」。
    Unreadable,
}

/// 持有秘密的字符串。
///
/// 手动实现 `Debug`，不使用 `#[derive(Debug)]`：`docs/日志与诊断.md` 要求持有 token
/// 的类型必须屏蔽 `Debug`。取用内容必须显式调用 [`Secret::expose`]，让每个使用点在
/// 代码审查时可见。
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// 取出明文。只允许用于构造请求头、回写原来源与身份比较。
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// 脱敏展示提示，例如 `n***@example.com`、`ab***yz`。
    ///
    /// 结果会进入 command 载荷，因此保留的字符必须少到无法反推账号：邮箱只留首字母
    /// 与域名，其余只留首尾各两位。
    pub fn hint(&self) -> String {
        let value = self.0.trim();
        if let Some((local, domain)) = value.split_once('@') {
            let head = local.chars().next().unwrap_or('*');
            return format!("{head}***@{domain}");
        }

        let chars: Vec<char> = value.chars().collect();
        if chars.len() <= 6 {
            return "***".to_owned();
        }

        let head: String = chars.iter().take(2).collect();
        let tail: String = chars
            .iter()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("{head}***{tail}")
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret(<redacted>)")
    }
}

/// 由身份字段算出的单向指纹，用于 `docs/额度领域模型.md` 第 4.1 节的身份变化判断。
///
/// 只保留摘要的前 16 个十六进制字符：足以区分不同账号，又不可读、不可展示。
/// 明文的 account id 与邮箱不进入返回值，因此这个指纹可以安全地写进额度缓存。
/// 所有身份字段都缺失时返回 `None`——「认不出身份」不等于「身份没变」，
/// 调用方必须把 `None` 当作无法比较，而不是与某个值相等。
pub fn identity_fingerprint(provider: &str, parts: &[Option<&Secret>]) -> Option<String> {
    use sha2::{Digest, Sha256};

    if parts.iter().all(Option::is_none) {
        return None;
    }

    let mut hasher = Sha256::new();
    hasher.update(provider.as_bytes());
    for part in parts {
        // 逐段加分隔符，避免不同切分方式产生同一个摘要。
        hasher.update([0x1f]);
        if let Some(secret) = part {
            hasher.update(secret.expose().as_bytes());
        }
    }

    let digest = hasher.finalize();
    Some(
        digest
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect(),
    )
}

/// 当前用户的主目录。
///
/// 不使用 Tauri 的 path API：凭据发现必须能在没有 `AppHandle` 的单元测试里调用。
/// 两个平台的环境变量名不同，这是本模块唯一的平台分支。
pub(crate) fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    const HOME_VARIABLE: &str = "USERPROFILE";
    #[cfg(not(windows))]
    const HOME_VARIABLE: &str = "HOME";

    std::env::var_os(HOME_VARIABLE)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// 去掉首尾空白，空串视为不存在。外部凭据文件里空字符串和字段缺失是等价的。
pub(crate) fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// 原子替换一个外部 JSON 文件，并保留它原有的权限位。
///
/// 凭据文件通常是 `0600`；`fs::File::create` 默认 `0644`，直接写会放宽权限，因此这里
/// 显式把原文件的 mode 复制到临时文件。写入失败时原文件保持不变。
pub(crate) fn replace_json_atomically(path: &Path, value: &serde_json::Value) -> io::Result<()> {
    let serialized = serde_json::to_vec_pretty(value)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    let directory = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "credential path has no parent")
    })?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "credential path has no name")
        })?;
    let temp_path = directory.join(format!("{file_name}.cctrace.tmp"));

    let existing_permissions = fs::metadata(path)
        .ok()
        .map(|metadata| metadata.permissions());
    {
        let mut file = fs::File::create(&temp_path)?;
        if let Some(permissions) = existing_permissions.clone() {
            file.set_permissions(permissions)?;
        }
        file.write_all(&serialized)?;
        file.sync_all()?;
    }

    match fs::rename(&temp_path, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp_path);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_prints_the_secret() {
        let secret = Secret::new("sk-live-do-not-print");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert!(!format!("{secret:?}").contains("do-not-print"));
    }

    #[test]
    fn hint_keeps_the_domain_but_not_the_local_part() {
        assert_eq!(Secret::new("nanvon@example.com").hint(), "n***@example.com");
    }

    #[test]
    fn hint_of_an_opaque_value_keeps_only_the_edges() {
        assert_eq!(Secret::new("acct_1234567890").hint(), "ac***90");
        assert_eq!(Secret::new("short").hint(), "***");
    }

    #[test]
    fn atomic_replacement_keeps_the_original_permissions() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(".credentials.json");
        fs::write(&path, r#"{"a":1}"#).expect("seed file");

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).expect("chmod");
        }

        let value = serde_json::json!({ "a": 2 });
        replace_json_atomically(&path, &value).expect("replace succeeds");

        let raw = fs::read_to_string(&path).expect("read back");
        assert!(raw.contains("\"a\": 2"));
        assert!(
            !dir.path().join(".credentials.json.cctrace.tmp").exists(),
            "the temporary file must not be left behind"
        );

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("metadata").permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "atomic replacement must not widen permissions"
            );
        }
    }

    #[test]
    fn a_fingerprint_is_stable_short_and_unreadable() {
        let account = Secret::new("acct_fixture_0001");
        let email = Secret::new("user@example.test");

        let first = identity_fingerprint("codex", &[Some(&account), Some(&email)]);
        let again = identity_fingerprint("codex", &[Some(&account), Some(&email)]);

        assert_eq!(first, again, "the same identity must fingerprint the same");
        let fingerprint = first.expect("an identity with fields has a fingerprint");
        assert_eq!(fingerprint.len(), 16);
        assert!(!fingerprint.contains("acct"));
        assert!(!fingerprint.contains("example"));
    }

    #[test]
    fn different_identities_and_providers_fingerprint_differently() {
        let one = Secret::new("acct_one");
        let other = Secret::new("acct_two");

        assert_ne!(
            identity_fingerprint("codex", &[Some(&one)]),
            identity_fingerprint("codex", &[Some(&other)])
        );
        assert_ne!(
            identity_fingerprint("codex", &[Some(&one)]),
            identity_fingerprint("claude", &[Some(&one)]),
            "the same account on two providers must not collide"
        );
    }

    #[test]
    fn field_boundaries_cannot_be_shifted_into_the_same_digest() {
        let ab = Secret::new("ab");
        let c = Secret::new("c");
        let a = Secret::new("a");
        let bc = Secret::new("bc");

        assert_ne!(
            identity_fingerprint("codex", &[Some(&ab), Some(&c)]),
            identity_fingerprint("codex", &[Some(&a), Some(&bc)])
        );
    }

    #[test]
    fn an_identity_with_no_fields_cannot_be_compared() {
        assert_eq!(identity_fingerprint("codex", &[None, None]), None);
    }

    #[test]
    fn non_empty_treats_blank_as_absent() {
        assert_eq!(non_empty(Some("  value  ")), Some("value".to_owned()));
        assert_eq!(non_empty(Some("   ")), None);
        assert_eq!(non_empty(None), None);
    }
}
