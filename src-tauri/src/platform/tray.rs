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

use super::desktop::{MainNavigationTarget, request_hide_compact, show_main, toggle_compact};
use super::strings::{Lang, native, provider_name};
use crate::app::AppCore;
use crate::contracts::{
    MenuBarWindowMode, ProviderAvailability, ProviderId, QuotaSnapshot, QuotaState, QuotaWindow,
    QuotaWindowKind,
};
use crate::scheduler::RefreshTrigger;

pub const TRAY_ID: &str = "cc-trace";

/// 没有可展示数值时的占位符。与 Swift 版 cc-bar 的菜单栏一致，
/// 刻意不用界面里的 em dash：菜单栏宽度紧张，两个 ASCII 短横更省。
const NO_VALUE: &str = "--";

/// 徽标的一段：一个 Provider 的标识与它的百分比文字。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
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

    // macOS 27 起，给状态栏图标绑定 NSMenu 会让系统接管左键（自动展开菜单），
    // tray-icon 的点击回调收不到事件。运行时探测到 expanded interface API 时
    // 改走 macos_status_item 的 session 路径：不绑定菜单、不处理点击事件；
    // 旧系统与 Windows 保持现状。
    #[cfg(target_os = "macos")]
    let expanded = super::macos_status_item::supported();
    #[cfg(not(target_os = "macos"))]
    let expanded = false;

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(strings.tooltip)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => open_main(app, MainNavigationTarget::Quota),
            MENU_REFRESH => refresh_all(app),
            MENU_SETTINGS => open_main(app, MainNavigationTarget::Settings),
            MENU_QUIT => app.exit(0),
            _ => {}
        });

    if !expanded {
        builder = builder
            .menu(&build_menu(app, lang)?)
            .show_menu_on_left_click(false)
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
    }

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

    let tray = builder.build(app)?;

    // Windows 托盘由运行时持有，句柄无需存活；macOS 的 expanded 分支下面还要用。
    #[cfg(not(target_os = "macos"))]
    drop(tray);

    #[cfg(target_os = "macos")]
    if expanded {
        let handle = app.handle().clone();
        // 状态栏图标已经建好，把底层的 NSStatusItem 交给 session 模块。
        // 拿不到 NSStatusItem 时必须失败可见：expanded 路径不绑定菜单也不处理
        // 点击事件，静默跳过会留下一个存在却点不动的图标；正常流程该分支不可达
        // （tray-icon 创建后即持有 NSStatusItem），宁可不启动也不悄悄坏。
        let installed = tray.with_inner_tray_icon(move |inner| match inner.ns_status_item() {
            Some(status_item) => {
                super::macos_status_item::install(&handle, &status_item, lang);
                true
            }
            None => false,
        })?;
        if !installed {
            return Err(tauri::Error::Io(std::io::Error::other(
                "macOS expanded interface: NSStatusItem unavailable",
            )));
        }
    }

    Ok(())
}

/// 语言变更后重建菜单，让原生文案与界面保持一致。
///
/// tooltip 不在这里恢复成产品名：它承载额度文本，由 [`present_quota`] 拥有。
pub fn relocalize(app: &AppHandle, lang: Lang) -> tauri::Result<()> {
    // macOS 27 的右键菜单不绑定 status item：语言切换只重建菜单内容，
    // 重新 set_menu 会把左键重新交还给菜单。
    #[cfg(target_os = "macos")]
    if super::macos_status_item::supported() {
        super::macos_status_item::rebuild_context_menu(lang);
        return Ok(());
    }

    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    tray.set_menu(Some(build_menu(app, lang)?))?;
    Ok(())
}

/// 按最新额度重画系统区域。
///
/// 失败一律静默：系统区域展示不到位不该影响刷新本身，图标也不会因此消失。
///
/// tooltip 按设置里的服务矩阵与「菜单栏窗口」模式生成；macOS 徽标位图保持
/// 每个服务一段主要额度，不随模式变化（悬浮窗与 tooltip 之外的取舍见 ADR-0031）。
pub fn present_quota(
    app: &AppHandle,
    state: &QuotaState,
    menu_bar: &[ProviderId],
    mode: MenuBarWindowMode,
) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };

    let _ = tray.set_tooltip(Some(tooltip_for(state, menu_bar, mode)));

    #[cfg(target_os = "macos")]
    let segments = badge_segments(state, menu_bar);

    #[cfg(target_os = "macos")]
    if let Some(image) = super::menubar_badge::render(&segments) {
        let icon = tauri::image::Image::new_owned(image.rgba, image.width, image.height);
        // 图标与模板状态原子更新，避免同一张图先按彩色、再按模板重复渲染。
        let _ = tray.set_icon_with_as_template(Some(icon), true);
    }
}

