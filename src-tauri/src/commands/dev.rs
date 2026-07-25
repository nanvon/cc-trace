//! 仅在 debug 构建中存在的验证辅助命令。
//!
//! 它切换的是 `providers::synthetic` 内部的合成场景，**不是第二套状态源**：
//! 数据仍然走 `AppCore` → `quota://updated` 这一条路径。release 构建里整个模块
//! 连同前端的场景切换器一起被编译掉。

use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::app::AppCore;
use crate::providers::synthetic::Scenario;

/// 切换验证场景，用于走查状态视觉矩阵的每一行。
#[tauri::command]
pub fn dev_set_scenario(app: AppHandle, core: State<'_, Arc<AppCore>>, scenario: Scenario) {
    core.inner().apply_scenario(&app, scenario);
}
