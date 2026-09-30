//! 应用偏好契约。
//!
//! 候选值与默认值属产品决策，见 `docs/产品范围.md`「基础设置」；持久化规则见
//! `docs/技术架构.md`「数据与恢复」。首次启动完成标记与其余偏好共用同一个 `schemaVersion`。
//!
//! v2 把 v1 的单一 `usage_service_visibility` 换成按服务分组的四组开关
//! （额度／菜单栏／悬浮窗／统计），对齐 cc-bar 的「服务与账号」矩阵，见
//! [ADR-0031](../../../docs/决策/ADR-0031-功能基准改为cc-bar-v1.1.1.md)。

use serde::{Deserialize, Serialize};

use super::quota::ProviderId;
use super::usage::UsageSource;

/// `settings.json` 的结构版本。与应用版本相互独立，不跟随版本号变化。
pub const SETTINGS_SCHEMA_VERSION: u32 = 2;

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

/// 额度自动刷新间隔与日志扫描间隔共用的档位。两个设置各自独立取值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum RefreshInterval {
    #[serde(rename = "1m")]
    OneMinute,
    #[default]
    #[serde(rename = "2m")]
    TwoMinutes,
    #[serde(rename = "3m")]
    ThreeMinutes,
    #[serde(rename = "5m")]
    FiveMinutes,
    #[serde(rename = "10m")]
    TenMinutes,
}

impl RefreshInterval {
    pub fn minutes(self) -> u64 {
        match self {
            Self::OneMinute => 1,
            Self::TwoMinutes => 2,
            Self::ThreeMinutes => 3,
            Self::FiveMinutes => 5,
            Self::TenMinutes => 10,
        }
    }

    pub fn seconds(self) -> u64 {
        self.minutes() * 60
    }
}

/// 排行口径：用量构成、高消耗对话、项目与对话列表的默认排序与占比依据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RankingBasis {
    #[default]
    Tokens,
    Cost,
}

/// 重置时间显示：剩余时长或具体时刻。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResetTimeDisplay {
    #[default]
    Duration,
    DateTime,
}

/// 系统区域入口承载哪个额度窗口。
///
/// Windows 托盘只能放静态图标与 tooltip，多段百分比的菜单栏标签不适用；这个设置决定
/// tooltip 里写主要额度、周额度还是两者，见 [ADR-0017] 与 [ADR-0031]。
/// [ADR-0017]: ../../../../docs/决策/ADR-0017-系统区域显示额度数字与余量分档.md
/// [ADR-0031]: ../../../../docs/决策/ADR-0031-功能基准改为cc-bar-v1.1.1.md
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MenuBarWindowMode {
    #[default]
    Primary,
    Weekly,
    Both,
}

/// 单个服务的四组开关。四组互相独立：关闭统计不影响额度轮询，关闭额度卡片不影响扫描。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceSettings {
    /// 额度卡片与后台额度轮询。
    pub quota: bool,
    /// 系统区域 tooltip 是否承载该服务的额度文本。
    pub menu_bar: bool,
    /// 桌面悬浮窗是否显示该服务行。
    pub hud: bool,
    /// 是否计入用量统计（侧栏筛选、KPI、图表、对话与项目页）。
    pub stats: bool,
}

impl ServiceSettings {
    pub const fn all(value: bool) -> Self {
        Self {
            quota: value,
            menu_bar: value,
            hud: value,
            stats: value,
        }
    }
}

impl Default for ServiceSettings {
    /// 单个服务对象里缺字段时按「开启」补齐；整个服务键缺失时由
    /// [`ServicesSettings`] 的默认值决定，两者不是一回事。
    fn default() -> Self {
        Self::all(true)
    }
}

/// 五个服务的开关矩阵。默认值与 cc-bar 一致：
/// Codex 与 Claude Code 全开；Antigravity 由首次检测到凭据时单独开启；
/// Cursor 与 Command Code 默认关闭，需用户手动开启。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicesSettings {
    pub codex: ServiceSettings,
    pub claude: ServiceSettings,
    pub antigravity: ServiceSettings,
    pub cursor: ServiceSettings,
    pub command_code: ServiceSettings,
    /// Pi／OpenCode／DSH 三个无额度的本地数据源共用统计开关；默认开启。
    pub local_agent_stats: bool,
}

/// 反序列化补丁：只覆盖文件里真实出现的键，其余保持服务默认值。
/// 直接用 `#[serde(default)]` 会把缺失的 Cursor 行补成「全开」，与默认关闭矛盾。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServicesSettingsPatch {
    codex: Option<ServiceSettings>,
    claude: Option<ServiceSettings>,
    antigravity: Option<ServiceSettings>,
    cursor: Option<ServiceSettings>,
    command_code: Option<ServiceSettings>,
    local_agent_stats: Option<bool>,
}

