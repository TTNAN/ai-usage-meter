//! Every `#[tauri::command]` the page can call, and the window-size logic that
//! belongs with them. Each one is a thin translation between the page's JSON
//! and the state and helpers the rest of the app owns.

use crate::lock::lock;
use crate::poller::{mock_mode, refresh_and_broadcast};
use crate::web;
use crate::window::{
    apply_shortcut, emit_edge, main_window, on_wayland, place, slide_window, update_tray,
};
use crate::AppState;
use ai_usage_core::providers::ServiceStatus;
use ai_usage_core::{edge, tray_icon};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, LogicalSize, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;

// ------------------------------------------------------------- commands

#[tauri::command]
pub(crate) fn get_status(state: State<AppState>) -> Vec<ServiceStatus> {
    lock(&state.cache).clone()
}

/// The "Low-quota alerts" switch. Turning it back on re-arms the window the
/// user is in, so the next low reading speaks once more.
#[tauri::command]
pub(crate) fn set_notify_enabled(state: State<AppState>, enabled: bool) {
    let was = state.notify_enabled.swap(enabled, Ordering::Relaxed);
    if enabled && !was {
        lock(&state.alerts).clear();
    }
}

/// "zh" or "en" — anything Chinese-looking becomes "zh", the rest "en".
pub(crate) fn normalize_lang(lang: &str) -> &'static str {
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
pub(crate) fn set_lang(state: State<AppState>, lang: String) {
    *lock(&state.notify_lang) = normalize_lang(&lang).to_string();
}

#[tauri::command]
pub(crate) async fn refresh_status(app: AppHandle) -> Result<Vec<ServiceStatus>, String> {
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
pub(crate) fn set_window_size(app: AppHandle, width: f64, height: f64) {
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
pub(crate) fn keep_in_place(app: &AppHandle, window: &WebviewWindow, width: f64, height: f64) {
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
pub(crate) fn set_custom_colors(app: AppHandle, colors: HashMap<String, String>) {
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
pub(crate) fn set_edge_enabled(app: AppHandle, enabled: bool) {
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
pub(crate) fn edge_state(app: AppHandle) -> serde_json::Value {
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
pub(crate) fn expand_docked(app: AppHandle) {
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
pub(crate) fn hide_window(app: AppHandle) {
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
pub(crate) fn set_tray_mode(app: AppHandle, mode: String) {
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
pub(crate) fn get_global_shortcut(state: State<AppState>) -> String {
    lock(&state.shortcut).clone().unwrap_or_default()
}

/// Switch the global shortcut. `keys` is an accelerator as the plugin spells
/// it (`CommandOrControl+Shift+U`); `""` turns it off. Returns the shortcut now
/// held, or why it could not be held (another app has it).
#[tauri::command]
pub(crate) fn set_global_shortcut(app: AppHandle, keys: String) -> Result<String, String> {
    apply_shortcut(&app, &keys)?;
    Ok(get_global_shortcut(app.state::<AppState>()))
}

#[tauri::command]
pub(crate) fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
pub(crate) fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
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
pub(crate) fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "mock": mock_mode(),
        "platform": std::env::consts::OS,
        // Linux only, and only where it changes what the UI may offer.
        "wayland": on_wayland(),
    })
}

#[tauri::command]
pub(crate) fn open_url(url: String) -> Result<(), String> {
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
pub(crate) fn web_logins(app: AppHandle) -> Vec<String> {
    web::signed_in(&app)
}

#[tauri::command]
pub(crate) async fn web_login(app: AppHandle, id: String) -> Result<(), String> {
    web::open_login(&app, &id)
}

#[tauri::command]
pub(crate) async fn web_logout(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || web::sign_out(&app, &id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub(crate) fn quit(app: AppHandle) {
    app.exit(0);
}
