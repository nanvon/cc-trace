//! 可展示错误契约。
//!
//! `AppError` 不替代 `ProviderAvailability`：可用性维度回答「为什么不能取得新数据」，
//! 本类型只补充 `Error` 这一维度取值下的**文案分支**，让界面能区分凭据类与协议类，
//! 见 `docs/文案与国际化.md` 第 4 节。
//!
//! 硬规则：错误载荷不携带 HTTP 状态码原文、响应体、请求头、Rust 枚举名、文件路径或凭据内容。

use serde::{Deserialize, Serialize};

/// 错误的用户可执行分支。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorKind {
    /// 凭据失效、被吊销或 token 刷新失败。下一步指向「在对应 CLI 重新登录」。
    Credentials,
    /// 响应无法解析、字段不兼容或其他非网络类失败。下一步指向「稍后重试或升级 CC Trace」。
    Protocol,
}

/// 展示层错误。前端按 `kind` 取标题、影响与下一步文案，不显示任何内部标识。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub kind: ErrorKind,
}

impl AppError {
    pub fn credentials() -> Self {
        Self {
            kind: ErrorKind::Credentials,
        }
    }

    pub fn protocol() -> Self {
        Self {
            kind: ErrorKind::Protocol,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_payload_only_carries_a_display_branch() {
        let json = serde_json::to_value(AppError::credentials()).expect("error serializes");
        assert_eq!(json["kind"], "credentials");
        assert_eq!(
            json.as_object().expect("object").len(),
            1,
            "AppError must not grow fields that could leak protocol detail"
        );
    }
}
