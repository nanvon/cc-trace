//! 系统区域图标与原生菜单。
//!
//! 菜单项固定四项，见 `docs/信息架构与核心流程.md` 第 4.2 节。「刷新额度」走与界面
//! 完全相同的刷新用例，因此不存在第二套刷新状态源。

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

use super::desktop::{MainNavigationTarget, hide_compact, show_main, toggle_compact};
use super::strings::{Lang, native};
use crate::app::AppCore;
use crate::scheduler::RefreshTrigger;

pub const TRAY_ID: &str = "cc-trace";

const MENU_OPEN: &str = "open";
const MENU_REFRESH: &str = "refresh";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";

pub fn install(app: &App, lang: Lang) -> tauri::Result<()> {
    let strings = native(lang);
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(strings.tooltip)
        .menu(&build_menu(app, lang)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => open_main(app, MainNavigationTarget::Quota),
            MENU_REFRESH => refresh_all(app),
            MENU_SETTINGS => open_main(app, MainNavigationTarget::Settings),
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
    // macOS 会按当前 Menu Bar 外观给 alpha 蒙版重新着色。
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

/// 语言变更后重建菜单，让原生文案与界面保持一致。
pub fn relocalize(app: &AppHandle, lang: Lang) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    tray.set_menu(Some(build_menu(app, lang)?))?;
    tray.set_tooltip(Some(native(lang).tooltip))?;
    Ok(())
}

fn build_menu<M: Manager<Wry>>(manager: &M, lang: Lang) -> tauri::Result<Menu<Wry>> {
    let strings = native(lang);

    let open = MenuItem::with_id(manager, MENU_OPEN, strings.open, true, None::<&str>)?;
    let refresh = MenuItem::with_id(manager, MENU_REFRESH, strings.refresh, true, None::<&str>)?;
    let settings = MenuItem::with_id(manager, MENU_SETTINGS, strings.settings, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(manager)?;
    let quit = MenuItem::with_id(manager, MENU_QUIT, strings.quit, true, None::<&str>)?;

    Menu::with_items(manager, &[&open, &refresh, &settings, &separator, &quit])
}

fn open_main(app: &AppHandle, target: MainNavigationTarget) {
    hide_compact(app);
    let _ = show_main(app, target);
}

/// 原生菜单的「刷新额度」与界面共用同一个用例：同一份请求合并、节流与退避。
fn refresh_all(app: &AppHandle) {
    let Some(core) = app.try_state::<Arc<AppCore>>() else {
        return;
    };
    let core = Arc::clone(core.inner());
    core.refresh_all(app, RefreshTrigger::Manual);
}
