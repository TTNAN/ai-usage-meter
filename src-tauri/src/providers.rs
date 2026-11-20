//! Usage providers: Claude, ChatGPT (Codex quota) and Grok.
//!
//! None of the three services publishes an official "how much of my
//! subscription is left" API. We read the same internal endpoints the
//! official CLIs use, reusing the login those CLIs already stored on this
//! machine. Read-only GET requests, at low frequency. When anything fails
//! we say so — we never invent numbers.
//!
//! | Service | Endpoint | Credential |
//! | ------- | -------- | ---------- |
//! | Claude  | `GET https://api.anthropic.com/api/oauth/usage` | Claude Code OAuth token: macOS Keychain item `Claude Code-credentials`, else `~/.claude/.credentials.json` |
//! | ChatGPT (Codex) | `GET https://chatgpt.com/backend-api/wham/usage`, falling back to the newest `token_count` event in `~/.codex/sessions` | `~/.codex/auth.json` (`$CODEX_HOME` respected; shared by Codex CLI and the ChatGPT desktop app) |
//! | Grok    | `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits` | Grok Build CLI: `~/.grok/auth.json` (`$GROK_HOME` respected) |
//!
//! Tokens are read at call time, held in memory only for the request, and
//! never written or sent anywhere except to the service they belong to.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// ------------------------------------------------------------------ model

/// One rate-limit window (e.g. "5-hour session", "weekly").
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    /// Stable key the UI localises: "session" | "weekly" | "weekly_opus" |
    /// "weekly_sonnet" | "period".
    pub kind: &'static str,
    /// Percent of the window already USED, 0–100.
    pub used_percent: f64,
    /// Unix seconds when this window resets, if known.
    pub resets_at: Option<i64>,
    /// Length of the window in seconds, if known.
    pub window_seconds: Option<i64>,
}

/// Connection state of one service.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Live numbers from the service.
    Connected,
    /// No CLI login found on this machine.
    NotConnected,
    /// Login found but the service rejected it (401/403).
    Expired,
    /// Request failed or the response was not understood.
    Unavailable,
    /// Demo data (AI_USAGE_METER_MOCK=1 or browser preview).
    Mock,
}

/// One service, as shown on one row.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub state: State,
    /// Subscription tier when the service reports it ("Max", "Pro", "Plus"…).
    pub plan: Option<String>,
    /// Windows, most urgent first (the first one is the headline number).
    pub windows: Vec<UsageWindow>,
    /// Short human-readable detail (why unavailable, what to run…).
    pub note: Option<String>,
    /// Unix seconds when this status was fetched.
    pub fetched_at: i64,
    /// True when the latest request failed and these are the last good
    /// numbers we had (kept so a single hiccup doesn't blank the row).
    pub stale: bool,
    /// Where the numbers came from: "api" (asked the service just now) or
    /// "session" (read from the CLI / desktop app's own latest session log;
    /// `fetched_at` is then the time of that session event).
    pub source: &'static str,
}

impl ServiceStatus {
    fn new(id: &'static str, label: &'static str, state: State) -> Self {
        ServiceStatus {
            id,
            label,
            state,
            plan: None,
            windows: Vec::new(),
            note: None,
            fetched_at: now_unix(),
            source: "api",
            stale: false,
        }
    }
    fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
    /// Highest used-percent across all windows — the binding constraint.
    pub fn peak_used(&self) -> Option<f64> {
        self.windows
            .iter()
            .map(|w| w.used_percent)
            .fold(None, |acc, p| Some(acc.map_or(p, |a: f64| a.max(p))))
    }
    pub fn has_numbers(&self) -> bool {
        matches!(self.state, State::Connected | State::Mock) && !self.windows.is_empty()
    }
}

// ---------------------------------------------------------------- helpers

pub fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.trim().is_empty())
}

