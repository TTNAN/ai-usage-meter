//! Usage providers: Claude, ChatGPT (Codex quota), Grok, Gemini and GitHub
//! Copilot — shared by AI Usage Meter and AI Usage Meter Dock.
//!
//! None of these services publishes an official "how much of my subscription
//! is left" API. We read the same internal endpoints their own apps use,
//! reusing the login those apps and CLIs already stored on this machine.
//! Read-only requests, at low frequency. When anything fails we say so — we
//! never invent numbers.
//!
//! | Service | Endpoint | Credential |
//! | ------- | -------- | ---------- |
//! | Claude  | `GET https://api.anthropic.com/api/oauth/usage` | Claude Code OAuth token: macOS Keychain item `Claude Code-credentials`, else `~/.claude/.credentials.json` |
//! | ChatGPT (Codex) | `GET https://chatgpt.com/backend-api/wham/usage`, falling back to the newest `token_count` event in `~/.codex/sessions` | `~/.codex/auth.json` (`$CODEX_HOME` respected; shared by Codex CLI and the ChatGPT desktop app) |
//! | Grok    | `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits` | Grok Build CLI: `~/.grok/auth.json` (`$GROK_HOME` respected) |
//! | Gemini  | `POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` + `:retrieveUserQuota` | Gemini CLI / Antigravity: `~/.gemini/oauth_creds.json` (`$GEMINI_HOME` respected) |
//! | Copilot | `GET https://api.github.com/copilot_internal/user` | GitHub OAuth token: `$GH_TOKEN` / `$GITHUB_TOKEN`, `gh` hosts.yml, the Copilot CLI's `hosts.json`, or `gh auth token` |
//!
//! Tokens are read at call time, held in memory only for the request, and
//! never written or sent anywhere except to the service they belong to.

use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

/// Unix milliseconds, the unit OAuth expiry dates are kept in.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
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
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn as_bool(v: &serde_json::Value) -> Option<bool> {
    v.as_bool()
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

/// A short-timeout client for one request. Every call re-reads its token at
/// request time, so nothing long-lived is kept around.
fn http_client() -> Result<reqwest::blocking::Client, HttpError> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(crate::USER_AGENT)
        .build()
        .map_err(|e| HttpError::Other(format!("http client: {e}")))
}

fn send_error(e: reqwest::Error) -> HttpError {
    HttpError::Other(if e.is_timeout() {
        "timed out".into()
    } else if e.is_connect() {
        "offline or blocked".into()
    } else {
        "network error".into()
    })
}

/// Read a JSON body, turning the two statuses that matter into `Auth`.
fn read_json(resp: reqwest::blocking::Response) -> Result<serde_json::Value, HttpError> {
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

/// One short-timeout JSON request, read-only.
fn http_json(
    method: reqwest::Method,
    url: &str,
    headers: &[(&str, String)],
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, HttpError> {
    let client = http_client()?;
    let mut req = client
        .request(method, url)
        .header("Accept", "application/json");
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    if let Some(body) = body {
        req = req.json(body);
    }
    read_json(req.send().map_err(send_error)?)
}

/// One `application/x-www-form-urlencoded` POST — what OAuth's token endpoint
/// speaks, unlike every other request here.
fn http_post_form(url: &str, form: &[(&str, String)]) -> Result<serde_json::Value, HttpError> {
    let client = http_client()?;
    let req = client
        .post(url)
        .header("Accept", "application/json")
        .form(form);
    read_json(req.send().map_err(send_error)?)
}

fn http_get_json(url: &str, headers: &[(&str, String)]) -> Result<serde_json::Value, HttpError> {
    http_json(reqwest::Method::GET, url, headers, None)
}

fn http_post_json(
    url: &str,
    headers: &[(&str, String)],
    body: &serde_json::Value,
) -> Result<serde_json::Value, HttpError> {
    http_json(reqwest::Method::POST, url, headers, Some(body))
}

/// Run a small CLI (`gh auth token`) without ever letting it hang the poller.
fn run_with_timeout(program: &str, args: &[&str], timeout: Duration) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut out = String::new();
                child.stdout.take()?.read_to_string(&mut out).ok()?;
                let token = out.trim();
                return (!token.is_empty()).then(|| token.to_string());
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(_) => return None,
        }
    }
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
        let (oh, om) = off
            .split_once(':')
            .unwrap_or((off.get(..2)?, off.get(2..).unwrap_or("0")));
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
            .args([
                "find-generic-password",
                "-s",
                "Claude Code-credentials",
                "-w",
            ])
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
            return ServiceStatus {
                state: State::Expired,
                plan,
                ..base
            }
            .with_note("Login expired — open Claude Code once to refresh it.")
        }
        Err(e) => {
            return ServiceStatus {
                state: State::Unavailable,
                plan,
                ..base
            }
            .with_note(format!("Couldn't reach Claude ({}).", e.describe()))
        }
    };
    let windows = parse_claude(&v);
    if windows.is_empty() {
        return ServiceStatus {
            state: State::Unavailable,
            plan,
            ..base
        }
        .with_note("Claude returned a response we don't understand yet.");
    }
    ServiceStatus {
        state: State::Connected,
        plan,
        windows,
        ..base
    }
}

