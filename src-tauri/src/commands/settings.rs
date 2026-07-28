use std::sync::Arc;

use tauri::{AppHandle, State};

use super::CommandError;
use crate::app::{self, AppCore};
use crate::contracts::{Settings, SettingsUpdate};
use crate::platform::autostart;

#[tauri::command]
pub fn settings_read(core: State<'_, Arc<AppCore>>) -> Settings {
    core.settings()
}

/// 更新偏好。写入成功后立即广播，界面无需重启即可生效。
///
/// 写入失败时保留原值并返回可展示的错误码，见 `docs/信息架构与核心流程.md` 第 7.3 节。
#[tauri::command]
pub fn settings_update(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
    update: SettingsUpdate,
) -> Result<Settings, CommandError> {
    let core = core.inner();
    let outcome = core
        .update_settings(&update)
        .map_err(|_| CommandError::SETTINGS_WRITE_FAILED)?;

    if update.launch_at_login.is_some() {
        autostart::apply(&app, outcome.settings.launch_at_login);
    }

    if outcome.schedule_changed && outcome.settings.onboarding.completed {
        app::start_auto_refresh(core, &app);
    }

    core.emit_settings(&app, &outcome.settings);
    Ok(outcome.settings)
}

/// 写入首次启动完成标记。这是唯一的写入入口。
///
/// 写入失败时保持未完成状态，下次启动仍会进入首次启动窗口。
#[tauri::command]
pub fn onboarding_complete(
    app: AppHandle,
    core: State<'_, Arc<AppCore>>,
) -> Result<Settings, CommandError> {
    let core = core.inner();
    let was_completed = core.settings().onboarding.completed;
    let settings = core
        .complete_onboarding()
        .map_err(|_| CommandError::SETTINGS_WRITE_FAILED)?;

    if !was_completed {
        app::start_auto_refresh(core, &app);
    }
    core.emit_settings(&app, &settings);
    Ok(settings)
}
