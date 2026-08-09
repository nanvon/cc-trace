//! CC Trace 桌面端。
//!
//! 分层见 `docs/技术架构.md`：Vue 只消费脱敏契约，业务与平台能力全部留在 Rust。

pub mod app;
pub mod commands;
pub mod contracts;
pub mod platform;
pub mod providers;
pub mod scheduler;
pub mod storage;
pub mod usage;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use app::AppCore;
use platform::desktop::{
    self, COMPACT_WINDOW, MAIN_WINDOW, MainNavigationTarget, ONBOARDING_WINDOW, hide_compact,
};
use platform::strings::Lang;
use scheduler::RefreshTrigger;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // 常驻托盘应用必须单实例：否则第二次启动会出现第二个图标，
        // 并让两个进程同时写同一份 settings.json。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            hide_compact(app);
            let onboarding_completed = app
                .try_state::<Arc<AppCore>>()
                .is_none_or(|core| core.settings().onboarding.completed);

            if onboarding_completed {
                let _ = desktop::show_main(app, MainNavigationTarget::Quota);
            } else {
                let _ = desktop::show_window(app, ONBOARDING_WINDOW);
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            // 与 Swift 版 cc-bar 一致：平时是纯托盘的 accessory 应用，不出现在 Dock；
            // 只有主窗口打开期间才临时变成 regular，见 `platform::desktop::show_main` /
            // `leave_main`。快捷键仍由前端统一处理 metaKey / ctrlKey。
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config_dir = app.path().app_config_dir()?;
            let (core, _load_issue) = AppCore::new(config_dir);
            let settings = core.settings();

            let status = commands::status::app_get_status();
            let lang = Lang::resolve(settings.language, &status.system_locale);

            platform::tray::install(app, lang)?;

            let handle = app.handle().clone();
            platform::autostart::apply(&handle, settings.launch_at_login);
            app.manage(Arc::clone(&core));

            // macOS 上未激活应用的窗口不会成为 key window，「失焦关闭」不可靠，
            // 由全局点击监听复刻 NSPopover 的 transient 行为，见 outside_click.rs。
            #[cfg(target_os = "macos")]
            platform::outside_click::install(&handle);

            // 缓存里已有快照时，菜单栏在第一次刷新完成前就显示上一份数值。
            core.emit_quota_state(&handle);

            // 已完成引导时启动后立即刷新。首次启动要先让用户看到权限说明，再由
            // 「检查本机」明确触发同一个刷新用例。
            if settings.onboarding.completed {
                core.refresh_all(&handle, RefreshTrigger::Startup);
                app::start_auto_refresh(&core, &handle);
                app::start_auto_usage_scan(&core);
            }

            // 官方服务状态不涉及凭据与权限，引导完成与否都立即拉取一次并进入
            // 5 分钟周期；失败保留旧值，不影响任何额度功能（ADR-0026）。
            app::start_auto_service_status(&core, &handle);

            // 首次启动未完成时优先进入引导，不直接弹出紧凑面板。
            if !settings.onboarding.completed {
                let _ = desktop::show_window(&handle, ONBOARDING_WINDOW);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // 点击面板外部即收起，与「点击外部关闭」的交互约定一致。Windows 的
            // 失焦事件可靠；macOS 上窗口未必成为 key，真正的关闭由
            // `outside_click` 的全局监听负责，这里只作为兜底。
            WindowEvent::Focused(false) if window.label() == COMPACT_WINDOW => {
                hide_compact(window.app_handle());
            }
            // 全局监听收不到本应用自己的点击：面板开着时点击主窗口，同样收起。
            WindowEvent::Focused(true) if window.label() == MAIN_WINDOW => {
                hide_compact(window.app_handle());
            }
            // 关闭窗口不等于退出应用：进程继续驻留系统区域。
            WindowEvent::CloseRequested { api, .. }
                if matches!(
                    window.label(),
                    COMPACT_WINDOW | MAIN_WINDOW | ONBOARDING_WINDOW
                ) =>
            {
                api.prevent_close();
                let _ = window.hide();
                if window.label() == MAIN_WINDOW {
                    desktop::leave_main(window.app_handle());
                }
            }
            _ => {}
        });

    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::status::app_get_status,
        commands::quota::quota_get_snapshot,
        commands::quota::quota_refresh,
        commands::service_status::service_status_get,
        commands::settings::settings_read,
        commands::settings::settings_update,
        commands::settings::onboarding_complete,
        commands::usage::usage_scan_start,
        commands::usage::usage_scan_cancel,
        commands::usage::usage_scan_status,
        commands::usage::usage_get_summary,
        commands::usage::usage_list_conversations,
        commands::usage::usage_get_conversation,
        commands::usage::usage_get_conversation_breakdown,
        commands::usage::usage_get_quota_history,
        commands::usage::usage_reprice,
        commands::usage::usage_refresh_pricing_catalog,
        commands::window::window_open_main,
        commands::window::window_open_settings,
        commands::window::window_open_onboarding,
        commands::window::window_open_compact,
        commands::window::window_hide_compact,
        commands::window::window_set_compact_height,
        commands::window::app_quit,
        commands::dev::dev_set_scenario,
    ]);

    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::status::app_get_status,
        commands::quota::quota_get_snapshot,
        commands::quota::quota_refresh,
        commands::service_status::service_status_get,
        commands::settings::settings_read,
        commands::settings::settings_update,
        commands::settings::onboarding_complete,
        commands::usage::usage_scan_start,
        commands::usage::usage_scan_cancel,
        commands::usage::usage_scan_status,
        commands::usage::usage_get_summary,
        commands::usage::usage_list_conversations,
        commands::usage::usage_get_conversation,
        commands::usage::usage_get_conversation_breakdown,
        commands::usage::usage_get_quota_history,
        commands::usage::usage_reprice,
        commands::usage::usage_refresh_pricing_catalog,
        commands::window::window_open_main,
        commands::window::window_open_settings,
        commands::window::window_open_onboarding,
        commands::window::window_open_compact,
        commands::window::window_hide_compact,
        commands::window::window_set_compact_height,
        commands::window::app_quit,
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("failed to run CC Trace");
}
