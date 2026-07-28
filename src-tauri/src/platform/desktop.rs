//! 窗口生命周期与系统区域锚定。
//!
//! 平台差异集中在这里：Vue 业务代码里没有任何 `isMac` / `isWindows` 判断。

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, LogicalSize, Manager, Position, Rect, Size, WebviewWindow};

pub const COMPACT_WINDOW: &str = "compact";
pub const MAIN_WINDOW: &str = "main";
pub const ONBOARDING_WINDOW: &str = "onboarding";
pub const EVENT_MAIN_NAVIGATION: &str = "navigation://main";

#[derive(Clone, Copy)]
pub enum MainNavigationTarget {
    Quota,
    Settings,
}

impl MainNavigationTarget {
    fn as_str(self) -> &'static str {
        match self {
            Self::Quota => "quota",
            Self::Settings => "settings",
        }
    }
}

/// 紧凑面板与系统区域图标之间的间隙。
const ANCHOR_GAP: f64 = 10.0;

/// 紧凑面板高度的允许区间，逻辑像素。
///
/// 面板高度由内容驱动：前端量出内容需要多高，这里 clamp 后应用到窗口，
/// 超过上限的部分在 lane 区内部滚动。取值见 `docs/设计方向与状态规范.md` 第 5.3 节。
///
/// 必须与 `tauri.conf.json` 里 compact 窗口的 `minHeight` / `maxHeight` 一致：
/// Tauri 不提供这两个值的 getter，且它自己也会按同样的边界截断 `set_size`，
/// 两处不一致会让 clamp 结果被二次截断。
pub const COMPACT_MIN_HEIGHT: f64 = 260.0;
pub const COMPACT_MAX_HEIGHT: f64 = 560.0;

/// 小于这个差值的高度变化不重设窗口。
///
/// 前端量测带亚像素抖动，逐次应用会在 Windows 上变成可见的位置抖动——那里每次
/// 高度变化都要跟着移动窗口。
const HEIGHT_EPSILON: f64 = 1.0;

/// 主点击去抖窗口。
///
/// 点击系统区域图标会先让紧凑面板失焦（从而隐藏），随后才到达 tray 的点击事件。
/// 只看 `is_visible()` 的话，第二次点击会「隐藏后立刻重新显示」，表现为面板关不掉。
/// 因此记录最近一次隐藏时刻，在这个窗口内把点击视为「关闭」而不是「打开」。
const TOGGLE_DEBOUNCE: Duration = Duration::from_millis(250);

/// 窗口不存在或无法显示。不携带 label、路径或系统错误原文。
#[derive(Debug, Clone, Copy)]
pub struct WindowError;

/// 系统区域图标的锚点，坐标系与 [`to_anchor_space`] 约定一致。
#[derive(Clone, Copy)]
struct Anchor {
    /// 图标的水平中心。
    center_x: f64,
    /// 面板要贴住的那条边：macOS 是图标底边，Windows 是图标顶边。
    edge_y: f64,
}

/// 把一个「物理」值换算到锚点使用的坐标系。
///
/// macOS 上 Tauri 的「物理坐标」其实是「逻辑点 × 被问对象自己的 scale factor」：
/// tray 事件按托盘所在屏换算，`work_area()` 按该显示器换算，`monitor_from_point()`
/// 却拿参数去比纯逻辑的显示器边界，`set_position(Physical)` 又会除以窗口当前所在屏
/// 的 scale factor。四处口径在多显示器（尤其混合 Retina 与 1x）下互不可比，面板会
/// 落到别的屏。所以这里一律换回逻辑点再运算，最后用 `Position::Logical` 输出。
///
/// Windows 的物理坐标是真实像素，全程可比，不做换算。
#[cfg(target_os = "macos")]
fn to_anchor_space(value: f64, scale: f64) -> f64 {
    value / scale
}

#[cfg(not(target_os = "macos"))]
fn to_anchor_space(value: f64, _scale: f64) -> f64 {
    value
}

/// 反推 tray 事件矩形用的是哪块屏的 scale factor。
///
/// `tray-icon` 按托盘所在屏的 backing scale factor 把矩形换算成「物理」值，却没有
/// 告诉我们那是哪块屏；只看数值区间也区分不出来——逻辑 x=2500 在 1x 屏上是 2500，
/// 按 2x 反推出的 1250 同样落在主屏范围内。菜单栏厚度是全系统统一的逻辑值（现代
/// macOS 为 24pt），Mac 的 backing scale factor 又只有 1 或 2，因此图标高度本身
/// 是这里唯一可靠的判据。
#[cfg(target_os = "macos")]
fn tray_rect_scale(icon_height: f64) -> f64 {
    if icon_height >= 36.0 { 2.0 } else { 1.0 }
}

