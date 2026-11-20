//! AI Usage Meter — a menu-bar (macOS) / system-tray (Windows) popover that
//! shows how much Claude, ChatGPT (Codex) and Grok quota you have left.
//!
//! * `providers` talks to each service (read-only, reusing CLI logins).
//! * `tray_icon` draws the live three-bar icon.
//! * This file wires the tray, the popover window and a background poller.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod providers;
mod tray_icon;
mod web;

use providers::{ServiceStatus, State as SvcState};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicI64, Ordering};
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

const WINDOW_WIDTH: f64 = 372.0;
const TICK: Duration = Duration::from_secs(30);

/// Minimum seconds between automatic fetches, per provider.
/// Claude's usage endpoint rate-limits aggressively, so it gets the most room.
fn poll_interval_secs(id: &str) -> i64 {
    match id {
        "codex" => 60,
        _ => 300,
    }
}
/// Floor for manual refreshes, so hammering ⌘R can't get anyone limited.
const MANUAL_FLOOR_SECS: i64 = 20;

fn mock_mode() -> bool {
    std::env::var("AI_USAGE_METER_MOCK").map(|v| v == "1").unwrap_or(false)
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
    last_fetch: Mutex<HashMap<&'static str, i64>>,
    /// Tray icon rectangle (physical px) from the last click, for anchoring.
    anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    /// "icon" (bars only) or "percent" (bars + tightest % in the menu bar).
    tray_mode: Mutex<String>,
    /// When the popover last hid because it lost focus.
    blur_hidden_at: AtomicI64,
}

// ------------------------------------------------------------- fetching

fn fetch_due(state: &AppState, force: bool) -> Vec<ServiceStatus> {
    if mock_mode() {
        let mut cache = state.cache.lock().unwrap();
        if cache.is_empty() {
            *cache = providers::mock_status();
        }
        return cache.clone();
    }

    let now = providers::now_unix();
    let due: Vec<&'static str> = {
        let cli = state.cli.lock().unwrap();
        let last = state.last_fetch.lock().unwrap();
        providers::IDS
            .iter()
            .copied()
            .filter(|id| {
                let since = now - last.get(id).copied().unwrap_or(0);
                let cached = cli.contains_key(id);
                !cached || since >= if force { MANUAL_FLOOR_SECS } else { poll_interval_secs(id) }
            })
            .collect()
    };
    if due.is_empty() {
        return state.cache.lock().unwrap().clone();
    }

    // Network calls happen outside the locks, in parallel.
    let fresh: Vec<ServiceStatus> = std::thread::scope(|s| {
        let handles: Vec<_> = due
            .iter()
            .map(|id| s.spawn(move || providers::fetch_one(id)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok().flatten()).collect()
    });

    {
    let mut cli = state.cli.lock().unwrap();
    let mut last = state.last_fetch.lock().unwrap();
    for f in fresh {
        last.insert(f.id, now);
        let merged = match cli.get(f.id) {
            // One failed request shouldn't blank a row that was fine a
            // minute ago: keep the last good numbers, clearly marked stale.
            Some(old)
                if f.state == SvcState::Unavailable
                    && old.state == SvcState::Connected
                    && now - old.fetched_at < 30 * 60 =>
            {
                ServiceStatus { stale: true, note: f.note.clone(), ..old.clone() }
            }
            _ => f,
        };
        cli.insert(merged.id, merged);
    }
    }
    rebuild_cache(state)
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
    let cli = state.cli.lock().unwrap();
    let web = state.web.lock().unwrap();
    let merged: Vec<ServiceStatus> = providers::IDS
        .iter()
        .filter_map(|id| pick(cli.get(id), web.get(id)))
        .collect();
    drop((cli, web));
    let mut cache = state.cache.lock().unwrap();
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
    status
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
            _ => format!("{}  unavailable", s.label),
        });
    }
    lines.join("\n")
}

fn update_tray(app: &AppHandle, status: &[ServiceStatus]) {
    let Some(tray) = app.tray_by_id("main") else { return };
    let mut left = [None; 3];
    for s in status {
        if let Some(i) = providers::IDS.iter().position(|id| *id == s.id) {
            left[i] = left_percent(s);
        }
    }
    let template = cfg!(target_os = "macos");
    let rgba = tray_icon::render(left, template);
    let _ = tray.set_icon(Some(Image::new_owned(rgba, tray_icon::SIZE, tray_icon::SIZE)));
    #[cfg(target_os = "macos")]
    let _ = tray.set_icon_as_template(true);
    let _ = tray.set_tooltip(Some(tooltip(status)));

    #[cfg(target_os = "macos")]
    {
        let state: State<AppState> = app.state();
        let mode = state.tray_mode.lock().unwrap().clone();
        let tightest = left.iter().flatten().cloned().fold(None, |a: Option<f64>, v| {
            Some(a.map_or(v, |a| a.min(v)))
        });
        let title = match (mode.as_str(), tightest) {
            ("percent", Some(v)) => Some(format!("{v:.0}%")),
            _ => None,
        };
        let _ = tray.set_title(title);
    }
}

