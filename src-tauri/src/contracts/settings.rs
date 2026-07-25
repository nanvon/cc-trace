//! 应用偏好契约。
//!
//! 候选值与默认值属产品决策，见 `docs/产品范围.md`「基础设置」；持久化规则见
//! `docs/技术架构.md`「数据与恢复」。首次启动完成标记与其余偏好共用同一个 `schemaVersion`。

use serde::{Deserialize, Serialize};

/// `settings.json` 的结构版本。与应用版本相互独立，不跟随版本号变化。
pub const SETTINGS_SCHEMA_VERSION: u32 = 1;

/// 界面语言。首版只有简体中文与英文，不为其他语言预留取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LanguagePreference {
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "zh-CN")]
    ZhCn,
    #[serde(rename = "en")]
    En,
}

/// 外观偏好。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AppearancePreference {
    #[default]
    System,
    Light,
    Dark,
}

/// 自动刷新间隔。首版不提供关闭自动刷新，因此没有 `Off` 取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RefreshInterval {
    #[serde(rename = "15m")]
    FifteenMinutes,
    #[default]
    #[serde(rename = "30m")]
    ThirtyMinutes,
    #[serde(rename = "60m")]
    SixtyMinutes,
}

impl RefreshInterval {
    pub fn minutes(self) -> u64 {
        match self {
            Self::FifteenMinutes => 15,
            Self::ThirtyMinutes => 30,
            Self::SixtyMinutes => 60,
        }
    }

    pub fn seconds(self) -> u64 {
        self.minutes() * 60
    }
}

/// 首次启动状态。`completed` 只由 `onboarding_complete` 写入。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OnboardingState {
    pub completed: bool,
    /// ISO 8601 UTC。
    pub completed_at: Option<String>,
}

/// CC Trace 自己的偏好。不读取、不迁移任何外部或 Swift 版 cc-bar 的标记。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub language: LanguagePreference,
    pub appearance: AppearancePreference,
    pub refresh_interval: RefreshInterval,
    pub launch_at_login: bool,
    pub onboarding: OnboardingState,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            language: LanguagePreference::default(),
            appearance: AppearancePreference::default(),
            refresh_interval: RefreshInterval::default(),
            launch_at_login: false,
            onboarding: OnboardingState::default(),
        }
    }
}

/// 部分更新载荷。省略的字段保持原值，避免前端把未展示的偏好覆盖成默认值。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsUpdate {
    pub language: Option<LanguagePreference>,
    pub appearance: Option<AppearancePreference>,
    pub refresh_interval: Option<RefreshInterval>,
    pub launch_at_login: Option<bool>,
}

impl SettingsUpdate {
    /// 把非空字段合并进现有设置。首次启动标记不在此路径写入。
    pub fn apply_to(&self, settings: &mut Settings) {
        if let Some(language) = self.language {
            settings.language = language;
        }
        if let Some(appearance) = self.appearance {
            settings.appearance = appearance;
        }
        if let Some(interval) = self.refresh_interval {
            settings.refresh_interval = interval;
        }
        if let Some(launch_at_login) = self.launch_at_login {
            settings.launch_at_login = launch_at_login;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_product_scope() {
        let settings = Settings::default();
        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
        assert_eq!(settings.refresh_interval, RefreshInterval::ThirtyMinutes);
        assert_eq!(settings.refresh_interval.minutes(), 30);
        assert_eq!(settings.language, LanguagePreference::System);
        assert_eq!(settings.appearance, AppearancePreference::System);
        assert!(!settings.launch_at_login);
        assert!(!settings.onboarding.completed);
    }

    #[test]
    fn language_uses_the_locale_tags_the_ui_expects() {
        let json = serde_json::to_value(LanguagePreference::ZhCn).expect("serializes");
        assert_eq!(json, "zh-CN");
        let json = serde_json::to_value(LanguagePreference::System).expect("serializes");
        assert_eq!(json, "system");
    }

    #[test]
    fn missing_fields_fall_back_to_safe_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"language":"en"}"#).expect("parses");
        assert_eq!(settings.language, LanguagePreference::En);
        assert_eq!(settings.refresh_interval, RefreshInterval::ThirtyMinutes);
        assert_eq!(settings.schema_version, SETTINGS_SCHEMA_VERSION);
    }

    #[test]
    fn partial_update_leaves_untouched_fields_alone() {
        let mut settings = Settings::default();
        settings.onboarding.completed = true;

        let update = SettingsUpdate {
            appearance: Some(AppearancePreference::Dark),
            ..SettingsUpdate::default()
        };
        update.apply_to(&mut settings);

        assert_eq!(settings.appearance, AppearancePreference::Dark);
        assert_eq!(settings.refresh_interval, RefreshInterval::ThirtyMinutes);
        assert!(
            settings.onboarding.completed,
            "settings_update must not reset the onboarding marker"
        );
    }
}
