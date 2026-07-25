//! CC Trace 自己的版本化持久化。
//!
//! 首版只持久化 `settings.json`（含首次启动完成标记）。额度缓存 `quota-cache.json`
//! 属第 12 阶段 Provider 最小闭环，此处不预留空实现。
//!
//! 本模块只读写 CC Trace 自己的数据目录，不读取、迁移、覆盖或删除 Swift 版 cc-bar 的
//! 任何数据，见 `docs/决策/ADR-0003-独立应用身份不迁移数据.md`。

mod settings_store;

pub use settings_store::{LoadIssue, SettingsStore};
