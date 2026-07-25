mod app;
mod commands;
mod contracts;
mod platform;
mod providers;
mod scheduler;
mod storage;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            commands::status::app_get_status
        ])
        .run(tauri::generate_context!())
        .expect("failed to run CC Trace");
}