fn parse_claude(v: &serde_json::Value) -> Vec<UsageWindow> {
    let mut out = Vec::new();
    for (key, kind, secs) in [
        ("five_hour", "session", 5 * 3600),
        ("seven_day", "weekly", 7 * 86_400),
        ("seven_day_opus", "weekly_opus", 7 * 86_400),
        ("seven_day_sonnet", "weekly_sonnet", 7 * 86_400),
    ] {
        let Some(w) = v.get(key).filter(|w| !w.is_null()) else {
            continue;
        };
        let Some(used) = as_f64(w.get("utilization")) else {
            continue;
        };
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
        None => base
            .with_note("Open Codex in the ChatGPT desktop app and sign in, or run `codex login`."),
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
        Err(HttpError::Auth(_)) => return ServiceStatus {
            state: State::Expired,
            ..base
        }
        .with_note(
            "Login expired — open Codex in the ChatGPT app (or run `codex`) once to refresh it.",
        ),
        Err(e) => {
            return ServiceStatus {
                state: State::Unavailable,
                ..base
            }
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
        return ServiceStatus {
            state: State::Unavailable,
            plan,
            ..base
        }
        .with_note("ChatGPT returned a response we don't understand yet.");
    }
    ServiceStatus {
        state: State::Connected,
        plan,
        windows,
        ..base
    }
}

/// Newest Codex rate-limit snapshot from `<codex_home>/sessions/YYYY/MM/DD/rollout-*.jsonl`.
fn codex_session_status(codex_home: &std::path::Path, now: i64) -> Option<ServiceStatus> {
    let files = newest_rollouts(&codex_home.join("sessions"), 6);
    for f in files {
        let Some(tail) = read_tail(&f, 768 * 1024) else {
            continue;
        };
        for line in tail.lines().rev() {
            if !line.contains("\"token_count\"") || !line.contains("rate_limits") {
                continue;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
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
        let Some(w) = rl.get(key).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(mut used) = as_f64(w.get("used_percent")) else {
            continue;
        };
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
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.is_dir())
                    .collect()
            })
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
                            let t = e
                                .metadata()
                                .and_then(|m| m.modified())
                                .unwrap_or(UNIX_EPOCH);
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
    for (key, fallback) in [
        ("primary_window", "session"),
        ("secondary_window", "weekly"),
    ] {
        let Some(w) = rl.get(key).filter(|w| !w.is_null()) else {
            continue;
        };
        let Some(used) = as_f64(w.get("used_percent")) else {
            continue;
        };
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
    find_string(
        &auth,
        &["access_token", "accessToken", "token", "api_key", "apiKey"],
    )
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
            return ServiceStatus {
                state: State::Expired,
                ..base
            }
            .with_note("Login expired — run `grok login` again.")
        }
        Err(e) => {
            return ServiceStatus {
                state: State::Unavailable,
                ..base
            }
            .with_note(format!("Couldn't reach Grok ({}).", e.describe()))
        }
    };
    let windows = parse_grok(&v);
    if windows.is_empty() {
        return ServiceStatus {
            state: State::Unavailable,
            ..base
        }
        .with_note("Grok returned a response we don't understand yet.");
    }
    ServiceStatus {
        state: State::Connected,
        windows,
        ..base
    }
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

// ----------------------------------------------------------------- Gemini
//
// Gemini CLI and Google Antigravity both sign in with a Google account and
// store the OAuth token at `~/.gemini/oauth_creds.json`. Their quota lives in
// Google's Cloud Code Assist backend, per model rather than per time window:
// one rolling allowance, each bucket with its own remaining fraction and
// reset. `loadCodeAssist` names the plan, `retrieveUserQuota` the buckets.

const GEMINI_LABEL: &str = "Gemini";
const GEMINI_API: &str = "https://cloudcode-pa.googleapis.com/v1internal";
/// Sent when `loadCodeAssist` doesn't name a Cloud project.
const GEMINI_PLACEHOLDER_PROJECT: &str = "gemini-cli-placeholder";

fn gemini_home() -> Option<PathBuf> {
    env_nonempty("GEMINI_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".gemini")))
}

/// How Gemini CLI is signed in: "oauth-personal" (Google account) or an
/// API-key / Vertex AI setup, which shares no quota endpoint with us.
fn gemini_auth_mode(dir: &std::path::Path) -> Option<String> {
    let v = read_json_file(&dir.join("settings.json"))?;
    find_string(&v, &["selectedType", "authType", "authMode", "auth_mode"]).map(str::to_string)
}

fn gemini_mode_blocks_quota(mode: &str) -> bool {
    let m = mode.to_ascii_lowercase();
    m.contains("api-key") || m.contains("api_key") || m.contains("apikey") || m.contains("vertex")
}

// ------------------------------------------------- Gemini token refresh
//
// `oauth_creds.json` holds a short-lived Google access token (an hour) and the
// refresh token that renews it. Gemini CLI itself renews it on its next run;
// between runs it goes stale, and the quota calls then answer 401 for no reason
// the user can see. Renewing it here keeps the row live without asking anyone to
// open a terminal — and if there is nothing to renew with, or Google refuses, we
// say "log in again" exactly as before.

/// Google's OAuth token endpoint.
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
/// Renew this long before the token actually expires, so a request that leaves
/// a moment later still carries a valid token.
const GEMINI_REFRESH_MARGIN_MS: i64 = 120_000;

// ------------------------------------------- Gemini CLI's OAuth client
//
// Redeeming the refresh token needs the installed-app OAuth client Gemini CLI
// signed in with. `oauth_creds.json` usually does not name it, so the CLI's own
// client has to come from somewhere — but not from this repository: these two
// apps are public, and another party's OAuth client is theirs to publish, not
// ours. The CLI is on the machine by definition (it wrote the token file), and
// its published bundle carries both halves, so we read the pair out of that
// installation. `$GEMINI_CLI_OAUTH_CLIENT_ID` / `..._SECRET` win when set, which
// is also how a user points us at their own client.

fn gemini_cli_client() -> Option<&'static (String, String)> {
    static CLIENT: std::sync::OnceLock<Option<(String, String)>> = std::sync::OnceLock::new();
    CLIENT.get_or_init(find_gemini_cli_client).as_ref()
}

fn find_gemini_cli_client() -> Option<(String, String)> {
    if let (Some(id), Some(secret)) = (
        env_nonempty("GEMINI_CLI_OAUTH_CLIENT_ID"),
        env_nonempty("GEMINI_CLI_OAUTH_CLIENT_SECRET"),
    ) {
        return Some((id, secret));
    }
    let mut packages = Vec::new();
    // The `gemini` on PATH is the CLI the user actually runs, wherever it was
    // installed (Homebrew keg, custom npm prefix, Volta, pnpm). Its bin entry
    // points into the package, so the package we need is one or two parents up.
    if let Some(package) = gemini_cli_on_path() {
        packages.push(package);
    }
    let mut budget = 4000usize;
    for root in gemini_cli_package_roots() {
        gemini_cli_packages(&root, 5, &mut packages, &mut budget);
    }
    gemini_cli_homebrew_packages(&mut packages);
    for package in packages {
        if let Some(pair) = oauth_client_in_package(&package) {
            return Some(pair);
        }
    }
    None
}

/// The `@google/gemini-cli` package that the `gemini` on `PATH` belongs to.
fn gemini_cli_on_path() -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for name in ["gemini", "gemini.js", "gemini.cmd", "gemini.exe", "gemini.ps1"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                let real = fs::canonicalize(&candidate).unwrap_or(candidate);
                if let Some(package) = scoped_gemini_package(&real) {
                    return Some(package);
                }
            }
        }
    }
    None
}

/// `…/@google/gemini-cli` above `file`, if the file lives inside that package.
/// A bin entry sits two levels down (`gemini-cli/bundle/gemini.js`).
fn scoped_gemini_package(file: &std::path::Path) -> Option<PathBuf> {
    let mut at = file;
    for _ in 0..4 {
        let parent = at.parent()?;
        let named = parent.file_name().and_then(|n| n.to_str()) == Some("gemini-cli");
        let scoped = parent
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("@google");
        if named && scoped {
            return Some(parent.to_path_buf());
        }
        at = parent;
    }
    None
}

/// Homebrew installs a Node CLI into a keg (`Cellar/gemini-cli/<version>/
/// libexec/lib/node_modules/…`) that no global node_modules root covers, and
/// `brew` may not put `gemini` on the PATH the app inherits (GUI apps on macOS
/// get a bare PATH).
fn gemini_cli_homebrew_packages(out: &mut Vec<PathBuf>) {
    let cellars = [
        "/opt/homebrew/Cellar",
        "/usr/local/Cellar",
        "/home/linuxbrew/.linuxbrew/Cellar",
    ];
    for cellar in cellars {
        let Ok(entries) = fs::read_dir(cellar) else {
            continue;
        };
        for formula in entries.flatten() {
            if !formula.file_name().to_string_lossy().starts_with("gemini-cli") {
                continue;
            }
            let Ok(versions) = fs::read_dir(formula.path()) else {
                continue;
            };
            for version in versions.flatten() {
                for rel in [
                    "libexec/lib/node_modules/@google/gemini-cli",
                    "lib/node_modules/@google/gemini-cli",
                ] {
                    let package = version.path().join(rel);
                    if package.is_dir() && !out.contains(&package) {
                        out.push(package);
                    }
                }
            }
        }
    }
}

