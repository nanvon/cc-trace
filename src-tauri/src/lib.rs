//! CC Trace 桌面端。
//!
//! 分层见 `docs/技术架构.md`：Vue 只消费脱敏契约，业务与平台能力全部留在 Rust。

pub mod app;
pub mod commands;
pub mod contracts;
pub mod diagnostics;
pub mod platform;
// Provider 的取数结果就是 `ProviderFetchOutcome`（成功与失败同型，调度层按它分支），
// 它带一份快照与身份，超过 clippy 的 128 字节阈值。这是有意的形态：每次刷新构造一次、
// 不在任何热路径上，为消掉这条告警把 Err 装箱只会给全部调用点加一层间接与
// `map_err(Box::new)` 噪音。阈值本身与性能无关，这里按噪声处理。
#[allow(clippy::result_large_err)]
pub mod providers;
pub mod scheduler;
pub mod storage;
pub mod usage;

use std::sync::Arc;

use tauri::{Manager, WindowEvent};

use app::AppCore;
use platform::desktop::{
    self, COMPACT_WINDOW, MAIN_WINDOW, MainNavigationTarget, ONBOARDING_WINDOW,
    request_hide_compact,
};
use platform::floating::FLOATING_WINDOW;
use platform::strings::Lang;
use scheduler::RefreshTrigger;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        // 常驻托盘应用必须单实例：否则第二次启动会出现第二个图标，
        // 并让两个进程同时写同一份 settings.json。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            request_hide_compact(app);
            let onboarding_completed = app
                .try_state::<Arc<AppCore>>()
                .is_none_or(|core| core.settings().onboarding.completed);

            if onboarding_completed {
                let _ = desktop::show_main(app, MainNavigationTarget::Quota);
            } else {
                let _ = desktop::show_window(app, ONBOARDING_WINDOW);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        // 悬浮窗右键菜单是应用级菜单；托盘菜单的事件也会到这里，靠 id 区分。
        .on_menu_event(|app, event| platform::floating::on_menu_event(app, event.id().as_ref()))
        .setup(|app| {
            // 与 Swift 版 cc-bar 一致：平时是纯托盘的 accessory 应用，不出现在 Dock；
            // 只有主窗口打开期间才临时变成 regular，见 `platform::desktop::show_main` /
            // `leave_main`。快捷键仍由前端统一处理 metaKey / ctrlKey。
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config_dir = app.path().app_config_dir()?;
            let (core, _load_issue) = AppCore::new(config_dir.clone());
            let settings = core.settings();
            diagnostics::log::init(config_dir.join("logs"), settings.verbose_logging);
            diagnostics::log::info(
                "app",
                "startup",
                &[
                    ("version", env!("CARGO_PKG_VERSION")),
                    ("os", std::env::consts::OS),
                ],
            );

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

            // 悬浮窗按设置显示；位置与屏幕外回落由平台层处理。
            platform::floating::sync(&handle, &settings);

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

            // 启动时按设置检查一次更新；失败只写日志与内存状态，不打扰用户。
            if settings.check_updates_on_start {
                let update_handle = handle.clone();
                tauri::async_runtime::spawn(async move {
                    let status = diagnostics::update::check(env!("CARGO_PKG_VERSION")).await;
                    let _ = tauri::Emitter::emit(
                        &update_handle,
                        commands::diagnostics::EVENT_UPDATE_STATUS,
                        &status,
                    );
                });
            }

            // 首次启动未完成时优先进入引导，不直接弹出紧凑面板。
            if !settings.onboarding.completed {
                let _ = desktop::show_window(&handle, ONBOARDING_WINDOW);
            }

            Ok(())
        })
        .on_window_event(|window, event| match event {
            // 点击面板外部即收起，与「点击外部关闭」的交互约定一致。Windows 的
            // 失焦事件可靠；macOS 26 及以下由 `outside_click` 的全局监听补齐，
            // macOS 27 expanded session 活跃时则交给 AppKit 管理。
            WindowEvent::Focused(false) if window.label() == COMPACT_WINDOW => {
                // macOS 27 的 expanded session 自己管理焦点生命周期。面板内部的
                // 第一次点击可能伴随窗口激活／焦点迁移；这里若继续按旧逻辑 cancel，
                // didEnd 会抢在 WebView 的 click 前隐藏窗口。
                #[cfg(target_os = "macos")]
                if platform::macos_status_item::has_active_session() {
                    return;
                }
                request_hide_compact(window.app_handle());
            }
            // 全局监听收不到本应用自己的点击：面板开着时点击主窗口，同样收起。
            WindowEvent::Focused(true) if window.label() == MAIN_WINDOW => {
                request_hide_compact(window.app_handle());
            }
            // 关闭窗口不等于退出应用：进程继续驻留系统区域。
            WindowEvent::CloseRequested { api, .. }
                if matches!(
                    window.label(),
                    COMPACT_WINDOW | MAIN_WINDOW | ONBOARDING_WINDOW | FLOATING_WINDOW
                ) =>
            {
                api.prevent_close();
                if window.label() == COMPACT_WINDOW {
                    // macOS 27 下先结束 expanded session，窗口隐藏由 didEnd
                    // 回调兜底；main 与 onboarding 不参与 session，直接隐藏。
                    desktop::request_hide_compact(window.app_handle());
                } else {
                    let _ = window.hide();
                    if window.label() == MAIN_WINDOW {
                        desktop::leave_main(window.app_handle());
                    }
                }
            }
            // 悬浮窗被拖动：去抖后持久化位置并吸附屏幕边缘。
            WindowEvent::Moved(_) if window.label() == FLOATING_WINDOW => {
                platform::floating::on_moved(window.app_handle());
            }
            _ => {}
        });

    #[cfg(debug_assertions)]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::status::app_get_status,
        commands::diagnostics::diagnostics_export,
        commands::diagnostics::credential_sources_get,
        commands::diagnostics::diagnostics_reveal_logs,
        commands::diagnostics::update_check,
        commands::diagnostics::update_status,
        commands::diagnostics::update_open_release,
        commands::quota::quota_get_snapshot,
        commands::quota::quota_refresh,
        commands::codex_accounts::codex_reset_credits,
        commands::codex_accounts::codex_accounts_get,
        commands::codex_accounts::codex_accounts_import,
        commands::codex_accounts::codex_accounts_update,
        commands::codex_accounts::codex_accounts_remove,
        commands::codex_accounts::codex_accounts_reorder,
        commands::command_code::command_code_credential_state,
        commands::command_code::command_code_set_api_key,
        commands::command_code::command_code_clear_api_key,
        commands::service_status::service_status_get,
        commands::settings::settings_read,
        commands::settings::settings_update,
        commands::settings::onboarding_complete,
        commands::usage::usage_scan_start,
        commands::usage::usage_scan_cancel,
        commands::usage::usage_scan_status,
        commands::usage::usage_get_summary,
        commands::usage::usage_list_conversations,
        commands::usage::usage_list_conversation_projects,
        commands::usage::usage_list_projects,
        commands::usage::usage_get_project_breakdown,
        commands::usage::usage_reveal_project,
        commands::usage::usage_get_conversation,
        commands::usage::usage_get_conversation_breakdown,
        commands::usage::usage_get_quota_history,
        commands::usage::usage_reprice,
        commands::usage::usage_refresh_pricing_catalog,
        commands::usage::usage_rebuild_data,
        commands::window::window_open_main,
        commands::window::window_open_settings,
        commands::window::window_open_onboarding,
        commands::window::window_open_compact,
        commands::window::window_hide_compact,
        commands::window::window_set_compact_height,
        commands::window::window_set_floating_size,
        commands::window::window_floating_context_menu,
        commands::window::app_quit,
        commands::dev::dev_set_scenario,
    ]);

    #[cfg(not(debug_assertions))]
    let builder = builder.invoke_handler(tauri::generate_handler![
        commands::status::app_get_status,
        commands::diagnostics::diagnostics_export,
        commands::diagnostics::credential_sources_get,
        commands::diagnostics::diagnostics_reveal_logs,
        commands::diagnostics::update_check,
        commands::diagnostics::update_status,
        commands::diagnostics::update_open_release,
        commands::quota::quota_get_snapshot,
        commands::quota::quota_refresh,
        commands::codex_accounts::codex_reset_credits,
        commands::codex_accounts::codex_accounts_get,
        commands::codex_accounts::codex_accounts_import,
        commands::codex_accounts::codex_accounts_update,
        commands::codex_accounts::codex_accounts_remove,
        commands::codex_accounts::codex_accounts_reorder,
        commands::command_code::command_code_credential_state,
        commands::command_code::command_code_set_api_key,
        commands::command_code::command_code_clear_api_key,
        commands::service_status::service_status_get,
        commands::settings::settings_read,
        commands::settings::settings_update,
        commands::settings::onboarding_complete,
        commands::usage::usage_scan_start,
        commands::usage::usage_scan_cancel,
        commands::usage::usage_scan_status,
        commands::usage::usage_get_summary,
        commands::usage::usage_list_conversations,
        commands::usage::usage_list_conversation_projects,
        commands::usage::usage_list_projects,
        commands::usage::usage_get_project_breakdown,
        commands::usage::usage_reveal_project,
        commands::usage::usage_get_conversation,
        commands::usage::usage_get_conversation_breakdown,
        commands::usage::usage_get_quota_history,
        commands::usage::usage_reprice,
        commands::usage::usage_refresh_pricing_catalog,
        commands::usage::usage_rebuild_data,
        commands::window::window_open_main,
        commands::window::window_open_settings,
        commands::window::window_open_onboarding,
        commands::window::window_open_compact,
        commands::window::window_hide_compact,
        commands::window::window_set_compact_height,
        commands::window::window_set_floating_size,
        commands::window::window_floating_context_menu,
        commands::window::app_quit,
    ]);

    builder
        .run(tauri::generate_context!())
        .expect("failed to run CC Trace");
}
