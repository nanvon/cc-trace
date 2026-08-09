//! macOS 27 的 Expanded Interface Session 适配（运行时桥）。
//!
//! macOS 27 起，给状态栏图标绑定 `NSMenu` 后系统会接管左键（自动展开菜单），
//! tray-icon 的点击回调收不到事件，紧凑面板因此打不开。Apple 提供了
//! `NSStatusItem.expandedInterfaceDelegate`：绑定 delegate 后系统在左键点击时
//! 开始／结束 Expanded Interface Session，并通过 `statusItem:didBegin…` 与
//! `statusItemDidEndExpandedInterfaceSession:animated:` 通知应用，见
//! <https://developer.apple.com/documentation/appkit/nsstatusitem/expandedinterfacedelegate>。
//!
//! 本模块用 Objective-C runtime 动态探测并调用这套 macOS 27 API：SDK 仍是 26，
//! 编译期没有这些声明，全部走 `sel!` 宏 + `msg_send!`（selector 在运行时注册），
//! 且所有动态调用集中在这里。旧系统探测不到 selector 时本模块整体不生效，
//! 行为与 macOS 26 及以下完全一致。
//!
//! 事件流：左键由系统管理（didBegin → 显示紧凑面板，didEnd → 隐藏）；右键
//! 不走系统路径，由本模块的 local monitor 在右键按下时手动弹出未绑定的
//! `NSMenu`（绑定菜单会再次让系统接管左键，绝不能在 macOS 27 上调用
//! `set_menu` / `setMenu:`）。

use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicPtr, Ordering};

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, NSObject};
use objc2::{ClassType, define_class, msg_send, sel};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventType, NSMenu, NSMenuItem, NSStatusItem};
use objc2_foundation::{MainThreadMarker, NSInteger, NSString};
use tauri::AppHandle;

use super::desktop::{self, MainNavigationTarget};
use super::strings::{Lang, native};
use crate::platform::tray::{self, TRAY_ID};

/// 右键菜单项的 tag，与菜单项一一对应。
const TAG_OPEN: NSInteger = 0;
const TAG_REFRESH: NSInteger = 1;
const TAG_SETTINGS: NSInteger = 2;
const TAG_QUIT: NSInteger = 3;

/// AppHandle 供回调与菜单分发使用；进程内只装一次。
static APP: OnceLock<AppHandle> = OnceLock::new();

/// delegate 的强持有。`expandedInterfaceDelegate` 是 weak 属性，不持有会在
/// 第一次回调前被释放；与 `outside_click` 的监听句柄一样泄漏到进程结束。
static DELEGATE: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

/// 右键菜单（未绑定到状态栏图标）。语言切换时整体重建。
static CONTEXT_MENU: AtomicPtr<NSMenu> = AtomicPtr::new(std::ptr::null_mut());

/// 右键点击 local monitor 的句柄，与 `outside_click` 的 MONITOR 同模式。
static MONITOR: AtomicPtr<AnyObject> = AtomicPtr::new(std::ptr::null_mut());

/// 系统是否支持 expanded interface（macOS 27+）。探测 selector 存在性，
/// 不依赖版本号，保证旧系统和新 SDK 都正确。
pub fn supported() -> bool {
    static SUPPORTED: OnceLock<bool> = OnceLock::new();
    *SUPPORTED.get_or_init(|| {
        let Some(status_item_class) = AnyClass::get(c"NSStatusItem") else {
            return false;
        };
        unsafe {
            msg_send![status_item_class, instancesRespondToSelector: sel!(setExpandedInterfaceDelegate:)]
        }
    })
}

/// 安装 expanded interface delegate 与右键菜单。只在 [`supported`] 为真时调用；
/// `status_item` 来自 tray-icon 底层（`tray.with_inner_tray_icon`）。
pub fn install(app: &AppHandle, status_item: &NSStatusItem, lang: Lang) {
    if APP.get().is_some() {
        return;
    }
    let _ = APP.set(app.clone());

    let delegate: Retained<StatusItemDelegate> =
        unsafe { msg_send![StatusItemDelegate::class(), new] };
    let leaked = Box::leak(Box::new(delegate));
    DELEGATE.store(Retained::as_ptr(leaked) as *mut AnyObject, Ordering::SeqCst);

    unsafe {
        let _: () = msg_send![status_item, setExpandedInterfaceDelegate: &**leaked];
    }

    install_status_item_mouse_monitor(app);
    rebuild_context_menu(lang);
}

