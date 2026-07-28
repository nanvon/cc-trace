//! macOS 菜单栏徽标位图：Provider 标识 + Provider 返回第一项额度的剩余百分比。
//!
//! 形态与几何参数按 [ADR-0017](../../../docs/决策/ADR-0017-系统区域显示额度数字与余量分档.md)
//! 对齐 Swift 版 cc-bar 的可见内容。由于 `resvg`／`tiny-skia` 的实际墨量比
//! AppKit／Core Text 更轻，这里在相同 18pt 最终画布内使用轻量光学补偿：
//! 标识 19pt、系统菜单栏字体 13.5pt Medium；图标与文字间距 3pt、两段之间 9pt。
//!
//! 输出是 RGBA，交给 Tauri 时必须同时 `icon_as_template(true)`——macOS 只取 alpha
//! 通道，按当前菜单栏外观重新着色，因此这里画的是**单色**位图，不是彩色百分比。
//!
//! 与 cc-bar 一致，每段按当前百分比文字的实测宽度紧凑排布；两段之间始终是 9pt，
//! 菜单栏 item 宽度会随数字位数自然变化。
//!
//! Tauri 底层的 `tray-icon` 会把任意图片统一设置成 18pt 高。画布因此直接按最终
//! 18pt 设计，并以 2× 输出 36px：如果先画 22pt／44px 再交给 Tauri，整张图会被
//! 缩到 18pt，18pt 标识和 13pt 文字都会变小，Retina 上还会多一次重采样。
//!
//! 标识资源约定：`icons/providers/*.svg` 必须是 `viewBox="0 0 100 100"` 且只含
//! `<path>`，本模块按 `<path` 到 `</svg>` 截取内容后重新包裹。

use resvg::tiny_skia;
use resvg::usvg;

use super::tray::BadgeSegment;
use crate::contracts::ProviderId;

/// 逻辑 pt → 物理像素的倍率。Retina 菜单栏需要 2× backing，否则 1× 位图会被放大并发虚。
const SCALE: f32 = 2.0;

/// 标识边长，pt。比 cc-bar 的 AppKit 18pt 放大 1pt，补偿 `resvg` 的较轻边缘。
const ICON_SIZE: f32 = 19.0;
/// Tauri `tray-icon` 在 macOS 上最终设置的图片高度，pt。
const HEIGHT: f32 = 18.0;
/// 标识与它的百分比之间。
const ICON_TEXT_GAP: f32 = 3.0;
/// 两个 Provider 段之间。
const SEGMENT_GAP: f32 = 9.0;
/// 菜单栏文字尺寸，pt。原生基准是 13pt；增加 0.5pt 补偿非 Core Text 栅格化。
const FONT_SIZE: f32 = 13.5;

/// macOS 系统 UI 字体。小字号优先匹配 Text optical face；末尾只保留系统级兜底。
const FONT_FAMILY: &str = "'.SF NS Text', '.AppleSystemUIFont', 'SF Pro', 'Helvetica Neue'";
/// AppKit 的 Regular 经 Core Text 绘制比当前模板位图更饱满，因此提高一档光学重量。
const FONT_WEIGHT: u16 = 500;

const CODEX_LOGO: &str = include_str!("../../icons/providers/codex.svg");
const CLAUDE_LOGO: &str = include_str!("../../icons/providers/claude.svg");

/// 渲染结果：RGBA 像素与尺寸。
pub struct BadgeImage {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

fn logo_source(provider: ProviderId) -> &'static str {
    match provider {
        ProviderId::Codex => CODEX_LOGO,
        ProviderId::Claude => CLAUDE_LOGO,
    }
}

/// 从标识 SVG 里取出绘制内容，丢掉它自己的根元素尺寸。
fn logo_body(svg: &str) -> &str {
    let start = match svg.find("<path") {
        Some(index) => index,
        None => return "",
    };
    let end = svg.rfind("</svg>").unwrap_or(svg.len());
    if end <= start {
        return "";
    }
    &svg[start..end]
}

fn options() -> usvg::Options<'static> {
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    options
}

/// 一段文字在当前字体下的宽度。测不出来时退回一个保守估计，绝不返回 0 让布局塌掉。
fn text_width(text: &str, options: &usvg::Options) -> f32 {
    let probe = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="400" height="40"><text x="0" y="30" font-family="{FONT_FAMILY}" font-size="{FONT_SIZE}" font-weight="{FONT_WEIGHT}" font-variant-numeric="tabular-nums">{text}</text></svg>"#
    );

    usvg::Tree::from_str(&probe, options)
        .ok()
        .map(|tree| tree.root().abs_bounding_box().width())
        .filter(|width| *width > 0.0)
        .unwrap_or_else(|| text.chars().count() as f32 * FONT_SIZE * 0.6)
}

