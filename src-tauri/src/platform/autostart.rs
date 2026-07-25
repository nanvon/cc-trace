//! 开机自动启动。
//!
//! 注册标识见 `docs/技术架构.md`「项目身份」，与 Swift 版 cc-bar 完全独立。
//! 失败不阻断应用：设置仍然保存，只是系统侧未生效。

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

/// 把系统的开机启动状态对齐到设置值。
pub fn apply(app: &AppHandle, enabled: bool) {
    let manager = app.autolaunch();
    let _ = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
}
