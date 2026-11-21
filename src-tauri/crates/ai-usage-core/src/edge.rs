//! Screen-edge docking: which edge the window is tucked against, when it
//! should snap or let go, and the eased frames that slide it in and out.
//!
//! Nothing here knows about Tauri — `main.rs` measures the window and the
//! monitor, hands the numbers over, and acts on the answer — so the snapping
//! rules and the animation curve are unit-tested on their own (`cargo test`).

/// How close (in logical px) to a screen edge a dropped window has to land to
/// snap against it.
pub const SNAP_ZONE: f64 = 24.0;

/// How far it has to be dragged back in before it lets go of the edge again.
/// Wider than [`SNAP_ZONE`], so a window sitting on the edge does not flicker
/// between docked and free.
pub const RELEASE_ZONE: f64 = 48.0;

/// How much of the window (logical px) stays on screen once it is tucked away:
/// just enough for the grab handle.
pub const HANDLE: f64 = 12.0;

/// Frames in one slide and the pause between them: 18 × 12 ms ≈ 215 ms, which
/// reads as a single quick movement rather than a series of steps.
pub const FRAMES: u32 = 18;
pub const FRAME_MS: u64 = 12;

/// Which screen edge the window is tucked against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Dock {
    /// Free-floating, like a plain tray popover.
    #[default]
    None,
    Left,
    Right,
}

impl Dock {
    /// What the frontend compares against (`"left"`, `"right"`, `"none"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Dock::None => "none",
            Dock::Left => "left",
            Dock::Right => "right",
        }
    }

    pub fn is_docked(self) -> bool {
        self != Dock::None
    }
}

/// Which edge a window at `x` (physical px, window width `w`) is close enough
/// to on a monitor spanning `[mx, mx + mw]`. `scale` is physical px per
/// logical px, so the zone feels the same on a Retina screen.
pub fn detect(x: f64, w: f64, mx: f64, mw: f64, scale: f64) -> Dock {
    let zone = SNAP_ZONE * scale;
    if x - mx <= zone {
        Dock::Left
    } else if (mx + mw) - (x + w) <= zone {
        Dock::Right
    } else {
        Dock::None
    }
}

/// Whether a docked window has been carried far enough inland to let go.
pub fn released(x: f64, w: f64, mx: f64, mw: f64, scale: f64, dock: Dock) -> bool {
    let reach = RELEASE_ZONE * scale;
    match dock {
        Dock::Left => x - mx > reach,
        Dock::Right => (mx + mw) - (x + w) > reach,
        Dock::None => true,
    }
}

/// The dock state after a settled drag: keep the current edge while the window
/// is still near it, let go once it is carried away, and pick up an edge when
/// it was dropped against one.
pub fn next(dock: Dock, x: f64, w: f64, mx: f64, mw: f64, scale: f64) -> Dock {
    if dock.is_docked() && !released(x, w, mx, mw, scale, dock) {
        return dock;
    }
    detect(x, w, mx, mw, scale)
}

/// Window x with the whole window on screen against `dock`'s edge.
pub fn visible_x(dock: Dock, w: f64, mx: f64, mw: f64) -> f64 {
    match dock {
        Dock::Right => mx + mw - w,
        _ => mx,
    }
}

/// Window x with only [`HANDLE`] logical px left on screen.
pub fn hidden_x(dock: Dock, w: f64, mx: f64, mw: f64, scale: f64) -> f64 {
    let handle = HANDLE * scale;
    match dock {
        Dock::Right => mx + mw - handle,
        _ => mx - w + handle,
    }
}