#[cfg(not(target_os = "macos"))]
fn tray_rect_scale(_icon_height: f64) -> f64 {
    1.0
}

/// 由系统区域图标的矩形算出锚点。
///
/// 用图标矩形而不是光标位置：光标落在图标的哪个像素是随机的，面板会跟着左右偏，
/// 顶边还会压进菜单栏。矩形描述的是图标本身，面板才落在图标正下方。
fn anchor_from_tray_rect(rect: Rect) -> Anchor {
    // Tauri 把 `tray-icon` 的矩形原样包成 `Position::Physical` / `Size::Physical`，
    // 这里的 `1.0` 只是把那份原值取出来，不是任何显示器的 scale factor。
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);

    let scale = tray_rect_scale(size.height);
    let left = to_anchor_space(position.x, scale);
    let width = to_anchor_space(size.width, scale);

    #[cfg(target_os = "macos")]
    let edge_y = to_anchor_space(position.y + size.height, scale);
    #[cfg(not(target_os = "macos"))]
    let edge_y = to_anchor_space(position.y, scale);

    Anchor {
        center_x: left + width / 2.0,
        edge_y,
    }
}

fn last_hidden_at() -> &'static Mutex<Option<Instant>> {
    static LAST_HIDDEN: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    LAST_HIDDEN.get_or_init(|| Mutex::new(None))
}

fn last_anchor() -> &'static Mutex<Option<Anchor>> {
    static LAST_ANCHOR: OnceLock<Mutex<Option<Anchor>>> = OnceLock::new();
    LAST_ANCHOR.get_or_init(|| Mutex::new(None))
}

/// 打开已存在的窗口实例并聚焦。所有窗口都由配置预创建，这里不会创建第二个实例。
pub fn show_window(app: &AppHandle, label: &str) -> Result<(), WindowError> {
    let window = app.get_webview_window(label).ok_or(WindowError)?;

    present_window(&window)
}

/// 把一次性导航目标发给主窗口，再显示并聚焦同一个原生窗口。
pub fn show_main(app: &AppHandle, target: MainNavigationTarget) -> Result<(), WindowError> {
    let window = app.get_webview_window(MAIN_WINDOW).ok_or(WindowError)?;

    window
        .emit(EVENT_MAIN_NAVIGATION, target.as_str())
        .map_err(|_| WindowError)?;
    present_window(&window)
}