/// 系统区域里启用的服务一段，顺序固定 Codex → Claude → Antigravity → Cursor →
/// Command Code，与界面一致。启用集合来自设置的服务矩阵，不由系统区域自己判断。
/// 只有 macOS 的徽标位图消费它；Windows 上仅测试引用，不算死代码。
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn badge_segments(state: &QuotaState, menu_bar: &[ProviderId]) -> Vec<BadgeSegment> {
    ProviderId::ORDER
        .iter()
        .filter(|provider| menu_bar.contains(provider))
        .map(|provider| BadgeSegment {
            provider: *provider,
            text: primary_window_text(state, *provider),
        })
        .collect()
}

/// 能拿来展示数值的快照。离线与错误连旧快照一起隐藏（见状态模型），限流保留最后一次数值。
fn usable_snapshot(state: &QuotaState, provider: ProviderId) -> Option<&QuotaSnapshot> {
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
}

/// Provider 返回的第一项额度，与卡片第一行使用同一主次规则。
fn primary_window_text(state: &QuotaState, provider: ProviderId) -> String {
    usable_snapshot(state, provider)
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

/// Windows 通知区域 tooltip 的容量：`NOTIFYICONDATAW.szTip` 是 `WCHAR[128]`，含结尾
/// 空字符，可用 127 个 UTF-16 码元（Microsoft Learn 的 NOTIFYICONDATAW 页）。
/// tray-icon 0.24 的 `set_tooltip` 会把超长文本按 128 截断且不补结尾空字符，
/// 所以必须在这里先收进 127，不依赖它兜底。
const TOOLTIP_MAX_UTF16: usize = 127;

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// 某个服务在当前模式下要展示的额度窗口。
///
/// Cursor 的 Total／Auto／API 是同一计费周期的不同维度，不是主／周窗口，
/// 无论哪种模式都只取第一项；没有周窗口的服务在周模式下退回主要额度，
/// 不让整个服务从 tooltip 里消失。
fn tooltip_windows(
    snapshot: &QuotaSnapshot,
    provider: ProviderId,
    mode: MenuBarWindowMode,
) -> Vec<&QuotaWindow> {
    let primary = snapshot.windows.first();
    let weekly = snapshot
        .windows
        .iter()
        .find(|window| window.kind == QuotaWindowKind::Weekly);

    let picked: Vec<&QuotaWindow> = if provider == ProviderId::Cursor {
        primary.into_iter().collect()
    } else {
        match mode {
            MenuBarWindowMode::Primary => primary.into_iter().collect(),
            MenuBarWindowMode::Weekly => weekly.or(primary).into_iter().collect(),
            MenuBarWindowMode::Both => primary.into_iter().chain(weekly).collect(),
        }
    };

    // 主要额度本身就是周窗口时，Both 不重复写一遍。
    let mut unique: Vec<&QuotaWindow> = Vec::with_capacity(picked.len());
    for window in picked {
        if !unique.iter().any(|seen| seen.id == window.id) {
            unique.push(window);
        }
    }
    unique
}

fn window_value(window: &QuotaWindow) -> String {
    if window.unlimited {
        "∞".to_string()
    } else {
        percent_text(window.remaining_percent)
    }
}

/// 一个服务在 tooltip 里的一段，例如 `Codex 62%` 或 `Codex 62% / 41%`。
fn tooltip_segment(state: &QuotaState, provider: ProviderId, mode: MenuBarWindowMode) -> String {
    let values = usable_snapshot(state, provider)
        .map(|snapshot| tooltip_windows(snapshot, provider, mode))
        .unwrap_or_default();
    let value = if values.is_empty() {
        NO_VALUE.to_string()
    } else {
        values
            .into_iter()
            .map(window_value)
            .collect::<Vec<_>>()
            .join(" / ")
    };
    format!("{} {}", provider_name(provider), value)
}

/// 把若干段收进 [`TOOLTIP_MAX_UTF16`]。
///
/// 先用 ` · ` 分隔；放不下改用单空格；仍放不下就整段丢弃末尾的服务并以 `…` 结尾。
/// 服务顺序固定，被丢的永远是排在最后的，且不会把一个数字切成半截。
fn fit_tooltip(segments: &[String]) -> String {
    if segments.is_empty() {
        return String::new();
    }

    for separator in [" · ", " "] {
        let joined = segments.join(separator);
        if utf16_len(&joined) <= TOOLTIP_MAX_UTF16 {
            return joined;
        }
    }

    for keep in (1..segments.len()).rev() {
        let joined = format!("{}…", segments[..keep].join(" "));
        if utf16_len(&joined) <= TOOLTIP_MAX_UTF16 {
            return joined;
        }
    }

    // 只剩一段还放不下（服务名加数值远小于 127，正常不可达）：按字符硬截断。
    let mut out = String::new();
    for ch in segments[0].chars() {
        if utf16_len(&out) + ch.len_utf16() + 1 > TOOLTIP_MAX_UTF16 {
            break;
        }
        out.push(ch);
    }
    out.push('…');
    out
}

/// tooltip 是 Windows 上唯一的额度载体，在 macOS 上则是位图的文字等价物。
/// 没有勾选任何服务时保持产品名，不留空 tooltip。
fn tooltip_for(state: &QuotaState, menu_bar: &[ProviderId], mode: MenuBarWindowMode) -> String {
    let segments: Vec<String> = ProviderId::ORDER
        .iter()
        .filter(|provider| menu_bar.contains(provider))
        .map(|provider| tooltip_segment(state, *provider, mode))
        .collect();

    if segments.is_empty() {
        return native(Lang::En).tooltip.to_string();
    }
    fit_tooltip(&segments)
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

pub(crate) fn open_main(app: &AppHandle, target: MainNavigationTarget) {
    request_hide_compact(app);
    let _ = show_main(app, target);
}

/// 原生菜单与 macOS 27 右键菜单的「刷新额度」与界面共用同一个用例：
/// 同一份请求合并、节流与退避。
pub(crate) fn refresh_all(app: &AppHandle) {
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
            unlimited: false,
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
    fn enabled_providers_always_get_a_segment_in_a_stable_order() {
        let enabled = [ProviderId::Codex, ProviderId::Claude];
        let segments = badge_segments(
            &state(vec![with_windows(
                ProviderId::Claude,
                vec![window(QuotaWindowKind::FiveHour, 78.0)],
            )]),
            &enabled,
        );

        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].provider, ProviderId::Codex);
        assert_eq!(segments[0].text, NO_VALUE);
        assert_eq!(segments[1].provider, ProviderId::Claude);
        assert_eq!(segments[1].text, "78%");
    }

    #[test]
    fn a_disabled_service_never_reaches_the_system_area() {
        let enabled = [ProviderId::Codex];
        let segments = badge_segments(
            &state(vec![with_windows(
                ProviderId::Claude,
                vec![window(QuotaWindowKind::FiveHour, 78.0)],
            )]),
            &enabled,
        );

        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].provider, ProviderId::Codex);
    }

    fn tooltip(state: &QuotaState, enabled: &[ProviderId], mode: MenuBarWindowMode) -> String {
        tooltip_for(state, enabled, mode)
    }

    fn dual(provider: ProviderId, five: f64, week: f64) -> ProviderSnapshot {
        with_windows(
            provider,
            vec![
                window(QuotaWindowKind::FiveHour, five),
                window(QuotaWindowKind::Weekly, week),
            ],
        )
    }

    #[test]
    fn the_tooltip_names_the_enabled_providers() {
        let state = state(vec![
            with_windows(
                ProviderId::Codex,
                vec![window(QuotaWindowKind::FiveHour, 62.0)],
            ),
            with_windows(
                ProviderId::Claude,
                vec![window(QuotaWindowKind::FiveHour, 78.0)],
            ),
        ]);

        assert_eq!(
            tooltip(
                &state,
                &[ProviderId::Codex, ProviderId::Claude],
                MenuBarWindowMode::Primary
            ),
            "Codex 62% · Claude Code 78%"
        );
    }

    #[test]
    fn the_tooltip_lists_the_five_services_in_product_order() {
        assert_eq!(
            tooltip(
                &state(Vec::new()),
                &ProviderId::ORDER,
                MenuBarWindowMode::Primary
            ),
            "Codex -- · Claude Code -- · Antigravity -- · Cursor -- · Command Code --"
        );
    }

    #[test]
    fn the_window_mode_picks_primary_weekly_or_both() {
        let state = state(vec![dual(ProviderId::Codex, 62.0, 41.0)]);
        let enabled = [ProviderId::Codex];

        assert_eq!(
            tooltip(&state, &enabled, MenuBarWindowMode::Primary),
            "Codex 62%"
        );
        assert_eq!(
            tooltip(&state, &enabled, MenuBarWindowMode::Weekly),
            "Codex 41%"
        );
        assert_eq!(
            tooltip(&state, &enabled, MenuBarWindowMode::Both),
            "Codex 62% / 41%"
        );
    }

    #[test]
    fn a_service_without_a_weekly_window_falls_back_to_its_primary_window() {
        let state = state(vec![with_windows(
            ProviderId::Antigravity,
            vec![window(QuotaWindowKind::Unknown, 55.0)],
        )]);
        let enabled = [ProviderId::Antigravity];

        assert_eq!(
            tooltip(&state, &enabled, MenuBarWindowMode::Weekly),
            "Antigravity 55%"
        );
        assert_eq!(
            tooltip(&state, &enabled, MenuBarWindowMode::Both),
            "Antigravity 55%"
        );
    }

    #[test]
    fn cursor_always_shows_its_first_window_only() {
        let state = state(vec![dual(ProviderId::Cursor, 30.0, 20.0)]);
        let enabled = [ProviderId::Cursor];

        for mode in [
            MenuBarWindowMode::Primary,
            MenuBarWindowMode::Weekly,
            MenuBarWindowMode::Both,
        ] {
            assert_eq!(tooltip(&state, &enabled, mode), "Cursor 30%");
        }
    }

    #[test]
    fn an_unlimited_window_reads_as_infinity_not_a_percentage() {
        let mut unlimited = window(QuotaWindowKind::Total, 100.0);
        unlimited.unlimited = true;
        let state = state(vec![with_windows(ProviderId::Cursor, vec![unlimited])]);

        assert_eq!(
            tooltip(&state, &[ProviderId::Cursor], MenuBarWindowMode::Primary),
            "Cursor ∞"
        );
    }

    #[test]
    fn no_enabled_service_keeps_the_product_name() {
        assert_eq!(
            tooltip(&state(Vec::new()), &[], MenuBarWindowMode::Both),
            "CC Trace"
        );
    }

    #[test]
    fn a_full_matrix_in_both_mode_still_fits_the_windows_tooltip_limit() {
        let providers: Vec<ProviderSnapshot> = ProviderId::ORDER
            .iter()
            .map(|provider| dual(*provider, 100.0, 100.0))
            .collect();
        let text = tooltip(
            &state(providers),
            &ProviderId::ORDER,
            MenuBarWindowMode::Both,
        );

        assert!(utf16_len(&text) <= TOOLTIP_MAX_UTF16, "{text}");
        // 空格分隔仍放不下时丢弃末尾服务，并以省略号标明。
        assert!(text.starts_with("Codex 100% / 100%"), "{text}");
    }

    #[test]
    fn dropping_trailing_services_never_cuts_a_segment_in_half() {
        let segments: Vec<String> = (0..12)
            .map(|index| format!("Service{index} 100%"))
            .collect();
        let text = fit_tooltip(&segments);

        assert!(utf16_len(&text) <= TOOLTIP_MAX_UTF16);
        assert!(text.ends_with('…'));
        let body = text.trim_end_matches('…');
        assert!(
            body.split(' ')
                .collect::<Vec<_>>()
                .chunks(2)
                .all(|pair| pair.len() == 2)
        );
    }

    #[test]
    fn the_limit_counts_utf16_code_units_not_chars() {
        // 每个 U+1F600 占两个码元；60 个共 120 码元，再加名称就超过 127。
        let wide = format!("{} 1%", "😀".repeat(60));
        let text = fit_tooltip(&[wide, "B 2%".to_string()]);
        assert!(utf16_len(&text) <= TOOLTIP_MAX_UTF16);
    }
}
