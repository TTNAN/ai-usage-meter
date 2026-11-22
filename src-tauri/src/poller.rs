//! The background poller: which service is due, how a failed poll folds into
//! the numbers already on screen, and the low-quota notifications a fetch can
//! raise. Nothing here touches a window except to redraw the tray icon.

use crate::lock::lock;
use crate::window::update_tray;
use crate::AppState;
use ai_usage_core::providers::{ServiceStatus, State as SvcState};
use ai_usage_core::{notify, providers};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_notification::NotificationExt;

/// Minimum seconds between automatic fetches, per provider.
///
/// Codex costs nothing to poll: its numbers are read from the session log the
/// Codex CLI already wrote on this machine, not from the network. Every other
/// provider is one request to a service whose endpoints were never meant for
/// a poller, and Claude's usage endpoint rate-limits hardest — so they all
/// wait five minutes (already five times the Codex interval), which keeps a
/// row at most a few minutes behind without hammering anyone.
pub(crate) fn poll_interval_secs(id: &str) -> i64 {
    match id {
        "codex" => 60,
        _ => 300,
    }
}
/// Floor for manual refreshes, so hammering ⌘R can't get anyone limited.
const MANUAL_FLOOR_SECS: i64 = 20;

pub(crate) fn mock_mode() -> bool {
    std::env::var("AI_USAGE_METER_MOCK")
        .map(|v| v == "1")
        .unwrap_or(false)
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ------------------------------------------------------------- fetching

pub(crate) fn fetch_due(state: &AppState, force: bool) -> Vec<ServiceStatus> {
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
pub(crate) fn fold_status(
    fresh: ServiceStatus,
    previous: Option<&ServiceStatus>,
    now: i64,
) -> ServiceStatus {
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
pub(crate) fn pick(
    cli: Option<&ServiceStatus>,
    web: Option<&ServiceStatus>,
) -> Option<ServiceStatus> {
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

pub(crate) fn rebuild_cache(state: &AppState) -> Vec<ServiceStatus> {
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
pub(crate) fn refresh_and_broadcast(app: &AppHandle, force: bool) -> Vec<ServiceStatus> {
    let state: State<AppState> = app.state();
    let status = fetch_due(&state, force);
    update_tray(app, &status);
    let _ = app.emit("usage", &status);
    low_quota_alerts(app, &status);
    status
}

/// One native notification when a quota runs low. Only live numbers count:
/// demo data, stale readings and services that aren't connected never notify.
pub(crate) fn low_quota_alerts(app: &AppHandle, status: &[ServiceStatus]) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
