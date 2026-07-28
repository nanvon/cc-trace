//! Tray、窗口、开机启动、系统路径与平台差异的适配层。
//!
//! 所有 `#[cfg(target_os = ...)]` 分支都收敛在这里；Vue 业务代码不做平台判断。

pub mod autostart;
pub mod desktop;
pub mod keychain;
/// 菜单栏徽标位图只有 macOS 用得上：Windows 托盘不支持图标旁并排文字，见 ADR-0017。
#[cfg(target_os = "macos")]
pub mod menubar_badge;
pub mod strings;
pub mod tray;
