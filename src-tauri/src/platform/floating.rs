//! 桌面悬浮窗（Floating HUD）的窗口行为。
//!
//! 对应 cc-bar 的 `FloatingPanelController`：置顶、无边框、不进任务栏、不抢焦点、
//! 可拖动并记住位置、屏幕外回落、拖到屏幕边 20 点内吸附。窗口内容由前端
//! `FloatingView` 用与紧凑面板相同的额度事件渲染，这里不持有任何额度数据。
//!
//! 坐标约定沿用 `desktop.rs` 的「锚点空间」：Windows 是虚拟桌面的物理像素，
//! macOS 是逻辑点（见 [`to_anchor_space`] 的说明）。设置里的 `hud.position` 存的就是
//! 这个空间里的窗口左上角。多显示器、混合 DPI 下物理像素在 Windows 上全局可比，
//! 因此不会因为窗口落在另一块缩放不同的屏上而错位。
//!
//! 平台事实（均未在 Windows 实机验证）：
//! - `skipTaskbar`、`alwaysOnTop`、`focusable: false` 都是 Tauri 窗口配置，Windows 上
//!   分别对应 `WS_EX_TOOLWINDOW` 类效果、`HWND_TOPMOST` 与 `WS_EX_NOACTIVATE`。
//! - 无边框透明窗口在 Windows 上 `shadow: false` 才没有 1px 白边。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tauri::menu::{ContextMenu, Menu, MenuItem};
use tauri::{AppHandle, LogicalSize, Manager, Position, Size};

use super::desktop::{MainNavigationTarget, to_anchor_space};
use super::strings::{Lang, native};
use crate::app::AppCore;
use crate::contracts::{HudPosition, HudSettings, Settings, SettingsUpdate};

pub const FLOATING_WINDOW: &str = "floating";

const MENU_HIDE: &str = "floating-hide";
const MENU_SETTINGS: &str = "floating-settings";

/// 默认位置离屏幕右上角的边距，逻辑像素。与 cc-bar 一致。
const DEFAULT_MARGIN: f64 = 16.0;
/// 拖到距工作区边缘小于这个距离时吸附，逻辑像素。与 cc-bar 一致。
const SNAP_THRESHOLD: f64 = 20.0;
/// 拖动停止多久后才持久化并吸附。窗口移动没有「拖动结束」事件，用去抖近似，
/// 与 cc-bar 的 200ms 一致。
const SETTLE_DELAY: Duration = Duration::from_millis(200);
/// 窗口至少要有这个比例的面积落在某个工作区内才算「可达」，否则回落到默认位置。
/// cc-bar 只要求相交；只露出 1 像素的窗口用户无法再抓住，所以这里收紧。
const MIN_VISIBLE_RATIO: f64 = 0.25;

/// 前端量出的内容尺寸的允许区间，逻辑像素。
const MIN_WIDTH: f64 = 120.0;
const MAX_WIDTH: f64 = 360.0;
const MIN_HEIGHT: f64 = 32.0;
const MAX_HEIGHT: f64 = 400.0;

/// 锚点空间里的矩形。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    fn right(&self) -> f64 {
        self.x + self.w
    }

    fn bottom(&self) -> f64 {
        self.y + self.h
    }

    fn overlap_area(&self, other: &Rect) -> f64 {
        let w = (self.right().min(other.right()) - self.x.max(other.x)).max(0.0);
        let h = (self.bottom().min(other.bottom()) - self.y.max(other.y)).max(0.0);
        w * h
    }
}

/// 窗口是否足够多地落在任一工作区里。
pub fn is_reachable(window: Rect, areas: &[Rect]) -> bool {
    let full = window.w * window.h;
    if full <= 0.0 {
        return false;
    }
    areas
        .iter()
        .any(|area| window.overlap_area(area) / full >= MIN_VISIBLE_RATIO)
}

/// 主屏工作区右上角，留 `margin` 边距。
pub fn default_origin(window: Rect, primary: Rect, margin: f64) -> (f64, f64) {
    (primary.right() - window.w - margin, primary.y + margin)
}

/// 距工作区某条边小于 `threshold` 时贴到那条边。左优先于右、上优先于下，与 cc-bar 一致。
pub fn snap_origin(window: Rect, area: Rect, threshold: f64) -> (f64, f64) {
    let mut x = window.x;
    let mut y = window.y;

    if (window.x - area.x).abs() < threshold {
        x = area.x;
    } else if (area.right() - window.right()).abs() < threshold {
        x = area.right() - window.w;
    }

    if (window.y - area.y).abs() < threshold {
        y = area.y;
    } else if (area.bottom() - window.bottom()).abs() < threshold {
        y = area.bottom() - window.h;
    }

    (x, y)
}