impl<'de> Deserialize<'de> for ServicesSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let patch = ServicesSettingsPatch::deserialize(deserializer)?;
        let mut settings = ServicesSettings::default();
        if let Some(value) = patch.codex {
            settings.codex = value;
        }
        if let Some(value) = patch.claude {
            settings.claude = value;
        }
        if let Some(value) = patch.antigravity {
            settings.antigravity = value;
        }
        if let Some(value) = patch.cursor {
            settings.cursor = value;
        }
        if let Some(value) = patch.command_code {
            settings.command_code = value;
        }
        if let Some(value) = patch.local_agent_stats {
            settings.local_agent_stats = value;
        }
        Ok(settings)
    }
}

impl Default for ServicesSettings {
    fn default() -> Self {
        Self {
            codex: ServiceSettings::all(true),
            claude: ServiceSettings::all(true),
            // 检测到 Antigravity 登录时由首次检测写入 true，默认关闭。
            antigravity: ServiceSettings::all(false),
            cursor: ServiceSettings::all(false),
            command_code: ServiceSettings::all(false),
            local_agent_stats: true,
        }
    }
}

impl ServicesSettings {
    pub fn get(&self, provider: ProviderId) -> ServiceSettings {
        match provider {
            ProviderId::Codex => self.codex,
            ProviderId::Claude => self.claude,
            ProviderId::Antigravity => self.antigravity,
            ProviderId::Cursor => self.cursor,
            ProviderId::CommandCode => self.command_code,
        }
    }

    pub fn set(&mut self, provider: ProviderId, value: ServiceSettings) {
        match provider {
            ProviderId::Codex => self.codex = value,
            ProviderId::Claude => self.claude = value,
            ProviderId::Antigravity => self.antigravity = value,
            ProviderId::Cursor => self.cursor = value,
            ProviderId::CommandCode => self.command_code = value,
        }
    }

    /// 计入统计的数据源集合。Antigravity 与 Command Code 没有本地用量，不参与统计。
    pub fn stats_sources(&self) -> Vec<UsageSource> {
        let mut sources = Vec::new();
        if self.codex.stats {
            sources.push(UsageSource::Codex);
        }
        if self.claude.stats {
            sources.push(UsageSource::Claude);
        }
        if self.pi_stats() {
            sources.push(UsageSource::Pi);
        }
        if self.opencode_stats() {
            sources.push(UsageSource::Opencode);
        }
        if self.dsh_stats() {
            sources.push(UsageSource::Dsh);
        }
        if self.cursor.stats {
            sources.push(UsageSource::Cursor);
        }
        sources
    }

    /// Pi／OpenCode／DSH 没有额度，只有统计开关，共用 `local_agent_stats`。
    pub fn pi_stats(&self) -> bool {
        self.local_agent_stats
    }

    pub fn opencode_stats(&self) -> bool {
        self.local_agent_stats
    }

    pub fn dsh_stats(&self) -> bool {
        self.local_agent_stats
    }
}

/// Command Code 凭据读取偏好。自动模式按「CLI → Pi → OpenCode → 环境变量 → 手动」
/// 依次尝试；手动模式只看用户填写的 API Key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CommandCodeCredentialPreference {
    #[default]
    Automatic,
    Manual,
}

/// 桌面悬浮窗位置（逻辑像素，左上角原点）。屏幕外或未设置时回落到默认右上角。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HudPosition {
    pub x: f64,
    pub y: f64,
}

/// 桌面悬浮窗偏好。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HudSettings {
    /// 默认关闭，需用户在设置中开启（cc-bar 默认值）。
    pub enabled: bool,
    pub position: Option<HudPosition>,
}

/// 首次启动状态。`completed` 只由 `onboarding_complete` 写入。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OnboardingState {
    pub completed: bool,
    /// ISO 8601 UTC。
    pub completed_at: Option<String>,
}

/// v1 遗留的统计服务可见性字段。只读用于迁移，不再写入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LegacyUsageServiceVisibility {
    pub codex: Option<bool>,
    pub claude: Option<bool>,
    pub pi: Option<bool>,
    pub opencode: Option<bool>,
}

