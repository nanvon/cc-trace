//! 窗口生命周期与系统区域锚定。
//!
//! 平台差异集中在这里：Vue 业务代码里没有任何 `isMac` / `isWindows` 判断。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager, PhysicalPosition, Position, WebviewWindow};

pub const COMPACT_WINDOW: &str = "compact";
pub const MAIN_WINDOW: &str = "main";
pub const SETTINGS_WINDOW: &str = "settings";
pub const ONBOARDING_WINDOW: &str = "onboarding";

/// 紧凑面板与系统区域图标之间的间隙。
const ANCHOR_GAP: f64 = 10.0;

/// 主点击去抖窗口。
///
/// 点击系统区域图标会先让紧凑面板失焦（从而隐藏），随后才到达 tray 的点击事件。
/// 只看 `is_visible()` 的话，第二次点击会「隐藏后立刻重新显示」，表现为面板关不掉。
/// 因此记录最近一次隐藏时刻，在这个窗口内把点击视为「关闭」而不是「打开」。
const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(250);

/// 窗口不存在或无法显示。不携带 label、路径或系统错误原文。
#[derive(Debug, Clone, Copy)]
pub struct WindowError;

fn last_hidden_at() -> &'static Mutex<Option<Instant>> {
    static LAST_HIDDEN: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LAST_HIDDEN.get_or_init(|| Mutex::new(None))
}

fn last_anchor() -> &'static Mutex<Option<PhysicalPosition<f64>>> {
    static LAST_ANCHOR: OnceLock<Mutex<Option<PhysicalPosition<f64>>>> = OnceLock::new();
    LAST_ANCHOR.get_or_init(|| Mutex::new(None))
}

/// 打开已存在的窗口实例并聚焦。所有窗口都由配置预创建，这里不会创建第二个实例。
pub fn show_window(app: &AppHandle, label: &str) -> Result<(), WindowError> {
    let window = app.get_webview_window(label).ok_or(WindowError)?;

    window.unminimize().map_err(|_| WindowError)?;
    window.show().map_err(|_| WindowError)?;
    window.set_focus().map_err(|_| WindowError)
}

/// 收起紧凑面板，并记录隐藏时刻供主点击去抖使用。
pub fn hide_compact(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(COMPACT_WINDOW) {
        let was_visible = window.is_visible().unwrap_or(false);
        let _ = window.hide();
        if was_visible {
            *last_hidden_at().lock().expect("debounce lock") = Some(Instant::now());
        }
    }
}

/// 系统区域图标的主点击：打开或关闭紧凑面板。
pub fn toggle_compact(app: &AppHandle, cursor: PhysicalPosition<f64>) -> Result<(), WindowError> {
    *last_anchor().lock().expect("anchor lock") = Some(cursor);

    let window = app.get_webview_window(COMPACT_WINDOW).ok_or(WindowError)?;
    let visible = window.is_visible().map_err(|_| WindowError)?;
    let just_hidden = last_hidden_at()
        .lock()
        .expect("debounce lock")
        .is_some_and(|instant| instant.elapsed() < TOGGLE_DEBOUNCE);

    if visible || just_hidden {
        hide_compact(app);
        return Ok(());
    }

    present_compact(app, &window, cursor)
}

/// 在最近一次锚点处打开紧凑面板。没有锚点时退回当前显示器的系统区域一侧。
pub fn show_compact_at_anchor(app: &AppHandle) -> Result<(), WindowError> {
    let window = app.get_webview_window(COMPACT_WINDOW).ok_or(WindowError)?;
    let anchor = match *last_anchor().lock().expect("anchor lock") {
        Some(anchor) => anchor,
        None => default_anchor(app)?,
    };

    present_compact(app, &window, anchor)
}

fn present_compact(
    app: &AppHandle,
    window: &WebviewWindow,
    anchor: PhysicalPosition<f64>,
) -> Result<(), WindowError> {
    position_compact(app, window, anchor)?;
    window.show().map_err(|_| WindowError)?;
    window.set_focus().map_err(|_| WindowError)
}

/// 系统区域在两个平台的位置不同：macOS 在顶部菜单栏，Windows 在右下角托盘。
fn default_anchor(app: &AppHandle) -> Result<PhysicalPosition<f64>, WindowError> {
    let monitor = app
        .primary_monitor()
        .map_err(|_| WindowError)?
        .ok_or(WindowError)?;
    let area = monitor.work_area();

    let x = area.position.x as f64 + area.size.width as f64 / 2.0;
    #[cfg(target_os = "macos")]
    let y = area.position.y as f64;
    #[cfg(not(target_os = "macos"))]
    let y = area.position.y as f64 + area.size.height as f64;

    Ok(PhysicalPosition::new(x, y))
}

/// 把紧凑面板放在锚点附近，并约束在锚点所在显示器的工作区内。
fn position_compact(
    app: &AppHandle,
    window: &WebviewWindow,
    anchor: PhysicalPosition<f64>,
) -> Result<(), WindowError> {
    let window_size = window.outer_size().map_err(|_| WindowError)?;
    let monitor = app
        .monitor_from_point(anchor.x, anchor.y)
        .map_err(|_| WindowError)?
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or(WindowError)?;
    let work_area = monitor.work_area();

    let left = work_area.position.x as f64;
    let top = work_area.position.y as f64;
    let right = left + work_area.size.width as f64;
    let bottom = top + work_area.size.height as f64;

    let preferred_x = anchor.x - window_size.width as f64 / 2.0;
    #[cfg(target_os = "macos")]
    let preferred_y = anchor.y + ANCHOR_GAP;
    #[cfg(not(target_os = "macos"))]
    let preferred_y = anchor.y - window_size.height as f64 - ANCHOR_GAP;

    let max_x = (right - window_size.width as f64).max(left);
    let max_y = (bottom - window_size.height as f64).max(top);
    let x = preferred_x.clamp(left, max_x).round() as i32;
    let y = preferred_y.clamp(top, max_y).round() as i32;

    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|_| WindowError)
}
