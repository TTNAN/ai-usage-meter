//! Draws the live menu-bar / tray icon: three tiny "battery" bars for the
//! three services closest to running out, filled to the share that is still
//! LEFT. No image crates — just an anti-aliased RGBA buffer.
//!
//! * macOS: a monochrome *template* image, so the system tints it for light
//!   and dark menu bars like every native icon.
//! * Windows / Linux: each bar carries its service's accent colour on a soft
//!   track, so the icon matches the rows or tiles in the app's window —
//!   including a colour the user picked there (see [`CustomColors`]).

use std::collections::HashMap;

/// Icon edge in pixels (22 pt menu bar @2x).
pub const SIZE: u32 = 44;

/// How many bars the icon has (and therefore how many services it shows).
pub const SLOTS: usize = 3;

/// Bars to show when fewer than [`SLOTS`] services have numbers: the original
/// three, so the icon never changes shape just because nothing is connected.
const FALLBACK_IDS: [&str; 3] = ["claude", "codex", "grok"];

/// Accent per service, matching the popover's `--c-…` colours in
/// `src/styles.css` (the Dock asks for its own palette through [`render_with`]).
pub fn default_accent(id: &str) -> Option<[u8; 3]> {
    Some(match id {
        "claude" => [0xE0, 0x8A, 0x6B],  // clay
        "codex" => [0x6E, 0x9B, 0xFF],   // azure
        "grok" => [0xB3, 0x8C, 0xFF],    // violet
        "gemini" => [0x3F, 0xC4, 0xD4],  // cyan
        "copilot" => [0x5F, 0xC2, 0x7E], // green
        _ => return None,
    })
}

/// Colours the user picked in the app, keyed by service id. Decided in the UI
/// (which also stores them in `localStorage`) and handed over with
/// `set_custom_colors`; services missing here keep their default accent.
pub type CustomColors = HashMap<String, [u8; 3]>;

/// Parse `#RRGGBB` (or the short `#RGB`) into an RGB triple. The UI always
/// sends the long form; short and unprefixed spellings are accepted so the
/// command is forgiving about what it is given.
pub fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let body = hex.trim().trim_start_matches('#');
    let long;
    let body = match body.len() {
        3 => {
            long = body.chars().flat_map(|c| [c, c]).collect::<String>();
            long.as_str()
        }
        6 => body,
        _ => return None,
    };
    if !body.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(body, 16).ok()?;
    Some([(v >> 16) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8])
}

/// The colour a bar is drawn in: the user's choice when there is one, the
/// service's accent from `accent` otherwise.
fn accent_for(
    id: &str,
    custom: &CustomColors,
    accent: fn(&str) -> Option<[u8; 3]>,
) -> Option<[u8; 3]> {
    custom.get(id).copied().or_else(|| accent(id))
}

const NEUTRAL_ACCENT: [u8; 3] = [0x9A, 0x9A, 0xA2];