/// 与窗口重叠最多的工作区；都不重叠时取第一块。
fn dominant_area(window: Rect, areas: &[Rect]) -> Option<Rect> {
    areas.iter().copied().max_by(|a, b| {
        window
            .overlap_area(a)
            .partial_cmp(&window.overlap_area(b))
            .unwrap_or(std::cmp::Ordering::Equal)
    })
}

fn clamp_size(width: f64, height: f64) -> (f64, f64) {
    let fix = |value: f64, min: f64, max: f64| {
        if value.is_finite() {
            value.clamp(min, max)
        } else {
            min
        }
    };
    (
        fix(width, MIN_WIDTH, MAX_WIDTH),
        fix(height, MIN_HEIGHT, MAX_HEIGHT),
    )
}

/// 窗口运行期状态。只在主线程与去抖任务之间共享，粒度很小，用一把锁足够。
#[derive(Default)]
struct HudState {
    /// 最近一次由我们自己设置的位置。真实位置与它一致时不视为用户拖动，
    /// 避免默认位置被当成「用户保存的位置」写进设置。
    intended: Option<(f64, f64)>,
    /// 当前位置是否是回落出来的默认位置。内容尺寸变化时默认位置要跟着重算
    /// （靠右上角对齐），用户保存过的位置则保持左上角不动。
    at_default: bool,
}

fn state() -> &'static Mutex<HudState> {
    static STATE: OnceLock<Mutex<HudState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HudState::default()))
}

/// 拖动去抖代数。每次移动加一，延迟任务醒来发现代数变了就放弃。
static MOVE_GENERATION: AtomicU64 = AtomicU64::new(0);

/// 所有显示器的工作区，锚点空间。
fn work_areas(app: &AppHandle) -> Vec<Rect> {
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .map(|monitor| {
            let area = monitor.work_area();
            let scale = monitor.scale_factor();
            Rect {
                x: to_anchor_space(area.position.x as f64, scale),
                y: to_anchor_space(area.position.y as f64, scale),
                w: to_anchor_space(area.size.width as f64, scale),
                h: to_anchor_space(area.size.height as f64, scale),
            }
        })
        .collect()
}

fn primary_area(app: &AppHandle) -> Option<Rect> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let area = monitor.work_area();
    let scale = monitor.scale_factor();
    Some(Rect {
        x: to_anchor_space(area.position.x as f64, scale),
        y: to_anchor_space(area.position.y as f64, scale),
        w: to_anchor_space(area.size.width as f64, scale),
        h: to_anchor_space(area.size.height as f64, scale),
    })
}

/// 窗口当前的矩形，锚点空间。
fn window_rect(window: &tauri::WebviewWindow) -> Option<Rect> {
    let position = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().ok()?;
    Some(Rect {
        x: to_anchor_space(position.x as f64, scale),
        y: to_anchor_space(position.y as f64, scale),
        w: to_anchor_space(size.width as f64, scale),
        h: to_anchor_space(size.height as f64, scale),
    })
}

/// 把锚点空间的左上角应用到窗口。
///
/// Windows 的物理像素全局可比，直接 `Physical`；macOS 的锚点空间是逻辑点，
/// 用 `Logical` 才不会被窗口当前所在屏的 scale factor 二次换算。
fn apply_origin(window: &tauri::WebviewWindow, x: f64, y: f64) {
    #[cfg(target_os = "macos")]
    let position = Position::Logical(tauri::LogicalPosition::new(x, y));
    #[cfg(not(target_os = "macos"))]
    let position = Position::Physical(tauri::PhysicalPosition::new(
        x.round() as i32,
        y.round() as i32,
    ));

    let _ = window.set_position(position);
    state().lock().expect("hud state lock").intended = Some((x, y));
}

/// 把窗口放到设置里保存的位置；没有保存、或保存的位置已不在任何屏上时放到默认位置。
fn place(app: &AppHandle, window: &tauri::WebviewWindow, hud: &HudSettings) {
    let Some(rect) = window_rect(window) else {
        return;
    };
    let areas = work_areas(app);

    if let Some(saved) = hud.position {
        let candidate = Rect {
            x: saved.x,
            y: saved.y,
            ..rect
        };
        if is_reachable(candidate, &areas) {
            state().lock().expect("hud state lock").at_default = false;
            apply_origin(window, saved.x, saved.y);
            return;
        }
    }

    place_default(app, window, rect);
}

fn place_default(app: &AppHandle, window: &tauri::WebviewWindow, rect: Rect) {
    let Some(primary) = primary_area(app) else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    // 边距是逻辑像素；锚点空间在 Windows 上是物理像素，先乘回 scale。
    let margin = to_anchor_space(DEFAULT_MARGIN * scale, scale);
    let (x, y) = default_origin(rect, primary, margin);
    state().lock().expect("hud state lock").at_default = true;
    apply_origin(window, x, y);
}

