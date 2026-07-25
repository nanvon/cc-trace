use tauri::AppHandle;

use crate::platform::desktop::{MAIN_WINDOW, SETTINGS_WINDOW, hide_compact, show_window};

#[tauri::command]
pub(crate) fn window_open_main(app: AppHandle) -> Result<(), String> {
    hide_compact(&app);
    show_window(&app, MAIN_WINDOW)
}

#[tauri::command]
pub(crate) fn window_open_settings(app: AppHandle) -> Result<(), String> {
    hide_compact(&app);
    show_window(&app, SETTINGS_WINDOW)
}

#[tauri::command]
pub(crate) fn app_quit(app: AppHandle) {
    app.exit(0);
}