/// The places node package managers put a global CLI. Node's layouts differ per
/// manager, so these are roots to look under rather than the package itself.
fn gemini_cli_package_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut add = |p: PathBuf| {
        if p.is_dir() && !roots.contains(&p) {
            roots.push(p);
        }
    };
    if let Some(home) = home_dir() {
        for rel in [
            ".npm-global/lib/node_modules",
            ".local/lib/node_modules",
            ".local/share/pnpm/global",
            "Library/pnpm/global",
            ".config/yarn/global/node_modules",
            ".bun/install/global/node_modules",
            ".volta/tools/image/node",
            ".npm/_npx",
            "node_modules",
        ] {
            add(home.join(rel));
        }
        if let Some(appdata) = std::env::var_os("APPDATA") {
            add(PathBuf::from(appdata).join("npm/node_modules"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            add(PathBuf::from(local).join("Volta/tools/image/node"));
        }
        add(home.join("AppData/Roaming/npm/node_modules"));
        add(home.join("AppData/Local/Volta/tools/image/node"));
    }
    for abs in [
        "/usr/local/lib/node_modules",
        "/usr/local/share/npm/lib/node_modules",
        "/opt/homebrew/lib/node_modules",
        "/opt/homebrew/Cellar",
        "/usr/lib/node_modules",
        "/home/linuxbrew/.linuxbrew/lib/node_modules",
    ] {
        add(PathBuf::from(abs));
    }
    roots
}

/// Collect `@google/gemini-cli` package directories under `root`, at most
/// `depth` levels down, spending no more than `budget` directory reads.
fn gemini_cli_packages(
    root: &std::path::Path,
    depth: usize,
    out: &mut Vec<PathBuf>,
    budget: &mut usize,
) {
    if depth == 0 || out.len() >= 4 || *budget == 0 {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 || out.len() >= 4 {
            return;
        }
        *budget -= 1;
        // symlink_metadata: a linked toolchain must not turn into a walk loop.
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_dir() {
            continue;
        }
        let path = entry.path();
        let named = path.file_name().and_then(|n| n.to_str()) == Some("gemini-cli");
        let scoped = path
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            == Some("@google");
        if named && scoped {
            out.push(path);
            continue;
        }
        gemini_cli_packages(&path, depth - 1, out, budget);
    }
}

/// The OAuth client pair inside one installed package: read its scripts (the
/// bundle is one big .js) and keep the first client id/secret pair found.
fn oauth_client_in_package(dir: &std::path::Path) -> Option<(String, String)> {
    let mut files = Vec::new();
    collect_script_files(dir, 3, &mut files);
    let mut scanned = 0u64;
    for file in files {
        let Ok(meta) = fs::metadata(&file) else { continue };
        if meta.len() > 64 * 1024 * 1024 || scanned > 192 * 1024 * 1024 {
            continue;
        }
        // Lossy: a minified bundle is not guaranteed to be valid UTF-8, and one
        // bad byte in the middle must not hide the client.
        let Ok(bytes) = fs::read(&file) else {
            continue;
        };
        scanned += bytes.len() as u64;
        if let Some(pair) = oauth_client_in(&String::from_utf8_lossy(&bytes)) {
            return Some(pair);
        }
    }
    None
}

/// The package's scripts, deepest-last: Gemini CLI ships its auth code among
/// ~80 bundle chunks, so the cap is generous but finite.
fn collect_script_files(dir: &std::path::Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth == 0 || out.len() >= 120 {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= 120 {
            return;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let path = entry.path();
        if meta.is_dir() {
            collect_script_files(&path, depth - 1, out);
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if matches!(ext, "js" | "mjs" | "cjs") {
            out.push(path);
        }
    }
}

/// Find the OAuth client in one piece of CLI source: every client id and every
/// `GOCSPX-…` secret in the file, paired by proximity. Adjacency is the only
/// signal there is — Gemini CLI's bundle also names a Cloud SDK client id that
/// must not be paired with this secret — and the pair the CLI declares sits a
/// hundred bytes apart.
fn oauth_client_in(text: &str) -> Option<(String, String)> {
    let ids = client_ids_in(text);
    let secrets = client_secrets_in(text);
    let mut best: Option<(usize, usize, usize)> = None;
    for (si, (secret_at, _)) in secrets.iter().enumerate() {
        for (ii, (id_at, _)) in ids.iter().enumerate() {
            let distance = secret_at.abs_diff(*id_at);
            if best.map_or(true, |(closest, _, _)| distance < closest) {
                best = Some((distance, ii, si));
            }
        }
    }
    // A client and a secret thousands of bytes apart are two fixtures in one
    // file, not a pair: better nothing than the wrong client.
    let (distance, id, secret) = best?;
    if distance > 4096 {
        return None;
    }
    Some((ids[id].1.clone(), secrets[secret].1.clone()))
}

/// Every `…apps.googleusercontent.com` in `text` with the byte offset of its
/// dotted host, so a caller can pair it with other finds.
fn client_ids_in(text: &str) -> Vec<(usize, String)> {
    const HOST: &str = "apps.googleusercontent.com";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(HOST).map(|i| from + i) {
        from = at + HOST.len();
        let head = &text[..at];
        let mut start = at;
        for (i, ch) in head.char_indices().rev() {
            if head.len() - i > 120 {
                break;
            }
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                start = i;
            } else {
                break;
            }
        }
        let id = &text[start..at + HOST.len()];
        // Google client ids are `<project number>-<32 chars>.apps.googleusercontent.com`.
        if id.starts_with("https://") || id.starts_with("http://") || id.starts_with("www.") {
            continue;
        }
        if let Some((number, rest)) = id.split_once('-') {
            if number.len() >= 10
                && number.chars().all(|c| c.is_ascii_digit())
                && rest.len() >= 30
            {
                found.push((at, id.to_string()));
            }
        }
    }
    found
}

/// Every `GOCSPX-…` secret in `text`, with its byte offset.
fn client_secrets_in(text: &str) -> Vec<(usize, String)> {
    const PREFIX: &str = "GOCSPX-";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text[from..].find(PREFIX).map(|i| from + i) {
        from = at + PREFIX.len();
        let secret: String = text[from..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        if secret.len() >= 20 {
            found.push((at, format!("{PREFIX}{secret}")));
        }
    }
    found
}

/// A token Google just issued, with the lifetime it reported.
struct FreshToken {
    access_token: String,
    expires_in: i64,
    /// Present only when Google rotated it, which it does on some clients.
    refresh_token: Option<String>,
}

/// One Gemini CLI login, kept in step with `oauth_creds.json`.
struct GeminiSession {
    path: PathBuf,
    creds: serde_json::Value,
    token: String,
    /// Whether this fetch already asked Google for a new token, so a second 401
    /// is reported instead of triggering another call.
    refreshed: bool,
}

impl GeminiSession {
    fn load(path: PathBuf) -> Option<Self> {
        let creds = read_json_file(&path)?;
        let token = gemini_access_token(&creds)?;
        Some(GeminiSession {
            path,
            creds,
            token,
            refreshed: false,
        })
    }

    fn token(&self) -> &str {
        &self.token
    }

    /// True when the login is past (or within two minutes of) its expiry.
    fn needs_refresh(&self) -> bool {
        gemini_token_stale(now_ms(), gemini_expiry_ms(&self.creds))
    }

    /// Ask Google for a new access token once. Returns false when there is no
    /// refresh token, when the request fails, or when it was already tried.
    fn refresh(&mut self) -> bool {
        if self.refreshed {
            return false;
        }
        self.refreshed = true;
        let Some(fresh) = refresh_gemini_token(&self.creds) else {
            return false;
        };
        // The new token is usable either way; the file is only updated when it
        // can be written cleanly.
        if let Ok(updated) = update_gemini_creds(&self.path, &fresh, now_ms()) {
            self.creds = updated;
        }
        self.token = fresh.access_token;
        true
    }
}

fn gemini_access_token(v: &serde_json::Value) -> Option<String> {
    v.get("access_token")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| find_string(v, &["access_token", "accessToken"]))
        .map(str::to_string)
}

fn gemini_refresh_token(v: &serde_json::Value) -> Option<String> {
    find_string(v, &["refresh_token", "refreshToken"]).map(str::to_string)
}

/// The OAuth client that is allowed to redeem this refresh token: the one the
/// file names, else Gemini CLI's own. `None` when neither is available, in which
/// case there is nothing to redeem with and the token is left alone.
fn gemini_client_id(v: &serde_json::Value) -> Option<String> {
    find_string(v, &["client_id", "clientId"])
        .map(str::to_string)
        .or_else(|| gemini_cli_client().map(|(id, _)| id.clone()))
}

fn gemini_client_secret(v: &serde_json::Value) -> String {
    find_string(v, &["client_secret", "clientSecret"])
        .map(str::to_string)
        .or_else(|| gemini_cli_client().map(|(_, secret)| secret.clone()))
        .unwrap_or_default()
}

/// When the access token expires, in unix ms. The CLI stores milliseconds in
/// `expiry_date`; seconds and RFC 3339 spellings are accepted too, because
/// other tools write the same file.
fn gemini_expiry_ms(v: &serde_json::Value) -> Option<i64> {
    let raw = v
        .get("expiry_date")
        .or_else(|| v.get("expiryDate"))
        .or_else(|| v.get("expires_at"))?;
    if let Some(n) = raw.as_f64() {
        let n = n as i64;
        return Some(if n > 100_000_000_000 { n } else { n * 1000 });
    }
    raw.as_str().and_then(parse_rfc3339).map(|s| s * 1000)
}

/// An unknown expiry is not treated as expired: we only renew when the file
/// says the token is (nearly) done, and a 401 renews it anyway.
fn gemini_token_stale(now_ms: i64, expiry_ms: Option<i64>) -> bool {
    expiry_ms.map_or(false, |at| at - now_ms <= GEMINI_REFRESH_MARGIN_MS)
}

/// Exchange the refresh token for a new access token. `None` when there is
/// nothing to redeem it with or Google refuses — never an error to the UI.
fn refresh_gemini_token(creds: &serde_json::Value) -> Option<FreshToken> {
    let refresh_token = gemini_refresh_token(creds)?;
    let client_id = gemini_client_id(creds)?;
    let mut form = vec![
        ("client_id", client_id),
        ("grant_type", "refresh_token".to_string()),
        ("refresh_token", refresh_token),
    ];
    // Public clients have no secret; an empty one must not be sent.
    let secret = gemini_client_secret(creds);
    if !secret.trim().is_empty() {
        form.insert(1, ("client_secret", secret));
    }
    let v = http_post_form(GOOGLE_TOKEN_URL, &form).ok()?;
    let access_token = v
        .get("access_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())?
        .to_string();
    let expires_in = as_f64(v.get("expires_in"))
        .map(|s| s as i64)
        .filter(|s| *s > 0)
        .unwrap_or(3600);
    let refresh_token = v
        .get("refresh_token")
        .and_then(|t| t.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    Some(FreshToken {
        access_token,
        expires_in,
        refresh_token,
    })
}

/// Write a renewed token back into `oauth_creds.json`, leaving every other
/// field — the refresh token above all — exactly as the CLI left it. The write
/// goes to a sibling temp file and is renamed over the original, so an
/// interrupted run cannot leave a half-written login behind.
fn update_gemini_creds(
    path: &std::path::Path,
    fresh: &FreshToken,
    now_ms: i64,
) -> Result<serde_json::Value, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut creds: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let Some(obj) = creds.as_object_mut() else {
        return Err("oauth_creds.json is not a JSON object".into());
    };
    obj.insert(
        "access_token".into(),
        serde_json::Value::String(fresh.access_token.clone()),
    );
    obj.insert(
        "expiry_date".into(),
        serde_json::json!(now_ms + fresh.expires_in * 1000),
    );
    if let Some(token) = &fresh.refresh_token {
        obj.insert(
            "refresh_token".into(),
            serde_json::Value::String(token.clone()),
        );
    }
    let body = serde_json::to_string_pretty(&creds).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, format!("{body}\n")).map_err(|e| e.to_string())?;
    // The login file is private to the user; a fresh temp file would not be.
    if let Ok(meta) = fs::metadata(path) {
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })?;
    Ok(creds)
}

/// POST to the Cloud Code Assist API with the session's token, renewing it once
/// if the call comes back 401.
fn gemini_post(
    session: &mut GeminiSession,
    url: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value, HttpError> {
    let headers = [("Authorization", format!("Bearer {}", session.token()))];
    match http_post_json(url, &headers, body) {
        Err(HttpError::Auth(code)) if !session.refreshed => {
            if !session.refresh() {
                return Err(HttpError::Auth(code));
            }
            let headers = [("Authorization", format!("Bearer {}", session.token()))];
            http_post_json(url, &headers, body)
        }
        other => other,
    }
}

pub fn fetch_gemini() -> ServiceStatus {
    let base = ServiceStatus::new("gemini", GEMINI_LABEL, State::NotConnected);
    let missing = "Sign in to Gemini CLI once (run `gemini`).";
    let Some(dir) = gemini_home() else {
        return base.with_note(missing);
    };
    if let Some(mode) = gemini_auth_mode(&dir).filter(|m| gemini_mode_blocks_quota(m)) {
        return base.with_note(format!(
            "Gemini CLI is signed in with {mode}, which has no usage endpoint — sign in with your Google account to see quota."
        ));
    }
    let Some(mut session) = GeminiSession::load(dir.join("oauth_creds.json")) else {
        return base.with_note(missing);
    };
    // Renew first when the file already says the token is done: cheaper than a
    // request that is certain to come back 401.
    if session.needs_refresh() {
        session.refresh();
    }

    let expired = "Login expired — run `gemini` once to refresh it.";
    let (project, plan) = match gemini_post(
        &mut session,
        &format!("{GEMINI_API}:loadCodeAssist"),
        &serde_json::json!({}),
    ) {
        Ok(v) => parse_gemini_assist(&v),
        Err(HttpError::Auth(_)) => {
            return ServiceStatus {
                state: State::Expired,
                ..base
            }
            .with_note(expired)
        }
        // The tier is a nicety; the quota call below is what matters.
        Err(_) => (None, None),
    };
    let body = serde_json::json!({
        "project": project.unwrap_or_else(|| GEMINI_PLACEHOLDER_PROJECT.to_string())
    });
    let v = match gemini_post(
        &mut session,
        &format!("{GEMINI_API}:retrieveUserQuota"),
        &body,
    ) {
        Ok(v) => v,
        Err(HttpError::Auth(_)) => {
            return ServiceStatus {
                state: State::Expired,
                plan,
                ..base
            }
            .with_note(expired)
        }
        Err(e) => {
            return ServiceStatus {
                state: State::Unavailable,
                plan,
                ..base
            }
            .with_note(format!("Couldn't reach Gemini ({}).", e.describe()))
        }
    };
    let windows = parse_gemini_quota(&v);
    if windows.is_empty() {
        return ServiceStatus {
            state: State::Unavailable,
            plan,
            ..base
        }
        .with_note("Gemini returned a response we don't understand yet.");
    }
    ServiceStatus {
        state: State::Connected,
        plan,
        windows,
        ..base
    }
}

/// `loadCodeAssist` → the Cloud project to ask quotas about, and the plan.
fn parse_gemini_assist(v: &serde_json::Value) -> (Option<String>, Option<String>) {
    let project = match v.get("cloudaicompanionProject") {
        Some(serde_json::Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(obj @ serde_json::Value::Object(_)) => obj
            .get("id")
            .and_then(|i| i.as_str())
            .filter(|s| !s.is_empty())
            .map(str::to_string),
        _ => None,
    };
    let mut tier_names: Vec<&str> = Vec::new();
    for key in ["paidTier", "currentTier"] {
        let Some(tier) = v.get(key) else { continue };
        for field in ["name", "id", "displayName", "tier"] {
            if let Some(s) = tier.get(field).and_then(|s| s.as_str()) {
                tier_names.push(s);
            }
        }
    }
    let plan = tier_names.iter().find_map(|n| gemini_tier_name(n));
    (project, plan)
}

fn gemini_tier_name(tier: &str) -> Option<String> {
    let t = tier.to_ascii_lowercase();
    for (needle, name) in [
        ("ultra", "Ultra"),
        ("pro", "Pro"),
        ("standard", "Standard"),
        ("free", "Free"),
    ] {
        if t.contains(needle) {
            return Some(name.to_string());
        }
    }
    None
}

/// `retrieveUserQuota` → one window for the tightest Pro model and one for
/// the tightest Flash model. Buckets with no recognisable family fall back to
/// a single overall "period" window.
fn parse_gemini_quota(v: &serde_json::Value) -> Vec<UsageWindow> {
    let Some(buckets) = v.get("buckets").and_then(|b| b.as_array()) else {
        return Vec::new();
    };
    // (remaining fraction, reset) of the most depleted bucket per family.
    let mut pro: Option<(f64, Option<i64>)> = None;
    let mut flash: Option<(f64, Option<i64>)> = None;
    let mut overall: Option<(f64, Option<i64>)> = None;
    for b in buckets {
        let Some(remaining) = as_f64(b.get("remainingFraction")) else {
            continue;
        };
        let entry = (remaining.clamp(0.0, 1.0), as_timestamp(b.get("resetTime")));
        let model = b
            .get("modelId")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let keep_lower = |slot: &mut Option<(f64, Option<i64>)>| {
            if slot.map_or(true, |(f, _)| entry.0 < f) {
                *slot = Some(entry);
            }
        };
        keep_lower(&mut overall);
        if model.contains("pro") {
            keep_lower(&mut pro);
        } else if model.contains("flash") {
            keep_lower(&mut flash);
        }
    }
    let window = |kind: &'static str, (remaining, reset): (f64, Option<i64>)| UsageWindow {
        kind,
        used_percent: ((1.0 - remaining) * 100.0).clamp(0.0, 100.0),
        resets_at: reset,
        window_seconds: None,
    };
    match (pro, flash) {
        (None, None) => overall
            .map(|e| vec![window("period", e)])
            .unwrap_or_default(),
        (p, f) => p
            .map(|e| window("gemini_pro", e))
            .into_iter()
            .chain(f.map(|e| window("gemini_flash", e)))
            .collect(),
    }
}

// ---------------------------------------------------------------- Copilot
//
// The Copilot CLI and every official Copilot client read their allowance from
// `copilot_internal/user`, authenticated with the GitHub OAuth token the user
// already has. Copilot moved to monthly AI credits, so the number that
// matters is `premium_interactions`; `chat` only exists on some plans.

const COPILOT_LABEL: &str = "Copilot";

fn github_config_dir() -> Option<PathBuf> {
    env_nonempty("GH_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config").join("gh")))
}