/// 按设置同步显示状态。启动、设置变更时调用。
///
/// 已可见的窗口不重新定位：设置页回写 `hud` 时带的位置可能比窗口当前位置旧。
pub fn sync(app: &AppHandle, settings: &Settings) {
    let Some(window) = app.get_webview_window(FLOATING_WINDOW) else {
        return;
    };

    if !settings.hud.enabled {
        let _ = window.hide();
        return;
    }

    if window.is_visible().unwrap_or(false) {
        return;
    }

    place(app, &window, &settings.hud);
    // `focusable: false` 的窗口显示时不会激活，不打断用户当前的输入焦点。
    let _ = window.show();
}

/// 前端报告内容需要的尺寸，逻辑像素。
///
/// 跟紧凑面板一样，前端只报「内容多大」，是否接受、收到什么区间都在这里决定，
/// 前端因此拿不到任意改窗口尺寸的能力。
pub fn resize(app: &AppHandle, width: f64, height: f64) {
    let Some(window) = app.get_webview_window(FLOATING_WINDOW) else {
        return;
    };
    let (width, height) = clamp_size(width, height);
    let _ = window.set_size(Size::Logical(LogicalSize::new(width, height)));

    // 默认位置是靠右上角对齐的，宽度变了就要重算；用户放过的位置保持左上角不动。
    let at_default = state().lock().expect("hud state lock").at_default;
    if at_default
        && window.is_visible().unwrap_or(false)
        && let Some(rect) = window_rect(&window)
    {
        // `set_size` 在部分平台是异步生效的：用刚设的逻辑尺寸换算，不读回窗口。
        let scale = window.scale_factor().unwrap_or(1.0);
        let fresh = Rect {
            w: to_anchor_space(width * scale, scale),
            h: to_anchor_space(height * scale, scale),
            ..rect
        };
        place_default(app, &window, fresh);
    }
}

/// 窗口被移动（用户拖动或我们自己设置位置）。去抖后持久化并吸附。
pub fn on_moved(app: &AppHandle) {
    let generation = MOVE_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(SETTLE_DELAY).await;
        if MOVE_GENERATION.load(Ordering::SeqCst) != generation {
            return;
        }
        settle(&app);
    });
}

fn settle(app: &AppHandle) {
    let Some(window) = app.get_webview_window(FLOATING_WINDOW) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let Some(rect) = window_rect(&window) else {
        return;
    };

    // 与我们自己设置的位置一致：不是用户拖动，不写设置。
    let intended = state().lock().expect("hud state lock").intended;
    if let Some((x, y)) = intended
        && (x - rect.x).abs() < 1.0
        && (y - rect.y).abs() < 1.0
    {
        return;
    }

    let areas = work_areas(app);
    let (x, y) = match dominant_area(rect, &areas) {
        Some(area) => {
            let scale = window.scale_factor().unwrap_or(1.0);
            let threshold = to_anchor_space(SNAP_THRESHOLD * scale, scale);
            snap_origin(rect, area, threshold)
        }
        None => (rect.x, rect.y),
    };

    state().lock().expect("hud state lock").at_default = false;
    if (x - rect.x).abs() >= 1.0 || (y - rect.y).abs() >= 1.0 {
        apply_origin(&window, x, y);
    } else {
        state().lock().expect("hud state lock").intended = Some((x, y));
    }

    persist_position(app, x, y);
}

fn persist_position(app: &AppHandle, x: f64, y: f64) {
    let Some(core) = app.try_state::<Arc<AppCore>>() else {
        return;
    };
    let core = Arc::clone(core.inner());
    let current = core.settings().hud;
    let update = SettingsUpdate {
        hud: Some(HudSettings {
            position: Some(HudPosition { x, y }),
            ..current
        }),
        ..SettingsUpdate::default()
    };
    if let Ok(outcome) = core.update_settings(&update) {
        core.emit_settings(app, &outcome.settings);
    }
}

/// 右键菜单：隐藏悬浮窗（等同关闭总开关）、打开设置。
pub fn show_context_menu(app: &AppHandle) {
    let Some(window) = app
        .get_webview_window(FLOATING_WINDOW)
        .map(|webview| webview.as_ref().window())
    else {
        return;
    };
    let lang = current_lang(app);
    let strings = native(lang);

    let build = || -> tauri::Result<Menu<tauri::Wry>> {
        let hide = MenuItem::with_id(app, MENU_HIDE, strings.hide_floating, true, None::<&str>)?;
        let settings = MenuItem::with_id(app, MENU_SETTINGS, strings.settings, true, None::<&str>)?;
        Menu::with_items(app, &[&hide, &settings])
    };
    if let Ok(menu) = build() {
        let _ = menu.popup(window);
    }
}

