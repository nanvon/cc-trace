use tauri::AppHandle;

use super::CommandError;
use crate::platform::desktop::{
    self, MAIN_WINDOW, ONBOARDING_WINDOW, SETTINGS_WINDOW, hide_compact, show_window,
};

/// 打开并聚焦主窗口，同时收起紧凑面板，避免两个入口争夺焦点。
#[tauri::command]
pub fn window_open_main(app: AppHandle) -> Result<(), CommandError> {
    hide_compact(&app);
    show_window(&app, MAIN_WINDOW).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

#[tauri::command]
pub fn window_open_settings(app: AppHandle) -> Result<(), CommandError> {
    hide_compact(&app);
    show_window(&app, SETTINGS_WINDOW).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

#[tauri::command]
pub fn window_open_onboarding(app: AppHandle) -> Result<(), CommandError> {
    hide_compact(&app);
    show_window(&app, ONBOARDING_WINDOW).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

/// 收起紧凑面板。`Esc` 与失焦之外的所有隐藏路径都经过这里。
#[tauri::command]
pub fn window_hide_compact(app: AppHandle) {
    hide_compact(&app);
}

/// 打开紧凑面板并锚定到系统区域图标。首次启动完成后由前端调用。
#[tauri::command]
pub fn window_open_compact(app: AppHandle) -> Result<(), CommandError> {
    desktop::show_compact_at_anchor(&app).map_err(|_| CommandError::WINDOW_UNAVAILABLE)
}

/// 结束常驻进程。只有明确的「退出 CC Trace」走这里。
#[tauri::command]
pub fn app_quit(app: AppHandle) {
    app.exit(0);
}
