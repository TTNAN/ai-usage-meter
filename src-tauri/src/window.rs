//! Everything that moves, shows or hides the window, plus the tray icon and the
//! global shortcut: the tray's own picture and tooltip, the screen-edge maths
//! that tucks the popover away, the drag watcher's target, and the one key
//! combination that summons it.
//!
//! Nothing in the `Moved` path may block — see `AppState::move_note`.

use crate::lock::lock;
use crate::poller::now_ms;
use crate::AppState;
use ai_usage_core::providers::{ServiceStatus, State as SvcState};
use ai_usage_core::{edge, providers, tray_icon};
use std::sync::atomic::Ordering;
use std::time::Duration;
use tauri::{
    image::Image, AppHandle, Emitter, Manager, PhysicalPosition, Rect, State, WebviewWindow,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// ⌘⇧U on macOS, Ctrl+Shift+U elsewhere. The UI can switch or turn it off.
pub(crate) const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+U";

/// Whether a login is a Wayland session, from the two variables that say so.
///
/// `XDG_SESSION_TYPE` is the session's own answer and wins whenever it says
/// anything: an X11 session can still inherit a `WAYLAND_DISPLAY` from whatever
/// launched it, and reading that alone used to put the window's tucking and the
/// shortcut's error message in the wrong world. `WAYLAND_DISPLAY` is only the
/// fallback for a login that never set the session type.
pub(crate) fn is_wayland_env(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    match session_type.map(str::trim).filter(|t| !t.is_empty()) {
        Some(kind) => kind.eq_ignore_ascii_case("wayland"),
        None => wayland_display.map(|v| !v.is_empty()).unwrap_or(false),
    }
}

/// Linux on Wayland: the compositor places windows itself (an app may not
/// move its own) and hands out no global shortcut the way X11 does. Everything
/// that slides or tucks the window, and the shortcut's error message, asks
/// this first; on macOS and Windows it is always false.
pub(crate) fn on_wayland() -> bool {
    if !cfg!(target_os = "linux") {
        return false;
    }
    // `std::env` reads a non-Unicode value as an error; such a value is not a
    // session type this app knows, so it counts as unset here.
    let session = std::env::var("XDG_SESSION_TYPE").ok();
    let display = std::env::var("WAYLAND_DISPLAY").ok();
    is_wayland_env(session.as_deref(), display.as_deref())
}

/// Minimum seconds between automatic fetches, per provider.
///
/// Codex costs nothing to poll: its numbers are read from the session log the
/// Codex CLI already wrote on this machine, not from the network. Every other
/// provider is one request to a service whose endpoints were never meant for
/// a poller, and Claude's usage endpoint rate-limits hardest — so they all
/// wait five minutes (already five times the Codex interval), which keeps a
/// row at most a few minutes behind without hammering anyone.

// ----------------------------------------------------------------- tray

pub(crate) fn left_percent(s: &ServiceStatus) -> Option<f64> {
    if s.has_numbers() {
        s.peak_used().map(|u| (100.0 - u).max(0.0))
    } else {
        None
    }
}

pub(crate) fn tooltip(status: &[ServiceStatus]) -> String {
    let mut lines = vec!["AI Usage Meter".to_string()];
    for s in status {
        lines.push(match (s.state, left_percent(s)) {
            (SvcState::Connected, Some(l)) => format!("{}  {:.0}% left", s.label, l),
            (SvcState::Mock, Some(l)) => format!("{}  {:.0}% left (demo)", s.label, l),
            (SvcState::NotConnected, _) => format!("{}  not connected", s.label),
            (SvcState::Expired, _) => format!("{}  login expired", s.label),
            (SvcState::Changed, _) => format!("{}  API changed", s.label),
            _ => format!("{}  unavailable", s.label),
        });
    }
    lines.join("\n")
}

pub(crate) fn update_tray(app: &AppHandle, status: &[ServiceStatus]) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    // Three bars for the three services closest to running out; see
    // `tray_icon::pick_slots`.
    let items: Vec<(&'static str, Option<f64>)> = providers::IDS
        .iter()
        .map(|id| {
            (
                *id,
                status.iter().find(|s| s.id == *id).and_then(left_percent),
            )
        })
        .collect();
    let ids = tray_icon::pick_slots(&items);
    let left: [Option<f64>; tray_icon::SLOTS] = std::array::from_fn(|i| {
        ids[i].and_then(|id| items.iter().find(|(item, _)| *item == id)?.1)
    });
    let template = cfg!(target_os = "macos");
    let custom = lock(&app.state::<AppState>().custom_colors).clone();
    let rgba = tray_icon::render(left, ids, template, &custom);
    let _ = tray.set_icon(Some(Image::new_owned(
        rgba,
        tray_icon::SIZE,
        tray_icon::SIZE,
    )));
    #[cfg(target_os = "macos")]
    let _ = tray.set_icon_as_template(true);
    let _ = tray.set_tooltip(Some(tooltip(status)));

    #[cfg(target_os = "macos")]
    {
        let state: State<AppState> = app.state();
        let mode = lock(&state.tray_mode).clone();
        let tightest = left
            .iter()
            .flatten()
            .cloned()
            .fold(None, |a: Option<f64>, v| Some(a.map_or(v, |a| a.min(v))));
        let title = match (mode.as_str(), tightest) {
            ("percent", Some(v)) => Some(format!("{v:.0}%")),
            _ => None,
        };
        let _ = tray.set_title(title);
    }
}

// ------------------------------------------------------------------ edge
//
// The window can be dragged anywhere and tucked against the left or right
// screen edge. While tucked it slides almost off the screen on blur, leaving a
// 12 px handle, and slides back in when that handle (or the tray icon) is
// clicked. `edge` holds the arithmetic; this section does the moving.

/// Physical size and position of the window plus the monitor it is on.
struct Geometry {
    x: f64,
    y: f64,
    w: f64,
    mx: f64,
    mw: f64,
    scale: f64,
}

pub(crate) fn geometry(window: &WebviewWindow) -> Option<Geometry> {
    let pos = window.outer_position().ok()?;
    let size = window.outer_size().ok()?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let monitor = window.current_monitor().ok().flatten()?;
    let (mp, ms) = (monitor.position(), monitor.size());
    Some(Geometry {
        x: pos.x as f64,
        y: pos.y as f64,
        w: size.width as f64,
        mx: mp.x as f64,
        mw: ms.width as f64,
        scale,
    })
}

/// Move the window and remember that we did, so the resulting `Moved` event is
/// not read as a drag.
pub(crate) fn place(window: &WebviewWindow, state: &AppState, x: f64, y: f64) {
    let (x, y) = (x.round(), y.round());
    state.move_note.note(x, y);
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}

pub(crate) fn emit_edge(app: &AppHandle) {
    let state: State<AppState> = app.state();
    let dock = *lock(&state.edge_dock);
    let payload = serde_json::json!({
        "edge": dock.as_str(),
        "hidden": state.edge_hidden.load(Ordering::Relaxed),
        "enabled": state.edge_enabled.load(Ordering::Relaxed),
    });
    let _ = app.emit("edge-state", payload);
}

/// A drag has come to rest: snap against an edge the window was dropped near,
/// let go when it was carried inland, and keep the edge otherwise.
pub(crate) fn settle_window(app: &AppHandle) {
    let state: State<AppState> = app.state();
    if !state.edge_enabled.load(Ordering::Relaxed) || state.edge_anim.load(Ordering::Relaxed) {
        return;
    }
    let Some(window) = main_window(app) else {
        return;
    };
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let Some(g) = geometry(&window) else { return };
    let before = *lock(&state.edge_dock);
    let after = edge::next(before, g.x, g.w, g.mx, g.mw, g.scale);
    if after != before {
        *lock(&state.edge_dock) = after;
        state.edge_hidden.store(false, Ordering::Relaxed);
        emit_edge(app);
    }
    let Some(x) = docked_x(&state, after, &g) else {
        return;
    };
    if (g.x - x).abs() > 0.5 {
        place(&window, &state, x, g.y);
    }
}

/// Where a docked window belongs right now (tucked or out), `None` when it is
/// not docked at all.
pub(crate) fn docked_x(state: &AppState, dock: edge::Dock, g: &Geometry) -> Option<f64> {
    if !dock.is_docked() {
        return None;
    }
    Some(if state.edge_hidden.load(Ordering::Relaxed) {
        edge::hidden_x(dock, g.w, g.mx, g.mw, g.scale)
    } else {
        edge::visible_x(dock, g.w, g.mx, g.mw)
    })
}

/// Slide the window out of view (`tuck`) or back in, one eased frame at a time.
pub(crate) fn slide_window(app: &AppHandle, tuck: bool) {
    let state: State<AppState> = app.state();
    let dock = *lock(&state.edge_dock);
    if !dock.is_docked() {
        return;
    }
    if state.edge_anim.swap(true, Ordering::Relaxed) {
        return;
    }
    let Some(window) = main_window(app) else {
        state.edge_anim.store(false, Ordering::Relaxed);
        return;
    };
    let Some(g) = geometry(&window) else {
        state.edge_anim.store(false, Ordering::Relaxed);
        return;
    };
    let out = edge::visible_x(dock, g.w, g.mx, g.mw);
    let tucked = edge::hidden_x(dock, g.w, g.mx, g.mw, g.scale);
    let (from, to) = if tuck {
        (g.x.min(out), tucked)
    } else {
        (g.x, out)
    };
    let handle = app.clone();
    std::thread::spawn(move || {
        let state: State<AppState> = handle.state();
        // Tell the UI first: the card fades while the window travels.
        state.edge_hidden.store(tuck, Ordering::Relaxed);
        emit_edge(&handle);
        for x in edge::frames(from, to) {
            place(&window, &state, x as f64, g.y);
            std::thread::sleep(Duration::from_millis(edge::FRAME_MS));
        }
        state.edge_anim.store(false, Ordering::Relaxed);
        if !tuck {
            let _ = window.set_focus();
        }
        emit_edge(&handle);
    });
}

// --------------------------------------------------------------- window

pub(crate) fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Place the popover under the menu-bar icon (macOS) or above the tray icon
/// (Windows taskbar), clamped to the screen it is on. A Linux tray reports no
/// icon rectangle, so there it opens at the top-right instead — the corner
/// every common panel sits in. A docked window is left at its edge instead.
///
/// Wayland ignores everything here: the compositor places the window itself.
pub(crate) fn position_window(app: &AppHandle, window: &WebviewWindow) {
    let Ok(size) = window.outer_size() else {
        return;
    };
    let (w, h) = (size.width as f64, size.height as f64);
    let scale = window.scale_factor().unwrap_or(1.0);
    let margin = 8.0 * scale;
    let state: State<AppState> = app.state();
    let anchor = *lock(&state.anchor);

    let monitors = window.available_monitors().unwrap_or_default();
    let point = anchor.map(|(x, y, aw, _)| (x + aw / 2.0, y));
    let monitor = point
        .and_then(|(px, py)| {
            monitors.iter().find(|m| {
                let (p, s) = (m.position(), m.size());
                px >= p.x as f64
                    && px < p.x as f64 + s.width as f64
                    && py >= p.y as f64
                    && py < p.y as f64 + s.height as f64
            })
        })
        .cloned()
        .or_else(|| window.current_monitor().ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return };
    let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
    let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);

    // Docked: the edge decides, not the tray icon.
    let dock = *lock(&state.edge_dock);
    if dock.is_docked() {
        let x = if state.edge_hidden.load(Ordering::Relaxed) {
            edge::hidden_x(dock, w, mx, mw, scale)
        } else {
            edge::visible_x(dock, w, mx, mw)
        };
        let y = window
            .outer_position()
            .map(|p| p.y as f64)
            .unwrap_or(my + margin)
            .clamp(my, (my + mh - h).max(my));
        place(window, &state, x, y);
        return;
    }

    let (x, y) = match anchor {
        Some((ax, ay, aw, ah)) => {
            let x = ax + aw / 2.0 - w / 2.0;
            let y = if ay < my + mh / 2.0 {
                ay + ah + margin * 0.5
            } else {
                ay - h - margin * 0.5
            };
            (x, y)
        }
        // Opened from the menu, before any click: top-right on macOS and on
        // Linux (whose panels usually sit at the top), bottom-right on Windows.
        None if cfg!(any(target_os = "macos", target_os = "linux")) => {
            (mx + mw - w - margin, my + 32.0 * scale)
        }
        None => (mx + mw - w - margin, my + mh - h - 56.0 * scale),
    };
    let x = x.clamp(mx + margin, (mx + mw - w - margin).max(mx + margin));
    let y = y.clamp(my, (my + mh - h).max(my));
    place(window, &state, x, y);
}

