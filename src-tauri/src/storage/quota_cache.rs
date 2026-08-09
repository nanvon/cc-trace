//! `quota-cache.json` 的版本化持久化。
//!
//! 规则见 `docs/技术架构.md`「数据与恢复」：
//!
//! - 文件包含 `schemaVersion`。
//! - 临时文件写入 → 同步 → 原子替换；写入失败保留上一份有效文件。
//! - **损坏时直接删除并重新获取**，不像 `settings.json` 那样保留 `.corrupt` 副本：
//!   缓存可以完全重建，留一份坏文件没有诊断价值。
//!
//! 缓存保存展示所需的快照与展示身份（含完整账号，见 ADR-0025），**不保存凭据、
//! 响应原文或 token**。身份摘要（单向指纹）用于重启后仍能发现身份变化，
//! 见 `docs/额度领域模型.md` 第 4.1 节。

use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::contracts::{ProviderId, ProviderIdentity, QuotaSnapshot};

pub const QUOTA_CACHE_SCHEMA_VERSION: u32 = 1;

const CACHE_FILE: &str = "quota-cache.json";
const TEMP_FILE: &str = "quota-cache.json.tmp";

/// 一个 Provider 的最新有效快照。首版只保留最新一份，不建立历史序列。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CachedProvider {
    pub provider: ProviderId,
    pub identity: Option<ProviderIdentity>,
    /// 身份指纹，见 `providers::credentials::identity_fingerprint`。单向摘要，
    /// 不可反推 account id 或邮箱。
    pub identity_key: Option<String>,
    pub snapshot: QuotaSnapshot,
    /// 该快照对应的成功刷新时刻，ISO 8601 UTC。
    pub last_success_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaCache {
    pub schema_version: u32,
    #[serde(default)]
    pub providers: Vec<CachedProvider>,
}

impl QuotaCache {
    pub fn new(providers: Vec<CachedProvider>) -> Self {
        Self {
            schema_version: QUOTA_CACHE_SCHEMA_VERSION,
            providers,
        }
    }
}

#[derive(Debug)]
pub struct QuotaCacheStore {
    directory: PathBuf,
}

impl QuotaCacheStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(CACHE_FILE)
    }

    /// 读取缓存。任何问题都返回 `None` 并清掉坏文件——缓存缺失只是慢一次，不是错误。
    pub fn load(&self) -> Option<QuotaCache> {
        let path = self.path();

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
            Err(_) => return None,
        };

        match serde_json::from_str::<QuotaCache>(&raw) {
            Ok(cache) if cache.schema_version == QUOTA_CACHE_SCHEMA_VERSION => Some(cache),
            // 版本不认识或内容损坏都直接丢弃：下一次刷新会重建。
            _ => {
                let _ = fs::remove_file(&path);
                None
            }
        }
    }

    /// 原子写入。写入失败时上一份有效缓存保持不变。
    pub fn save(&self, cache: &QuotaCache) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;

        let serialized = serde_json::to_vec_pretty(cache)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        let temp_path = self.directory.join(TEMP_FILE);
        {
            let mut file = fs::File::create(&temp_path)?;
            file.write_all(&serialized)?;
            file.sync_all()?;
        }

        fs::rename(&temp_path, self.path())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{QuotaWindow, QuotaWindowKind};

    fn store() -> (tempfile::TempDir, QuotaCacheStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = QuotaCacheStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    fn cached(provider: ProviderId) -> CachedProvider {
        CachedProvider {
            provider,
            identity: Some(ProviderIdentity {
                account: Some("user@example.test".to_owned()),
                plan: Some("plus".to_owned()),
            }),
            identity_key: Some("0123456789abcdef".to_owned()),
            snapshot: QuotaSnapshot {
                windows: vec![QuotaWindow {
                    id: "codex.five-hour".to_owned(),
                    kind: QuotaWindowKind::FiveHour,
                    display_name: None,
                    used_percent: 32.0,
                    remaining_percent: 68.0,
                    resets_at: Some("2026-07-27T12:00:00+00:00".to_owned()),
                    window_seconds: Some(18_000),
                    is_active: true,
                    is_primary: true,
                }],
                captured_at: "2026-07-27T08:00:00+00:00".to_owned(),
            },
            last_success_at: "2026-07-27T08:00:00+00:00".to_owned(),
        }
    }

    #[test]
    fn a_missing_cache_is_not_an_error() {
        let (_dir, store) = store();
        assert_eq!(store.load(), None);
    }

    #[test]
    fn round_trips_through_an_atomic_write() {
        let (dir, store) = store();
        let cache = QuotaCache::new(vec![cached(ProviderId::Codex), cached(ProviderId::Claude)]);

        store.save(&cache).expect("save succeeds");

        assert_eq!(store.load(), Some(cache));
        assert!(!dir.path().join(TEMP_FILE).exists());
    }

    #[test]
    fn a_corrupt_cache_is_deleted_instead_of_quarantined() {
        let (dir, store) = store();
        fs::write(dir.path().join(CACHE_FILE), "{ not json").expect("write");

        assert_eq!(store.load(), None);
        assert!(
            !dir.path().join(CACHE_FILE).exists(),
            "a rebuildable cache has no diagnostic value once broken"
        );
    }

    #[test]
    fn an_unknown_schema_version_is_discarded_in_both_directions() {
        let (dir, store) = store();

        for version in [
            QUOTA_CACHE_SCHEMA_VERSION + 1,
            QUOTA_CACHE_SCHEMA_VERSION - 1,
        ] {
            fs::write(
                dir.path().join(CACHE_FILE),
                format!(r#"{{"schemaVersion":{version},"providers":[]}}"#),
            )
            .expect("write");

            assert_eq!(store.load(), None, "schema version {version}");
        }
    }

    #[test]
    fn the_cache_never_carries_anything_that_could_be_a_secret() {
        let cache = QuotaCache::new(vec![cached(ProviderId::Codex)]);
        let json = serde_json::to_string(&cache).expect("cache serializes");

        for forbidden in ["accessToken", "refreshToken", "token", "Bearer"] {
            assert!(
                !json.contains(forbidden),
                "the quota cache must never grow a {forbidden} field"
            );
        }
    }
}
