//! `settings.json` 的版本化持久化。
//!
//! 规则见 `docs/技术架构.md`「数据与恢复」：
//!
//! - 文件包含 `schemaVersion`。
//! - 临时文件写入 → 同步 → 原子替换；写入失败保留上一份有效文件。
//! - 解析失败时回退到安全默认值，并保留可诊断但不含秘密的记录。
//!
//! 设置文件里没有任何凭据或秘密，因此损坏副本可以安全保留供诊断。

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use crate::contracts::{SETTINGS_SCHEMA_VERSION, Settings};

const SETTINGS_FILE: &str = "settings.json";
const TEMP_FILE: &str = "settings.json.tmp";
const CORRUPT_FILE: &str = "settings.json.corrupt";

/// 读取设置时遇到的可诊断问题。不含文件内容，也不含秘密。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadIssue {
    /// 文件不存在：首次启动的正常情况。
    Missing,
    /// 文件存在但无法解析，已回退到安全默认值。
    Corrupt,
    /// 文件的 `schemaVersion` 高于本版本可理解的范围，已回退到安全默认值。
    UnsupportedSchema,
    /// 读取失败（权限、IO）。
    Unreadable,
}

#[derive(Debug)]
pub struct SettingsStore {
    directory: PathBuf,
}

impl SettingsStore {
    pub fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    fn path(&self) -> PathBuf {
        self.directory.join(SETTINGS_FILE)
    }

    /// 读取设置。任何失败都回退到安全默认值，不阻塞应用启动。
    pub fn load(&self) -> (Settings, Option<LoadIssue>) {
        let path = self.path();

        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return (Settings::default(), Some(LoadIssue::Missing));
            }
            Err(_) => return (Settings::default(), Some(LoadIssue::Unreadable)),
        };

        match serde_json::from_str::<Settings>(&raw) {
            Ok(settings) if settings.schema_version > SETTINGS_SCHEMA_VERSION => {
                self.quarantine(&path);
                (Settings::default(), Some(LoadIssue::UnsupportedSchema))
            }
            Ok(mut settings) => {
                settings.schema_version = SETTINGS_SCHEMA_VERSION;
                (settings, None)
            }
            Err(_) => {
                self.quarantine(&path);
                (Settings::default(), Some(LoadIssue::Corrupt))
            }
        }
    }

    /// 原子写入。写入失败时上一份有效文件保持不变。
    pub fn save(&self, settings: &Settings) -> io::Result<()> {
        fs::create_dir_all(&self.directory)?;

        let serialized = serde_json::to_vec_pretty(settings)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

        let temp_path = self.directory.join(TEMP_FILE);
        {
            let mut file = fs::File::create(&temp_path)?;
            file.write_all(&serialized)?;
            file.sync_all()?;
        }

        fs::rename(&temp_path, self.path())
    }

    /// 把无法解析的文件挪到一边，保留给用户与诊断，同时让下一次写入从干净状态开始。
    fn quarantine(&self, path: &Path) {
        let _ = fs::rename(path, self.directory.join(CORRUPT_FILE));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{
        AppearancePreference, LanguagePreference, OnboardingState, RefreshInterval,
    };

    fn store() -> (tempfile::TempDir, SettingsStore) {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = SettingsStore::new(dir.path().to_path_buf());
        (dir, store)
    }

    #[test]
    fn missing_file_yields_defaults_without_failing() {
        let (_dir, store) = store();
        let (settings, issue) = store.load();

        assert_eq!(issue, Some(LoadIssue::Missing));
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn round_trips_through_an_atomic_write() {
        let (_dir, store) = store();

        let settings = Settings {
            language: LanguagePreference::En,
            appearance: AppearancePreference::Dark,
            refresh_interval: RefreshInterval::OneMinute,
            launch_at_login: true,
            onboarding: OnboardingState {
                completed: true,
                completed_at: Some("2026-07-25T00:00:00Z".to_owned()),
            },
            ..Settings::default()
        };

        store.save(&settings).expect("save succeeds");
        let (loaded, issue) = store.load();

        assert_eq!(issue, None);
        assert_eq!(loaded, settings);
    }

    #[test]
    fn no_temp_file_is_left_behind() {
        let (dir, store) = store();
        store.save(&Settings::default()).expect("save succeeds");

        assert!(!dir.path().join(TEMP_FILE).exists());
        assert!(dir.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults_and_is_quarantined() {
        let (dir, store) = store();
        fs::write(dir.path().join(SETTINGS_FILE), "{ not json").expect("write");

        let (settings, issue) = store.load();

        assert_eq!(issue, Some(LoadIssue::Corrupt));
        assert_eq!(settings, Settings::default());
        assert!(dir.path().join(CORRUPT_FILE).exists());
        assert!(!dir.path().join(SETTINGS_FILE).exists());
    }

    #[test]
    fn a_newer_schema_version_is_not_silently_downgraded() {
        let (dir, store) = store();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            format!(r#"{{"schemaVersion": {}}}"#, SETTINGS_SCHEMA_VERSION + 1),
        )
        .expect("write");

        let (settings, issue) = store.load();

        assert_eq!(issue, Some(LoadIssue::UnsupportedSchema));
        assert_eq!(settings, Settings::default());
        assert!(dir.path().join(CORRUPT_FILE).exists());
    }

    #[test]
    fn a_partial_file_keeps_the_fields_it_does_have() {
        let (dir, store) = store();
        fs::write(
            dir.path().join(SETTINGS_FILE),
            r#"{"schemaVersion":1,"appearance":"dark"}"#,
        )
        .expect("write");

        let (settings, issue) = store.load();

        assert_eq!(issue, None);
        assert_eq!(settings.appearance, AppearancePreference::Dark);
        assert_eq!(settings.refresh_interval, RefreshInterval::TwoMinutes);
    }
}