fn read_json_file(path: &PathBuf) -> Option<serde_json::Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Depth-first search for the first non-empty string under any of `keys`.
/// Credential files change shape between CLI versions; this keeps us
/// working across them.
fn find_string<'a>(v: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if keys.contains(&k.as_str()) {
                    if let Some(s) = val.as_str().filter(|s| !s.is_empty()) {
                        return Some(s);
                    }
                }
            }
            map.values().find_map(|val| find_string(val, keys))
        }
        serde_json::Value::Array(items) => items.iter().find_map(|i| find_string(i, keys)),
        _ => None,
    }
}

fn as_f64(v: Option<&serde_json::Value>) -> Option<f64> {
    let v = v?;
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// Accept unix seconds, unix milliseconds, or an RFC 3339 string.
fn as_timestamp(v: Option<&serde_json::Value>) -> Option<i64> {
    let v = v?;
    if let Some(n) = v.as_f64() {
        let n = n as i64;
        return Some(if n > 100_000_000_000 { n / 1000 } else { n });
    }
    v.as_str().and_then(parse_rfc3339)
}

enum HttpError {
    Auth(u16),
    Status(u16),
    Other(String),
}

impl HttpError {
    fn describe(&self) -> String {
        match self {
            HttpError::Auth(c) | HttpError::Status(c) => format!("HTTP {c}"),
            HttpError::Other(e) => e.clone(),
        }
    }
}

fn http_get_json(url: &str, headers: &[(&str, String)]) -> Result<serde_json::Value, HttpError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("ai-usage-meter/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| HttpError::Other(format!("http client: {e}")))?;
    let mut req = client.get(url).header("Accept", "application/json");
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    let resp = req.send().map_err(|e| {
        HttpError::Other(if e.is_timeout() {
            "timed out".into()
        } else if e.is_connect() {
            "offline or blocked".into()
        } else {
            "network error".into()
        })
    })?;
    let code = resp.status().as_u16();
    if code == 401 || code == 403 {
        return Err(HttpError::Auth(code));
    }
    if !resp.status().is_success() {
        return Err(HttpError::Status(code));
    }
    resp.json::<serde_json::Value>()
        .map_err(|_| HttpError::Other("unexpected response".into()))
}

/// RFC 3339 → unix seconds, honouring the UTC offset
/// (`2026-02-08T04:59:59Z`, `2026-02-08T12:59:59.123+08:00`).
pub fn parse_rfc3339(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once(['T', 't', ' '])?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Split the clock from the offset.
    let (clock, offset_secs) = if let Some(c) = rest.strip_suffix(['Z', 'z']) {
        (c, 0)
    } else if let Some(idx) = rest.rfind(['+', '-']) {
        let (c, off) = rest.split_at(idx);
        let sign = if off.starts_with('-') { -1 } else { 1 };
        let off = &off[1..];
        let (oh, om) = off.split_once(':').unwrap_or((off.get(..2)?, off.get(2..).unwrap_or("0")));
        let oh: i64 = oh.parse().ok()?;
        let om: i64 = if om.is_empty() { 0 } else { om.parse().ok()? };
        (c, sign * (oh * 3600 + om * 60))
    } else {
        (rest, 0)
    };

    let mut t = clock.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().unwrap_or("0").split('.').next()?.parse().ok()?;

    // Days since epoch (Howard Hinnant's civil algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + hour * 3600 + min * 60 + sec - offset_secs)
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + &c.as_str().to_lowercase(),
        None => String::new(),
    }
}

fn kind_for_seconds(secs: Option<i64>, fallback: &'static str) -> &'static str {
    match secs {
        Some(s) if s <= 6 * 3600 => "session",
        Some(s) if (6 * 86_400..=8 * 86_400).contains(&s) => "weekly",
        Some(_) => "period",
        None => fallback,
    }
}

// ----------------------------------------------------------------- Claude

fn claude_config_dir() -> Option<PathBuf> {
    env_nonempty("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".claude")))
}

/// Claude Code keeps its OAuth credentials in the macOS Keychain; on Linux
/// and Windows they live in `~/.claude/.credentials.json`.
fn claude_credentials() -> Option<serde_json::Value> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("/usr/bin/security")
            .args(["find-generic-password", "-s", "Claude Code-credentials", "-w"])
            .output()
            .ok();
        if let Some(out) = out.filter(|o| o.status.success()) {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(text.trim()) {
                return Some(v);
            }
        }
    }
    read_json_file(&claude_config_dir()?.join(".credentials.json"))
}

