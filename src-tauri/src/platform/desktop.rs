use tauri::{AppHandle, Manager, PhysicalPosition, Position};

pub(crate) const COMPACT_WINDOW: &str = "compact";
pub(crate) const MAIN_WINDOW: &str = "main";
pub(crate) const SETTINGS_WINDOW: &str = "settings";

pub(crate) fn show_window(app: &AppHandle, label: &str) -> Result<(), String> {
    let window = app
        .get_webview_window(label)
        .ok_or_else(|| format!("window not found: {label}"))?;

    window.unminimize().map_err(|error| error.to_string())?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

pub(crate) fn hide_compact(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(COMPACT_WINDOW) {
        let _ = window.hide();
    }
}

pub(crate) fn toggle_compact(app: &AppHandle, cursor: PhysicalPosition<f64>) -> Result<(), String> {
    let window = app
        .get_webview_window(COMPACT_WINDOW)
        .ok_or_else(|| "compact window not found".to_string())?;

    if window.is_visible().map_err(|error| error.to_string())? {
        window.hide().map_err(|error| error.to_string())?;
        return Ok(());
    }

    position_compact(app, &window, cursor)?;
    window.show().map_err(|error| error.to_string())?;
    window.set_focus().map_err(|error| error.to_string())
}

fn position_compact(
    app: &AppHandle,
    window: &tauri::WebviewWindow,
    cursor: PhysicalPosition<f64>,
) -> Result<(), String> {
    let window_size = window.outer_size().map_err(|error| error.to_string())?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(|error| error.to_string())?
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or_else(|| "no monitor available for compact window".to_string())?;
    let work_area = monitor.work_area();

    let left = work_area.position.x as f64;
    let top = work_area.position.y as f64;
    let right = left + work_area.size.width as f64;
    let bottom = top + work_area.size.height as f64;
    let gap = 10.0;

    let preferred_x = cursor.x - window_size.width as f64 / 2.0;
    #[cfg(target_os = "macos")]
    let preferred_y = cursor.y + gap;
    #[cfg(not(target_os = "macos"))]
    let preferred_y = cursor.y - window_size.height as f64 - gap;

    let max_x = (right - window_size.width as f64).max(left);
    let max_y = (bottom - window_size.height as f64).max(top);
    let x = preferred_x.clamp(left, max_x).round() as i32;
    let y = preferred_y.clamp(top, max_y).round() as i32;

    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|error| error.to_string())
}