pub(crate) fn show_window(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    let state: State<AppState> = app.state();
    // Tucked away at an edge: come back out instead of jumping under the tray.
    let tucked = state.edge_hidden.load(Ordering::Relaxed) && lock(&state.edge_dock).is_docked();
    if tucked {
        let _ = window.show();
        state.shown_at.store(now_ms(), Ordering::Relaxed);
        let _ = window.set_focus();
        let _ = app.emit("popover-shown", ());
        slide_window(app, false);
        return;
    }
    position_window(app, &window);
    let _ = window.show();
    state.shown_at.store(now_ms(), Ordering::Relaxed);
    let _ = window.set_focus();
    let _ = app.emit("popover-shown", ());
}

pub(crate) fn toggle_window(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    let state: State<AppState> = app.state();
    let docked = state.edge_enabled.load(Ordering::Relaxed) && lock(&state.edge_dock).is_docked();
    if window.is_visible().unwrap_or(false) {
        if state.edge_hidden.load(Ordering::Relaxed) {
            slide_window(app, false);
        } else if docked {
            // Docked and out: the tray tucks it back against the edge.
            slide_window(app, true);
        } else {
            let _ = window.hide();
        }
        return;
    }
    // Clicking the tray icon while the popover is open first blurs (and
    // hides) it; don't immediately re-open it on the same click.
    let hidden_at = state.blur_hidden_at.load(Ordering::Relaxed);
    if now_ms() - hidden_at < 350 {
        return;
    }
    show_window(app);
}