pub fn fetch_claude() -> ServiceStatus {
    let base = ServiceStatus::new("claude", "Claude", State::NotConnected);

    let (token, plan) = if let Some(t) = env_nonempty("CLAUDE_CODE_OAUTH_TOKEN") {
        (t, None)
    } else {
        let Some(creds) = claude_credentials() else {
            return base.with_note("Sign in to Claude Code once (run `claude`).");
        };
        let oauth = creds.get("claudeAiOauth").unwrap_or(&creds);
        let token = oauth
            .get("accessToken")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .or_else(|| find_string(&creds, &["accessToken", "access_token"]).map(str::to_string));
        let Some(token) = token else {
            return base.with_note("Sign in to Claude Code once (run `claude`).");
        };
        let plan = oauth
            .get("subscriptionType")
            .and_then(|v| v.as_str())
            .map(title_case);
        (token, plan)
    };

    let res = http_get_json(
        "https://api.anthropic.com/api/oauth/usage",
        &[
            ("Authorization", format!("Bearer {token}")),
            ("anthropic-beta", "oauth-2025-04-20".to_string()),
        ],
    );
    let v = match res {
        Ok(v) => v,
        Err(HttpError::Auth(_)) => {
            return ServiceStatus { state: State::Expired, plan, ..base }
                .with_note("Login expired — open Claude Code once to refresh it.")
        }
        Err(e) => {
            return ServiceStatus { state: State::Unavailable, plan, ..base }
                .with_note(format!("Couldn't reach Claude ({}).", e.describe()))
        }
    };
    let windows = parse_claude(&v);
    if windows.is_empty() {
        return ServiceStatus { state: State::Unavailable, plan, ..base }
            .with_note("Claude returned a response we don't understand yet.");
    }
    ServiceStatus { state: State::Connected, plan, windows, ..base }
}

fn parse_claude(v: &serde_json::Value) -> Vec<UsageWindow> {
    let mut out = Vec::new();
    for (key, kind, secs) in [
        ("five_hour", "session", 5 * 3600),
        ("seven_day", "weekly", 7 * 86_400),
        ("seven_day_opus", "weekly_opus", 7 * 86_400),
        ("seven_day_sonnet", "weekly_sonnet", 7 * 86_400),
    ] {
        let Some(w) = v.get(key).filter(|w| !w.is_null()) else { continue };
        let Some(used) = as_f64(w.get("utilization")) else { continue };
        // Model-specific weekly windows are noise until they are in use.
        if kind != "session" && kind != "weekly" && used <= 0.0 {
            continue;
        }
        out.push(UsageWindow {
            kind,
            used_percent: used.clamp(0.0, 100.0),
            resets_at: as_timestamp(w.get("resets_at")),
            window_seconds: Some(secs),
        });
    }
    out
}

// ------------------------------------------------------------------ Codex