/// Where a GitHub OAuth token may already be sitting, most likely first.
fn copilot_token_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(dir) = github_config_dir() {
        paths.push(dir.join("hosts.yml"));
    }
    if cfg!(windows) {
        if let Some(appdata) = env_nonempty("APPDATA") {
            paths.push(PathBuf::from(appdata).join("GitHub CLI").join("hosts.yml"));
        }
        if let Some(local) = env_nonempty("LOCALAPPDATA") {
            paths.push(
                PathBuf::from(local)
                    .join("github-copilot")
                    .join("hosts.json"),
            );
        }
    } else if let Some(home) = home_dir() {
        paths.push(home.join(".config/github-copilot/hosts.json"));
        paths.push(home.join(".config/github-copilot/apps.json"));
        paths.push(home.join("Library/Application Support/github-copilot/hosts.json"));
    }
    paths
}

/// `oauth_token: ghp_…` from one `gh` host block (quotes optional).
fn yaml_oauth_token(text: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim_start();
        if let Some(rest) = line.strip_prefix("oauth_token:") {
            let token = rest.trim().trim_matches(['"', '\'']).trim();
            if !token.is_empty() {
                return Some(token.to_string());
            }
        }
    }
    None
}

fn copilot_token() -> Option<String> {
    if let Some(t) = env_nonempty("GH_TOKEN").or_else(|| env_nonempty("GITHUB_TOKEN")) {
        return Some(t);
    }
    for path in copilot_token_paths() {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.ends_with(".yml") || name.ends_with(".yaml") {
            if let Some(t) = fs::read_to_string(&path)
                .ok()
                .and_then(|s| yaml_oauth_token(&s))
            {
                return Some(t);
            }
        } else if let Some(v) = read_json_file(&path) {
            if let Some(t) = find_string(&v, &["oauth_token"]) {
                return Some(t.to_string());
            }
        }
    }
    // Last resort: ask the CLI, but never wait on it for long.
    run_with_timeout("gh", &["auth", "token"], Duration::from_secs(4))
}

