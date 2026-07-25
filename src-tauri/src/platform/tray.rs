use serde::Serialize;
use tauri::{
    Emitter,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

use super::desktop::{MAIN_WINDOW, SETTINGS_WINDOW, hide_compact, show_window, toggle_compact};

const MENU_OPEN: &str = "open";
const MENU_REFRESH: &str = "refresh";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PreviewRefreshEvent {
    source: &'static str,
}

pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, MENU_OPEN, "打开 CC Trace", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, MENU_REFRESH, "刷新额度", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, MENU_SETTINGS, "设置", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, "退出 CC Trace", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &refresh, &settings, &separator, &quit])?;

    let builder = TrayIconBuilder::with_id("cc-trace")
        .tooltip("CC Trace")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => {
                hide_compact(app);
                let _ = show_window(app, MAIN_WINDOW);
            }
            MENU_REFRESH => {
                let _ = app.emit(
                    "shell://refresh-preview",
                    PreviewRefreshEvent {
                        source: "tray-menu",
                    },
                );
            }
            MENU_SETTINGS => {
                hide_compact(app);
                let _ = show_window(app, SETTINGS_WINDOW);
            }
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                position,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = toggle_compact(tray.app_handle(), position);
            }
        });

    #[cfg(target_os = "macos")]
    // macOS recolors the alpha mask to match the active Menu Bar appearance.
    let builder = builder
        .icon(tauri::include_image!("icons/tray-symbol.png"))
        .icon_as_template(true);

    #[cfg(not(target_os = "macos"))]
    let builder = builder.icon(
        app.default_window_icon()
            .expect("CC Trace bundle icon is required")
            .clone(),
    );

    builder.build(app)?;
    Ok(())
}
