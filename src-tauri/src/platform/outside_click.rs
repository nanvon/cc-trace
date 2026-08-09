//! macOS 全局点击监听：复刻 NSPopover 的 transient 关闭行为。
//!
//! compact 面板是普通 WebView 窗口。macOS 上未激活应用的窗口不会成为 key
//! window，`Focused(false)`（来自 `windowDidResignKey`）因此可能不触发，
//! 「点击外部关闭」失效。这里用 NSEvent 全局监听补齐旧系统与无 expanded
//! session 的显示路径：每次左键按下时判断点击点是否落在面板 frame 内，不在
//! 则收起。macOS 27 expanded session 活跃时不参与，由 AppKit 管理关闭时机。
//!
//! 全局监听收不到本应用自己的事件。托盘图标的点击由 `tray.rs` 的 toggle 与
//! 主点击去抖处理（全局监听先收起，tray 事件到达时已在去抖窗口内，不会重新
//! 弹出）；点击自己的主窗口在 `lib.rs` 的 `Focused(true)` 分支补齐。
//!
//! 坐标系：`NSEvent::mouseLocation()` 与 `NSWindow::frame()` 同为 AppKit 屏幕
//! 坐标（左下角原点、逻辑点），可直接比较，不需要 scale 换算。

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicPtr, Ordering};

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSEvent, NSEventMask, NSWindow};
use objc2_foundation::NSRect;
use tauri::{AppHandle, Manager};

use super::desktop::{COMPACT_WINDOW, request_hide_compact};

/// 全局监听句柄必须保持存活，否则 AppKit 会移除监听。句柄与进程同生命周期，
/// 用 `Box::leak` 泄漏持有；`Retained<AnyObject>` 不是 `Send`，这里只存指针。
static MONITOR: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

/// 安装全局点击监听。进程生命周期内只装一次，与窗口预创建策略一致。
pub fn install(app: &AppHandle) {
    if !MONITOR.load(Ordering::SeqCst).is_null() {
        return;
    }

    let app = app.clone();
    // AppKit 的 addGlobalMonitor 会 copy 并持有 block，RcBlock 只需存活到调用返回。
    let block = block2::RcBlock::new(move |_event: NonNull<NSEvent>| {
        let Some(frame) = compact_frame(&app) else {
            return;
        };
        let point = NSEvent::mouseLocation();
        if !click_inside_frame(
            point.x,
            point.y,
            frame.origin.x,
            frame.origin.y,
            frame.size.width,
            frame.size.height,
        ) {
            // expanded session 活跃时这里会先请求 AppKit cancel，再由 didEnd
            // 隐藏窗口；普通显示路径则直接隐藏。面板内部事件由本应用接收，
            // global monitor 不会抢在 WebView click 前触发。
            request_hide_compact(&app);
        }
    });

    if let Some(monitor) =
        NSEvent::addGlobalMonitorForEventsMatchingMask_handler(NSEventMask::LeftMouseDown, &block)
    {
        let leaked = Box::leak(Box::new(monitor));
        MONITOR.store(Retained::as_ptr(leaked) as *mut AnyObject, Ordering::SeqCst);
    }
}

/// 取紧凑面板当前的屏幕 frame；面板不可见时不返回，避免对隐藏窗口反复判定。
fn compact_frame(app: &AppHandle) -> Option<NSRect> {
    let window = app.get_webview_window(COMPACT_WINDOW)?;
    if !window.is_visible().unwrap_or(false) {
        return None;
    }
    let raw: *mut c_void = window.ns_window().ok()?;
    // 指针由 Tauri 持有，这里只借用不接管所有权。
    let ns_window: &NSWindow = unsafe { &*raw.cast::<NSWindow>() };
    Some(ns_window.frame())
}

/// 点击点是否落在窗口 frame 内。AppKit 屏幕坐标，左下角原点。
fn click_inside_frame(px: f64, py: f64, x: f64, y: f64, w: f64, h: f64) -> bool {
    (x..x + w).contains(&px) && (y..y + h).contains(&py)
}

#[cfg(test)]
mod tests {
    use super::click_inside_frame;

    #[test]
    fn click_inside_the_frame_stays_open() {
        assert!(click_inside_frame(200.0, 300.0, 100.0, 200.0, 380.0, 392.0));
    }

    #[test]
    fn click_outside_the_frame_hides() {
        assert!(!click_inside_frame(50.0, 300.0, 100.0, 200.0, 380.0, 392.0));
        assert!(!click_inside_frame(
            200.0, 100.0, 100.0, 200.0, 380.0, 392.0
        ));
    }

    #[test]
    fn click_on_the_left_border_is_inside() {
        assert!(click_inside_frame(100.0, 300.0, 100.0, 200.0, 380.0, 392.0));
    }
}