fn present_window(window: &WebviewWindow) -> Result<(), WindowError> {
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
pub fn toggle_compact(app: &AppHandle, icon_rect: Rect) -> Result<(), WindowError> {
    let anchor = anchor_from_tray_rect(icon_rect);
    *last_anchor().lock().expect("anchor lock") = Some(anchor);

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

    present_compact(app, &window, anchor)
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
    anchor: Anchor,
) -> Result<(), WindowError> {
    let height = window.outer_size().map_err(|_| WindowError)?.height as f64;

    position_compact(app, window, anchor, height)?;
    window.show().map_err(|_| WindowError)?;
    window.set_focus().map_err(|_| WindowError)
}

/// 把紧凑面板的高度调整到内容需要的高度。
///
/// 前端只上报「内容需要多高」，窗口尺寸的归属仍然在这里：面板高度和锚点位置是
/// 同一件事的两面，只有平台层同时知道两者才能不产生跳变。
pub fn resize_compact(app: &AppHandle, content_height: f64) -> Result<(), WindowError> {
    let window = app.get_webview_window(COMPACT_WINDOW).ok_or(WindowError)?;
    let target = clamp_compact_height(content_height);

    let scale = window.scale_factor().map_err(|_| WindowError)?;
    let current = window.inner_size().map_err(|_| WindowError)?;
    if (current.height as f64 / scale - target).abs() < HEIGHT_EPSILON {
        return Ok(());
    }
    let width = current.width as f64 / scale;

    // 高度和位置是同一件事的两面。Windows 的面板锚在底边，高度变了不重新锚定就会
    // 向下长出去盖住托盘图标；macOS 虽然锚在顶边，但面板长到超出工作区底部时同样
    // 要靠这里 clamp 回去。顺序不能反：先移到新位置再改尺寸，窗口是「先让出空隙、
    // 再长高填满」；反过来则是「先盖住托盘、再跳回来」。
    //
    // 面板不可见时不必重定位：下一次 present_compact 本来就会按新尺寸重新锚定。
    if window.is_visible().unwrap_or(false) {
        let anchor = match *last_anchor().lock().expect("anchor lock") {
            Some(anchor) => anchor,
            None => default_anchor(app)?,
        };
        position_compact(app, &window, anchor, target * scale)?;
    }

    window
        .set_size(Size::Logical(LogicalSize::new(width, target)))
        .map_err(|_| WindowError)
}

/// 把上报的内容高度收进允许区间。非有限值退回下限，不把窗口设成 NaN 尺寸。
fn clamp_compact_height(content_height: f64) -> f64 {
    if !content_height.is_finite() {
        return COMPACT_MIN_HEIGHT;
    }
    content_height.clamp(COMPACT_MIN_HEIGHT, COMPACT_MAX_HEIGHT)
}

/// 系统区域在两个平台的位置不同：macOS 在顶部菜单栏，Windows 在右下角托盘。
fn default_anchor(app: &AppHandle) -> Result<Anchor, WindowError> {
    let monitor = app
        .primary_monitor()
        .map_err(|_| WindowError)?
        .ok_or(WindowError)?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area();

    let left = to_anchor_space(area.position.x as f64, scale);
    let width = to_anchor_space(area.size.width as f64, scale);
    let top = to_anchor_space(area.position.y as f64, scale);

    #[cfg(target_os = "macos")]
    let edge_y = top;
    #[cfg(not(target_os = "macos"))]
    let edge_y = top + to_anchor_space(area.size.height as f64, scale);

    Ok(Anchor {
        center_x: left + width / 2.0,
        edge_y,
    })
}

/// 把紧凑面板放在锚点附近，并约束在锚点所在显示器的工作区内。
///
/// 高度由调用方传入而不是在这里读 `outer_size()`：改尺寸后系统上报的尺寸未必已经
/// 更新，读到旧值会把窗口定位到错误的位置。
fn position_compact(
    app: &AppHandle,
    window: &WebviewWindow,
    anchor: Anchor,
    window_height: f64,
) -> Result<(), WindowError> {
    // 尺寸和 scale factor 取自同一个窗口，这个换算与它当前落在哪块屏无关。
    let window_scale = window.scale_factor().map_err(|_| WindowError)?;
    let width = to_anchor_space(
        window.outer_size().map_err(|_| WindowError)?.width as f64,
        window_scale,
    );
    let height = to_anchor_space(window_height, window_scale);

    let monitor = app
        .monitor_from_point(anchor.center_x, anchor.edge_y)
        .map_err(|_| WindowError)?
        .or_else(|| app.primary_monitor().ok().flatten())
        .ok_or(WindowError)?;
    let monitor_scale = monitor.scale_factor();
    let work_area = monitor.work_area();

    let left = to_anchor_space(work_area.position.x as f64, monitor_scale);
    let top = to_anchor_space(work_area.position.y as f64, monitor_scale);
    let right = left + to_anchor_space(work_area.size.width as f64, monitor_scale);
    let bottom = top + to_anchor_space(work_area.size.height as f64, monitor_scale);

    let preferred_x = anchor.center_x - width / 2.0;
    #[cfg(target_os = "macos")]
    let preferred_y = anchor.edge_y + ANCHOR_GAP;
    #[cfg(not(target_os = "macos"))]
    let preferred_y = anchor.edge_y - height - ANCHOR_GAP;

    let max_x = (right - width).max(left);
    let max_y = (bottom - height).max(top);
    let x = preferred_x.clamp(left, max_x).round();
    let y = preferred_y.clamp(top, max_y).round();

    // macOS 必须用 Logical：`set_position(Physical)` 会除以窗口当前所在屏的 scale
    // factor，而目标屏未必是同一块，跨屏时面板会被这个除法送回原来那块屏。
    #[cfg(target_os = "macos")]
    let position = Position::Logical(tauri::LogicalPosition::new(x, y));
    #[cfg(not(target_os = "macos"))]
    let position = Position::Physical(tauri::PhysicalPosition::new(x as i32, y as i32));

    window.set_position(position).map_err(|_| WindowError)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_height_inside_the_range_is_used_as_is() {
        assert_eq!(clamp_compact_height(392.0), 392.0);
    }

    #[test]
    fn content_shorter_than_the_floor_keeps_the_panel_usable() {
        assert_eq!(clamp_compact_height(120.0), COMPACT_MIN_HEIGHT);
    }

    #[test]
    fn content_taller_than_the_ceiling_scrolls_instead_of_growing() {
        assert_eq!(clamp_compact_height(900.0), COMPACT_MAX_HEIGHT);
    }

    /// 前端量测理论上不会给出非有限值，但窗口尺寸设成 NaN 是不可恢复的。
    #[test]
    fn non_finite_content_height_falls_back_to_the_floor() {
        assert_eq!(clamp_compact_height(f64::NAN), COMPACT_MIN_HEIGHT);
        assert_eq!(clamp_compact_height(f64::INFINITY), COMPACT_MIN_HEIGHT);
        assert_eq!(clamp_compact_height(-1.0), COMPACT_MIN_HEIGHT);
    }
}