// ------------------------------------------------------ global shortcut
//
// One key combination, held system-wide: ⌘⇧U / Ctrl+Shift+U unless the user
// picks another or turns it off in Settings. It brings the popover out of the
// edge it is tucked against, or puts it away again.

/// Hold `keys` as the global shortcut, replacing whatever was held before.
/// An empty string turns it off. A combination another app already owns comes
/// back as an error — nothing crashes, and the previous shortcut stays gone
/// only because the new one could not be registered.
pub(crate) fn apply_shortcut(app: &AppHandle, keys: &str) -> Result<(), String> {
    let state: State<AppState> = app.state();
    let manager = app.global_shortcut();
    let _ = manager.unregister_all();
    *lock(&state.shortcut) = None;
    let keys = keys.trim();
    if keys.is_empty() || keys.eq_ignore_ascii_case("off") {
        return Ok(());
    }
    // Wayland gives a key combination to no app it is not focused on. The X11
    // plugin would still report success, and the key would do nothing — so say
    // no, and let the UI explain.
    if on_wayland() {
        return Err("Wayland does not allow a global shortcut".into());
    }
    manager
        .on_shortcut(keys, |app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_window_from_shortcut(app);
            }
        })
        .map_err(|e| format!("{keys}: {e}"))?;
    *lock(&state.shortcut) = Some(keys.to_string());
    Ok(())
}