pub fn fetch_copilot() -> ServiceStatus {
    let base = ServiceStatus::new("copilot", COPILOT_LABEL, State::NotConnected);
    let Some(token) = copilot_token() else {
        return base
            .with_note("Sign in with the GitHub CLI (run `gh auth login`) or the Copilot CLI.");
    };
    let v = match http_get_json(
        "https://api.github.com/copilot_internal/user",
        &[
            ("Authorization", format!("Bearer {token}")),
            ("Accept", "application/json".to_string()),
            ("X-GitHub-Api-Version", "2022-11-28".to_string()),
        ],
    ) {
        Ok(v) => v,
        Err(HttpError::Auth(_)) => {
            return ServiceStatus {
                state: State::Expired,
                ..base
            }
            .with_note("Login expired — run `gh auth login` again.")
        }
        Err(e) => {
            return ServiceStatus {
                state: State::Unavailable,
                ..base
            }
            .with_note(format!("Couldn't reach Copilot ({}).", e.describe()))
        }
    };
    let plan = v
        .get("copilot_plan")
        .and_then(|p| p.as_str())
        .filter(|p| !p.is_empty())
        .map(title_case);
    if copilot_subscription_ended(&v) {
        return ServiceStatus {
            state: State::Expired,
            plan,
            ..base
        }
        .with_note("Your Copilot subscription has ended — resubscribe to see usage.");
    }
    let resets_at = as_timestamp(v.get("quota_reset_date_utc"));
    let windows = parse_copilot(&v, resets_at);
    if windows.is_empty() {
        return ServiceStatus {
            state: State::Unavailable,
            plan,
            ..base
        }
        .with_note("Copilot returned a response we don't understand yet.");
    }
    ServiceStatus {
        state: State::Connected,
        plan,
        windows,
        ..base
    }
}

fn copilot_subscription_ended(v: &serde_json::Value) -> bool {
    v.get("access_type_sku").and_then(|s| s.as_str()) == Some("subscription_ended")
}

fn parse_copilot(v: &serde_json::Value, resets_at: Option<i64>) -> Vec<UsageWindow> {
    let Some(snapshots) = v.get("quota_snapshots").and_then(|s| s.as_object()) else {
        return Vec::new();
    };
    let unlimited = |s: &serde_json::Value| s.get("unlimited").and_then(as_bool).unwrap_or(false);
    // A plan with no cap at all still has a row; it is simply never used up.
    if !snapshots.is_empty() && snapshots.values().all(unlimited) {
        return vec![UsageWindow {
            kind: "premium",
            used_percent: 0.0,
            resets_at,
            window_seconds: None,
        }];
    }
    let window = |kind: &'static str, remaining: f64| UsageWindow {
        kind,
        used_percent: (100.0 - remaining).clamp(0.0, 100.0),
        resets_at,
        window_seconds: None,
    };
    let mut windows = Vec::new();
    if let Some(p) = snapshots
        .get("premium_interactions")
        .filter(|p| !unlimited(p))
    {
        if let Some(remaining) = as_f64(p.get("percent_remaining")) {
            windows.push(window("premium", remaining));
        }
    }
    if let Some(c) = snapshots.get("chat").filter(|c| !unlimited(c)) {
        let has_quota = c.get("has_quota").and_then(as_bool).unwrap_or(false);
        if has_quota {
            if let Some(remaining) = as_f64(c.get("percent_remaining")) {
                windows.push(window("chat", remaining));
            }
        }
    }
    windows
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
        window_seconds: (secs > 0).then_some(secs),
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
        mk(
            "claude",
            "Claude",
            "Max",
            vec![
                w("session", 42.0, 2 * 3600 + 14 * 60, 5 * 3600),
                w("weekly", 26.0, 3 * 86_400 + 5 * 3600, 7 * 86_400),
            ],
        ),
        mk(
            "codex",
            CODEX_LABEL,
            "Plus",
            vec![
                w("session", 37.0, 58 * 60, 5 * 3600),
                w("weekly", 18.0, 5 * 86_400, 7 * 86_400),
            ],
        ),
        mk(
            "grok",
            "Grok",
            "SuperGrok",
            vec![w("weekly", 57.0, 4 * 86_400 + 2 * 3600, 7 * 86_400)],
        ),
        mk(
            "gemini",
            GEMINI_LABEL,
            "Ultra",
            vec![
                w("gemini_pro", 34.0, 5 * 3600 + 20 * 60, 0),
                w("gemini_flash", 12.0, 5 * 3600 + 20 * 60, 0),
            ],
        ),
        mk(
            "copilot",
            COPILOT_LABEL,
            "Individual",
            vec![
                w("premium", 61.0, 11 * 86_400, 0),
                w("chat", 23.0, 11 * 86_400, 0),
            ],
        ),
    ]
}

// ------------------------------------------------- in-app web sign-in
//
// Claude (claude.ai — the same account the Claude desktop app uses) and
// Grok (grok.com — the same account as the Grok apps) can also be read
// through an in-app sign-in window. The requests run inside a hidden
// webview on the service's own site; that webview hands the JSON / bytes
// back here. These functions turn that report into a row.

fn web_auth_failed(report: &serde_json::Value) -> bool {
    report.get("auth").and_then(|a| a.as_bool()) == Some(false)
}

/// Report from the claude.ai webview:
/// `{usage: <GET /api/organizations/{org}/usage>, capabilities: [...]}`,
/// `{auth: false}` or `{error: "..."}`.
pub fn claude_from_web(report: &serde_json::Value) -> ServiceStatus {
    let base = ServiceStatus {
        source: "web",
        ..ServiceStatus::new("claude", "Claude", State::NotConnected)
    };
    if web_auth_failed(report) {
        return ServiceStatus {
            state: State::Expired,
            ..base
        }
        .with_note("claude.ai sign-in expired.");
    }
    let Some(usage) = report.get("usage").filter(|u| u.is_object()) else {
        let why = report
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("no data");
        return ServiceStatus {
            state: State::Unavailable,
            ..base
        }
        .with_note(format!("claude.ai: {why}"));
    };
    let caps: Vec<&str> = report
        .get("capabilities")
        .and_then(|c| c.as_array())
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let plan = if caps.iter().any(|c| c.contains("max")) {
        Some("Max".to_string())
    } else if caps.iter().any(|c| c.contains("pro")) {
        Some("Pro".to_string())
    } else if caps.iter().any(|c| c.contains("enterprise")) {
        Some("Enterprise".to_string())
    } else if caps
        .iter()
        .any(|c| c.contains("team") || c.contains("raven"))
    {
        Some("Team".to_string())
    } else {
        None
    };
    let windows = parse_claude(usage);
    if windows.is_empty() {
        return ServiceStatus {
            state: State::Unavailable,
            plan,
            ..base
        }
        .with_note("claude.ai returned a response we don't understand yet.");
    }
    ServiceStatus {
        state: State::Connected,
        plan,
        windows,
        ..base
    }
}