/// 结束当前 expanded session。由「请求收起紧凑面板」路径调用。
///
/// 返回是否真的结束了活跃 session：为 `true` 时窗口隐藏应交给 didEnd 回调
/// （等 AppKit 完成 session 收尾）；为 `false` 时没有 session 需要结束，
/// 调用方直接隐藏窗口。
pub fn end_active_session() -> bool {
    let Some(app) = APP.get() else {
        return false;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };
    tray.with_inner_tray_icon(|inner| {
        let Some(status_item) = inner.ns_status_item() else {
            return false;
        };
        let session: Option<Retained<AnyObject>> =
            unsafe { msg_send![&status_item, expandedInterfaceSession] };
        let Some(session) = session else { return false };
        unsafe {
            let _: () = msg_send![&session, cancel];
        }
        true
    })
    .unwrap_or(false)
}

/// 当前是否存在由系统管理的 expanded session。
///
/// session 活跃期间，点击外部与焦点迁移由 AppKit 判定；旧版的失焦／全局点击
/// 监听不能再主动取消，否则面板内部的第一次点击也可能在 WebView 收到 `click`
/// 前结束 session。
pub fn has_active_session() -> bool {
    let Some(app) = APP.get() else {
        return false;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };
    tray.with_inner_tray_icon(|inner| {
        let Some(status_item) = inner.ns_status_item() else {
            return false;
        };
        let session: Option<Retained<AnyObject>> =
            unsafe { msg_send![&status_item, expandedInterfaceSession] };
        session.is_some()
    })
    .unwrap_or(false)
}

/// 语言变更后重建右键菜单。菜单不绑定状态栏图标，只替换保存的引用；
/// 绝不调用 `set_menu`——那会把左键重新交还给菜单。
pub fn rebuild_context_menu(lang: Lang) {
    let Some(delegate) = delegate_ref() else {
        return;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let strings = native(lang);
    let action = sel!(ccTraceMenuAction:);

    unsafe {
        let menu = NSMenu::new(mtm);
        menu.setTitle(&NSString::from_str("CC Trace"));
        let add_item = |title: &'static str, tag: NSInteger| {
            let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                mtm.alloc(),
                &NSString::from_str(title),
                Some(action),
                &NSString::new(),
            );
            item.setTag(tag);
            item.setTarget(Some(delegate));
            menu.addItem(&item);
        };
        add_item(strings.open, TAG_OPEN);
        add_item(strings.refresh, TAG_REFRESH);
        add_item(strings.settings, TAG_SETTINGS);
        menu.addItem(&NSMenuItem::separatorItem(mtm));
        add_item(strings.quit, TAG_QUIT);

        let old = CONTEXT_MENU.swap(Retained::into_raw(menu), Ordering::SeqCst);
        if !old.is_null() {
            let _ = Retained::from_raw(old);
        }
    }
}

/// 状态栏鼠标监听。
///
/// - 第一次左键保持原事件，让 AppKit 开始 expanded session；
/// - session 活跃时再次左键，主动 cancel 并消费事件，避免系统紧接着重开；
/// - 右键手动弹出未绑定的上下文菜单，并消费事件。
fn install_status_item_mouse_monitor(app: &AppHandle) {
    if !MONITOR.load(Ordering::SeqCst).is_null() {
        return;
    }
    let app = app.clone();
    let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
        let event_ref = unsafe { event.as_ref() };
        match event_ref.r#type() {
            NSEventType::LeftMouseDown
                if has_active_session() && event_is_on_status_item(&app, event_ref) =>
            {
                end_active_session();
                std::ptr::null_mut()
            }
            NSEventType::RightMouseDown if show_context_menu_if_on_status_item(&app, event_ref) => {
                std::ptr::null_mut()
            }
            _ => event.as_ptr(),
        }
    });
    if let Some(monitor) = unsafe {
        NSEvent::addLocalMonitorForEventsMatchingMask_handler(
            NSEventMask::LeftMouseDown | NSEventMask::RightMouseDown,
            &block,
        )
    } {
        let leaked = Box::leak(Box::new(monitor));
        MONITOR.store(Retained::as_ptr(leaked) as *mut AnyObject, Ordering::SeqCst);
    }
}

