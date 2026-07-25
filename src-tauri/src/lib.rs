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
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            platform::tray::install(app)?;
            Ok(())
        })
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::Focused(false)
                if window.label() == platform::desktop::COMPACT_WINDOW =>
            {
                let _ = window.hide();
            }
            tauri::WindowEvent::CloseRequested { api, .. }
                if matches!(
                    window.label(),
                    platform::desktop::COMPACT_WINDOW
                        | platform::desktop::MAIN_WINDOW
                        | platform::desktop::SETTINGS_WINDOW
                ) =>
            {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::status::app_get_status,
            commands::window::window_open_main,
            commands::window::window_open_settings,
            commands::window::app_quit
        ])
        .run(tauri::generate_context!())
        .expect("failed to run CC Trace");
}