// --------------------------------------------------------------- window

fn main_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("main")
}

/// Place the popover under the menu-bar icon (macOS) or above the tray
/// icon (Windows taskbar), clamped to the screen it is on.
fn position_window(app: &AppHandle, window: &WebviewWindow) {
    let Ok(size) = window.outer_size() else { return };
    let (w, h) = (size.width as f64, size.height as f64);
    let scale = window.scale_factor().unwrap_or(1.0);
    let margin = 8.0 * scale;
    let anchor = *app.state::<AppState>().anchor.lock().unwrap();

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

    let (x, y) = match anchor {
        Some((ax, ay, aw, ah)) => {
            let x = ax + aw / 2.0 - w / 2.0;
            let y = if ay < my + mh / 2.0 { ay + ah + margin * 0.5 } else { ay - h - margin * 0.5 };
            (x, y)
        }
        // Opened from the menu, before any click: top-right on macOS,
        // bottom-right everywhere else.
        None if cfg!(target_os = "macos") => (mx + mw - w - margin, my + 32.0 * scale),
        None => (mx + mw - w - margin, my + mh - h - 56.0 * scale),
    };
    let x = x.clamp(mx + margin, (mx + mw - w - margin).max(mx + margin));
    let y = y.clamp(my, (my + mh - h).max(my));
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

fn show_window(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    position_window(app, &window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit("popover-shown", ());
}

fn toggle_window(app: &AppHandle) {
    let Some(window) = main_window(app) else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    // Clicking the tray icon while the popover is open first blurs (and
    // hides) it; don't immediately re-open it on the same click.
    let hidden_at = app.state::<AppState>().blur_hidden_at.load(Ordering::Relaxed);
    if now_ms() - hidden_at < 350 {
        return;
    }
    show_window(app);
}

// ------------------------------------------------------------- commands

#[tauri::command]
fn get_status(state: State<AppState>) -> Vec<ServiceStatus> {
    state.cache.lock().unwrap().clone()
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

/// The popover sizes itself to its content.
#[tauri::command]
fn set_height(app: AppHandle, height: f64) {
    let Some(window) = main_window(&app) else { return };
    let height = height.clamp(160.0, 900.0);
    let _ = window.set_size(LogicalSize::new(WINDOW_WIDTH, height));
    if window.is_visible().unwrap_or(false) {
        position_window(&app, &window);
    }
}

#[tauri::command]
fn hide_window(app: AppHandle) {
    if let Some(w) = main_window(&app) {
        let _ = w.hide();
    }
}

#[tauri::command]
fn set_tray_mode(app: AppHandle, mode: String) {
    let state: State<AppState> = app.state();
    *state.tray_mode.lock().unwrap() = if mode == "percent" { mode } else { "icon".into() };
    let status = state.cache.lock().unwrap().clone();
    update_tray(&app, &status);
}

#[tauri::command]
fn get_autostart(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    let launcher = app.autolaunch();
    let res = if enabled { launcher.enable() } else { launcher.disable() };
    res.map_err(|e| e.to_string())?;
    Ok(launcher.is_enabled().unwrap_or(enabled))
}

#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "mock": mock_mode(),
        "platform": std::env::consts::OS,
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
    let r = std::process::Command::new("explorer").arg(&url).spawn();
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
    // CI runs this on real macOS / Windows runners.
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

    let state = AppState { tray_mode: Mutex::new("icon".into()), ..Default::default() };

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
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
                tray_icon::render([None; 3], cfg!(target_os = "macos")),
                tray_icon::SIZE,
                tray_icon::SIZE,
            );
            TrayIconBuilder::with_id("main")
                .icon(icon)
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("AI Usage Meter")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_window(app),
                    "refresh" => {
                        let app = app.clone();
                        std::thread::spawn(move || {
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
                window.on_window_event(move |event| {
                    if let WindowEvent::Focused(false) = event {
                        if let Some(w) = main_window(&handle) {
                            if w.is_visible().unwrap_or(false) && !cfg!(debug_assertions) {
                                let _ = w.hide();
                                handle
                                    .state::<AppState>()
                                    .blur_hidden_at
                                    .store(now_ms(), Ordering::Relaxed);
                            }
                        }
                    }
                });
            }

            *app.state::<AppState>().web_logins.lock().unwrap() = web::load_logins(app.handle());

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
            set_height,
            hide_window,
            set_tray_mode,
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
    let scale = main_window(app).and_then(|w| w.scale_factor().ok()).unwrap_or(1.0);
    let pos = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    *app.state::<AppState>().anchor.lock().unwrap() = Some((pos.x, pos.y, size.width, size.height));
}