/// 应用级菜单事件入口。托盘菜单的事件也会经过这里，靠 id 前缀区分，不相关的忽略。
pub fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        MENU_HIDE => {
            let Some(core) = app.try_state::<Arc<AppCore>>() else {
                return;
            };
            let core = Arc::clone(core.inner());
            let current = core.settings().hud;
            let update = SettingsUpdate {
                hud: Some(HudSettings {
                    enabled: false,
                    ..current
                }),
                ..SettingsUpdate::default()
            };
            if let Ok(outcome) = core.update_settings(&update) {
                core.emit_settings(app, &outcome.settings);
                sync(app, &outcome.settings);
            }
        }
        MENU_SETTINGS => super::tray::open_main(app, MainNavigationTarget::Settings),
        _ => {}
    }
}

fn current_lang(app: &AppHandle) -> Lang {
    let language = app
        .try_state::<Arc<AppCore>>()
        .map(|core| core.settings().language)
        .unwrap_or_default();
    Lang::resolve(
        language,
        &crate::commands::status::app_get_status().system_locale,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: Rect = Rect {
        x: 0.0,
        y: 0.0,
        w: 1920.0,
        h: 1080.0,
    };

    fn hud(x: f64, y: f64) -> Rect {
        Rect {
            x,
            y,
            w: 200.0,
            h: 80.0,
        }
    }

    #[test]
    fn a_window_inside_a_screen_is_reachable() {
        assert!(is_reachable(hud(100.0, 100.0), &[SCREEN]));
    }

    #[test]
    fn a_window_left_on_an_unplugged_monitor_is_not_reachable() {
        // 副屏在主屏右侧，拔掉后保存的位置落在 x=2500。
        assert!(!is_reachable(hud(2500.0, 100.0), &[SCREEN]));
    }

    #[test]
    fn a_window_with_only_a_sliver_visible_is_not_reachable() {
        assert!(!is_reachable(hud(1910.0, 100.0), &[SCREEN]));
    }

    #[test]
    fn a_window_on_a_second_monitor_with_negative_origin_is_reachable() {
        let left = Rect {
            x: -1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        assert!(is_reachable(hud(-1000.0, 50.0), &[left, SCREEN]));
    }

    #[test]
    fn the_default_position_is_the_top_right_corner_of_the_primary_work_area() {
        assert_eq!(
            default_origin(hud(0.0, 0.0), SCREEN, 16.0),
            (1920.0 - 200.0 - 16.0, 16.0)
        );
    }

    #[test]
    fn snapping_pulls_the_window_to_a_near_edge_only() {
        // 距右边 10、距上边 5：都在阈值内。
        let (x, y) = snap_origin(hud(1710.0, 5.0), SCREEN, 20.0);
        assert_eq!((x, y), (1720.0, 0.0));

        // 距左边 300：不动。
        let (x, y) = snap_origin(hud(300.0, 400.0), SCREEN, 20.0);
        assert_eq!((x, y), (300.0, 400.0));
    }

    #[test]
    fn snapping_prefers_the_left_and_top_edge_when_both_are_close() {
        let tiny = Rect {
            x: 0.0,
            y: 0.0,
            w: 30.0,
            h: 30.0,
        };
        let window = Rect {
            x: 5.0,
            y: 5.0,
            w: 20.0,
            h: 20.0,
        };
        assert_eq!(snap_origin(window, tiny, 20.0), (0.0, 0.0));
    }

    #[test]
    fn the_dominant_area_is_the_one_holding_most_of_the_window() {
        let right = Rect {
            x: 1920.0,
            y: 0.0,
            w: 1920.0,
            h: 1080.0,
        };
        // 窗口 200 宽，160 在副屏、40 在主屏。
        assert_eq!(
            dominant_area(hud(1880.0, 100.0), &[SCREEN, right]),
            Some(right)
        );
    }

    #[test]
    fn content_size_is_clamped_and_non_finite_values_fall_back_to_the_floor() {
        assert_eq!(clamp_size(50.0, 10.0), (MIN_WIDTH, MIN_HEIGHT));
        assert_eq!(clamp_size(9999.0, 9999.0), (MAX_WIDTH, MAX_HEIGHT));
        assert_eq!(clamp_size(f64::NAN, f64::INFINITY), (MIN_WIDTH, MIN_HEIGHT));
        assert_eq!(clamp_size(200.0, 80.0), (200.0, 80.0));
    }
}