/// 渲染徽标。`segments` 为空时返回 `None`——调用方负责回退到静态图标，图标不消失。
pub fn render(segments: &[BadgeSegment]) -> Option<BadgeImage> {
    if segments.is_empty() {
        return None;
    }

    let options = options();
    let text_widths = segments
        .iter()
        .map(|segment| text_width(&segment.text, &options).ceil())
        .collect::<Vec<_>>();
    let total_width = text_widths
        .iter()
        .map(|width| ICON_SIZE + ICON_TEXT_GAP + *width)
        .sum::<f32>()
        + SEGMENT_GAP * (segments.len() as f32 - 1.0);

    let icon_y = ((HEIGHT - ICON_SIZE) / 2.0).floor();
    // SVG 的 y 是基线。13pt 系统字在 18pt 高里居中，基线约在中线下 0.35em 处。
    let baseline = (HEIGHT / 2.0 + FONT_SIZE * 0.35).round();
    let logo_scale = ICON_SIZE / 100.0;

    let mut body = String::new();
    let mut origin = 0.0;
    for (index, (segment, width)) in segments.iter().zip(text_widths.iter()).enumerate() {
        if index > 0 {
            origin += SEGMENT_GAP;
        }
        body.push_str(&format!(
            r#"<g transform="translate({origin} {icon_y}) scale({logo_scale})">{}</g>"#,
            logo_body(logo_source(segment.provider))
        ));
        body.push_str(&format!(
            r#"<text x="{}" y="{baseline}" font-family="{FONT_FAMILY}" font-size="{FONT_SIZE}" font-weight="{FONT_WEIGHT}" font-variant-numeric="tabular-nums" fill="black">{}</text>"#,
            origin + ICON_SIZE + ICON_TEXT_GAP,
            escape_text(&segment.text),
        ));
        origin += ICON_SIZE + ICON_TEXT_GAP + *width;
    }

    let document = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{total_width}" height="{HEIGHT}" viewBox="0 0 {total_width} {HEIGHT}">{body}</svg>"#
    );

    let tree = usvg::Tree::from_str(&document, &options).ok()?;
    // 画布是物理像素，内容按 SCALE 放大绘制：几何参数保持逻辑 pt 可读。
    let width = (total_width * SCALE).ceil() as u32;
    let height = (HEIGHT * SCALE) as u32;
    let mut pixmap = tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(SCALE, SCALE),
        &mut pixmap.as_mut(),
    );

    Some(BadgeImage {
        rgba: pixmap.take(),
        width,
        height,
    })
}

/// 百分比文本只可能出现数字、`%`、`<` 和占位符，但仍然按 XML 规则转义。
fn escape_text(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(provider: ProviderId, text: &str) -> BadgeSegment {
        BadgeSegment {
            provider,
            text: text.to_string(),
        }
    }

    #[test]
    fn both_logos_expose_drawable_paths() {
        assert!(logo_body(CODEX_LOGO).starts_with("<path"));
        assert!(logo_body(CLAUDE_LOGO).starts_with("<path"));
        assert!(!logo_body(CODEX_LOGO).contains("</svg>"));
    }

    #[test]
    fn a_malformed_logo_yields_no_body_instead_of_panicking() {
        assert_eq!(logo_body("<svg></svg>"), "");
    }

    #[test]
    fn an_empty_badge_falls_back_to_the_static_icon() {
        assert!(render(&[]).is_none());
    }

    #[test]
    fn retina_badge_matches_tauris_eighteen_point_display_height() {
        let image = render(&[segment(ProviderId::Codex, "62%")]).expect("badge renders");
        assert_eq!(image.height, 36);
        assert_eq!(image.rgba.len(), (image.width * image.height * 4) as usize);
    }

    #[test]
    fn the_system_ui_font_actually_resolves() {
        // 字体栈必须真的命中系统字体，否则会静默回退而不是报错。
        let options = options();
        let probe = "100%";
        let resolved = text_width(probe, &options);
        let fallback = probe.chars().count() as f32 * FONT_SIZE * 0.6;
        assert!(
            (resolved - fallback).abs() > f32::EPSILON,
            "text measurement fell back to the crude estimate: {resolved}"
        );
    }

    #[test]
    fn badge_width_tracks_the_current_text_like_ccbar() {
        let narrow = render(&[segment(ProviderId::Codex, "5%")]).expect("badge renders");
        let wide = render(&[segment(ProviderId::Codex, "100%")]).expect("badge renders");
        assert!(wide.width > narrow.width);
    }

    #[test]
    fn two_segments_add_the_segment_gap_once() {
        let one = render(&[segment(ProviderId::Codex, "62%")]).expect("badge renders");
        let two = render(&[
            segment(ProviderId::Codex, "62%"),
            segment(ProviderId::Claude, "78%"),
        ])
        .expect("badge renders");

        // 两段宽度 = 单段 × 2 + 一个段间距，全部按物理像素计；允许 1px 取整误差
        let expected = one.width * 2 + (SEGMENT_GAP * SCALE) as u32;
        assert!(two.width.abs_diff(expected) <= 1, "got {}", two.width);
    }

    #[test]
    fn something_gets_drawn_into_the_alpha_channel() {
        let image = render(&[segment(ProviderId::Codex, "62%")]).expect("badge renders");
        assert!(
            image.rgba.chunks_exact(4).any(|pixel| pixel[3] > 0),
            "template image needs a non-empty alpha mask"
        );
    }
}
