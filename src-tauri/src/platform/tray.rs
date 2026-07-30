//! 系统区域图标与原生菜单。
//!
//! 菜单项固定四项，见 `docs/信息架构与核心流程.md` 第 4.2 节。「刷新额度」走与界面
//! 完全相同的刷新用例，因此不存在第二套刷新状态源。
//!
//! 图标承载两个 Provider 返回的第一项额度剩余百分比（ADR-0017）：
//! - macOS 把「标识 + 百分比」渲染成模板位图，由系统按菜单栏外观着色。
//! - Windows 托盘不支持图标旁并排文字，同一份文本只进 tooltip。
//!
//! 没有任何数据时图标仍然存在，只是百分比位置显示占位符。

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, AppHandle, Manager, Wry};

use super::desktop::{MainNavigationTarget, hide_compact, show_main, toggle_compact};
use super::strings::{Lang, native, provider_name};
use crate::app::AppCore;
use crate::contracts::{ProviderAvailability, ProviderId, QuotaState};
use crate::scheduler::RefreshTrigger;

pub const TRAY_ID: &str = "cc-trace";

/// 没有可展示数值时的占位符。与 Swift 版 cc-bar 的菜单栏一致，
/// 刻意不用界面里的 em dash：菜单栏宽度紧张，两个 ASCII 短横更省。
const NO_VALUE: &str = "--";

/// 徽标的一段：一个 Provider 的标识与它的百分比文字。
pub struct BadgeSegment {
    pub provider: ProviderId,
    /// 已格式化的展示文本，例如 `62%`。没有数值时是 [`NO_VALUE`]。
    pub text: String,
}

const MENU_OPEN: &str = "open";
const MENU_REFRESH: &str = "refresh";
const MENU_SETTINGS: &str = "settings";
const MENU_QUIT: &str = "quit";

pub fn install(app: &App, lang: Lang) -> tauri::Result<()> {
    let strings = native(lang);
    let builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(strings.tooltip)
        .menu(&build_menu(app, lang)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => open_main(app, MainNavigationTarget::Quota),
            MENU_REFRESH => refresh_all(app),
            MENU_SETTINGS => open_main(app, MainNavigationTarget::Settings),
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // 锚点用图标矩形而不是事件里的光标位置：光标落在图标的哪个像素是随机的。
            if let TrayIconEvent::Click {
                rect,
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = toggle_compact(tray.app_handle(), rect);
            }
        });

    #[cfg(target_os = "macos")]
    // macOS 会按当前 Menu Bar 外观给 alpha 蒙版重新着色。
    let builder = builder
        .icon(tauri::include_image!("icons/tray-symbol.png"))
        .icon_as_template(true);

    #[cfg(not(target_os = "macos"))]
    let builder = builder.icon(
        app.default_window_icon()
            .expect("CC Trace bundle icon is required")
            .clone(),
    );

    builder.build(app)?;
    Ok(())
}

/// 语言变更后重建菜单，让原生文案与界面保持一致。
///
/// tooltip 不在这里恢复成产品名：它承载额度文本，由 [`present_quota`] 拥有。
pub fn relocalize(app: &AppHandle, lang: Lang) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    tray.set_menu(Some(build_menu(app, lang)?))?;
    Ok(())
}

/// 按最新额度重画系统区域。
///
/// 失败一律静默：系统区域展示不到位不该影响刷新本身，图标也不会因此消失。
pub fn present_quota(app: &AppHandle, state: &QuotaState) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let segments = badge_segments(state);
    let _ = tray.set_tooltip(Some(tooltip_text(&segments)));

    #[cfg(target_os = "macos")]
    if let Some(image) = super::menubar_badge::render(&segments) {
        let icon = tauri::image::Image::new_owned(image.rgba, image.width, image.height);
        // 图标与模板状态原子更新，避免同一张图先按彩色、再按模板重复渲染。
        let _ = tray.set_icon_with_as_template(Some(icon), true);
    }
}

/// 每个 Provider 一段，顺序固定 Codex → Claude Code，与界面一致。
fn badge_segments(state: &QuotaState) -> Vec<BadgeSegment> {
    ProviderId::ORDER
        .iter()
        .map(|provider| BadgeSegment {
            provider: *provider,
            text: primary_window_text(state, *provider),
        })
        .collect()
}

/// Provider 返回的第一项额度，与卡片第一行使用同一主次规则。
fn primary_window_text(state: &QuotaState, provider: ProviderId) -> String {
    state
        .providers
        .iter()
        .find(|snapshot| snapshot.provider == provider)
        .filter(|snapshot| {
            !matches!(
                snapshot.availability,
                ProviderAvailability::NoCredentials
                    | ProviderAvailability::Unsupported
                    | ProviderAvailability::Offline
                    | ProviderAvailability::Error
            )
        })
        .and_then(|snapshot| snapshot.snapshot.as_ref())
        .and_then(|snapshot| snapshot.windows.first())
        .map(|window| percent_text(window.remaining_percent))
        .unwrap_or_else(|| NO_VALUE.to_string())
}

/// 与前端 `lib/format.ts` 的 `formatPercent` 同口径：还剩一点点时显示 `<1%`，
/// 不四舍五入成会被读成「已经用完」的 `0%`。
fn percent_text(remaining: f64) -> String {
    if remaining > 0.0 && remaining < 1.0 {
        return "<1%".to_string();
    }
    format!("{}%", remaining.round() as i64)
}

/// tooltip 是 Windows 上唯一的额度载体，在 macOS 上则是位图的文字等价物。
fn tooltip_text(segments: &[BadgeSegment]) -> String {
    segments
        .iter()
        .map(|segment| format!("{} {}", provider_name(segment.provider), segment.text))
        .collect::<Vec<_>>()
        .join(" · ")
}

