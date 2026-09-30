//! 导入的 Codex 副账号：元数据与凭据的分离持久化。
//!
//! 拆成两个域，理由与 [`crate::storage`] 的整体划分一致：
//!
//! - **元数据**（别名、邮箱、计划、是否显示、导入时刻）写 `codex-accounts.json`，
//!   是普通偏好数据，损坏时回退空列表并保留 `.corrupt` 副本。
//! - **凭据**（粘贴进来的 `auth.json` 内容）写系统秘密存储，
//!   见 [`crate::platform::secret_store`] 与 [ADR-0035]。
//!
//! 秘密槽位由账号身份的短哈希派生，**不用下标**：删除或重排账号时下标会变，
//! 用下标做槽位会把 A 的凭据读成 B 的。
//!
//! [ADR-0035]: ../../../../docs/决策/ADR-0035-服务矩阵与Windows凭据存储.md

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::platform::secret_store::{self, SecretRead, SecretWrite};

pub const IMPORTED_ACCOUNTS_SCHEMA_VERSION: u32 = 1;
const FILE_NAME: &str = "codex-accounts.json";

/// 一个导入的 Codex 副账号的元数据。凭据不在这里。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedCodexAccount {
    /// 复合身份 `{chatgpt_account_id}:{chatgpt_user_id}`；只有 account id 时是单段。
    /// 它是对外键，也是秘密槽位的来源。
    pub id: String,
    /// 用户起的别名；空串表示回落到 `email`，再回落到「账号 N」。
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub plan: Option<String>,
    /// 是否显示在紧凑面板与悬浮窗。隐藏不影响额度轮询与历史记录。
    #[serde(default = "default_true")]
    pub visible: bool,
    /// 凭据是不是 Codex personal access token（不透明、不能续期）。
    #[serde(default)]
    pub personal_access_token: bool,
    pub added_at: String,
}

const fn default_true() -> bool {
    true
}

impl ImportedCodexAccount {
    /// 界面优先取别名，其次邮箱，最后退回 `Codex 账号`。
    pub fn display_name(&self, fallback: &str) -> String {
        if !self.alias.trim().is_empty() {
            return self.alias.trim().to_owned();
        }
        if let Some(email) = self
            .email
            .as_deref()
            .filter(|value| !value.trim().is_empty())
        {
            return email.trim().to_owned();
        }
        fallback.to_owned()
    }

    /// 用于 `ChatGPT-Account-Id` 请求头的纯 account id。
    pub fn chatgpt_account_id(&self) -> &str {
        match self.id.split_once(':') {
            Some((account_id, _)) => account_id,
            None => &self.id,
        }
    }

    /// 身份短哈希：秘密槽位与额度主体标识都用它，保证重排与删除不影响其他账号。
    pub fn identity_hash(&self) -> String {
        identity_hash(&self.id)
    }
}

/// 身份短哈希：16 位十六进制，足够避免同一台机器上的碰撞，且不含账号明文。
pub fn identity_hash(id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"cc-trace-codex-account-v1");
    hasher.update(id.as_bytes());
    let digest = hasher.finalize();
    digest
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// 凭据在系统秘密存储里的槽位。
pub fn secret_slot(identity_hash: &str) -> String {
    format!("codex-imported-{identity_hash}")
}

/// `codex-accounts.json` 的版本化读写。
#[derive(Debug, Clone)]
pub struct ImportedCodexStore {
    directory: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Payload {
    version: u32,
    accounts: Vec<ImportedCodexAccount>,
}

impl ImportedCodexStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    pub fn path(&self) -> PathBuf {
        self.directory.join(FILE_NAME)
    }

