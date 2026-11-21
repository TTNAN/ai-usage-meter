//! AI Usage Meter — a menu-bar (macOS) / system-tray (Windows, Linux) app that
//! shows how much Claude, ChatGPT (Codex), Grok, Gemini and GitHub Copilot
//! quota you have left. One window, two shapes: the rings popover (372 px, every
//! window and countdown in a row) and the dock panel (five brand squircles on one
//! tray, as wide as its tiles), switched in the UI and sized from there.
//!
//! * `ai-usage-core` talks to each service (read-only, reusing CLI logins),
//!   draws the live tray icon and decides the low-quota notifications.
//! * This file wires the tray, the window (both shapes), the global shortcut
//!   and a background poller.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod web;

use ai_usage_core::providers::{ServiceStatus, State as SvcState};
use ai_usage_core::{edge, notify, providers, tray_icon};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Rect, State, WebviewWindow,
    WindowEvent,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_notification::NotificationExt;

/// Locks a mutex without letting one thread's panic spread through the app.
///
/// A poisoned mutex still holds whatever was in it, and for every value here
/// a slightly stale map or list is far better than a tray app that cannot
/// answer again: a poll is retried in minutes, but a poisoned `unwrap()` makes
/// every later caller panic too, including the ones on the UI thread.
fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

const TICK: Duration = Duration::from_secs(30);
/// ⌘⇧U on macOS, Ctrl+Shift+U elsewhere. The UI can switch or turn it off.
const DEFAULT_SHORTCUT: &str = "CommandOrControl+Shift+U";

/// Whether a login is a Wayland session, from the two variables that say so.
///
/// `XDG_SESSION_TYPE` is the session's own answer and wins whenever it says
/// anything: an X11 session can still inherit a `WAYLAND_DISPLAY` from whatever
/// launched it, and reading that alone used to put the window's tucking and the
/// shortcut's error message in the wrong world. `WAYLAND_DISPLAY` is only the
/// fallback for a login that never set the session type.
fn is_wayland_env(session_type: Option<&str>, wayland_display: Option<&str>) -> bool {
    match session_type.map(str::trim).filter(|t| !t.is_empty()) {
        Some(kind) => kind.eq_ignore_ascii_case("wayland"),
        None => wayland_display.map(|v| !v.is_empty()).unwrap_or(false),
    }
}

