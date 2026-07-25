//! 自动刷新、请求合并、节流、429 退避和 Provider 隔离。
//!
//! - [`params`]：参数基线的唯一真值。
//! - [`backoff`]：按 Provider 独立的退避阶梯。
//! - [`runtime`]：三维状态转换、请求合并与节流决策。
//!
//! 编排（定时器、事件广播）在 `crate::app`，因为它需要持有 Tauri `AppHandle`。

pub mod backoff;
pub mod params;
pub mod runtime;

pub use runtime::{ProviderRuntime, RefreshDecision, RefreshTrigger};
