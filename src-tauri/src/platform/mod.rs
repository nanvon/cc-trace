//! Tray、窗口、开机启动、系统路径与平台差异的适配层。
//!
//! 所有 `#[cfg(target_os = ...)]` 分支都收敛在这里；Vue 业务代码不做平台判断。

pub mod autostart;
pub mod desktop;
pub mod keychain;
/// macOS 27 的 Expanded Interface Session 适配：左键由系统管理，右键菜单手动
/// 弹出。旧系统没有这套 API 时整个模块不生效，见 macos_status_item.rs。
#[cfg(target_os = "macos")]
pub mod macos_status_item;
/// 菜单栏徽标位图只有 macOS 用得上：Windows 托盘不支持图标旁并排文字，见 ADR-0017。
#[cfg(target_os = "macos")]
pub mod menubar_badge;
/// compact 面板「点击外部关闭」的全局监听只有 macOS 用得上：Windows 的失焦事件
/// 可靠，见 outside_click.rs。
#[cfg(target_os = "macos")]
pub mod outside_click;
pub mod strings;
pub mod tray;