/// Linux on Wayland: the compositor places windows itself (an app may not
/// move its own) and hands out no global shortcut the way X11 does. Everything
/// that slides or tucks the window, and the shortcut's error message, asks
/// this first; on macOS and Windows it is always false.
fn on_wayland() -> bool {
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
fn poll_interval_secs(id: &str) -> i64 {
    match id {
        "codex" => 60,
        _ => 300,
    }
}
/// Floor for manual refreshes, so hammering ⌘R can't get anyone limited.
const MANUAL_FLOOR_SECS: i64 = 20;

fn mock_mode() -> bool {
    std::env::var("AI_USAGE_METER_MOCK")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[derive(Default)]
struct AppState {
    /// What the popover shows: the CLI result merged with the web result.
    cache: Mutex<Vec<ServiceStatus>>,
    /// Latest result per service from the CLI logins.
    cli: Mutex<HashMap<&'static str, ServiceStatus>>,
    /// Latest result per service from the in-app web sign-in.
    web: Mutex<HashMap<&'static str, ServiceStatus>>,
    web_logins: Mutex<HashSet<String>>,
    web_last_poll: Mutex<HashMap<&'static str, i64>>,
    web_last_report: Mutex<HashMap<&'static str, i64>>,
    web_use_home: Mutex<HashSet<&'static str>>,
    /// Which generation of hidden worker each site is on, so a deferred close
    /// after a report can't take down a window a newer poll just took over.
    web_worker_epoch: Mutex<HashMap<&'static str, u64>>,
    last_fetch: Mutex<HashMap<&'static str, i64>>,
    /// Tray icon rectangle (physical px) from the last click, for anchoring.
    anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    /// "icon" (bars only) or "percent" (bars + tightest % in the menu bar).
    tray_mode: Mutex<String>,
    /// The "Low-quota alerts" switch in Settings (on unless turned off).
    notify_enabled: AtomicBool,
    /// The interface language the popover resolved ("zh" or "en"), so the
    /// low-quota notifications are written in it too. "auto" is settled in
    /// the UI, which sends the answer down with `set_lang`.
    notify_lang: Mutex<String>,
    /// What has already been announced, so one reset window is announced once.
    alerts: Mutex<notify::Alerts>,
    /// When the popover lost focus. Set on the way out, so a tray click that
    /// arrives just after it can tell that the popover is on its way down.
    blur_hidden_at: AtomicI64,
    /// When the popover last became visible, so the hide a blur schedules does
    /// nothing if a tray click has reopened the window in the meantime.
    shown_at: AtomicI64,
    /// The size last handed to `set_size`, in thousandths of a logical px:
    /// the page measures itself again after every resize and reports the new
    /// size back, and without this the two keep answering each other. The
    /// width is part of it now that Dock view is as wide as its own tiles.
    applied_w: AtomicI64,
    applied_h: AtomicI64,
    /// Accent colours picked in Settings, for the Windows / Linux tray bars.
    /// (macOS tints its own template image instead.)
    custom_colors: Mutex<tray_icon::CustomColors>,
    /// Which screen edge the window is tucked against (`None` = a plain tray
    /// popover that just hides on blur).
    edge_dock: Mutex<edge::Dock>,
    /// Whether it is currently slid mostly off-screen.
    edge_hidden: AtomicBool,
    /// The "Tuck at screen edge" switch (on unless turned off).
    edge_enabled: AtomicBool,
    /// One slide at a time; window moves during a slide are ours.
    edge_anim: AtomicBool,
    /// Bumped whenever the user moves the window, so the watcher below can tell
    /// a finished drag from one still in progress.
    move_seq: AtomicI64,
    /// The `move_seq` value already settled, so each drag is judged once.
    move_settled: AtomicI64,
    /// Where our own `set_position` is putting the window, so the `Moved`
    /// event it causes is not mistaken for a drag. Deliberately lock-free: the
    /// `Moved` handler runs on the UI thread, and a `Mutex` on that path is one
    /// re-entrant lock away from a window that never answers again.
    move_note: edge::MoveNote,
    /// The global shortcut currently held, or `None` when it is switched off.
    shortcut: Mutex<Option<String>>,
}

// ------------------------------------------------------------- fetching

fn fetch_due(state: &AppState, force: bool) -> Vec<ServiceStatus> {
    if mock_mode() {
        let mut cache = lock(&state.cache);
        if cache.is_empty() {
            *cache = providers::mock_status();
        }
        return cache.clone();
    }

    let now = providers::now_unix();
    let due: Vec<&'static str> = {
        let cli = lock(&state.cli);
        let last = lock(&state.last_fetch);
        providers::IDS
            .iter()
            .copied()
            .filter(|id| {
                let since = now - last.get(id).copied().unwrap_or(0);
                let cached = cli.contains_key(id);
                !cached
                    || since
                        >= if force {
                            MANUAL_FLOOR_SECS
                        } else {
                            poll_interval_secs(id)
                        }
            })
            .collect()
    };
    if due.is_empty() {
        return lock(&state.cache).clone();
    }

    // Network calls happen outside the locks, in parallel.
    let fresh: Vec<ServiceStatus> = std::thread::scope(|s| {
        let handles: Vec<_> = due
            .iter()
            .map(|id| s.spawn(move || providers::fetch_one(id)))
            .collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    });

    {
        let mut cli = lock(&state.cli);
        let mut last = lock(&state.last_fetch);
        for f in fresh {
            last.insert(f.id, now);
            let merged = {
                let previous = cli.get(f.id);
                fold_status(f, previous, now)
            };
            cli.insert(merged.id, merged);
        }
    }
    rebuild_cache(state)
}

/// How long a failed poll may keep showing the numbers from before it.
const STALE_GRACE_SECS: i64 = 30 * 60;

/// Folds a fresh answer into the one already shown for that service.
///
/// One failed request shouldn't blank a row that was fine a minute ago: an
/// `Unavailable` answer keeps the last good numbers, clearly marked stale, with
/// the failure's reason as the note. A `Changed` answer is not a failure — the
/// service did answer, and what it said no longer fits what this app reads —
/// so it passes straight through. Held back as stale `Connected` it would keep
/// the rings and the dock showing numbers the service has already moved past,
/// for up to half an hour after the app knew.
fn fold_status(fresh: ServiceStatus, previous: Option<&ServiceStatus>, now: i64) -> ServiceStatus {
    match previous {
        Some(old)
            if fresh.state == SvcState::Unavailable
                && old.state == SvcState::Connected
                && now - old.fetched_at < STALE_GRACE_SECS =>
        {
            ServiceStatus {
                stale: true,
                note: fresh.note.clone(),
                ..old.clone()
            }
        }
        _ => fresh,
    }
}

/// Pick what a row shows when both a CLI login and an in-app web sign-in
/// can answer. They read the same usage pool, so any live answer will do.
fn pick(cli: Option<&ServiceStatus>, web: Option<&ServiceStatus>) -> Option<ServiceStatus> {
    let live = |s: &&ServiceStatus| s.state == SvcState::Connected && !s.stale;
    if let Some(c) = cli.filter(live) {
        return Some(c.clone());
    }
    if let Some(w) = web.filter(live) {
        return Some(w.clone());
    }
    if let Some(c) = cli.filter(|c| c.state == SvcState::Connected) {
        return Some(c.clone());
    }
    match (cli, web) {
        (Some(c), Some(w)) if c.state == SvcState::NotConnected => Some(w.clone()),
        (None, Some(w)) => Some(w.clone()),
        (Some(c), _) => Some(c.clone()),
        (None, None) => None,
    }
}

fn rebuild_cache(state: &AppState) -> Vec<ServiceStatus> {
    let cli = lock(&state.cli);
    let web = lock(&state.web);
    let merged: Vec<ServiceStatus> = providers::IDS
        .iter()
        .filter_map(|id| pick(cli.get(id), web.get(id)))
        .collect();
    drop((cli, web));
    let mut cache = lock(&state.cache);
    *cache = merged;
    cache.clone()
}

/// A web report just landed: refresh what the tray and popover show.
pub(crate) fn apply_web_results(app: &AppHandle) {
    let state: State<AppState> = app.state();
    let status = rebuild_cache(&state);
    update_tray(app, &status);
    let _ = app.emit("usage", &status);
}

/// Fetch, then push the result to the tray and the popover.
fn refresh_and_broadcast(app: &AppHandle, force: bool) -> Vec<ServiceStatus> {
    let state: State<AppState> = app.state();
    let status = fetch_due(&state, force);
    update_tray(app, &status);
    let _ = app.emit("usage", &status);
    low_quota_alerts(app, &status);
    status
}

/// One native notification when a quota runs low. Only live numbers count:
/// demo data, stale readings and services that aren't connected never notify.
fn low_quota_alerts(app: &AppHandle, status: &[ServiceStatus]) {
    if mock_mode() {
        return;
    }
    let state: State<AppState> = app.state();
    if !state.notify_enabled.load(Ordering::Relaxed) {
        return;
    }
    let readings: Vec<notify::Reading> = status
        .iter()
        .filter(|s| s.state == SvcState::Connected && !s.stale)
        .filter_map(|s| {
            // The window that binds first, which is the row's bold number.
            let w = s
                .windows
                .iter()
                .max_by(|a, b| a.used_percent.total_cmp(&b.used_percent))?;
            Some(notify::Reading {
                id: s.id,
                label: s.label,
                window: w.kind,
                left: (100.0 - w.used_percent).clamp(0.0, 100.0),
                resets_at: w.resets_at,
            })
        })
        .collect();
    let due = {
        let lang = lock(&state.notify_lang).clone();
        lock(&state.alerts).due(providers::now_unix(), &readings, &lang)
    };
    for alert in due {
        let _ = app
            .notification()
            .builder()
            .title(alert.title)
            .body(alert.body)
            .show();
    }
}

// ----------------------------------------------------------------- tray

fn left_percent(s: &ServiceStatus) -> Option<f64> {
    if s.has_numbers() {
        s.peak_used().map(|u| (100.0 - u).max(0.0))
    } else {
        None
    }
}

fn tooltip(status: &[ServiceStatus]) -> String {
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

fn update_tray(app: &AppHandle, status: &[ServiceStatus]) {
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

fn geometry(window: &WebviewWindow) -> Option<Geometry> {
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
fn place(window: &WebviewWindow, state: &AppState, x: f64, y: f64) {
    let (x, y) = (x.round(), y.round());
    state.move_note.note(x, y);
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}

fn emit_edge(app: &AppHandle) {
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
fn settle_window(app: &AppHandle) {
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
fn docked_x(state: &AppState, dock: edge::Dock, g: &Geometry) -> Option<f64> {
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
fn slide_window(app: &AppHandle, tuck: bool) {
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

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Place the popover under the menu-bar icon (macOS) or above the tray icon
/// (Windows taskbar), clamped to the screen it is on. A Linux tray reports no
/// icon rectangle, so there it opens at the top-right instead — the corner
/// every common panel sits in. A docked window is left at its edge instead.
///
/// Wayland ignores everything here: the compositor places the window itself.
fn position_window(app: &AppHandle, window: &WebviewWindow) {
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

fn show_window(app: &AppHandle) {
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

fn toggle_window(app: &AppHandle) {
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
fn apply_shortcut(app: &AppHandle, keys: &str) -> Result<(), String> {
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
fn toggle_window_from_shortcut(app: &AppHandle) {
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

// ------------------------------------------------------------- commands

#[tauri::command]
fn get_status(state: State<AppState>) -> Vec<ServiceStatus> {
    lock(&state.cache).clone()
}

/// The "Low-quota alerts" switch. Turning it back on re-arms the window the
/// user is in, so the next low reading speaks once more.
#[tauri::command]
fn set_notify_enabled(state: State<AppState>, enabled: bool) {
    let was = state.notify_enabled.swap(enabled, Ordering::Relaxed);
    if enabled && !was {
        lock(&state.alerts).clear();
    }
}

/// "zh" or "en" — anything Chinese-looking becomes "zh", the rest "en".
fn normalize_lang(lang: &str) -> &'static str {
    if lang.trim().to_ascii_lowercase().starts_with("zh") {
        "zh"
    } else {
        "en"
    }
}

/// The popover tells the notifier which language it ended up showing (its
/// "Auto" setting resolved against the system locale), so a low-quota
/// notification arrives in the same language as the window it belongs to.
#[tauri::command]
fn set_lang(state: State<AppState>, lang: String) {
    *lock(&state.notify_lang) = normalize_lang(&lang).to_string();
}

#[tauri::command]
async fn refresh_status(app: AppHandle) -> Result<Vec<ServiceStatus>, String> {
    if !mock_mode() {
        web::poll_due(&app, true);
    }
    tauri::async_runtime::spawn_blocking(move || refresh_and_broadcast(&app, true))
        .await
        .map_err(|e| e.to_string())
}

/// The window sizes itself to its content: the rings view is 372 wide, the
/// compact Dock view is exactly as wide as its tiles, and the settings drawer
/// brings its own height. Both numbers come from the page, which is the only
/// part that knows how many rows or tiles it is showing.
#[tauri::command]
fn set_window_size(app: AppHandle, width: f64, height: f64) {
    let Some(window) = main_window(&app) else {
        return;
    };
    // A floor for a one-tile dock and a ceiling for a small screen; the page
    // measures the rest.
    let width = width.clamp(240.0, 900.0);
    let height = height.clamp(120.0, 900.0);
    let state: State<AppState> = app.state();
    // Only resize when the size really changed. The page measures itself
    // again after every resize and reports the result back, so a rounding
    // difference of one pixel would otherwise have the two answering each
    // other for as long as the window is open — and this command runs on the
    // UI thread, where that is exactly what "Not Responding" looks like.
    let (w, h) = (
        (width * 1000.0).round() as i64,
        (height * 1000.0).round() as i64,
    );
    let same_w = state.applied_w.swap(w, Ordering::Relaxed) == w;
    let same_h = state.applied_h.swap(h, Ordering::Relaxed) == h;
    if same_w && same_h {
        return;
    }
    let _ = window.set_size(LogicalSize::new(width, height));
    if window.is_visible().unwrap_or(false) {
        keep_in_place(&app, &window, width, height);
    }
}

/// After the window changes shape, put it back somewhere sensible: a window
/// tucked against an edge follows that edge, and a free one keeps the corner
/// it was at, moved only as far as the new size needs to stay on screen.
/// (The rings view and the dock panel have very different widths, so a window
/// opened near the right edge would otherwise hang off it.)
///
/// The new size is passed in rather than read back from the window: right
/// after `set_size`, `outer_size` can still answer with the old size.
fn keep_in_place(app: &AppHandle, window: &WebviewWindow, width: f64, height: f64) {
    let state: State<AppState> = app.state();
    let scale = window.scale_factor().unwrap_or(1.0);
    let (pw, ph) = (width * scale, height * scale);
    let Some(monitor) = window.current_monitor().ok().flatten() else {
        return;
    };
    let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
    let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);
    let origin = window
        .outer_position()
        .ok()
        .map(|p| (p.x as f64, p.y as f64));
    let (mut x, mut y) = origin.unwrap_or((mx, my));
    let dock = *lock(&state.edge_dock);
    if dock.is_docked() {
        // The edge owns the x (a tucked window belongs mostly off-screen, so
        // it must not be clamped back on); only the height can move it.
        x = if state.edge_hidden.load(Ordering::Relaxed) {
            edge::hidden_x(dock, pw, mx, mw, scale)
        } else {
            edge::visible_x(dock, pw, mx, mw)
        };
        y = y.clamp(my, (my + mh - ph).max(my));
    } else {
        x = x.clamp(mx, (mx + mw - pw).max(mx));
        y = y.clamp(my, (my + mh - ph).max(my));
    }
    let moved = origin
        .map(|(ox, oy)| (ox - x).abs() > 0.5 || (oy - y).abs() > 0.5)
        .unwrap_or(true);
    if moved {
        place(window, &state, x, y);
    }
}

#[tauri::command]
fn set_custom_colors(app: AppHandle, colors: HashMap<String, String>) {
    let state: State<AppState> = app.state();
    let parsed: tray_icon::CustomColors = colors
        .iter()
        .filter_map(|(id, hex)| tray_icon::parse_hex(hex).map(|rgb| (id.clone(), rgb)))
        .collect();
    *lock(&state.custom_colors) = parsed;
    let status = lock(&state.cache).clone();
    update_tray(&app, &status);
}

/// The "Tuck at screen edge" switch. Turning it off brings a tucked window
/// back out before it stops applying.
///
/// Wayland keeps the switch off whatever the UI asks: the compositor decides
/// where a window sits, so sliding it off the edge could never work there.
#[tauri::command]
fn set_edge_enabled(app: AppHandle, enabled: bool) {
    let state: State<AppState> = app.state();
    let enabled = enabled && !on_wayland();
    state.edge_enabled.store(enabled, Ordering::Relaxed);
    if !enabled && state.edge_hidden.swap(false, Ordering::Relaxed) {
        slide_window(&app, false);
    }
    emit_edge(&app);
}

/// What the UI needs to draw (or hide) its grab handle.
#[tauri::command]
fn edge_state(app: AppHandle) -> serde_json::Value {
    let state: State<AppState> = app.state();
    let dock = *lock(&state.edge_dock);
    serde_json::json!({
        "edge": dock.as_str(),
        "hidden": state.edge_hidden.load(Ordering::Relaxed),
        "enabled": state.edge_enabled.load(Ordering::Relaxed),
    })
}

/// The grab handle (or the tray icon) asks a tucked window to slide back in.
#[tauri::command]
fn expand_docked(app: AppHandle) {
    let state: State<AppState> = app.state();
    if !lock(&state.edge_dock).is_docked() {
        return;
    }
    let Some(window) = main_window(&app) else {
        return;
    };
    let _ = window.show();
    if state.edge_hidden.load(Ordering::Relaxed) {
        slide_window(&app, false);
    } else {
        let _ = window.set_focus();
    }
}

/// Esc: put the popover away. A docked window tucks back against its edge
/// instead of vanishing, so the grab handle stays.
#[tauri::command]
fn hide_window(app: AppHandle) {
    let state: State<AppState> = app.state();
    if state.edge_enabled.load(Ordering::Relaxed) && lock(&state.edge_dock).is_docked() {
        slide_window(&app, true);
        return;
    }
    if let Some(w) = main_window(&app) {
        let _ = w.hide();
    }
}

#[tauri::command]
fn set_tray_mode(app: AppHandle, mode: String) {
    let state: State<AppState> = app.state();
    *lock(&state.tray_mode) = if mode == "percent" {
        mode
    } else {
        "icon".into()
    };
    let status = lock(&state.cache).clone();
    update_tray(&app, &status);
}

/// The shortcut currently held (`""` when it is switched off), so Settings can
/// draw the right option after a restart.
#[tauri::command]
fn get_global_shortcut(state: State<AppState>) -> String {
    lock(&state.shortcut).clone().unwrap_or_default()
}

/// Switch the global shortcut. `keys` is an accelerator as the plugin spells
/// it (`CommandOrControl+Shift+U`); `""` turns it off. Returns the shortcut now
/// held, or why it could not be held (another app has it).
#[tauri::command]
fn set_global_shortcut(app: AppHandle, keys: String) -> Result<String, String> {
    apply_shortcut(&app, &keys)?;
    Ok(get_global_shortcut(app.state::<AppState>()))
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let res = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    res.map_err(|e| e.to_string())?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "mock": mock_mode(),
        "platform": std::env::consts::OS,
        // Linux only, and only where it changes what the UI may offer.
        "wayland": on_wayland(),
    })
}

#[tauri::command]
fn open_url(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err("only https links can be opened".into());
    }
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let r = {
        // A GUI process has no console for the child to join, so Windows would
        // make it one: CREATE_NO_WINDOW keeps the browser's launch invisible.
        use std::os::windows::process::CommandExt;
        std::process::Command::new("explorer")
            .arg(&url)
            .creation_flags(0x0800_0000)
            .spawn()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&url).spawn();
    r.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
fn web_logins(app: AppHandle) -> Vec<String> {
    web::signed_in(&app)
}

#[tauri::command]
async fn web_login(app: AppHandle, id: String) -> Result<(), String> {
    web::open_login(&app, &id)
}

#[tauri::command]
async fn web_logout(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || web::sign_out(&app, &id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ------------------------------------------------------------------ main

fn main() {
    // --smoke-test: no window, no tray. Fetch status (mock when
    // AI_USAGE_METER_MOCK=1, otherwise real discovery), print JSON, exit 0.
    // CI runs this on real macOS, Windows and Linux runners.
    if std::env::args().any(|a| a == "--smoke-test") {
        let status = providers::fetch_all(mock_mode());
        match serde_json::to_string_pretty(&status) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("smoke-test: could not serialise status: {e}");
                std::process::exit(1);
            }
        }
        println!("smoke-test ok: {} services", status.len());
        return;
    }

    let state = AppState {
        tray_mode: Mutex::new("icon".into()),
        notify_enabled: AtomicBool::new(true),
        notify_lang: Mutex::new("en".into()),
        ..Default::default()
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(state)
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let show = MenuItemBuilder::with_id("show", "Open AI Usage Meter").build(app)?;
            let refresh = MenuItemBuilder::with_id("refresh", "Refresh now").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&show, &refresh])
                .separator()
                .items(&[&quit])
                .build()?;

            let icon = Image::new_owned(
                tray_icon::render(
                    [None; tray_icon::SLOTS],
                    [None; tray_icon::SLOTS],
                    cfg!(target_os = "macos"),
                    &tray_icon::CustomColors::new(),
                ),
                tray_icon::SIZE,
                tray_icon::SIZE,
            );
            TrayIconBuilder::with_id("main")
                .icon(icon)
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("AI Usage Meter")
                .menu(&menu)
                .show_menu_on_left_click(false)
                // macOS and Windows hand the icon's own clicks back; Linux
                // (libappindicator) does not — there a click opens this menu,
                // which is why every action is in it as well.
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_window(app),
                    "refresh" => {
                        let app = app.clone();
                        std::thread::spawn(move || {
                            // Online accounts are polled here too, exactly like
                            // the in-app refresh: a tray-menu refresh shouldn't
                            // leave a web-signed-in service behind.
                            if !mock_mode() {
                                web::poll_due(&app, true);
                            }
                            refresh_and_broadcast(&app, true);
                        });
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        store_anchor(app, &rect);
                        toggle_window(app);
                    }
                })
                .build(app)?;

            if let Some(window) = main_window(app.handle()) {
                #[cfg(target_os = "macos")]
                {
                    use tauri::window::{Effect, EffectState, EffectsBuilder};
                    let _ = window.set_effects(
                        EffectsBuilder::new()
                            .effect(Effect::Popover)
                            .state(EffectState::Active)
                            .radius(16.0)
                            .build(),
                    );
                    let _ = window.set_visible_on_all_workspaces(true);
                }
                #[cfg(target_os = "windows")]
                {
                    let _ = window.set_shadow(false);
                }
                let handle = app.handle().clone();
                window.on_window_event(move |event| match event {
                    // A user drag bumps the sequence; the watcher below turns
                    // "nothing moved for 180 ms" into a settled drag. Moves we
                    // caused ourselves are recognised and ignored.
                    //
                    // Nothing in here may block. This runs on the UI thread,
                    // so a lock it has to wait for — or takes twice — is a
                    // window that is drawn but never answers again.
                    WindowEvent::Moved(pos) => {
                        let state: State<AppState> = handle.state();
                        let (x, y) = (pos.x as f64, pos.y as f64);
                        // Our own `set_position` announced this one.
                        if state.move_note.claim(x, y) {
                            return;
                        }
                        if state.edge_anim.load(Ordering::Relaxed) {
                            return;
                        }
                        state.move_seq.fetch_add(1, Ordering::Relaxed);
                    }
                    WindowEvent::Focused(false) => {
                        if cfg!(debug_assertions) {
                            return;
                        }
                        let Some(w) = main_window(&handle) else {
                            return;
                        };
                        if !w.is_visible().unwrap_or(false) {
                            return;
                        }
                        // Hiding a window from inside its own focus-loss event
                        // re-enters the message loop of the webview the focus
                        // is leaving, which Windows does not survive; do it a
                        // moment later, once that event is behind us.
                        let app = handle.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(30));
                            let state: State<AppState> = app.state();
                            // A tray click has overtaken us and put the popover
                            // back up: leave it alone.
                            if now_ms() - state.shown_at.load(Ordering::Relaxed) < 300 {
                                return;
                            }
                            let Some(w) = main_window(&app) else {
                                return;
                            };
                            if !w.is_visible().unwrap_or(false) || w.is_focused().unwrap_or(false) {
                                return;
                            }
                            // Tucked against an edge: slide away instead of
                            // disappearing, leaving the grab handle.
                            let tucks = state.edge_enabled.load(Ordering::Relaxed)
                                && lock(&state.edge_dock).is_docked();
                            if tucks {
                                slide_window(&app, true);
                            } else {
                                state.blur_hidden_at.store(now_ms(), Ordering::Relaxed);
                                let _ = w.hide();
                            }
                        });
                    }
                    _ => {}
                });
            }

            *lock(&app.state::<AppState>().web_logins) = web::load_logins(app.handle());

            // The global shortcut is held from the first moment, before the UI
            // has had time to send its saved choice; a combination another app
            // owns is reported in the log and simply leaves it off.
            if let Err(e) = apply_shortcut(app.handle(), DEFAULT_SHORTCUT) {
                eprintln!("global shortcut {DEFAULT_SHORTCUT} not registered: {e}");
            }

            // Edge watcher: one quiet thread for the life of the app. It looks
            // for a drag that has stopped moving and hands it to
            // `settle_window`, which snaps or releases the edge.
            let watcher = app.handle().clone();
            std::thread::spawn(move || {
                let mut last = 0i64;
                let mut changed = std::time::Instant::now();
                loop {
                    std::thread::sleep(Duration::from_millis(80));
                    let state: State<AppState> = watcher.state();
                    let seq = state.move_seq.load(Ordering::Relaxed);
                    if seq == state.move_settled.load(Ordering::Relaxed) {
                        last = seq;
                        continue;
                    }
                    if seq != last {
                        last = seq;
                        changed = std::time::Instant::now();
                        continue;
                    }
                    if changed.elapsed() >= Duration::from_millis(180) {
                        settle_window(&watcher);
                        state.move_settled.store(seq, Ordering::Relaxed);
                    }
                }
            });

            // Background poller: first fetch right away, then every TICK.
            // Per-provider intervals in `fetch_due` keep the real request
            // rate low.
            let handle = app.handle().clone();
            std::thread::spawn(move || loop {
                if !mock_mode() {
                    web::poll_due(&handle, false);
                }
                refresh_and_broadcast(&handle, false);
                std::thread::sleep(TICK);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            refresh_status,
            set_notify_enabled,
            set_lang,
            set_window_size,
            hide_window,
            set_tray_mode,
            get_global_shortcut,
            set_global_shortcut,
            set_custom_colors,
            set_edge_enabled,
            edge_state,
            expand_docked,
            get_autostart,
            set_autostart,
            app_info,
            open_url,
            web_logins,
            web_login,
            web_logout,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Usage Meter");
}

fn store_anchor(app: &AppHandle, rect: &Rect) {
    let scale = main_window(app)
        .and_then(|w| w.scale_factor().ok())
        .unwrap_or(1.0);
    let pos = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    *lock(&app.state::<AppState>().anchor) = Some((pos.x, pos.y, size.width, size.height));
}

#[cfg(test)]
mod tests {
    //! `cargo test` builds this binary's own tests on the CI runners, so the
    //! pieces below — the note the `Moved` handler reads on the UI thread, the
    //! folding of a failed poll into the last good numbers, and which session
    //! counts as Wayland — are exercised on Windows and macOS and not only on a
    //! developer's Linux machine.
    use super::*;
    use ai_usage_core::edge::MoveNote;

    /// A status with just the fields these tests look at.
    fn status(state: SvcState, note: Option<&str>, fetched_at: i64) -> ServiceStatus {
        ServiceStatus {
            id: "claude",
            label: "Claude",
            state,
            plan: None,
            windows: Vec::new(),
            note: note.map(str::to_string),
            fetched_at,
            stale: false,
            source: "api",
        }
    }

    #[test]
    fn a_failed_poll_keeps_the_last_good_numbers_for_half_an_hour() {
        let good = status(SvcState::Connected, None, 1_000);
        let failed = status(SvcState::Unavailable, Some("claude.ai: timed out"), 1_300);
        let merged = fold_status(failed, Some(&good), 1_300);
        assert_eq!(merged.state, SvcState::Connected, "a hiccup is not news");
        assert!(merged.stale, "and it is marked stale");
        assert_eq!(merged.fetched_at, 1_000, "the readings are the old ones");
        assert_eq!(
            merged.note.as_deref(),
            Some("claude.ai: timed out"),
            "the row still says why it could not be checked"
        );

        // The grace window is 30 minutes: at its edge the failure is the news.
        let at_edge = 1_000 + STALE_GRACE_SECS;
        let failed = status(SvcState::Unavailable, None, at_edge);
        let merged = fold_status(failed, Some(&good), at_edge);
        assert_eq!(merged.state, SvcState::Unavailable);
        assert!(!merged.stale, "past the window nothing is held back");
        assert_eq!(merged.fetched_at, at_edge);
    }

    #[test]
    fn a_changed_api_shows_at_once_instead_of_a_stale_connected_row() {
        let good = status(SvcState::Connected, None, 1_000);
        let changed = status(SvcState::Changed, Some("API has probably changed."), 1_100);
        let merged = fold_status(changed, Some(&good), 1_100);
        assert_eq!(
            merged.state,
            SvcState::Changed,
            "the service answered that it moved: show that now"
        );
        assert!(!merged.stale, "a changed API is not a stale reading");
        assert_eq!(merged.fetched_at, 1_100);
        assert_eq!(merged.note.as_deref(), Some("API has probably changed."));

        // With nothing to fold into, and with a previous failure, it is the answer.
        assert_eq!(
            fold_status(status(SvcState::Changed, None, 5), None, 5).state,
            SvcState::Changed
        );
        let expired = status(SvcState::Expired, None, 100);
        assert_eq!(
            fold_status(status(SvcState::Changed, None, 200), Some(&expired), 200).state,
            SvcState::Changed
        );
    }

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
