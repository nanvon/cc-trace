use serde::Serialize;

/// 应用身份与运行环境。版本号从 `CARGO_PKG_VERSION` 读取，界面不得再硬编码第二份。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub name: &'static str,
    pub version: &'static str,
    pub platform: &'static str,
    /// 由 Rust 平台层解析的系统语言 BCP 47 标签，供「跟随系统」使用。
    /// 前端不读 `navigator.language`，见 `docs/文案与国际化.md` 第 1 节。
    pub system_locale: String,
}
