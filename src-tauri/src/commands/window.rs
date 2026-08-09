use tauri::AppHandle;

use super::CommandError;
use crate::platform::desktop::{
    self, MainNavigationTarget, ONBOARDING_WINDOW, request_hide_compact, show_main, show_window,
};

/// 打开并聚焦主窗口，同时收起紧凑面板，避免两个入口争夺焦点。
#[tauri::command]
pub fn window_open_main(app: AppHandle) -> Result<(), CommandError> {
    request_hide_compact(&app);
    show_main(&app, MainNavigationTarget::Quota).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

#[tauri::command]
pub fn window_open_settings(app: AppHandle) -> Result<(), CommandError> {
    request_hide_compact(&app);
    show_main(&app, MainNavigationTarget::Settings).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

#[tauri::command]
pub fn window_open_onboarding(app: AppHandle) -> Result<(), CommandError> {
    request_hide_compact(&app);
    show_window(&app, ONBOARDING_WINDOW).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

/// 收起紧凑面板。`Esc` 与失焦之外的所有隐藏路径都经过这里。
#[tauri::command]
pub fn window_hide_compact(app: AppHandle) {
    request_hide_compact(&app);
}

/// 打开紧凑面板并锚定到系统区域图标。首次启动完成后由前端调用。
#[tauri::command]
pub fn window_open_compact(app: AppHandle) -> Result<(), CommandError> {
    desktop::show_compact_at_anchor(&app).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

/// 把紧凑面板的高度对齐到内容需要的高度，单位是逻辑像素。
///
/// 前端只报「内容需要多高」，收进允许区间、决定是否连带重新锚定都在平台层，
/// 前端因此拿不到任意改窗口尺寸的能力。
#[tauri::command]
pub fn window_set_compact_height(app: AppHandle, content_height: f64) -> Result<(), CommandError> {
    desktop::resize_compact(&app, content_height).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

/// 结束常驻进程。只有明确的「退出 CC Trace」走这里。
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}