/// CC Trace 自己的偏好。不读取、不迁移任何外部或 Swift 版 cc-bar 的标记。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub schema_version: u32,
    pub language: LanguagePreference,
    pub appearance: AppearancePreference,
    /// 额度自动刷新间隔。
    pub refresh_interval: RefreshInterval,
    /// 本地日志扫描间隔；默认 5 分钟，启动后立即扫描一次。
    pub scan_interval: RefreshInterval,
    pub launch_at_login: bool,
    /// 隐私模式：全局隐藏账号、项目与对话身份，保留真实用量统计。
    pub privacy_mode: bool,
    /// 服务状态圆点：只控制紧凑面板的 Statuspage 状态点是否绘制，
    /// 后台 5 分钟拉取不受开关影响，见 [ADR-0026]。
    /// [ADR-0026]: ../../../../docs/决策/ADR-0026-Statuspage状态链进入首版.md
    pub show_service_status: bool,
    /// 系统区域承载哪个额度窗口。
    pub menu_bar_window_mode: MenuBarWindowMode,
    /// Command Code 的凭据读取偏好。
    pub command_code_credential: CommandCodeCredentialPreference,
    pub services: ServicesSettings,
    pub hud: HudSettings,
    pub ranking_basis: RankingBasis,
    pub reset_time_display: ResetTimeDisplay,
    /// 启动时检查更新；默认开启。
    pub check_updates_on_start: bool,
    /// 详细日志；默认关闭。
    pub verbose_logging: bool,
    pub onboarding: OnboardingState,
    /// v1 遗留字段，仅出现在旧文件里，不会被写回。
    #[serde(rename = "usageServiceVisibility", default, skip_serializing)]
    pub legacy_usage_service_visibility: Option<LegacyUsageServiceVisibility>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            schema_version: SETTINGS_SCHEMA_VERSION,
            language: LanguagePreference::default(),
            appearance: AppearancePreference::default(),
            refresh_interval: RefreshInterval::TwoMinutes,
            scan_interval: RefreshInterval::FiveMinutes,
            launch_at_login: false,
            privacy_mode: false,
            show_service_status: true,
            menu_bar_window_mode: MenuBarWindowMode::Primary,
            command_code_credential: CommandCodeCredentialPreference::default(),
            services: ServicesSettings::default(),
            hud: HudSettings::default(),
            ranking_basis: RankingBasis::default(),
            reset_time_display: ResetTimeDisplay::default(),
            check_updates_on_start: true,
            verbose_logging: false,
            onboarding: OnboardingState::default(),
            legacy_usage_service_visibility: None,
        }
    }
}

impl Settings {
    /// 把 v1 的 `usageServiceVisibility` 折算进 `services.*.stats`，然后丢弃旧字段。
    /// 字段缺失表示 v1 没写过该开关，保持当前值不动。
    pub fn migrate_legacy(&mut self) {
        let Some(legacy) = self.legacy_usage_service_visibility.take() else {
            return;
        };

        if let Some(value) = legacy.codex {
            self.services.codex.stats = value;
        }
        if let Some(value) = legacy.claude {
            self.services.claude.stats = value;
        }
        // v1 允许逐个关闭 Pi／OpenCode；v2 的本地数据源共用总开关，
        // 只在两者都被显式关掉时才关掉总开关，避免把未写过的开关误判成关闭。
        let agent_sources = [legacy.pi, legacy.opencode];
        if agent_sources.iter().all(|value| *value == Some(false)) {
            self.services.local_agent_stats = false;
        }
    }

    /// 某个服务是否参与额度轮询。
    pub fn quota_enabled(&self, provider: ProviderId) -> bool {
        self.services.get(provider).quota
    }
}

