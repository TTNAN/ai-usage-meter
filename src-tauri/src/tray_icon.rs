//! Draws the live menu-bar / tray icon: three tiny "battery" bars, one per
//! service, filled to the share that is still LEFT. No image crates — just
//! an anti-aliased RGBA buffer.
//!
//! * macOS: a monochrome *template* image, so the system tints it for light
//!   and dark menu bars like every native icon.
//! * Windows / Linux: the services' accent colours on a soft track.

/// Icon edge in pixels (22 pt menu bar @2x).
pub const SIZE: u32 = 44;

const ACCENTS: [[u8; 3]; 3] = [
    [0xE0, 0x8A, 0x6B], // Claude — clay
    [0x6E, 0x9B, 0xFF], // Codex  — azure
    [0xB3, 0x8C, 0xFF], // Grok   — violet
];

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

/// Render the icon. `left` is the percent remaining per service
/// (Claude, Codex, Grok); `None` draws an empty track.
pub fn render(left: [Option<f64>; 3], template: bool) -> Vec<u8> {
    let n = SIZE as usize;
    let mut buf = vec![0u8; n * n * 4];
    let (bar_w, gap, r) = (8.0f32, 5.0f32, 3.0f32);
    let total = bar_w * 3.0 + gap * 2.0;
    let x_start = (SIZE as f32 - total) / 2.0;
    let (top, bottom) = (7.0f32, 37.0f32);

    for (i, value) in left.iter().enumerate() {
        let x0 = x_start + i as f32 * (bar_w + gap);
        let x1 = x0 + bar_w;
        let frac = value.map(|v| (v / 100.0).clamp(0.0, 1.0) as f32).unwrap_or(0.0);
        // Keep a visible sliver whenever there is anything left at all.
        let fill_h = if frac > 0.0 { ((bottom - top) * frac).max(r * 2.0) } else { 0.0 };
        let fill_top = bottom - fill_h;
        let (track_rgb, track_a, fill_rgb) = if template {
            ([0, 0, 0], 0.28, [0, 0, 0])
        } else {
            ([0x9A, 0x9A, 0xA2], 0.35, ACCENTS[i])
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

    #[test]
    fn renders_expected_buffer() {
        let px = render([Some(58.0), Some(63.0), None], true);
        assert_eq!(px.len(), (SIZE * SIZE * 4) as usize);
        assert!(px.chunks(4).any(|p| p[3] == 255), "a full bar draws opaque pixels");
        assert!(px.chunks(4).any(|p| p[3] == 0), "background stays transparent");
    }
}
