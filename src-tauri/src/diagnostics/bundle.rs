//! 诊断包内容：版本、平台、设置摘要、各服务状态与最近日志。
//!
//! 只写枚举、布尔、计数与时间戳；不写路径、账号、邮箱、导入账号名称与任何凭据。
//! 日志尾部在拼入前再过一遍 [`super::redact::redact`]。

use serde_json::{Value, json};

use crate::contracts::{ProviderSnapshot, QuotaState, Settings};

use super::log;
use super::redact::redact;

const LOG_TAIL_LINES: usize = 400;

fn enum_label<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".to_owned())
}

/// 设置摘要：序列化后去掉悬浮窗坐标（位置属于屏幕布局，不是排查所需）。
pub fn settings_summary(settings: &Settings) -> Value {
    let mut value = serde_json::to_value(settings).unwrap_or(Value::Null);
    if let Some(hud) = value.get_mut("hud").and_then(Value::as_object_mut) {
        hud.remove("position");
    }
    value
}

fn provider_summary(snapshot: &ProviderSnapshot) -> Value {
    json!({
        "subject": snapshot.subject_id,
        "provider": snapshot.provider.key(),
        "kind": enum_label(&snapshot.kind),
        "refresh": enum_label(&snapshot.refresh),
        "freshness": enum_label(&snapshot.freshness),
        "availability": enum_label(&snapshot.availability),
        "hasSnapshot": snapshot.snapshot.is_some(),
        "windowCount": snapshot.snapshot.as_ref().map(|s| s.windows.len()).unwrap_or(0),
        "errorKind": snapshot.error.as_ref().map(|error| enum_label(&error.kind)),
        "lastSuccessAt": snapshot.last_success_at,
        "lastAttemptAt": snapshot.last_attempt_at,
        "retryAfter": snapshot.retry_after,
    })
}

/// 组装诊断文本。`imported_accounts` 只是数量。
pub fn build(
    version: &str,
    settings: &Settings,
    quota: &QuotaState,
    imported_accounts: usize,
) -> String {
    let report = json!({
        "app": {
            "name": "CC Trace",
            "version": version,
            "os": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "generatedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        },
        "settings": settings_summary(settings),
        "importedCodexAccounts": imported_accounts,
        "services": quota.providers.iter().map(provider_summary).collect::<Vec<_>>(),
    });

    let mut text = String::new();
    text.push_str("# CC Trace diagnostics (redacted)\n");
    text.push_str(&serde_json::to_string_pretty(&report).unwrap_or_default());
    text.push_str("\n\n# Log tail\n");
    for line in log::tail(LOG_TAIL_LINES) {
        text.push_str(&redact(&line));
        text.push('\n');
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_summary_drops_hud_position() {
        let mut settings = Settings::default();
        settings.hud.position = Some(crate::contracts::HudPosition { x: 10.0, y: 20.0 });
        let value = settings_summary(&settings);
        assert!(value["hud"].get("position").is_none());
    }

    #[test]
    fn bundle_has_no_paths_or_labels() {
        let quota = QuotaState {
            providers: vec![ProviderSnapshot::initial(
                crate::contracts::ProviderId::Codex,
            )],
        };
        let text = build("0.0.0", &Settings::default(), &quota, 2);
        assert!(text.contains("\"provider\": \"codex\""));
        assert!(text.contains("\"importedCodexAccounts\": 2"));
        assert!(!text.contains("/Users/"));
    }
}