fn build_menu<M: Manager<Wry>>(manager: &M, lang: Lang) -> tauri::Result<Menu<Wry>> {
    let strings = native(lang);

    let open = MenuItem::with_id(manager, MENU_OPEN, strings.open, true, None::<&str>)?;
    let refresh = MenuItem::with_id(manager, MENU_REFRESH, strings.refresh, true, None::<&str>)?;
    let settings = MenuItem::with_id(manager, MENU_SETTINGS, strings.settings, true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(manager)?;
    let quit = MenuItem::with_id(manager, MENU_QUIT, strings.quit, true, None::<&str>)?;

    Menu::with_items(manager, &[&open, &refresh, &settings, &separator, &quit])
}

fn open_main(app: &AppHandle, target: MainNavigationTarget) {
    hide_compact(app);
    let _ = show_main(app, target);
}

/// 原生菜单的「刷新额度」与界面共用同一个用例：同一份请求合并、节流与退避。
fn refresh_all(app: &AppHandle) {
    let Some(core) = app.try_state::<Arc<AppCore>>() else {
        return;
    };
    let core = Arc::clone(core.inner());
    core.refresh_all(app, RefreshTrigger::Manual);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{ProviderSnapshot, QuotaSnapshot, QuotaWindow, QuotaWindowKind};

    fn window(kind: QuotaWindowKind, remaining: f64) -> QuotaWindow {
        QuotaWindow {
            id: format!("test.{kind:?}"),
            kind,
            display_name: None,
            used_percent: 100.0 - remaining,
            remaining_percent: remaining,
            resets_at: None,
            window_seconds: None,
            is_active: true,
            is_primary: false,
        }
    }

    fn with_windows(provider: ProviderId, windows: Vec<QuotaWindow>) -> ProviderSnapshot {
        ProviderSnapshot {
            snapshot: Some(QuotaSnapshot {
                windows,
                captured_at: "2026-07-27T10:00:00Z".to_string(),
            }),
            ..ProviderSnapshot::initial(provider)
        }
    }

    fn state(providers: Vec<ProviderSnapshot>) -> QuotaState {
        QuotaState { providers }
    }

    #[test]
    fn a_provider_without_a_snapshot_shows_the_placeholder() {
        let state = state(vec![ProviderSnapshot::initial(ProviderId::Codex)]);
        assert_eq!(primary_window_text(&state, ProviderId::Codex), NO_VALUE);
    }

    #[test]
    fn a_provider_missing_from_the_state_shows_the_placeholder() {
        assert_eq!(
            primary_window_text(&state(vec![]), ProviderId::Claude),
            NO_VALUE
        );
    }

    #[test]
    fn the_first_returned_window_reaches_the_menu_bar() {
        let state = state(vec![with_windows(
            ProviderId::Codex,
            vec![
                window(QuotaWindowKind::Weekly, 41.0),
                window(QuotaWindowKind::FiveHour, 62.0),
            ],
        )]);
        assert_eq!(primary_window_text(&state, ProviderId::Codex), "41%");
    }

    #[test]
    fn the_first_returned_unknown_window_is_still_the_display_primary() {
        let state = state(vec![with_windows(
            ProviderId::Codex,
            vec![window(QuotaWindowKind::Unknown, 62.0)],
        )]);
        assert_eq!(primary_window_text(&state, ProviderId::Codex), "62%");
    }

    #[test]
    fn offline_and_error_hide_even_a_stale_snapshot_from_the_system_area() {
        for availability in [ProviderAvailability::Offline, ProviderAvailability::Error] {
            let mut provider = with_windows(
                ProviderId::Codex,
                vec![window(QuotaWindowKind::FiveHour, 62.0)],
            );
            provider.availability = availability;

            assert_eq!(
                primary_window_text(&state(vec![provider]), ProviderId::Codex),
                NO_VALUE,
                "{availability:?} must use the documented system-area placeholder"
            );
        }
    }

    #[test]
    fn rate_limiting_keeps_the_last_known_value_in_the_system_area() {
        let mut provider = with_windows(
            ProviderId::Codex,
            vec![window(QuotaWindowKind::FiveHour, 62.0)],
        );
        provider.availability = ProviderAvailability::RateLimited;

        assert_eq!(
            primary_window_text(&state(vec![provider]), ProviderId::Codex),
            "62%"
        );
    }

    #[test]
    fn a_sliver_of_quota_is_not_rounded_down_to_zero() {
        assert_eq!(percent_text(0.4), "<1%");
        assert_eq!(percent_text(0.0), "0%");
        assert_eq!(percent_text(61.6), "62%");
        assert_eq!(percent_text(100.0), "100%");
    }

    #[test]
    fn both_providers_always_get_a_segment_in_a_stable_order() {
        let segments = badge_segments(&state(vec![with_windows(
            ProviderId::Claude,
            vec![window(QuotaWindowKind::FiveHour, 78.0)],
        )]));

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].provider, ProviderId::Codex);
        assert_eq!(segments[0].text, NO_VALUE);
        assert_eq!(segments[1].provider, ProviderId::Claude);
        assert_eq!(segments[1].text, "78%");
    }

    #[test]
    fn the_tooltip_names_both_providers() {
        let segments = badge_segments(&state(vec![
            with_windows(
                ProviderId::Codex,
                vec![window(QuotaWindowKind::FiveHour, 62.0)],
            ),
            with_windows(
                ProviderId::Claude,
                vec![window(QuotaWindowKind::FiveHour, 78.0)],
            ),
        ]));

        assert_eq!(tooltip_text(&segments), "Codex 62% · Claude Code 78%");
    }
}
