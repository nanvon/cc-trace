use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app::AppCore;
use crate::contracts::{ProviderId, QuotaState};
use crate::scheduler::RefreshTrigger;

/// 读取当前展示状态。启动时先用它渲染，再等 `quota://updated`。
#[tauri::command]
pub fn quota_get_snapshot(core: State<'_, Arc<AppCore>>) -> QuotaState {
    core.quota_state()
}

/// 手动刷新。省略 `provider` 时刷新全部。
///
/// 请求合并、30 秒节流与退避都由调度层保证；**手动刷新不得绕过退避**，
/// 退避期内本命令不发起真实请求，只让界面拿到可再次尝试的时间。
#[tauri::command]
pub fn quota_refresh(app: AppHandle, core: State<'_, Arc<AppCore>>, provider: Option<ProviderId>) {
    let core = core.inner();
    match provider {
        Some(provider) => core.refresh_provider(&app, provider, RefreshTrigger::Manual),
        None => core.refresh_all(&app, RefreshTrigger::Manual),
    }
}
