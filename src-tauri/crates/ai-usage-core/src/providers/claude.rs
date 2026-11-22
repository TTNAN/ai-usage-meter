//! Claude: the Claude Code OAuth token, the usage request and its parse.

use super::*;

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

/// What the row says when claude.ai answered, but in a shape this app cannot
/// read: the app is behind, not the login.
const CLAUDE_WEB_SHAPE_NOTE: &str =
    "claude.ai answered in a format this app doesn't recognise — its API has probably changed.";

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
        // The page answered, but with nothing this app can read as usage: the
        // shape moved, so the login is not the problem.
        return ServiceStatus {
            state: State::Changed,
            plan,
            ..base
        }
        .with_note(CLAUDE_WEB_SHAPE_NOTE);
    }
    ServiceStatus {
        state: State::Connected,
        plan,
        windows,
        ..base
    }
}

// A grok.com report. Three ways it can go wrong, and the row must not confuse
// them:
//
// * the session was refused (401/403, or gRPC `UNAUTHENTICATED`) — the
//   account is signed out, so sign in again;
// * the request never got an answer (`{error: "HTTP 502"}`) — retried on the
//   next poll, nothing for the user to do;
// * grok.com answered, but not in a shape this app can read — its API moved,
//   and the app needs updating. Reported as `Changed`, never as "not

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn claude_web_calls_an_unreadable_answer_a_changed_api() {
        // Signed in and answered, but the usage object no longer holds the
        // windows this app reads: the login is fine, the app is behind.
        let st = claude_from_web(&json!({
            "capabilities": ["chat"],
            "usage": {"used_something_else": 1}
        }));
        assert_eq!(st.state, State::Changed);
        assert!(st.note.unwrap().contains("API has probably changed"));
    }
}
