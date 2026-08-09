use std::sync::Arc;

use tauri::State;

use crate::app::AppCore;
use crate::contracts::ServiceStatusState;

/// 读取当前官方服务状态。启动时先用它渲染，再等 `service-status://updated`。
///
/// 与额度状态是两条独立状态链（[ADR-0026]），载荷是公开的 Statuspage 信息。
/// [ADR-0026]: ../../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md
#[tauri::command]
pub fn service_status_get(core: State<'_, Arc<AppCore>>) -> ServiceStatusState {
    core.service_status()
}
