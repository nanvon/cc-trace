//! CC Trace 自己的版本化持久化。
//!
//! - `settings.json`：偏好与首次启动完成标记，损坏时回退默认值并保留 `.corrupt` 副本。
//! - `quota-cache.json`：每个 Provider 的最新有效脱敏快照，损坏时直接删除并重新获取。
//! - `usage.db`：Token、对话元数据、扫描水位与额度历史；不保存原始消息或外部路径。
//!
//! 两个文件都只包含脱敏数据：凭据、token 与响应原文永远不进入本模块。外部凭据来源的
//! token 回写属于 `providers::credentials`，不走这里。
//!
//! 本模块只读写 CC Trace 自己的数据目录，不读取、迁移、覆盖或删除 Swift 版 cc-bar 的
//! 任何数据，见 `docs/决策/ADR-0003-独立应用身份不迁移数据.md`。

mod quota_cache;
mod settings_store;
mod usage_db;

pub use quota_cache::{CachedProvider, QuotaCache, QuotaCacheStore};
pub use settings_store::{LoadIssue, SettingsStore};
pub use usage_db::{CommitResult, UsageDb, UsageDbError};