fn codex_home() -> Option<PathBuf> {
    env_nonempty("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".codex")))
}

fn codex_auth() -> Option<(String, Option<String>)> {
    if let Some(t) = env_nonempty("CODEX_ACCESS_TOKEN") {
        return Some((t, env_nonempty("CHATGPT_ACCOUNT_ID")));
    }
    let auth = read_json_file(&codex_home()?.join("auth.json"))?;
    let tokens = auth.get("tokens");
    let token = tokens
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| find_string(&auth, &["access_token", "accessToken"]).map(str::to_string))?;
    let account = tokens
        .and_then(|t| t.get("account_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some((token, account))
}

const CODEX_LABEL: &str = "ChatGPT";

pub fn fetch_codex() -> ServiceStatus {
    let base = ServiceStatus::new("codex", CODEX_LABEL, State::NotConnected);
    let api = match codex_auth() {
        None => base.with_note("Open Codex in the ChatGPT desktop app and sign in, or run `codex login`."),
        Some((token, account)) => fetch_codex_api(base, &token, account),
    };
    if api.state == State::Connected {
        return api;
    }
    // No usable token (e.g. the desktop app keeps it in the OS credential
    // store) or the request failed: fall back to the rate limits the app /
    // CLI itself recorded in its newest session log.
    match codex_home().and_then(|h| codex_session_status(&h, now_unix())) {
        Some(session) => session,
        None => api,
    }
}

fn fetch_codex_api(base: ServiceStatus, token: &str, account: Option<String>) -> ServiceStatus {
    let mut headers: Vec<(&str, String)> = vec![("Authorization", format!("Bearer {token}"))];
    if let Some(a) = account {
        headers.push(("chatgpt-account-id", a));
    }
    let v = match http_get_json("https://chatgpt.com/backend-api/wham/usage", &headers) {
        Ok(v) => v,
        Err(HttpError::Auth(_)) => {
            return ServiceStatus { state: State::Expired, ..base }
                .with_note("Login expired — open Codex in the ChatGPT app (or run `codex`) once to refresh it.")
        }
        Err(e) => {
            return ServiceStatus { state: State::Unavailable, ..base }
                .with_note(format!("Couldn't reach ChatGPT ({}).", e.describe()))
        }
    };
    let plan = v
        .get("plan_type")
        .and_then(|p| p.as_str())
        .filter(|p| !p.is_empty())
        .map(title_case);
    let windows = parse_codex(&v, now_unix());
    if windows.is_empty() {
        return ServiceStatus { state: State::Unavailable, plan, ..base }
            .with_note("ChatGPT returned a response we don't understand yet.");
    }
    ServiceStatus { state: State::Connected, plan, windows, ..base }
}

/// Newest Codex rate-limit snapshot from `<codex_home>/sessions/YYYY/MM/DD/rollout-*.jsonl`.
fn codex_session_status(codex_home: &std::path::Path, now: i64) -> Option<ServiceStatus> {
    let files = newest_rollouts(&codex_home.join("sessions"), 6);
    for f in files {
        let Some(tail) = read_tail(&f, 768 * 1024) else { continue };
        for line in tail.lines().rev() {
            if !line.contains("\"token_count\"") || !line.contains("rate_limits") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            if let Some(st) = parse_codex_session_line(&v, now) {
                return Some(st);
            }
        }
    }
    None
}

fn parse_codex_session_line(v: &serde_json::Value, now: i64) -> Option<ServiceStatus> {
    let payload = v.get("payload").unwrap_or(v);
    if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
        return None;
    }
    let rl = payload.get("rate_limits").filter(|r| r.is_object())?;
    // Only the main Codex limit (other limit ids are per-model extras).
    if let Some(id) = rl.get("limit_id").and_then(|i| i.as_str()) {
        if id != "codex" {
            return None;
        }
    }
    let at = v
        .get("timestamp")
        .and_then(|t| t.as_str())
        .and_then(parse_rfc3339)
        .unwrap_or(now);
    let mut windows = Vec::new();
    for (key, fallback) in [("primary", "session"), ("secondary", "weekly")] {
        let Some(w) = rl.get(key).filter(|w| w.is_object()) else { continue };
        let Some(mut used) = as_f64(w.get("used_percent")) else { continue };
        let secs = as_f64(w.get("window_minutes")).map(|m| (m * 60.0) as i64);
        let mut resets_at = as_timestamp(w.get("resets_at"))
            .or_else(|| as_f64(w.get("resets_in_seconds")).map(|s| at + s as i64));
        if let Some(r) = resets_at {
            if r <= now {
                // The window rolled over since this event: nothing used yet.
                used = 0.0;
                resets_at = None;
            }
        }
        windows.push(UsageWindow {
            kind: kind_for_seconds(secs, fallback),
            used_percent: used.clamp(0.0, 100.0),
            resets_at,
            window_seconds: secs,
        });
    }
    if windows.is_empty() {
        return None;
    }
    let plan = rl
        .get("plan_type")
        .and_then(|p| p.as_str())
        .filter(|p| !p.is_empty())
        .map(title_case);
    Some(ServiceStatus {
        id: "codex",
        label: CODEX_LABEL,
        state: State::Connected,
        plan,
        windows,
        note: Some("From your latest Codex session.".into()),
        fetched_at: at,
        stale: false,
        source: "session",
    })
}

/// Plain `.jsonl` rollouts from the newest day folders, newest first.
fn newest_rollouts(root: &std::path::Path, max: usize) -> Vec<PathBuf> {
    fn sorted_dirs(p: &std::path::Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
            .unwrap_or_default();
        v.sort();
        v.reverse();
        v
    }
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    let mut days = 0;
    'outer: for y in sorted_dirs(root) {
        for m in sorted_dirs(&y) {
            for d in sorted_dirs(&m) {
                if let Ok(rd) = fs::read_dir(&d) {
                    for e in rd.flatten() {
                        let p = e.path();
                        if p.extension().and_then(|x| x.to_str()) == Some("jsonl") {
                            let t = e.metadata().and_then(|m| m.modified()).unwrap_or(UNIX_EPOCH);
                            files.push((t, p));
                        }
                    }
                }
                days += 1;
                if days >= 3 {
                    break 'outer;
                }
            }
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().take(max).map(|(_, p)| p).collect()
}

fn read_tail(path: &std::path::Path, max: u64) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    let mut s = String::from_utf8_lossy(&buf).into_owned();
    if start > 0 {
        // Drop the partial first line.
        if let Some(i) = s.find('\n') {
            s.drain(..=i);
        }
    }
    Some(s)
}

fn parse_codex(v: &serde_json::Value, now: i64) -> Vec<UsageWindow> {
    let rl = v.get("rate_limit").unwrap_or(v);
    let mut out = Vec::new();
    for (key, fallback) in [("primary_window", "session"), ("secondary_window", "weekly")] {
        let Some(w) = rl.get(key).filter(|w| !w.is_null()) else { continue };
        let Some(used) = as_f64(w.get("used_percent")) else { continue };
        let secs = as_f64(w.get("limit_window_seconds")).map(|s| s as i64);
        let resets_at = as_timestamp(w.get("reset_at"))
            .or_else(|| as_f64(w.get("reset_after_seconds")).map(|s| now + s as i64));
        out.push(UsageWindow {
            kind: kind_for_seconds(secs, fallback),
            used_percent: used.clamp(0.0, 100.0),
            resets_at,
            window_seconds: secs,
        });
    }
    out
}

// ------------------------------------------------------------------- Grok

fn grok_token() -> Option<String> {
    if let Some(t) = env_nonempty("GROK_ACCESS_TOKEN") {
        return Some(t);
    }
    let dir = env_nonempty("GROK_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".grok")))?;
    let auth = read_json_file(&dir.join("auth.json"))?;
    find_string(&auth, &["access_token", "accessToken", "token", "api_key", "apiKey"])
        .map(str::to_string)
}

pub fn fetch_grok() -> ServiceStatus {
    let base = ServiceStatus::new("grok", "Grok", State::NotConnected);
    let Some(token) = grok_token() else {
        return base.with_note("Sign in to Grok Build once (run `grok login`).");
    };
    let v = match http_get_json(
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
        &[
            ("Authorization", format!("Bearer {token}")),
            ("x-xai-token-auth", "xai-grok-cli".to_string()),
        ],
    ) {
        Ok(v) => v,
        Err(HttpError::Auth(_)) => {
            return ServiceStatus { state: State::Expired, ..base }
                .with_note("Login expired — run `grok login` again.")
        }
        Err(e) => {
            return ServiceStatus { state: State::Unavailable, ..base }
                .with_note(format!("Couldn't reach Grok ({}).", e.describe()))
        }
    };
    let windows = parse_grok(&v);
    if windows.is_empty() {
        return ServiceStatus { state: State::Unavailable, ..base }
            .with_note("Grok returned a response we don't understand yet.");
    }
    ServiceStatus { state: State::Connected, windows, ..base }
}

fn parse_grok(v: &serde_json::Value) -> Vec<UsageWindow> {
    let used = as_f64(v.get("config").and_then(|c| c.get("creditUsagePercent")))
        .or_else(|| as_f64(v.get("creditUsagePercent")));
    let Some(used) = used else { return Vec::new() };
    let period = v.get("currentPeriod");
    let start = as_timestamp(period.and_then(|p| p.get("start")));
    let end = as_timestamp(period.and_then(|p| p.get("end")));
    let secs = match (start, end) {
        (Some(s), Some(e)) if e > s => Some(e - s),
        _ => None,
    };
    vec![UsageWindow {
        kind: kind_for_seconds(secs, "weekly"),
        used_percent: used.clamp(0.0, 100.0),
        resets_at: end,
        window_seconds: secs,
    }]
}

// ------------------------------------------------------------------- mock

/// Demo data for screenshots, local testing and CI smoke tests only.
/// Always labelled "mock" so the UI never passes it off as real.
pub fn mock_status() -> Vec<ServiceStatus> {
    let now = now_unix();
    let w = |kind, used, resets_in: i64, secs: i64| UsageWindow {
        kind,
        used_percent: used,
        resets_at: Some(now + resets_in),
        window_seconds: Some(secs),
    };
    let mk = |id, label, plan: &str, windows| ServiceStatus {
        id,
        label,
        state: State::Mock,
        plan: Some(plan.to_string()),
        windows,
        note: Some("Demo data".into()),
        fetched_at: now,
        stale: false,
        source: "api",
    };
    vec![
        mk("claude", "Claude", "Max", vec![
            w("session", 42.0, 2 * 3600 + 14 * 60, 5 * 3600),
            w("weekly", 26.0, 3 * 86_400 + 5 * 3600, 7 * 86_400),
        ]),
        mk("codex", CODEX_LABEL, "Plus", vec![
            w("session", 37.0, 58 * 60, 5 * 3600),
            w("weekly", 18.0, 5 * 86_400, 7 * 86_400),
        ]),
        mk("grok", "Grok", "SuperGrok", vec![w("weekly", 57.0, 4 * 86_400 + 2 * 3600, 7 * 86_400)]),
    ]
}

pub fn fetch_one(id: &str) -> Option<ServiceStatus> {
    match id {
        "claude" => Some(fetch_claude()),
        "codex" => Some(fetch_codex()),
        "grok" => Some(fetch_grok()),
        _ => None,
    }
}

pub const IDS: [&str; 3] = ["claude", "codex", "grok"];

/// Fetch every service in parallel. `mock` forces demo data.
pub fn fetch_all(mock: bool) -> Vec<ServiceStatus> {
    if mock {
        return mock_status();
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = IDS.iter().map(|id| s.spawn(move || fetch_one(id))).collect();
        handles
            .into_iter()
            .filter_map(|h| h.join().ok().flatten())
            .collect()
    })
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rfc3339_utc() {
        assert_eq!(parse_rfc3339("2026-02-08T04:59:59Z"), Some(1_770_526_799));
    }

    #[test]
    fn rfc3339_offsets_and_fractions() {
        assert_eq!(parse_rfc3339("2026-02-08T12:59:59.481+08:00"), Some(1_770_526_799));
        assert_eq!(parse_rfc3339("2026-02-07T23:59:59-05:00"), Some(1_770_526_799));
        assert_eq!(parse_rfc3339("2026-02-08T04:59:59+00:00"), Some(1_770_526_799));
        assert_eq!(parse_rfc3339("nonsense"), None);
    }

    #[test]
    fn claude_payload() {
        let v = json!({
            "five_hour": {"utilization": 42.0, "resets_at": "2026-02-08T04:59:59+00:00"},
            "seven_day": {"utilization": 26, "resets_at": "2026-02-10T00:00:00Z"},
            "seven_day_opus": null,
            "seven_day_sonnet": {"utilization": 0, "resets_at": null}
        });
        let w = parse_claude(&v);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].kind, "session");
        assert_eq!(w[0].resets_at, Some(1_770_526_799));
        assert_eq!(w[1].kind, "weekly");
        assert_eq!(w[1].used_percent, 26.0);
    }

    #[test]
    fn codex_session_line() {
        let line = r#"{"timestamp":"2026-11-20T06:00:00.000Z","type":"event_msg","payload":{"type":"token_count","info":null,"rate_limits":{"limit_id":"codex","primary":{"used_percent":37.0,"window_minutes":300,"resets_at":1795672800},"secondary":{"used_percent":12.5,"window_minutes":10080,"resets_at":1000},"plan_type":"plus"}}}"#;
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        let now = parse_rfc3339("2026-11-20T06:05:00Z").unwrap();
        let st = parse_codex_session_line(&v, now).unwrap();
        assert_eq!(st.source, "session");
        assert_eq!(st.plan.as_deref(), Some("Plus"));
        assert_eq!(st.windows[0].kind, "session");
        assert_eq!(st.windows[0].used_percent, 37.0);
        // secondary already reset -> counted as unused
        assert_eq!(st.windows[1].kind, "weekly");
        assert_eq!(st.windows[1].used_percent, 0.0);
        assert_eq!(st.windows[1].resets_at, None);
        assert_eq!(st.fetched_at, parse_rfc3339("2026-11-20T06:00:00Z").unwrap());
        // other limit ids are ignored
        let other = line.replace("\"limit_id\":\"codex\"", "\"limit_id\":\"codex_other\"");
        let v: serde_json::Value = serde_json::from_str(&other).unwrap();
        assert!(parse_codex_session_line(&v, now).is_none());
    }

    #[test]
    fn codex_session_files() {
        let root = std::env::temp_dir().join(format!("aum-test-{}", std::process::id()));
        let day = root.join("sessions/2026/11/20");
        fs::create_dir_all(&day).unwrap();
        let mut body = String::from("{\"type\":\"session_meta\"}\n");
        body.push_str(r#"{"timestamp":"2026-11-20T06:00:00Z","type":"event_msg","payload":{"type":"token_count","rate_limits":{"primary":{"used_percent":50.0,"window_minutes":300,"resets_in_seconds":600}}}}"#);
        body.push('\n');
        fs::write(day.join("rollout-2026-11-20T06-00-00-x.jsonl"), body).unwrap();
        let now = parse_rfc3339("2026-11-20T06:01:00Z").unwrap();
        let st = codex_session_status(&root, now).unwrap();
        assert_eq!(st.windows[0].used_percent, 50.0);
        assert_eq!(st.windows[0].resets_at, Some(now - 60 + 600));
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn codex_payload() {
        let v = json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary_window": {"used_percent": 37, "limit_window_seconds": 18000, "reset_after_seconds": 600},
                "secondary_window": {"used_percent": 18, "limit_window_seconds": 604800, "reset_at": 1770526799}
            }
        });
        let w = parse_codex(&v, 1000);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].kind, "session");
        assert_eq!(w[0].resets_at, Some(1600));
        assert_eq!(w[1].kind, "weekly");
        assert_eq!(w[1].resets_at, Some(1_770_526_799));
    }

    #[test]
    fn grok_payload() {
        let v = json!({
            "config": {"creditUsagePercent": 57.5},
            "currentPeriod": {"start": "2026-02-01T00:00:00Z", "end": "2026-02-08T00:00:00Z"}
        });
        let w = parse_grok(&v);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "weekly");
        assert_eq!(w[0].used_percent, 57.5);
        assert!(parse_grok(&json!({})).is_empty());
    }

    #[test]
    fn mock_is_labelled_and_complete() {
        let all = mock_status();
        assert_eq!(all.iter().map(|s| s.id).collect::<Vec<_>>(), IDS);
        for s in &all {
            assert_eq!(s.state, State::Mock);
            assert!(s.has_numbers());
            assert!(s.peak_used().is_some());
        }
    }

    #[test]
    fn find_string_walks_nested_json() {
        let v = json!({"a": {"tokens": {"access_token": "tok123"}}});
        assert_eq!(find_string(&v, &["access_token"]), Some("tok123"));
    }

    #[test]
    fn state_serialises_snake_case() {
        assert_eq!(serde_json::to_string(&State::NotConnected).unwrap(), "\"not_connected\"");
    }
}
