use crate::contracts::AppStatus;

#[tauri::command]
pub fn app_get_status() -> AppStatus {
    AppStatus {
        name: "CC Trace",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
    }
}