    /// 读取全部账号。文件缺失、损坏或版本不认识时返回空列表并把损坏文件留档，
    /// 不做部分解析：半个账号列表比空列表更容易让人误会。
    pub fn load(&self) -> Vec<ImportedCodexAccount> {
        let path = self.path();
        let Ok(raw) = fs::read_to_string(&path) else {
            return Vec::new();
        };
        match serde_json::from_str::<Payload>(&raw) {
            Ok(payload) if payload.version == IMPORTED_ACCOUNTS_SCHEMA_VERSION => payload.accounts,
            _ => {
                let _ = fs::rename(&path, path.with_extension("json.corrupt"));
                Vec::new()
            }
        }
    }

    /// 原子写入。写失败时原文件不变。
    pub fn save(&self, accounts: &[ImportedCodexAccount]) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;
        let payload = Payload {
            version: IMPORTED_ACCOUNTS_SCHEMA_VERSION,
            accounts: accounts.to_vec(),
        };
        let body = serde_json::to_vec_pretty(&payload)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        let path = self.path();
        let temporary = path.with_extension("json.tmp");
        {
            let mut file = fs::File::create(&temporary)?;
            file.write_all(&body)?;
            file.sync_all()?;
        }
        fs::rename(&temporary, &path)
    }

    pub fn load_credentials(&self, identity_hash: &str) -> SecretRead {
        secret_store::read(&secret_slot(identity_hash))
    }

    pub fn save_credentials(&self, identity_hash: &str, payload: &str) -> SecretWrite {
        secret_store::write(
            &secret_slot(identity_hash),
            &crate::providers::credentials::Secret::new(payload),
        )
    }

    pub fn remove_credentials(&self, identity_hash: &str) -> SecretWrite {
        secret_store::delete(&secret_slot(identity_hash))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: &str) -> ImportedCodexAccount {
        ImportedCodexAccount {
            id: id.to_owned(),
            alias: String::new(),
            email: Some("second@example.com".to_owned()),
            plan: Some("Plus".to_owned()),
            visible: true,
            personal_access_token: false,
            added_at: "2026-10-01T00:00:00Z".to_owned(),
        }
    }

    #[test]
    fn accounts_round_trip_through_the_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ImportedCodexStore::new(dir.path().to_path_buf());

        store.save(&[account("acct:user")]).expect("save");

        let loaded = store.load();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "acct:user");
        assert_eq!(loaded[0].chatgpt_account_id(), "acct");
        assert!(loaded[0].visible);
    }

    #[test]
    fn a_corrupt_file_falls_back_to_empty_and_keeps_a_copy() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ImportedCodexStore::new(dir.path().to_path_buf());
        fs::write(store.path(), "{ not json").expect("write");

        assert!(store.load().is_empty());
        assert!(store.path().with_extension("json.corrupt").exists());
    }

    #[test]
    fn an_unknown_version_is_not_partially_parsed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = ImportedCodexStore::new(dir.path().to_path_buf());
        fs::write(
            store.path(),
            r#"{"version":99,"accounts":[{"id":"a","addedAt":"now"}]}"#,
        )
        .expect("write");

        assert!(store.load().is_empty());
    }

    #[test]
    fn secret_slots_follow_identity_not_position() {
        let first = identity_hash("acct-a:user-1");
        let second = identity_hash("acct-b:user-2");

        assert_ne!(first, second);
        assert_eq!(secret_slot(&first), format!("codex-imported-{first}"));
        // 同一个 id 恒等，与它在列表里的位置无关。
        assert_eq!(first, identity_hash("acct-a:user-1"));
    }

    #[test]
    fn names_fall_back_in_a_fixed_order() {
        let mut value = account("acct");
        value.alias = "  工作账号 ".to_owned();
        assert_eq!(value.display_name("Codex 账号"), "工作账号");

        value.alias = String::new();
        assert_eq!(value.display_name("Codex 账号"), "second@example.com");

        value.email = None;
        assert_eq!(value.display_name("Codex 账号"), "Codex 账号");
    }

    #[test]
    fn the_identity_hash_never_contains_the_account_plaintext() {
        let hash = identity_hash("acct-secret:user-secret");
        assert!(!hash.contains("acct-secret"));
        assert_eq!(hash.len(), 16);
    }
}
