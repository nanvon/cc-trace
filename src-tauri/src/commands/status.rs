use crate::contracts::AppStatus;

/// 应用身份、版本与系统语言。
///
/// 版本号只有这一个来源，界面不得再硬编码；系统语言由本命令下发，前端不读
/// `navigator.language`，见 `docs/文案与国际化.md` 第 1 节。
#[tauri::command]
pub fn app_get_status() -> AppStatus {
    AppStatus {
        name: "CC Trace",
        version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        system_locale: sys_locale::get_locale().unwrap_or_else(|| "en".to_owned()),
    }
}
