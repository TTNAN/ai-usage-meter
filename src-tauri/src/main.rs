//! AI Usage Meter — menu-bar (macOS) / system-tray (Windows) app.
//!
//! Click the tray icon to open the Dock-style window with one card per
//! service (Claude, Codex, Grok). Usage data comes from `providers`;
//! see that module for endpoints and the reuse-CLI-login auth model.

mod providers;

use providers::ServiceStatus;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    AppHandle, Manager, State,
};

/// Minimum seconds between live fetches, per provider.
/// (Research: Claude >= 5 min, Codex 60 s, Grok 5-10 min.)
fn poll_interval_secs(id: &str) -> i64 {
    match id {
        "claude" => 300,
        "codex" => 60,
        "grok" => 300,
        _ => 300,
    }
}

fn mock_mode() -> bool {
    std::env::var("AI_USAGE_METER_MOCK").map(|v| v == "1").unwrap_or(false)
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

struct AppState {
    cache: Mutex<Vec<ServiceStatus>>,
    last_fetch: Mutex<HashMap<&'static str, i64>>,
}

fn fetch_due(state: &AppState, force: bool) -> Vec<ServiceStatus> {
    let now = now_unix();
    let mut cache = state.cache.lock().unwrap();
    let mut last = state.last_fetch.lock().unwrap();
    if cache.is_empty() || force {
        let fresh = providers::fetch_all(mock_mode());
        for s in &fresh {
            last.insert(s.id, now);
        }
        *cache = fresh;
        return cache.clone();
    }
    // Refresh only the providers whose poll interval has elapsed.
    let mut merged: Vec<ServiceStatus> = Vec::new();
    for cached in cache.iter() {
        let due = now - last.get(cached.id).copied().unwrap_or(0) >= poll_interval_secs(cached.id);
        if due && !mock_mode() {
            let fresh = match cached.id {
                "claude" => providers::fetch_claude(),
                "codex" => providers::fetch_codex(),
                "grok" => providers::fetch_grok(),
                _ => cached.clone(),
            };
            last.insert(cached.id, now);
            merged.push(fresh);
        } else {
            merged.push(cached.clone());
        }
    }
    *cache = merged.clone();
    merged
}

fn tray_summary(status: &[ServiceStatus]) -> String {
    let parts: Vec<String> = status
        .iter()
        .map(|s| match (s.state, s.used_percent) {
            ("connected", Some(p)) => format!("{} {:.0}% used", s.label, p),
            ("mock", Some(p)) => format!("{} {:.0}% (demo)", s.label, p),
            ("not_connected", _) => format!("{} not connected", s.label),
            _ => format!("{} unavailable", s.label),
        })
        .collect();
    parts.join(" · ")
}

#[tauri::command]
fn get_status(state: State<AppState>) -> Vec<ServiceStatus> {
    fetch_due(&state, false)
}

#[tauri::command]
fn refresh_status(state: State<AppState>, app: AppHandle) -> Vec<ServiceStatus> {
    let status = fetch_due(&state, true);
    update_tray(&app, &status);
    status
}

#[tauri::command]
fn open_url(url: String) {
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(&url).spawn();
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", &url])
        .spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
}

fn update_tray(app: &AppHandle, status: &[ServiceStatus]) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tray_summary(status)));
    }
}

fn toggle_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn main() {
    // --smoke-test: no window, no tray. Fetch status (mock if
    // AI_USAGE_METER_MOCK=1, otherwise real discovery), print JSON, exit 0.
    // Used by CI on real macOS / Windows runners to prove the binary runs.
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
        cache: Mutex::new(Vec::new()),
        last_fetch: Mutex::new(HashMap::new()),
    };

    tauri::Builder::default()
        .manage(state)
        .setup(|app| {
            let show = MenuItemBuilder::with_id("show", "Show AI Usage Meter").build(app)?;
            let refresh = MenuItemBuilder::with_id("refresh", "Refresh now").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
            let menu = MenuBuilder::new(app)
                .items(&[&show, &refresh])
                .separator()
                .items(&[&quit])
                .build()?;
            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AI Usage Meter")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => toggle_window(app),
                    "refresh" => {
                        let state: State<AppState> = app.state();
                        let status = fetch_due(&state, true);
                        update_tray(app, &status);
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        ..
                    } = event
                    {
                        toggle_window(tray.app_handle());
                    }
                })
                .build(app)?;
            // Initial fetch so the tooltip is useful from the start.
            let state: State<AppState> = app.state();
            let status = fetch_due(&state, false);
            update_tray(app, &status);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_status, refresh_status, open_url])
        .run(tauri::generate_context!())
        .expect("error while running AI Usage Meter");
}
