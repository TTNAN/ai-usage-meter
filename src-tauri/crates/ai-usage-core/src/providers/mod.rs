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
    /// The request failed (no network, a timeout, an HTTP error).
    Unavailable,
    /// The service answered, but in a shape this app cannot read: its API has
    /// probably changed and the app needs updating. Kept apart from
    /// `Unavailable` (retried shortly, nothing to do) and from `Expired` (the
    /// login works, so signing in again would not help).
    Changed,
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
    let mut cmd = Command::new(program);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // The app is a GUI process on Windows, so it owns no console and Windows
    // would make a child one — a black box flashing on screen every time
    // `gh auth token` runs. CREATE_NO_WINDOW keeps it invisible; the other
    // platforms have no console to suppress.
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
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

// --------------------------------------------------------------- providers
//
// Each service lives in its own module. Everything they share — the data
// model, HTTP and credential-file helpers — stays here.

mod claude;
mod codex;
mod copilot;
mod gemini;
mod grok;
mod grpc;
#[cfg(test)]
mod test_bytes;

use codex::CODEX_LABEL;
use copilot::COPILOT_LABEL;
use gemini::GEMINI_LABEL;

pub use claude::{claude_from_web, fetch_claude};
pub use codex::fetch_codex;
pub use copilot::fetch_copilot;
pub use gemini::fetch_gemini;
pub use grok::{fetch_grok, grok_from_web};

/// A claude.ai web report with `auth: false` means the saved session expired.
fn web_auth_failed(report: &serde_json::Value) -> bool {
    report.get("auth").and_then(|a| a.as_bool()) == Some(false)
}

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
        assert_eq!(
            serde_json::to_string(&State::NotConnected).unwrap(),
            "\"not_connected\""
        );
        // `src/main.js` reads these strings: a rename has to reach the UI.
        assert_eq!(
            serde_json::to_string(&State::Changed).unwrap(),
            "\"changed\""
        );
        assert_eq!(
            serde_json::to_string(&State::Unavailable).unwrap(),
            "\"unavailable\""
        );
    }
}
