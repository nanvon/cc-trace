//! Tauri command 边界。
//!
//! 每个 command 校验输入、调用 `crate::app` 的用例并返回明确 contract。
//! 错误映射成稳定的 `code`，由前端查 i18n 文案：载荷里不出现 Rust 枚举名、
//! 文件路径、系统错误原文或凭据内容。

pub mod quota;
pub mod settings;
pub mod status;
pub mod window;

#[cfg(debug_assertions)]
pub mod dev;

use serde::Serialize;

/// 可展示的命令失败。`code` 是稳定标识，不是给用户看的文本。
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandError {
    pub code: &'static str,
}

impl CommandError {
    /// 请求的窗口不存在或无法显示。
    pub const WINDOW_UNAVAILABLE: Self = Self {
        code: "windowUnavailable",
    };

    /// 设置写入失败，界面必须保留原值并明确提示。
    pub const SETTINGS_WRITE_FAILED: Self = Self {
        code: "settingsWriteFailed",
    };
}