/// 部分更新载荷。省略的字段保持原值，避免前端把未展示的偏好覆盖成默认值。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SettingsUpdate {
    pub language: Option<LanguagePreference>,
    pub appearance: Option<AppearancePreference>,
    pub refresh_interval: Option<RefreshInterval>,
    pub scan_interval: Option<RefreshInterval>,
    pub launch_at_login: Option<bool>,
    pub privacy_mode: Option<bool>,
    pub show_service_status: Option<bool>,
    pub menu_bar_window_mode: Option<MenuBarWindowMode>,
    pub command_code_credential: Option<CommandCodeCredentialPreference>,
    pub services: Option<ServicesSettings>,
    pub hud: Option<HudSettings>,
    pub ranking_basis: Option<RankingBasis>,
    pub reset_time_display: Option<ResetTimeDisplay>,
    pub check_updates_on_start: Option<bool>,
    pub verbose_logging: Option<bool>,
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
        if let Some(interval) = self.scan_interval {
            settings.scan_interval = interval;
        }
        if let Some(launch_at_login) = self.launch_at_login {
            settings.launch_at_login = launch_at_login;
        }
        if let Some(privacy_mode) = self.privacy_mode {
            settings.privacy_mode = privacy_mode;
        }
        if let Some(show_service_status) = self.show_service_status {
            settings.show_service_status = show_service_status;
        }
        if let Some(mode) = self.menu_bar_window_mode {
            settings.menu_bar_window_mode = mode;
        }
        if let Some(credential) = self.command_code_credential {
            settings.command_code_credential = credential;
        }
        if let Some(services) = self.services {
            settings.services = services;
        }
        if let Some(hud) = self.hud {
            settings.hud = hud;
        }
        if let Some(basis) = self.ranking_basis {
            settings.ranking_basis = basis;
        }
        if let Some(display) = self.reset_time_display {
            settings.reset_time_display = display;
        }
        if let Some(check) = self.check_updates_on_start {
            settings.check_updates_on_start = check;
        }
        if let Some(verbose) = self.verbose_logging {
            settings.verbose_logging = verbose;
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
        assert_eq!(settings.refresh_interval, RefreshInterval::TwoMinutes);
        assert_eq!(settings.refresh_interval.minutes(), 2);
        assert_eq!(settings.scan_interval, RefreshInterval::FiveMinutes);
        assert_eq!(settings.language, LanguagePreference::System);
        assert_eq!(settings.appearance, AppearancePreference::System);
        assert!(!settings.launch_at_login);
        assert!(!settings.onboarding.completed);
        assert!(settings.show_service_status, "服务状态圆点默认开启");
        assert_eq!(settings.menu_bar_window_mode, MenuBarWindowMode::Primary);
        assert_eq!(settings.ranking_basis, RankingBasis::Tokens);
        assert_eq!(settings.reset_time_display, ResetTimeDisplay::Duration);
        assert_eq!(
            settings.command_code_credential,
            CommandCodeCredentialPreference::Automatic
        );
        assert!(settings.check_updates_on_start);
        assert!(!settings.verbose_logging);
        assert!(!settings.hud.enabled);
        assert!(settings.hud.position.is_none());

        assert_eq!(settings.services.codex, ServiceSettings::all(true));
        assert_eq!(settings.services.claude, ServiceSettings::all(true));
        assert_eq!(settings.services.antigravity, ServiceSettings::all(false));
        assert_eq!(settings.services.cursor, ServiceSettings::all(false));
        assert_eq!(settings.services.command_code, ServiceSettings::all(false));
        assert!(settings.services.local_agent_stats);
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
        assert_eq!(settings.refresh_interval, RefreshInterval::TwoMinutes);
        assert_eq!(settings.scan_interval, RefreshInterval::FiveMinutes);
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
        assert_eq!(settings.refresh_interval, RefreshInterval::TwoMinutes);
        assert!(
            settings.onboarding.completed,
            "settings_update must not reset the onboarding marker"
        );
    }

    #[test]
    fn v1_statistics_visibility_migrates_into_the_service_matrix() {
        let raw = r#"{
            "schemaVersion": 1,
            "usageServiceVisibility": {
                "codex": true,
                "claude": false,
                "pi": false,
                "opencode": false
            }
        }"#;
        let mut settings: Settings = serde_json::from_str(raw).expect("parses");

        settings.migrate_legacy();

        assert!(settings.services.codex.stats);
        assert!(!settings.services.claude.stats);
        assert!(!settings.services.local_agent_stats);
        assert!(settings.legacy_usage_service_visibility.is_none());
        assert_eq!(settings.services.stats_sources(), vec![UsageSource::Codex]);
    }

    #[test]
    fn a_missing_legacy_field_does_not_turn_services_off() {
        let mut settings: Settings =
            serde_json::from_str(r#"{"schemaVersion":1}"#).expect("parses");

        settings.migrate_legacy();

        assert!(settings.services.codex.stats);
        assert!(settings.services.claude.stats);
        assert!(settings.services.local_agent_stats);
    }

    #[test]
    fn the_legacy_field_is_never_written_back() {
        let settings = Settings {
            legacy_usage_service_visibility: Some(LegacyUsageServiceVisibility {
                codex: Some(true),
                ..LegacyUsageServiceVisibility::default()
            }),
            ..Settings::default()
        };

        let json = serde_json::to_value(&settings).expect("serializes");
        assert!(json.get("usageServiceVisibility").is_none());
    }

    #[test]
    fn ranking_basis_uses_the_wire_values_the_ui_expects() {
        let json = serde_json::to_value(RankingBasis::Cost).expect("serializes");
        assert_eq!(json, "cost");
        let json = serde_json::to_value(MenuBarWindowMode::Both).expect("serializes");
        assert_eq!(json, "both");
    }
}