/// 左键事件是否落在本应用的状态栏图标窗口内。
fn event_is_on_status_item(app: &AppHandle, event: &NSEvent) -> bool {
    let event_ptr = event as *const NSEvent as usize;
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };
    tray.with_inner_tray_icon(move |inner| {
        let Some(status_item) = inner.ns_status_item() else {
            return false;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(button) = status_item.button(mtm) else {
            return false;
        };
        let Some(button_window) = button.window() else {
            return false;
        };
        let event_window = unsafe { (&*(event_ptr as *const NSEvent)).window(mtm) };
        event_window
            .as_ref()
            .is_some_and(|window| std::ptr::eq(&**window, &*button_window))
    })
    .unwrap_or(false)
}

/// 右键落在本应用的状态栏图标上时，在事件位置弹出上下文菜单并返回 true。
///
/// 事件窗口与 status item 按钮所在窗口是同一个才认定；事件窗口为空
/// （部分系统事件没有窗口）时不认定，宁可不弹。
///
/// 事件对象通过裸指针传入闭包：`NSEvent` 是 MainThreadOnly，不能被
/// `with_inner_tray_icon` 的 Send 闭包捕获；监听回调与弹出都发生在主线程，
/// 指针在整个调用期间有效。
fn show_context_menu_if_on_status_item(app: &AppHandle, event: &NSEvent) -> bool {
    let event_ptr = event as *const NSEvent as usize;
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return false;
    };
    tray.with_inner_tray_icon(move |inner| {
        let Some(status_item) = inner.ns_status_item() else {
            return false;
        };
        let Some(mtm) = MainThreadMarker::new() else {
            return false;
        };
        let Some(button) = status_item.button(mtm) else {
            return false;
        };
        let Some(button_window) = button.window() else {
            return false;
        };
        let event_window = unsafe { (&*(event_ptr as *const NSEvent)).window(mtm) };
        if event_window
            .as_ref()
            .is_none_or(|window| !std::ptr::eq(&**window, &*button_window))
        {
            return false;
        }
        let Some(menu) = context_menu_ref() else {
            return false;
        };
        let event = unsafe { &*(event_ptr as *const NSEvent) };
        NSMenu::popUpContextMenu_withEvent_forView(menu, event, &button);
        true
    })
    .unwrap_or(false)
}

fn delegate_ref() -> Option<&'static StatusItemDelegate> {
    let ptr = DELEGATE.load(Ordering::SeqCst);
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { &*(ptr as *const StatusItemDelegate) })
}

fn context_menu_ref() -> Option<&'static NSMenu> {
    let ptr = CONTEXT_MENU.load(Ordering::SeqCst);
    if ptr.is_null() {
        return None;
    }
    Some(unsafe { &*ptr })
}

/// 菜单 action 的分发入口。四项职责与原生菜单一致，见 `docs/信息架构与
/// 核心流程.md` 第 4.2 节；「刷新额度」与界面共用同一个刷新用例。
fn handle_menu_action(sender: *mut AnyObject) {
    let Some(app) = APP.get() else { return };
    if sender.is_null() {
        return;
    }
    let item = unsafe { &*(sender as *const NSMenuItem) };
    match item.tag() {
        TAG_OPEN => tray::open_main(app, MainNavigationTarget::Quota),
        TAG_REFRESH => tray::refresh_all(app),
        TAG_SETTINGS => tray::open_main(app, MainNavigationTarget::Settings),
        TAG_QUIT => app.exit(0),
        _ => {}
    }
}

define_class!(
    #[unsafe(super(NSObject))]
    struct StatusItemDelegate;

    impl StatusItemDelegate {
        /// 左键点击展开 session 时由系统调用：在这里显示紧凑面板。
        /// session 对象不需要保存，取消时通过 status item 的
        /// `expandedInterfaceSession` 查询。
        #[unsafe(method(statusItem:didBeginExpandedInterfaceSession:))]
        fn did_begin(&self, _status_item: *mut AnyObject, _session: *mut AnyObject) {
            let Some(app) = APP.get() else { return };
            let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
            let Ok(Some(rect)) = tray.rect() else {
                return;
            };
            let _ = desktop::show_compact_from_tray_rect(app, rect);
        }

        /// session 结束时由系统调用（含我们主动 cancel 之后）：隐藏紧凑面板。
        #[unsafe(method(statusItemDidEndExpandedInterfaceSession:animated:))]
        fn did_end(&self, _status_item: *mut AnyObject, _animated: bool) {
            if let Some(app) = APP.get() {
                desktop::hide_compact_now(app);
            }
        }

        /// 右键菜单项的回调，按 tag 分发。
        #[unsafe(method(ccTraceMenuAction:))]
        fn menu_action(&self, sender: *mut AnyObject) {
            handle_menu_action(sender);
        }
    }
);