/// The shortcut's own toggle: it never advances the tray anchor, and it brings
/// a window that is open but behind something to the front instead of hiding it.
pub(crate) fn toggle_window_from_shortcut(app: &AppHandle) {
    let Some(window) = main_window(app) else {
        return;
    };
    let state: State<AppState> = app.state();
    if !window.is_visible().unwrap_or(false) {
        show_window(app);
        return;
    }
    let tucked = state.edge_hidden.load(Ordering::Relaxed);
    if tucked {
        let _ = window.show();
        let _ = window.set_focus();
        let _ = app.emit("popover-shown", ());
        slide_window(app, false);
        return;
    }
    if !window.is_focused().unwrap_or(false) {
        // Open but behind another app: the shortcut means "show me".
        let _ = window.set_focus();
        let _ = app.emit("popover-shown", ());
        return;
    }
    // Visible and focused: put it away the way Esc does.
    if state.edge_enabled.load(Ordering::Relaxed) && lock(&state.edge_dock).is_docked() {
        slide_window(app, true);
    } else {
        let _ = window.hide();
        state.blur_hidden_at.store(now_ms(), Ordering::Relaxed);
    }
}

pub(crate) fn store_anchor(app: &AppHandle, rect: &Rect) {
    let scale = main_window(app)
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    let pos = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    *lock(&app.state::<AppState>().anchor) = Some((pos.x, pos.y, size.width, size.height));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai_usage_core::edge::MoveNote;

    #[test]
    fn a_wayland_session_is_told_by_the_session_type_first() {
        // An X11 login that inherited WAYLAND_DISPLAY from a parent process is
        // still X11: the session type is the answer.
        assert!(!is_wayland_env(Some("x11"), Some("wayland-0")));
        assert!(!is_wayland_env(Some("X11"), Some("wayland-0")));
        assert!(is_wayland_env(Some("wayland"), None));
        assert!(is_wayland_env(Some("Wayland"), Some("wayland-0")));
        // No session type at all: the display variable decides.
        assert!(is_wayland_env(None, Some("wayland-0")));
        assert!(!is_wayland_env(None, None));
        assert!(!is_wayland_env(None, Some("")));
        // An empty session type is not an answer either.
        assert!(is_wayland_env(Some(""), Some("wayland-0")));
        assert!(!is_wayland_env(Some("  "), None));
        // A session type this app does not know is not Wayland.
        assert!(!is_wayland_env(Some("tty"), Some("wayland-0")));
    }

    #[test]
    fn our_own_move_is_claimed_once_and_a_drag_is_not() {
        let note = MoveNote::new();
        assert!(!note.claim(10.0, 10.0), "nothing announced yet");
        note.note(120.0, 240.0);
        assert!(note.claim(120.0, 240.0), "the move we asked for");
        assert!(!note.claim(120.0, 240.0), "and only that one event");
        assert!(!note.claim(300.0, 400.0), "a drag of the user's is theirs");
    }

    /// The freeze this replaced: the handler used to lock a `Mutex` here and,
    /// on a match, lock it again on the same thread. A claim that could block
    /// would hang this test rather than fail it, so a timeout is what is
    /// asserted — this is the regression the lock-free note exists for.
    #[test]
    fn claiming_never_waits_for_the_thread_that_moves_the_window() {
        use std::sync::mpsc;
        use std::sync::Arc;
        use std::time::Duration;

        let note = Arc::new(MoveNote::new());
        let (tx, rx) = mpsc::channel();
        let moving = note.clone();
        std::thread::spawn(move || {
            for i in 0..10_000 {
                moving.note(10.0, i as f64);
                let _ = moving.claim(10.0, i as f64);
            }
            let _ = tx.send(());
        });
        for i in 0..10_000 {
            note.note(i as f64, 20.0);
            let _ = note.claim(i as f64, 20.0);
        }
        assert!(
            rx.recv_timeout(Duration::from_secs(10)).is_ok(),
            "a claim blocked the thread that moves the window"
        );
    }
}