/// How far along a slide is after `t` (0…1) with an ease-out cubic: quick off
/// the mark, soft landing.
pub fn ease_out_cubic(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// The window x positions of one slide, start and end included: [`FRAMES`]
/// steps, each a little smaller than the one before it.
pub fn frames(from: f64, to: f64) -> Vec<i32> {
    (0..=FRAMES)
        .map(|i| {
            let t = i as f64 / FRAMES as f64;
            (from + (to - from) * ease_out_cubic(t)).round() as i32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1440-wide monitor at 2× with a 744-wide window (372 logical).
    const MX: f64 = 0.0;
    const MW: f64 = 2880.0;
    const W: f64 = 744.0;
    const S: f64 = 2.0;

    #[test]
    fn detects_the_edge_inside_the_zone_only() {
        assert_eq!(detect(MX + 10.0, W, MX, MW, S), Dock::Left);
        assert_eq!(
            detect(MX + 48.0, W, MX, MW, S),
            Dock::Left,
            "exactly 24 logical px is still the zone"
        );
        assert_eq!(detect(MX + 49.0, W, MX, MW, S), Dock::None);
        assert_eq!(detect(MX + MW - W - 20.0, W, MX, MW, S), Dock::Right);
        assert_eq!(detect(MX + MW - W - 60.0, W, MX, MW, S), Dock::None);
        // In the middle nothing happens, and the zone grows with the scale.
        assert_eq!(detect(900.0, W, MX, MW, S), Dock::None);
        assert_eq!(detect(MX + 40.0, W, MX, MW, 1.0), Dock::None);
        assert_eq!(detect(MX + 40.0, W, MX, MW, 4.0), Dock::Left);
        // A monitor that does not start at zero (second display) is measured
        // from its own origin.
        assert_eq!(detect(2880.0 + 12.0, W, 2880.0, 1920.0, S), Dock::Left);
    }

    #[test]
    fn lets_go_when_carried_away() {
        assert!(
            !released(MX + 30.0, W, MX, MW, S, Dock::Left),
            "a nudge does not undock"
        );
        assert!(released(MX + 100.0, W, MX, MW, S, Dock::Left));
        assert!(!released(MX + MW - W - 30.0, W, MX, MW, S, Dock::Right));
        assert!(released(MX + MW - W - 100.0, W, MX, MW, S, Dock::Right));
        assert!(released(900.0, W, MX, MW, S, Dock::None));
    }

    #[test]
    fn keeps_holds_and_picks_up_edges() {
        // Dropped in the middle: stays free.
        assert_eq!(next(Dock::None, 900.0, W, MX, MW, S), Dock::None);
        // Dropped against the left edge: docks.
        assert_eq!(next(Dock::None, MX + 8.0, W, MX, MW, S), Dock::Left);
        assert_eq!(
            next(Dock::None, MX + MW - W - 4.0, W, MX, MW, S),
            Dock::Right
        );
        // Already docked and nudged a little: keeps the edge (no flicker).
        assert_eq!(next(Dock::Left, MX + 30.0, W, MX, MW, S), Dock::Left);
        // Carried away: released, and the drop spot decides again.
        assert_eq!(next(Dock::Left, 700.0, W, MX, MW, S), Dock::None);
        assert_eq!(
            next(Dock::Left, MX + MW - W - 2.0, W, MX, MW, S),
            Dock::Right
        );
    }

    #[test]
    fn tucked_leaves_exactly_the_handle_on_screen() {
        let left = hidden_x(Dock::Left, W, MX, MW, S);
        assert_eq!(
            left + W - MX,
            HANDLE * S,
            "right edge of the window, handle wide, is the screen edge"
        );
        let right = hidden_x(Dock::Right, W, MX, MW, S);
        assert_eq!(MX + MW - right, HANDLE * S);
        // Visible positions line the window up flush with the edge.
        assert_eq!(visible_x(Dock::Left, W, MX, MW), MX);
        assert_eq!(visible_x(Dock::Right, W, MX, MW), MX + MW - W);
    }

    #[test]
    fn ease_out_cubic_is_smooth_and_lands_exactly() {
        assert_eq!(ease_out_cubic(0.0), 0.0);
        assert_eq!(ease_out_cubic(1.0), 1.0);
        assert!((ease_out_cubic(0.5) - 0.875).abs() < 1e-9);
        // Clamped outside 0…1, and never going backwards.
        assert_eq!(ease_out_cubic(-2.0), 0.0);
        assert_eq!(ease_out_cubic(3.0), 1.0);
        let mut prev = 0.0;
        for i in 0..=20 {
            let v = ease_out_cubic(i as f64 / 20.0);
            assert!(v >= prev, "monotone at {i}");
            prev = v;
        }
    }

    #[test]
    fn frames_start_and_end_on_target_and_front_load_the_travel() {
        let from = 400.0;
        let to = hidden_x(Dock::Right, W, MX, MW, S);
        let f = frames(from, to);
        assert_eq!(f.len() as u32, FRAMES + 1, "start plus one frame per step");
        assert_eq!(f[0], 400, "starts where the window is");
        assert_eq!(*f.last().unwrap(), to.round() as i32, "ends exactly tucked");
        let total = (to - from).abs();
        // Ease-out: the first quarter of the time covers well over a quarter of
        // the distance, and the last quarter creeps.
        let q = FRAMES as usize / 4;
        let early = (f[q] as f64 - from).abs();
        assert!(early > total * 0.5, "front-loaded: {early} of {total}");
        let late = (f[FRAMES as usize] as f64 - f[FRAMES as usize - q] as f64).abs();
        assert!(late < total * 0.1, "soft landing: {late} of {total}");
        // Targets are whole pixels and never jump back and forth.
        for w in f.windows(2) {
            assert!(w[1] >= w[0], "monotone toward the right edge");
        }
        // Sliding back in is the same trip in reverse.
        let back = frames(to, from);
        assert_eq!(back[0], to.round() as i32);
        assert_eq!(*back.last().unwrap(), 400);
    }
}
