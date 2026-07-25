//! 原生控件的文案。
//!
//! 托盘菜单由 Rust 创建，因此它的文案无法走 Vue I18n。这里是原生侧的等价物：
//! 文案与 `src/i18n/locales/*` 中同名 key 保持一致，改一侧必须同时改另一侧。
//! 语言判定与设置共用同一条规则，不在原生层再读一次系统语言。

use crate::contracts::LanguagePreference;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhCn,
    En,
}

impl Lang {
    /// 「跟随系统」时用 Rust 平台层拿到的系统语言判定，其余情况直接采用用户选择。
    pub fn resolve(preference: LanguagePreference, system_locale: &str) -> Self {
        match preference {
            LanguagePreference::ZhCn => Self::ZhCn,
            LanguagePreference::En => Self::En,
            LanguagePreference::System => {
                if system_locale.to_ascii_lowercase().starts_with("zh") {
                    Self::ZhCn
                } else {
                    Self::En
                }
            }
        }
    }
}

/// 原生托盘菜单用到的全部字符串。
pub struct NativeStrings {
    pub tooltip: &'static str,
    pub open: &'static str,
    pub refresh: &'static str,
    pub settings: &'static str,
    pub quit: &'static str,
}

pub fn native(lang: Lang) -> NativeStrings {
    match lang {
        Lang::ZhCn => NativeStrings {
            tooltip: "CC Trace",
            open: "打开 CC Trace",
            refresh: "刷新额度",
            settings: "设置",
            quit: "退出 CC Trace",
        },
        Lang::En => NativeStrings {
            tooltip: "CC Trace",
            open: "Open CC Trace",
            refresh: "Refresh quota",
            settings: "Settings",
            quit: "Quit CC Trace",
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_choice_wins_over_the_system_locale() {
        assert_eq!(Lang::resolve(LanguagePreference::En, "zh-CN"), Lang::En);
        assert_eq!(Lang::resolve(LanguagePreference::ZhCn, "en-US"), Lang::ZhCn);
    }

    #[test]
    fn follow_system_matches_any_chinese_tag() {
        assert_eq!(
            Lang::resolve(LanguagePreference::System, "zh-Hans-CN"),
            Lang::ZhCn
        );
        assert_eq!(
            Lang::resolve(LanguagePreference::System, "ZH-TW"),
            Lang::ZhCn
        );
        assert_eq!(Lang::resolve(LanguagePreference::System, "en-GB"), Lang::En);
        assert_eq!(Lang::resolve(LanguagePreference::System, ""), Lang::En);
    }
}
