//! ChatGPT / Codex: the local session logs, credentials, request and parse.

use super::*;

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

pub(super) const CODEX_LABEL: &str = "ChatGPT";

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