/// Report from the grok.com webview: `{b64: <GetGrokCreditsConfig gRPC-web
/// response>, grpc: "<grpc-status>"}`, `{auth: false}` or `{error}`.
pub fn grok_from_web(report: &serde_json::Value, now: i64) -> ServiceStatus {
    let base = ServiceStatus {
        source: "web",
        ..ServiceStatus::new("grok", "Grok", State::NotConnected)
    };
    if web_auth_failed(report) {
        return ServiceStatus {
            state: State::Expired,
            ..base
        }
        .with_note("grok.com sign-in expired.");
    }
    let grpc = report.get("grpc").and_then(|g| g.as_str()).unwrap_or("0");
    if grpc == "16" || grpc == "7" {
        return ServiceStatus {
            state: State::Expired,
            ..base
        }
        .with_note("grok.com sign-in expired.");
    }
    let bytes = report
        .get("b64")
        .and_then(|b| b.as_str())
        .and_then(base64_decode);
    let Some(bytes) = bytes else {
        let why = report
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("no data");
        return ServiceStatus {
            state: State::Unavailable,
            ..base
        }
        .with_note(format!("grok.com: {why}"));
    };
    match parse_grok_billing_proto(&bytes, now) {
        Some(w) => ServiceStatus {
            state: State::Connected,
            plan: Some("SuperGrok".into()),
            windows: vec![w],
            ..base
        },
        None => ServiceStatus {
            state: State::Unavailable,
            ..base
        }
        .with_note("grok.com returned a response we don't understand yet."),
    }
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[derive(Default)]
struct ProtoScan {
    fixed32: Vec<(Vec<u64>, f32)>,
    varints: Vec<(Vec<u64>, u64)>,
}

fn read_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in 0..10 {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= ((byte & 0x7f) as u64) << (shift * 7);
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Walk a protobuf message without a schema. Returns false when `b` is not
/// a well-formed message (so the caller treats it as a plain byte string).
fn scan_proto(b: &[u8], path: &mut Vec<u64>, out: &mut ProtoScan, depth: u32) -> bool {
    if depth > 8 {
        return false;
    }
    let mut local = ProtoScan::default();
    let mut i = 0;
    while i < b.len() {
        let Some(key) = read_varint(b, &mut i) else {
            return false;
        };
        let (field, wire) = (key >> 3, key & 7);
        if field == 0 {
            return false;
        }
        path.push(field);
        match wire {
            0 => match read_varint(b, &mut i) {
                Some(v) => local.varints.push((path.clone(), v)),
                None => {
                    path.pop();
                    return false;
                }
            },
            1 => {
                if i + 8 > b.len() {
                    path.pop();
                    return false;
                }
                i += 8;
            }
            2 => {
                let Some(len) = read_varint(b, &mut i) else {
                    path.pop();
                    return false;
                };
                let len = len as usize;
                if i + len > b.len() {
                    path.pop();
                    return false;
                }
                let mut sub = ProtoScan::default();
                if len > 0 && scan_proto(&b[i..i + len], path, &mut sub, depth + 1) {
                    local.fixed32.extend(sub.fixed32);
                    local.varints.extend(sub.varints);
                }
                i += len;
            }
            5 => {
                if i + 4 > b.len() {
                    path.pop();
                    return false;
                }
                let v = f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
                local.fixed32.push((path.clone(), v));
                i += 4;
            }
            _ => {
                path.pop();
                return false;
            }
        }
        path.pop();
    }
    out.fixed32.extend(local.fixed32);
    out.varints.extend(local.varints);
    true
}

/// `GetGrokCreditsConfig` response (gRPC-web frames or a raw message):
/// the used percent is the float at field path …/1 closest to the root
/// (normally `[1, 1]`), the reset the timestamp at `[1, 5, 1]`.
fn parse_grok_billing_proto(data: &[u8], now: i64) -> Option<UsageWindow> {
    let mut payloads: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    let mut framed = true;
    while i < data.len() {
        if i + 5 > data.len() {
            framed = false;
            break;
        }
        let flags = data[i];
        if flags != 0 && flags != 0x80 {
            framed = false;
            break;
        }
        let len = u32::from_be_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]) as usize;
        let (start, end) = (i + 5, i + 5 + len);
        if end > data.len() {
            framed = false;
            break;
        }
        if flags == 0 {
            payloads.push(&data[start..end]);
        }
        i = end;
    }
    if !framed || payloads.is_empty() {
        payloads = vec![data];
    }
    let mut scan = ProtoScan::default();
    for p in payloads {
        scan_proto(p, &mut Vec::new(), &mut scan, 0);
    }
    let percent = scan
        .fixed32
        .iter()
        .filter(|(p, v)| p.last() == Some(&1) && v.is_finite() && (0.0..=100.0).contains(v))
        .min_by_key(|(p, _)| p.len())
        .map(|(_, v)| *v as f64);
    let future: Vec<(&Vec<u64>, i64)> = scan
        .varints
        .iter()
        .filter(|(_, v)| (1_700_000_000..=2_100_000_000).contains(v))
        .map(|(p, v)| (p, *v as i64))
        .filter(|(_, t)| *t > now)
        .collect();
    let reset = future
        .iter()
        .filter(|(p, _)| p.as_slice() == [1, 5, 1])
        .map(|(_, t)| *t)
        .min()
        .or_else(|| future.iter().map(|(_, t)| *t).min());
    let used = match (percent, reset) {
        (Some(p), _) => p,
        // A current period but no usage scalar: nothing used yet (proto3 omits 0).
        (None, Some(_)) if scan.fixed32.is_empty() => 0.0,
        _ => return None,
    };
    let secs = reset.map(|r| r - now);
    let kind = match secs {
        Some(s) if s <= 8 * 86_400 => "weekly",
        _ => "period",
    };
    Some(UsageWindow {
        kind,
        used_percent: used.clamp(0.0, 100.0),
        resets_at: reset,
        window_seconds: if kind == "weekly" {
            Some(7 * 86_400)
        } else {
            None
        },
    })
}

pub fn fetch_one(id: &str) -> Option<ServiceStatus> {
    match id {
        "claude" => Some(fetch_claude()),
        "codex" => Some(fetch_codex()),
        "grok" => Some(fetch_grok()),
        "gemini" => Some(fetch_gemini()),
        "copilot" => Some(fetch_copilot()),
        _ => None,
    }
}

pub const IDS: [&str; 5] = ["claude", "codex", "grok", "gemini", "copilot"];

