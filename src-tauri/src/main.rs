//! AI Usage Meter — a menu-bar (macOS) / system-tray (Windows, Linux) app that
//! shows how much Claude, ChatGPT (Codex), Grok, Gemini and GitHub Copilot
//! quota you have left. One window, two shapes: the rings popover (372 px, every
//! window and countdown in a row) and the dock panel (five brand squircles on one
//! tray, as wide as its tiles), switched in the UI and sized from there.
//!
//! * `ai-usage-core` talks to each service (read-only, reusing CLI logins),
//!   draws the live tray icon and decides the low-quota notifications.
//! * `poller` decides what to fetch next and folds a failed poll into the last
//!   good numbers; `window` moves, shows and hides the window, draws the tray
//!   icon and holds the global shortcut; `commands` is the page's API.
//! * This file owns the shared state, registers the plugins and wires them
//!   all together.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod lock;
mod poller;
mod updater;
mod version;
mod web;
mod window;

use ai_usage_core::providers::ServiceStatus;
use ai_usage_core::{edge, notify, providers, tray_icon};
use commands::*;
use lock::lock;
use poller::{mock_mode, now_ms, refresh_and_broadcast};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_notification::NotificationExt;
use updater::check_for_update;
use window::{
    apply_shortcut, main_window, settle_window, show_window, slide_window, store_anchor,
    toggle_window, DEFAULT_SHORTCUT,
};

const TICK: Duration = Duration::from_secs(30);

#[derive(Default)]
pub(crate) struct AppState {
    /// What the popover shows: the CLI result merged with the web result.
    pub(crate) cache: Mutex<Vec<ServiceStatus>>,
    /// Latest result per service from the CLI logins.
    pub(crate) cli: Mutex<HashMap<&'static str, ServiceStatus>>,
    /// Latest result per service from the in-app web sign-in.
    pub(crate) web: Mutex<HashMap<&'static str, ServiceStatus>>,
    pub(crate) web_logins: Mutex<HashSet<String>>,
    pub(crate) web_last_poll: Mutex<HashMap<&'static str, i64>>,
    pub(crate) web_last_report: Mutex<HashMap<&'static str, i64>>,
    pub(crate) web_use_home: Mutex<HashSet<&'static str>>,
    /// Which generation of hidden worker each site is on, so a deferred close
    /// after a report can't take down a window a newer poll just took over.
    pub(crate) web_worker_epoch: Mutex<HashMap<&'static str, u64>>,
    pub(crate) last_fetch: Mutex<HashMap<&'static str, i64>>,
    /// Tray icon rectangle (physical px) from the last click, for anchoring.
    pub(crate) anchor: Mutex<Option<(f64, f64, f64, f64)>>,
    /// "icon" (bars only) or "percent" (bars + tightest % in the menu bar).
    pub(crate) tray_mode: Mutex<String>,
    /// The "Low-quota alerts" switch in Settings (on unless turned off).
    pub(crate) notify_enabled: AtomicBool,
    /// The interface language the popover resolved ("zh" or "en"), so the
    /// low-quota notifications are written in it too. "auto" is settled in
    /// the UI, which sends the answer down with `set_lang`.
    pub(crate) notify_lang: Mutex<String>,
    /// What has already been announced, so one reset window is announced once.
    pub(crate) alerts: Mutex<notify::Alerts>,
    /// When the popover lost focus. Set on the way out, so a tray click that
    /// arrives just after it can tell that the popover is on its way down.
    pub(crate) blur_hidden_at: AtomicI64,
    /// When the popover last became visible, so the hide a blur schedules does
    /// nothing if a tray click has reopened the window in the meantime.
    pub(crate) shown_at: AtomicI64,
    /// The size last handed to `set_size`, in thousandths of a logical px:
    /// the page measures itself again after every resize and reports the new
    /// size back, and without this the two keep answering each other. The
    /// width is part of it now that Dock view is as wide as its own tiles.
    pub(crate) applied_w: AtomicI64,
    pub(crate) applied_h: AtomicI64,
    /// Accent colours picked in Settings, for the Windows / Linux tray bars.
    /// (macOS tints its own template image instead.)
    pub(crate) custom_colors: Mutex<tray_icon::CustomColors>,
    /// Which screen edge the window is tucked against (`None` = a plain tray
    /// popover that just hides on blur).
    pub(crate) edge_dock: Mutex<edge::Dock>,
    /// Whether it is currently slid mostly off-screen.
    pub(crate) edge_hidden: AtomicBool,
    /// The "Tuck at screen edge" switch (on unless turned off).
    pub(crate) edge_enabled: AtomicBool,
    /// One slide at a time; window moves during a slide are ours.
    pub(crate) edge_anim: AtomicBool,
    /// Bumped whenever the user moves the window, so the watcher below can tell
    /// a finished drag from one still in progress.
    pub(crate) move_seq: AtomicI64,
    /// The `move_seq` value already settled, so each drag is judged once.
    pub(crate) move_settled: AtomicI64,
    /// Where our own `set_position` is putting the window, so the `Moved`
    /// event it causes is not mistaken for a drag. Deliberately lock-free: the
    /// `Moved` handler runs on the UI thread, and a `Mutex` on that path is one
    /// re-entrant lock away from a window that never answers again.
    pub(crate) move_note: edge::MoveNote,
    /// The global shortcut currently held, or `None` when it is switched off.
    pub(crate) shortcut: Mutex<Option<String>>,
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
            check_for_update,
            open_url,
            web_logins,
            web_login,
            web_logout,
            quit
        ])
        .run(tauri::generate_context!())
        .expect("error while running AI Usage Meter");
}