/// Which services the three bars stand for, tightest remaining quota first.
/// Slots left over fall back to Claude / Codex / Grok, then stay empty.
pub fn pick_slots(items: &[(&'static str, Option<f64>)]) -> [Option<&'static str>; SLOTS] {
    let mut live: Vec<(&'static str, f64)> = items
        .iter()
        .filter_map(|(id, left)| left.map(|l| (*id, l)))
        .collect();
    live.sort_by(|a, b| a.1.total_cmp(&b.1));
    let mut slots: [Option<&'static str>; SLOTS] = [None; SLOTS];
    for (slot, (id, _)) in slots.iter_mut().zip(live) {
        *slot = Some(id);
    }
    for id in FALLBACK_IDS {
        if slots.iter().all(Option::is_some) {
            break;
        }
        let shown = slots.iter().flatten().any(|s| *s == id);
        let item = items.iter().find(|(i, _)| *i == id).map(|(_, left)| *left);
        if shown || item.is_none() {
            continue;
        }
        if let Some(free) = slots.iter_mut().find(|s| s.is_none()) {
            *free = Some(id);
        }
    }
    slots
}

/// Signed-distance coverage of a rounded rectangle at (px, py).
fn rr_coverage(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let cx = (x0 + x1) / 2.0;
    let cy = (y0 + y1) / 2.0;
    let hx = (x1 - x0) / 2.0 - r;
    let hy = (y1 - y0) / 2.0 - r;
    let qx = (px - cx).abs() - hx;
    let qy = (py - cy).abs() - hy;
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    let inside = qx.max(qy).min(0.0);
    let d = outside + inside - r;
    (0.5 - d).clamp(0.0, 1.0)
}

fn blend(dst: &mut [u8], rgb: [u8; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let a_src = alpha.clamp(0.0, 1.0);
    let a_dst = dst[3] as f32 / 255.0;
    let a_out = a_src + a_dst * (1.0 - a_src);
    for (d, s) in dst.iter_mut().zip(rgb) {
        let c = (s as f32 * a_src + *d as f32 * a_dst * (1.0 - a_src)) / a_out.max(1e-6);
        *d = c.round() as u8;
    }
    dst[3] = (a_out * 255.0).round() as u8;
}

/// Render the icon. `left` is the percent remaining per bar, `ids` says which
/// service each bar stands for (`None` draws an empty neutral track), and
/// `custom` holds the colours the user picked (macOS tints its template image,
/// so only Windows / Linux use them).
///
/// The bars use this crate's palette; an app whose window has its own colours
/// calls [`render_with`] instead.
pub fn render(
    left: [Option<f64>; SLOTS],
    ids: [Option<&str>; SLOTS],
    template: bool,
    custom: &CustomColors,
) -> Vec<u8> {
    render_with(left, ids, template, custom, default_accent)
}

/// [`render`] with the accent colours supplied by the caller: `accent` maps a
/// service id to the colour its bar should carry, and an id it does not know
/// falls back to the neutral track colour.
pub fn render_with(
    left: [Option<f64>; SLOTS],
    ids: [Option<&str>; SLOTS],
    template: bool,
    custom: &CustomColors,
    accent: fn(&str) -> Option<[u8; 3]>,
) -> Vec<u8> {
    let n = SIZE as usize;
    let mut buf = vec![0u8; n * n * 4];
    // macOS tints a template image itself, so hairlines read fine there. On
    // Windows and Linux the icon is drawn as-is (16 px on a 100% taskbar):
    // wider bars, a slightly tighter gap and a stronger track keep a single
    // active bar legible next to two empty ones.
    let (bar_w, gap, r, track_rgb, track_a) = if template {
        (8.0f32, 5.0f32, 3.0f32, [0, 0, 0], 0.28f32)
    } else {
        (9.0f32, 4.5f32, 3.0f32, [0x8E, 0x8E, 0x97], 0.5f32)
    };
    let total = bar_w * SLOTS as f32 + gap * (SLOTS as f32 - 1.0);
    let x_start = (SIZE as f32 - total) / 2.0;
    let (top, bottom) = (7.0f32, 37.0f32);

    for (i, value) in left.iter().enumerate() {
        let x0 = x_start + i as f32 * (bar_w + gap);
        let x1 = x0 + bar_w;
        let frac = value
            .map(|v| (v / 100.0).clamp(0.0, 1.0) as f32)
            .unwrap_or(0.0);
        // Keep a visible sliver whenever there is anything left at all.
        let fill_h = if frac > 0.0 {
            ((bottom - top) * frac).max(r * 2.0)
        } else {
            0.0
        };
        let fill_top = bottom - fill_h;
        let fill_rgb = if template {
            [0, 0, 0]
        } else {
            ids.get(i)
                .copied()
                .flatten()
                .and_then(|id| accent_for(id, custom, accent))
                .unwrap_or(NEUTRAL_ACCENT)
        };
        for y in 0..n {
            for x in 0..n {
                // 4× supersampling for crisp edges at 16 px on Windows.
                let mut track = 0.0;
                let mut fill = 0.0;
                for sy in 0..2 {
                    for sx in 0..2 {
                        let px = x as f32 + 0.25 + sx as f32 * 0.5;
                        let py = y as f32 + 0.25 + sy as f32 * 0.5;
                        track += rr_coverage(px, py, x0, top, x1, bottom, r);
                        if fill_h > 0.0 {
                            fill += rr_coverage(px, py, x0, fill_top, x1, bottom, r);
                        }
                    }
                }
                let idx = (y * n + x) * 4;
                blend(&mut buf[idx..idx + 4], track_rgb, track / 4.0 * track_a);
                blend(&mut buf[idx..idx + 4], fill_rgb, fill / 4.0);
            }
        }
    }
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_custom() -> CustomColors {
        CustomColors::new()
    }

    /// The middle bar's ink, apart from the shared track.
    fn bar_colour(id: Option<&str>, custom: &CustomColors) -> [u8; 3] {
        ink(render(
            [None, Some(100.0), None],
            [None, id, None],
            false,
            custom,
        ))
    }

    /// The last opaque, non-track pixel in a rendered icon.
    fn ink(px: Vec<u8>) -> [u8; 3] {
        let mut best = [0u8; 3];
        for p in px.chunks(4) {
            if p[3] == 255 && p[..3] != [0, 0, 0] {
                best = [p[0], p[1], p[2]];
            }
        }
        best
    }

    #[test]
    fn renders_expected_buffer() {
        let px = render(
            [Some(58.0), Some(63.0), None],
            [Some("claude"), Some("codex"), None],
            true,
            &no_custom(),
        );
        assert_eq!(px.len(), (SIZE * SIZE * 4) as usize);
        assert!(
            px.chunks(4).any(|p| p[3] == 255),
            "a full bar draws opaque pixels"
        );
        assert!(
            px.chunks(4).any(|p| p[3] == 0),
            "background stays transparent"
        );
    }

    #[test]
    fn non_template_icon_carries_more_ink() {
        // Windows and Linux show the icon at 16 px and do not tint it, so the
        // drawing itself has to hold up: the coloured version paints wider
        // bars and a stronger track than the macOS template.
        let visible = |px: Vec<u8>| px.chunks(4).filter(|p| p[3] > 100).count();
        let ids = [Some("claude"), None, None];
        let solid = visible(render([Some(58.0), None, None], ids, false, &no_custom()));
        let template = visible(render([Some(58.0), None, None], ids, true, &no_custom()));
        assert!(
            solid > template,
            "coloured icon draws more than the template: {solid} vs {template}"
        );
        // Two empty slots still read as empty bars, not as gaps.
        let tracks = render([None; 3], [None; 3], false, &no_custom())
            .chunks(4)
            .filter(|p| p[3] > 100)
            .count();
        assert!(tracks > 0, "empty slots keep a visible track");
    }

    #[test]
    fn each_slot_uses_its_own_service_colour() {
        let none = no_custom();
        assert_eq!(
            bar_colour(Some("gemini"), &none),
            default_accent("gemini").unwrap()
        );
        assert_eq!(
            bar_colour(Some("copilot"), &none),
            default_accent("copilot").unwrap()
        );
        assert_ne!(
            bar_colour(Some("gemini"), &none),
            bar_colour(Some("claude"), &none)
        );
        assert_eq!(bar_colour(None, &none), NEUTRAL_ACCENT);
        assert_eq!(bar_colour(Some("unknown-service"), &none), NEUTRAL_ACCENT);
    }

    #[test]
    fn render_with_takes_the_apps_own_palette() {
        // The Dock paints Gemini sky-blue where the popover paints it cyan: it
        // hands its palette to `render_with`, and the shared one is unchanged.
        let dock = |id: &str| match id {
            "gemini" => Some([0x38, 0xBD, 0xF8]),
            other => default_accent(other),
        };
        let docked = ink(render_with(
            [None, Some(100.0), None],
            [None, Some("gemini"), None],
            false,
            &no_custom(),
            dock,
        ));
        assert_eq!(docked, [0x38, 0xBD, 0xF8]);
        assert_eq!(
            bar_colour(Some("gemini"), &no_custom()),
            default_accent("gemini").unwrap(),
            "plain render keeps the shared palette"
        );
        // An id the palette does not know still gets the neutral track colour.
        let bare = |_: &str| None;
        let unknown = ink(render_with(
            [None, Some(100.0), None],
            [None, Some("mystery"), None],
            false,
            &no_custom(),
            bare,
        ));
        assert_eq!(unknown, NEUTRAL_ACCENT);
        // A colour the user picked still wins over either palette.
        let mut custom = CustomColors::new();
        custom.insert("gemini".into(), [0xF5, 0xB0, 0x4C]);
        assert_eq!(
            ink(render_with(
                [None, Some(100.0), None],
                [None, Some("gemini"), None],
                false,
                &custom,
                dock,
            )),
            [0xF5, 0xB0, 0x4C]
        );
    }

    #[test]
    fn custom_colours_repaint_the_bars() {
        // A colour picked in Settings replaces that service's accent, and only
        // that service's.
        let mut custom = CustomColors::new();
        custom.insert("gemini".into(), [0xF5, 0xB0, 0x4C]);
        assert_eq!(bar_colour(Some("gemini"), &custom), [0xF5, 0xB0, 0x4C]);
        assert_eq!(
            bar_colour(Some("claude"), &custom),
            default_accent("claude").unwrap(),
            "other services keep their accent"
        );
        // The template is monochrome whatever the user picked: macOS tints it.
        let px = render(
            [Some(100.0), None, None],
            [Some("claude"), None, None],
            true,
            &custom,
        );
        for p in px.chunks(4) {
            assert!(
                p[3] == 0 || (p[0] == 0 && p[1] == 0 && p[2] == 0),
                "template ink is black"
            );
        }
    }

    #[test]
    fn parses_the_hex_the_ui_sends() {
        assert_eq!(parse_hex("#E08A6B"), Some([0xE0, 0x8A, 0x6B]));
        assert_eq!(parse_hex("#e08a6b"), Some([0xE0, 0x8A, 0x6B]));
        assert_eq!(parse_hex("  e08a6b "), Some([0xE0, 0x8A, 0x6B]));
        assert_eq!(parse_hex("#f00"), Some([0xFF, 0x00, 0x00]));
        assert_eq!(parse_hex("#000000"), Some([0, 0, 0]));
        assert_eq!(parse_hex("#12345"), None, "wrong length");
        assert_eq!(parse_hex("#12345g"), None, "not hex");
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn slots_follow_the_tightest_three() {
        let items = [
            ("claude", Some(70.0)),
            ("codex", Some(55.0)),
            ("grok", Some(12.0)),
            ("gemini", Some(88.0)),
            ("copilot", None),
        ];
        assert_eq!(
            pick_slots(&items),
            [Some("grok"), Some("codex"), Some("claude")],
            "tightest remaining quota comes first"
        );
        // Only two live: the third slot is the next fallback, never a blank.
        let items = [
            ("claude", None),
            ("codex", Some(40.0)),
            ("grok", Some(90.0)),
            ("gemini", None),
        ];
        assert_eq!(
            pick_slots(&items),
            [Some("codex"), Some("grok"), Some("claude")]
        );
        // Three live that do not include Claude: no extra fallback sneaks in.
        let items = [
            ("claude", None),
            ("codex", Some(40.0)),
            ("grok", Some(90.0)),
            ("gemini", Some(60.0)),
        ];
        assert_eq!(
            pick_slots(&items),
            [Some("codex"), Some("gemini"), Some("grok")]
        );
        // Nothing connected at all: the original three, all empty.
        let items = [("claude", None), ("gemini", None)];
        assert_eq!(pick_slots(&items), [Some("claude"), None, None]);
        assert_eq!(pick_slots(&[]), [None; 3]);
    }
}