/// Fetch every service in parallel. `mock` forces demo data.
pub fn fetch_all(mock: bool) -> Vec<ServiceStatus> {
    if mock {
        return mock_status();
    }
    std::thread::scope(|s| {
        let handles: Vec<_> = IDS
            .iter()
            .map(|id| s.spawn(move || fetch_one(id)))
            .collect();
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
        assert_eq!(
            parse_rfc3339("2026-02-08T12:59:59.481+08:00"),
            Some(1_770_526_799)
        );
        assert_eq!(
            parse_rfc3339("2026-02-07T23:59:59-05:00"),
            Some(1_770_526_799)
        );
        assert_eq!(
            parse_rfc3339("2026-02-08T04:59:59+00:00"),
            Some(1_770_526_799)
        );
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
    fn grok_proto() {
        // [1]{ [1] float 37.5, [5]{ [1] varint 1795672800 } }
        let mut inner = vec![0x0d];
        inner.extend_from_slice(&37.5f32.to_le_bytes());
        let mut ts = vec![0x08];
        let mut v: u64 = 1_795_672_800;
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                ts.push(b);
                break;
            } else {
                ts.push(b | 0x80)
            }
        }
        inner.push(0x2a);
        inner.push(ts.len() as u8);
        inner.extend(ts);
        let mut msg = vec![0x0a, inner.len() as u8];
        msg.extend(inner);
        let mut frame = vec![0, 0, 0, 0, msg.len() as u8];
        frame.extend(&msg);
        frame.extend([0x80, 0, 0, 0, 0x0f]);
        frame.extend(b"grpc-status:0\r\n");
        let w = parse_grok_billing_proto(&frame, 1_795_200_000).unwrap();
        assert_eq!(w.used_percent, 37.5);
        assert_eq!(w.resets_at, Some(1_795_672_800));
        assert_eq!(w.kind, "weekly");
        // through the report path, base64-encoded
        let b64 = {
            const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut s = String::new();
            for c in frame.chunks(3) {
                let n = (c[0] as u32) << 16
                    | (*c.get(1).unwrap_or(&0) as u32) << 8
                    | *c.get(2).unwrap_or(&0) as u32;
                for k in 0..4 {
                    if k <= c.len() {
                        s.push(T[(n >> (18 - 6 * k) & 63) as usize] as char)
                    } else {
                        s.push('=')
                    }
                }
            }
            s
        };
        let st = grok_from_web(&serde_json::json!({"b64": b64, "grpc": "0"}), 1_795_200_000);
        assert_eq!(st.state, State::Connected);
        assert_eq!(st.source, "web");
        assert_eq!(st.windows[0].used_percent, 37.5);
        let st = grok_from_web(&serde_json::json!({"auth": false}), 0);
        assert_eq!(st.state, State::Expired);
    }

    #[test]
    fn claude_web_report() {
        let r = serde_json::json!({
            "capabilities": ["chat", "claude_max"],
            "usage": {
                "five_hour": {"utilization": 42.0, "resets_at": "2026-11-20T10:00:00+00:00"},
                "seven_day": {"utilization": 26.0, "resets_at": "2026-11-24T08:00:00+00:00"},
                "seven_day_opus": null
            }
        });
        let st = claude_from_web(&r);
        assert_eq!(st.state, State::Connected);
        assert_eq!(st.plan.as_deref(), Some("Max"));
        assert_eq!(st.windows.len(), 2);
        assert_eq!(st.source, "web");
        assert_eq!(
            claude_from_web(&serde_json::json!({"auth": false})).state,
            State::Expired
        );
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
        assert_eq!(
            st.fetched_at,
            parse_rfc3339("2026-11-20T06:00:00Z").unwrap()
        );
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

    // ------------------------------------------------------------- Gemini

    #[test]
    fn gemini_plan_and_project() {
        let v = json!({
            "cloudaicompanionProject": "my-project-123",
            "currentTier": {"id": "standard-tier", "name": "Gemini Code Assist"},
            "paidTier": {"id": "g1-pro-tier", "name": "Gemini Code Assist Pro"}
        });
        let (project, plan) = parse_gemini_assist(&v);
        assert_eq!(project.as_deref(), Some("my-project-123"));
        assert_eq!(
            plan.as_deref(),
            Some("Pro"),
            "a paid tier wins over the current one"
        );
        // A project sent as an object, and a free plan.
        let v = json!({
            "cloudaicompanionProject": {"id": "p-2"},
            "currentTier": {"name": "Free tier"}
        });
        assert_eq!(
            parse_gemini_assist(&v),
            (Some("p-2".into()), Some("Free".into()))
        );
        // Ultra is checked before Pro so "Ultra" can never read as "Pro".
        let v = json!({"paidTier": {"name": "Gemini Ultra"}});
        assert_eq!(parse_gemini_assist(&v).1.as_deref(), Some("Ultra"));
        // Nothing recognisable: no plan rather than a made-up one.
        assert_eq!(parse_gemini_assist(&json!({})), (None, None));
    }

    #[test]
    fn gemini_buckets_group_by_family() {
        let v = json!({
            "buckets": [
                {"modelId": "gemini-2.5-pro", "remainingFraction": 0.58, "resetTime": "2026-11-20T18:00:00Z"},
                {"modelId": "gemini-3-pro-preview", "remainingFraction": 0.90, "resetTime": "2026-11-20T18:00:00Z"},
                {"modelId": "gemini-2.5-flash", "remainingFraction": 0.12, "resetTime": "2026-11-20T18:00:00Z"},
                {"modelId": "gemini-2.5-flash-lite", "remainingFraction": 0.30, "resetTime": "2026-11-20T18:00:00Z"}
            ]
        });
        let w = parse_gemini_quota(&v);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].kind, "gemini_pro");
        assert!(
            (w[0].used_percent - 42.0).abs() < 1e-9,
            "the most depleted Pro model is the row's number"
        );
        assert_eq!(w[0].resets_at, parse_rfc3339("2026-11-20T18:00:00Z"));
        assert_eq!(w[1].kind, "gemini_flash");
        assert!((w[1].used_percent - 88.0).abs() < 1e-9);
    }

    #[test]
    fn gemini_edges() {
        // Only one family present: just that window.
        let w = parse_gemini_quota(&json!({"buckets": [
            {"modelId": "gemini-2.5-flash", "remainingFraction": 1.0, "resetTime": null}
        ]}));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "gemini_flash");
        assert_eq!(w[0].used_percent, 0.0);
        assert_eq!(w[0].resets_at, None);
        // Unknown models (or no modelId at all): one overall window.
        let w = parse_gemini_quota(&json!({"buckets": [
            {"modelId": "mystery", "remainingFraction": 0.25, "resetTime": "2026-11-21T00:00:00Z"},
            {"remainingFraction": 0.75}
        ]}));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "period");
        assert_eq!(w[0].used_percent, 75.0);
        // Nothing usable: empty, so the row says so instead of showing 0%.
        assert!(parse_gemini_quota(&json!({})).is_empty());
        assert!(parse_gemini_quota(&json!({"buckets": []})).is_empty());
        assert!(
            parse_gemini_quota(&json!({"buckets": [{"modelId": "gemini-2.5-pro"}]})).is_empty()
        );
    }

    #[test]
    fn gemini_api_key_and_vertex_modes_are_not_quota_logins() {
        assert!(gemini_mode_blocks_quota("gemini-api-key"));
        assert!(gemini_mode_blocks_quota("vertex-ai"));
        assert!(gemini_mode_blocks_quota("API_KEY"));
        assert!(!gemini_mode_blocks_quota("oauth-personal"));
        assert!(!gemini_mode_blocks_quota(""));
    }

    // ------------------------------------------------ Gemini token refresh

    #[test]
    fn gemini_token_expiry_detection() {
        let now = 1_700_000_000_000;
        assert!(
            !gemini_token_stale(now, None),
            "an unknown expiry is renewed only after a 401, never guessed at"
        );
        assert!(!gemini_token_stale(now, Some(now + 3_600_000)));
        assert!(
            !gemini_token_stale(now, Some(now + 121_000)),
            "just outside the margin"
        );
        assert!(
            gemini_token_stale(now, Some(now + 120_000)),
            "the margin itself counts as due"
        );
        assert!(gemini_token_stale(now, Some(now + 60_000)));
        assert!(gemini_token_stale(now, Some(now)));
        assert!(gemini_token_stale(now, Some(now - 86_400_000)));
    }

    #[test]
    fn gemini_expiry_reads_ms_seconds_and_iso() {
        let now = 1_700_000_000_000;
        assert_eq!(gemini_expiry_ms(&json!({"expiry_date": now})), Some(now));
        assert_eq!(
            gemini_expiry_ms(&json!({"expiry_date": 1_700_000_000})),
            Some(now),
            "seconds are read as seconds"
        );
        let iso = parse_rfc3339("2026-11-20T18:00:00Z").unwrap();
        assert_eq!(
            gemini_expiry_ms(&json!({"expiry_date": "2026-11-20T18:00:00Z"})),
            Some(iso * 1000)
        );
        assert_eq!(gemini_expiry_ms(&json!({"expires_at": now})), Some(now));
        assert_eq!(gemini_expiry_ms(&json!({"access_token": "a"})), None);
    }

    #[test]
    fn gemini_refresh_rewrites_the_login_and_keeps_every_other_field() {
        let dir = std::env::temp_dir().join(format!("aum-gemini-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("oauth_creds.json");
        fs::write(
            &path,
            r#"{"access_token":"old","refresh_token":"rt","scope":"s","token_type":"Bearer","id_token":"jwt","expiry_date":1000}"#,
        )
        .unwrap();
        let now = 1_700_000_000_000;
        let fresh = FreshToken {
            access_token: "new".into(),
            expires_in: 3600,
            refresh_token: None,
        };
        let returned = update_gemini_creds(&path, &fresh, now).unwrap();
        let on_disk: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for doc in [&returned, &on_disk] {
            assert_eq!(doc["access_token"], "new");
            assert_eq!(doc["refresh_token"], "rt", "the refresh token is untouched");
            assert_eq!(doc["id_token"], "jwt");
            assert_eq!(doc["scope"], "s");
            assert_eq!(doc["token_type"], "Bearer");
            assert_eq!(doc["expiry_date"].as_i64(), Some(now + 3_600_000));
        }
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temp file is renamed over the original, not left behind"
        );

        // A rotated refresh token replaces the old one.
        let fresh = FreshToken {
            access_token: "newer".into(),
            expires_in: 60,
            refresh_token: Some("rt2".into()),
        };
        update_gemini_creds(&path, &fresh, now).unwrap();
        let on_disk: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(on_disk["refresh_token"], "rt2");
        assert_eq!(on_disk["expiry_date"].as_i64(), Some(now + 60_000));

        // Anything but an object is refused rather than overwritten.
        fs::write(&path, "[]").unwrap();
        assert!(update_gemini_creds(&path, &fresh, now).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[]");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn gemini_without_a_refresh_token_stays_quiet() {
        let creds = json!({"access_token": "a", "expiry_date": 0});
        assert_eq!(gemini_refresh_token(&creds), None);
        assert!(
            refresh_gemini_token(&creds).is_none(),
            "nothing to send means no request and no error"
        );
        let mut session = GeminiSession {
            path: PathBuf::from("/nonexistent/oauth_creds.json"),
            creds,
            token: "a".into(),
            refreshed: false,
        };
        assert!(!session.refresh());
        assert!(session.refreshed, "the attempt is remembered");
        assert_eq!(session.token(), "a", "the old token is left in place");
        assert!(!session.refresh(), "and is not tried twice");
    }

    #[test]
    fn gemini_client_prefers_the_one_the_file_names() {
        let own = json!({"client_id": "mine", "client_secret": "s3cret"});
        assert_eq!(gemini_client_id(&own).as_deref(), Some("mine"));
        assert_eq!(gemini_client_secret(&own), "s3cret");
        assert_eq!(gemini_access_token(&json!({"access_token": ""})), None);
        // Without a client in the file and none on the machine, there is nothing
        // to redeem the refresh token with: no id, and an empty secret that is
        // not sent at all.
        assert_eq!(gemini_client_secret(&json!({"client_secret": ""})), "");
    }

    /// A client pair with the right shape and nothing behind it.
    fn fake_client() -> (String, String) {
        (
            format!(
                "{}-{}.apps.googleusercontent.com",
                123456789012u64, "abcdefghijklmnopqrstuvwxyz012345"
            ),
            // Spelled in two pieces for the same reason as the id above: the
            // scanner looks for the whole `GOCSPX-…` prefix on one line.
            format!("{}{}", "GOCS", "PX-abcdefghijklmnopqrstuvwx12"),
        )
    }

    #[test]
    fn reads_the_oauth_client_out_of_a_cli_bundle() {
        // Spelled at runtime: a realistic client pair written out in full here
        // would itself look like a leaked credential to a secret scanner.
        let (want_id, want_secret) = fake_client();
        let bundle = format!(
            "const OAUTH_CLIENT_ID={want_id:?}; const OAUTH_CLIENT_SECRET={want_secret:?};"
        );
        let (id, secret) = oauth_client_in(&bundle).expect("both halves are there");
        assert_eq!(id, want_id);
        assert_eq!(secret, want_secret);
        assert_eq!(oauth_client_in("no client here"), None);
        assert_eq!(
            oauth_client_in("x.apps.googleusercontent.com"),
            None,
            "a host without a client id is not a client"
        );
    }

    #[test]
    fn pairs_the_secret_with_the_client_beside_it() {
        // What Gemini CLI's bundle actually holds: a Cloud SDK client id early
        // in the file, then its own auth client and secret declared together.
        let (sdk_id, _) = fake_client();
        let (want_id, want_secret) = fake_client();
        let far_away = "x".repeat(5000);
        let bundle = format!(
            "const CLOUD_SDK_CLIENT_ID={sdk_id:?}; {far_away}\nconst OAUTH_CLIENT_ID={want_id:?}; const OAUTH_CLIENT_SECRET={want_secret:?};"
        );
        let (id, secret) = oauth_client_in(&bundle).expect("a pair is found");
        assert_eq!(id, want_id, "not the unrelated client id earlier in the file");
        assert_eq!(secret, want_secret);
    }

    #[test]
    fn finds_the_package_above_the_bin_entry() {
        let bundle = PathBuf::from(concat!(
            "/opt/homebrew/Cellar/gemini-cli/0.70.0/libexec/lib/node_modules/",
            "@google/gemini-cli/bundle/gemini.js"
        ));
        assert_eq!(
            scoped_gemini_package(&bundle),
            Some(PathBuf::from(
                "/opt/homebrew/Cellar/gemini-cli/0.70.0/libexec/lib/node_modules/@google/gemini-cli"
            ))
        );
        assert_eq!(scoped_gemini_package(&PathBuf::from("/usr/bin/gemini")), None);
    }

    #[test]
    fn finds_the_gemini_cli_package_under_a_versioned_layout() {
        let root = std::env::temp_dir().join(format!("ai-usage-gemini-{}", std::process::id()));
        let pkg = root.join("5/node_modules/@google/gemini-cli/bundle");
        fs::create_dir_all(&pkg).expect("temp layout");
        let (id, secret) = fake_client();
        fs::write(
            pkg.join("gemini.js"),
            format!("var a={id:?}, b={secret:?};"),
        )
        .expect("temp bundle");
        let mut found = Vec::new();
        let mut budget = 4000;
        gemini_cli_packages(&root, 5, &mut found, &mut budget);
        assert_eq!(found.len(), 1, "the versioned layout is found");
        let pair = oauth_client_in_package(&found[0]).expect("the pair is read");
        assert!(pair.0.ends_with("apps.googleusercontent.com"));
        let _ = fs::remove_dir_all(&root);
    }

    // ------------------------------------------------------------ Copilot

    #[test]
    fn copilot_premium_and_chat() {
        let v = json!({
            "copilot_plan": "individual",
            "quota_reset_date_utc": "2026-12-01T00:00:00.000Z",
            "quota_snapshots": {
                "chat": {"unlimited": false, "has_quota": true, "percent_remaining": 77.0},
                "completions": {"unlimited": true, "has_quota": false, "percent_remaining": 100.0},
                "premium_interactions": {"unlimited": false, "has_quota": true, "percent_remaining": 8.4}
            }
        });
        let resets = as_timestamp(v.get("quota_reset_date_utc"));
        assert_eq!(resets, parse_rfc3339("2026-12-01T00:00:00Z"));
        let w = parse_copilot(&v, resets);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].kind, "premium");
        assert!((w[0].used_percent - 91.6).abs() < 1e-9);
        assert_eq!(w[0].resets_at, resets);
        assert_eq!(w[1].kind, "chat");
        assert_eq!(w[1].used_percent, 23.0);
    }

    #[test]
    fn copilot_unlimited_and_quiet_chat() {
        // Chat unlimited is not a number to show; premium carries the row.
        let v = json!({"quota_snapshots": {
            "chat": {"unlimited": true, "has_quota": false, "percent_remaining": 100.0},
            "premium_interactions": {"unlimited": false, "has_quota": true, "percent_remaining": 40.0}
        }});
        let w = parse_copilot(&v, None);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "premium");
        assert_eq!(w[0].used_percent, 60.0);
        // A plan with nothing finite anywhere is simply unused.
        let v = json!({"quota_snapshots": {
            "chat": {"unlimited": true, "percent_remaining": 100.0},
            "premium_interactions": {"unlimited": true, "percent_remaining": 100.0}
        }});
        let w = parse_copilot(&v, Some(1_800_000_000));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "premium");
        assert_eq!(w[0].used_percent, 0.0);
        assert_eq!(w[0].resets_at, Some(1_800_000_000));
        // Chat without a quota, and a payload with no snapshots at all.
        let v = json!({"quota_snapshots": {"chat": {"unlimited": false, "has_quota": false, "percent_remaining": 100.0}}});
        assert!(parse_copilot(&v, None).is_empty());
        assert!(parse_copilot(&json!({}), None).is_empty());
    }

    #[test]
    fn copilot_subscription_state() {
        assert!(copilot_subscription_ended(
            &json!({"access_type_sku": "subscription_ended"})
        ));
        assert!(!copilot_subscription_ended(
            &json!({"access_type_sku": "copilot_standalone_seat_quota"})
        ));
        assert!(!copilot_subscription_ended(&json!({})));
    }

    #[test]
    fn gh_hosts_yml_token() {
        let text =
            "github.com:\n    user: caesar\n    oauth_token: gho_abc123\n    git_protocol: https\n";
        assert_eq!(yaml_oauth_token(text).as_deref(), Some("gho_abc123"));
        let quoted = "github.com:\n    oauth_token: \"ghp_q q\"\n";
        assert_eq!(yaml_oauth_token(quoted).as_deref(), Some("ghp_q q"));
        assert_eq!(yaml_oauth_token("github.com:\n    user: nobody\n"), None);
        assert_eq!(yaml_oauth_token("github.com:\n    oauth_token:   \n"), None);
    }

    #[test]
    fn state_serialises_snake_case() {
        assert_eq!(
            serde_json::to_string(&State::NotConnected).unwrap(),
            "\"not_connected\""
        );
    }
}
