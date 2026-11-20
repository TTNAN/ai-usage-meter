//! Usage providers: Claude, Codex and Grok.
//!
//! All three services expose NO official "how much subscription quota is
//! left" API. v1 therefore reads the same internal endpoints the official
//! CLIs use, reusing the login state those CLIs already stored on disk
//! (the pattern used by the `Quota` project). Only read-only GET requests,
//! at low frequency. If anything fails we report "unavailable" — we never
//! invent numbers.
//!
//! * Claude — GET https://api.anthropic.com/api/oauth/usage
//!   Bearer <Claude Code OAuth token>, header `anthropic-beta: oauth-2025-04-20`.
//!   Token lives in `~/.claude/.credentials.json` (`claudeAiOauth.accessToken`).
//!   Returns `five_hour.utilization` + `resets_at`, `seven_day.*`.
//! * Codex — GET https://chatgpt.com/backend-api/wham/usage
//!   Bearer <Codex OAuth access_token>, header `chatgpt-account-id`.
//!   Token lives in `~/.codex/auth.json` (`tokens.access_token`).
//!   Returns `rate_limit.primary_window` (5h) + `secondary_window` (7d).
//!   NOTE: this is Codex quota, NOT ChatGPT web-chat quota.
//! * Grok — GET https://cli-chat-proxy.grok.com/v1/billing?format=credits
//!   Bearer <token from ~/.grok/auth.json>, header `x-xai-token-auth: xai-grok-cli`.
//!   Returns `config.creditUsagePercent` (used, 0-100) + `currentPeriod`.
//!
//! Tokens are read from disk at call time and never written anywhere by
//! this app. Nothing is uploaded to any server.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One service's usage state, as shown on one card.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceStatus {
    pub id: &'static str,
    pub label: &'static str,
    /// "connected" | "not_connected" | "unavailable" | "mock"
    pub state: &'static str,
    /// Percent USED in the primary window (0-100), if known.
    pub used_percent: Option<f64>,
    /// Percent USED in the secondary (weekly) window, if known.
    pub secondary_used_percent: Option<f64>,
    /// Unix timestamp (seconds) when the primary window resets, if known.
    pub resets_at: Option<i64>,
    /// Unix timestamp (seconds) when the secondary window resets, if known.
    pub secondary_resets_at: Option<i64>,
    /// Short human-readable note (e.g. why unavailable).
    pub note: Option<String>,
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn read_json_file(path: &PathBuf) -> Option<serde_json::Value> {
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Walk a JSON value depth-first and return the first string found under
/// any of the given keys. Credential files change shape between CLI
/// versions; this keeps us working across them.
fn find_string<'a>(v: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    match v {
        serde_json::Value::Object(map) => {
            for (k, val) in map {
                if keys.contains(&k.as_str()) {
                    if let serde_json::Value::String(s) = val {
                        if !s.is_empty() {
                            return Some(s);
                        }
                    }
                }
                if let Some(found) = find_string(val, keys) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|item| find_string(item, keys))
        }
        _ => None,
    }
}

fn http_get_json(url: &str, headers: &[(&str, String)]) -> Result<serde_json::Value, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("ai-usage-meter/0.1.0")
        .build()
        .map_err(|e| format!("http client: {e}"))?;
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(*k, v.as_str());
    }
    let resp = req.send().map_err(|e| format!("request: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    resp.json::<serde_json::Value>()
        .map_err(|e| format!("bad json: {e}"))
}

fn parse_rfc3339(s: &str) -> Option<i64> {
    // Minimal RFC3339 parser: "2026-02-08T04:59:59Z" or with +00:00 offset.
    let s = s.trim();
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let time = time.trim_end_matches('Z');
    let time = time.split(['+', '-']).next().unwrap_or(time);
    let mut t = time.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().unwrap_or("0").split('.').next()?.parse().ok()?;
    // Days since epoch (civil algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(days * 86400 + hour * 3600 + min * 60 + sec)
}

// ---------------------------------------------------------------- Claude

fn claude_token() -> Option<String> {
    if let Ok(t) = std::env::var("CLAUDE_CODE_OAUTH_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    let home = home_dir()?;
    let creds = read_json_file(&home.join(".claude").join(".credentials.json"))?;
    // Official shape: { "claudeAiOauth": { "accessToken": "sk-ant-oat01-..." } }
    let direct = creds
        .get("claudeAiOauth")
        .and_then(|o| o.get("accessToken"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    direct.or_else(|| {
        find_string(&creds, &["accessToken", "access_token"])
            .map(|s| s.to_string())
    })
}

pub fn fetch_claude() -> ServiceStatus {
    let base = ServiceStatus {
        id: "claude",
        label: "Claude",
        state: "not_connected",
        used_percent: None,
        secondary_used_percent: None,
        resets_at: None,
        secondary_resets_at: None,
        note: Some("Claude Code is not logged in. Install Claude Code and run it once.".into()),
    };
    let token = match claude_token() {
        Some(t) => t,
        None => return base,
    };
    let url = "https://api.anthropic.com/api/oauth/usage";
    let res = http_get_json(
        url,
        &[
            ("Authorization", format!("Bearer {token}")),
            ("anthropic-beta", "oauth-2025-04-20".to_string()),
        ],
    );
    match res {
        Ok(v) => {
            let five = v.get("five_hour");
            let seven = v.get("seven_day");
            let used = five.and_then(|f| f.get("utilization")).and_then(|u| u.as_f64());
            let resets = five
                .and_then(|f| f.get("resets_at"))
                .and_then(|r| r.as_str())
                .and_then(parse_rfc3339);
            let seven_used = seven.and_then(|f| f.get("utilization")).and_then(|u| u.as_f64());
            let seven_resets = seven
                .and_then(|f| f.get("resets_at"))
                .and_then(|r| r.as_str())
                .and_then(parse_rfc3339);
            ServiceStatus {
                state: if used.is_some() { "connected" } else { "unavailable" },
                used_percent: used,
                secondary_used_percent: seven_used,
                resets_at: resets,
                secondary_resets_at: seven_resets,
                note: if used.is_none() {
                    Some("Claude returned an unexpected response.".into())
                } else {
                    None
                },
                ..base
            }
        }
        Err(e) => ServiceStatus {
            state: "unavailable",
            note: Some(format!("Could not reach Claude usage API ({e}). Showing nothing rather than stale numbers.")),
            ..base
        },
    }
}

// ----------------------------------------------------------------- Codex

fn codex_auth() -> Option<(String, Option<String>)> {
    if let Ok(t) = std::env::var("CODEX_ACCESS_TOKEN") {
        if !t.is_empty() {
            return Some((t, std::env::var("CHATGPT_ACCOUNT_ID").ok()));
        }
    }
    let home = home_dir()?;
    let auth = read_json_file(&home.join(".codex").join("auth.json"))?;
    // Official shape: { "tokens": { "access_token": "...", "account_id": "..." } }
    let token = auth
        .get("tokens")
        .and_then(|t| t.get("access_token"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| find_string(&auth, &["access_token"]).map(|s| s.to_string()))
        .or_else(|| find_string(&auth, &["OPENAI_API_KEY"]).map(|s| s.to_string()))?;
    let account = auth
        .get("tokens")
        .and_then(|t| t.get("account_id"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .or_else(|| find_string(&auth, &["account_id", "accountId"]).map(|s| s.to_string()));
    Some((token, account))
}

pub fn fetch_codex() -> ServiceStatus {
    let base = ServiceStatus {
        id: "codex",
        label: "Codex",
        state: "not_connected",
        used_percent: None,
        secondary_used_percent: None,
        resets_at: None,
        secondary_resets_at: None,
        note: Some("Codex CLI is not logged in. Install Codex and run `codex` once.".into()),
    };
    let (token, account) = match codex_auth() {
        Some(t) => t,
        None => return base,
    };
    let mut headers: Vec<(&str, String)> = vec![("Authorization", format!("Bearer {token}"))];
    if let Some(a) = account {
        headers.push(("chatgpt-account-id", a));
    }
    let res = http_get_json("https://chatgpt.com/backend-api/wham/usage", &headers);
    match res {
        Ok(v) => {
            let rl = v.get("rate_limit");
            let primary = rl.and_then(|r| r.get("primary_window"));
            let secondary = rl.and_then(|r| r.get("secondary_window"));
            let used = primary
                .and_then(|w| w.get("used_percent"))
                .and_then(|u| u.as_f64());
            let resets = primary
                .and_then(|w| w.get("reset_at"))
                .and_then(|r| r.as_i64());
            let seven_used = secondary
                .and_then(|w| w.get("used_percent"))
                .and_then(|u| u.as_f64());
            let seven_resets = secondary
                .and_then(|w| w.get("reset_at"))
                .and_then(|r| r.as_i64());
            ServiceStatus {
                state: if used.is_some() { "connected" } else { "unavailable" },
                used_percent: used,
                secondary_used_percent: seven_used,
                resets_at: resets,
                secondary_resets_at: seven_resets,
                note: if used.is_none() {
                    Some("Codex returned an unexpected response.".into())
                } else {
                    None
                },
                ..base
            }
        }
        Err(e) => ServiceStatus {
            state: "unavailable",
            note: Some(format!("Could not reach Codex usage API ({e}).")),
            ..base
        },
    }
}

// ------------------------------------------------------------------ Grok

fn grok_token() -> Option<String> {
    if let Ok(t) = std::env::var("GROK_ACCESS_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    let home = home_dir()?;
    let auth = read_json_file(&home.join(".grok").join("auth.json"))?;
    find_string(&auth, &["access_token", "accessToken", "token", "api_key", "apiKey"])
        .map(|s| s.to_string())
}

pub fn fetch_grok() -> ServiceStatus {
    let base = ServiceStatus {
        id: "grok",
        label: "Grok",
        state: "not_connected",
        used_percent: None,
        secondary_used_percent: None,
        resets_at: None,
        secondary_resets_at: None,
        note: Some("Grok CLI is not logged in. Install the Grok CLI and log in once.".into()),
    };
    let token = match grok_token() {
        Some(t) => t,
        None => return base,
    };
    let res = http_get_json(
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
        &[
            ("Authorization", format!("Bearer {token}")),
            ("x-xai-token-auth", "xai-grok-cli".to_string()),
        ],
    );
    match res {
        Ok(v) => {
            let used = v
                .get("config")
                .and_then(|c| c.get("creditUsagePercent"))
                .and_then(|u| u.as_f64())
                .or_else(|| {
                    v.get("creditUsagePercent").and_then(|u| u.as_f64())
                });
            let period_end = v
                .get("currentPeriod")
                .and_then(|p| p.get("end"))
                .and_then(|e| e.as_str())
                .and_then(parse_rfc3339)
                .or_else(|| {
                    v.get("currentPeriod")
                        .and_then(|p| p.get("end"))
                        .and_then(|e| e.as_i64())
                });
            ServiceStatus {
                state: if used.is_some() { "connected" } else { "unavailable" },
                used_percent: used,
                secondary_used_percent: None,
                // Grok's weekly pool resets at the end of the current period.
                resets_at: period_end,
                secondary_resets_at: None,
                note: if used.is_none() {
                    Some("Grok returned an unexpected response.".into())
                } else {
                    None
                },
                ..base
            }
        }
        Err(e) => ServiceStatus {
            state: "unavailable",
            note: Some(format!("Could not reach Grok billing API ({e}).")),
            ..base
        },
    }
}

// ------------------------------------------------------------------ mock

/// Demo data mirroring the numbers in the @Litx4i post, for local testing
/// and CI smoke tests only. The shipped app never shows mock data as real.
pub fn mock_status() -> Vec<ServiceStatus> {
    let now = now_unix();
    vec![
        ServiceStatus {
            id: "claude",
            label: "Claude",
            state: "mock",
            used_percent: Some(42.0),
            secondary_used_percent: Some(26.0),
            resets_at: Some(now + 180),
            secondary_resets_at: Some(now + 86400 * 3),
            note: Some("Demo data — Claude Code is not logged in on this machine.".into()),
        },
        ServiceStatus {
            id: "codex",
            label: "Codex",
            state: "mock",
            used_percent: Some(37.0),
            secondary_used_percent: Some(18.0),
            resets_at: Some(now + 60),
            secondary_resets_at: Some(now + 86400 * 5),
            note: Some("Demo data — Codex CLI is not logged in on this machine.".into()),
        },
        ServiceStatus {
            id: "grok",
            label: "Grok",
            state: "mock",
            used_percent: Some(57.0),
            secondary_used_percent: None,
            resets_at: Some(now + 300),
            secondary_resets_at: None,
            note: Some("Demo data — Grok CLI is not logged in on this machine.".into()),
        },
    ]
}

/// Fetch all three services. `mock` forces demo data.
pub fn fetch_all(mock: bool) -> Vec<ServiceStatus> {
    if mock {
        return mock_status();
    }
    vec![fetch_claude(), fetch_codex(), fetch_grok()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_parses() {
        // 2026-02-08T04:59:59Z
        assert_eq!(parse_rfc3339("2026-02-08T04:59:59Z"), Some(1770526799));
    }

    #[test]
    fn mock_status_has_three_services() {
        let all = mock_status();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "claude");
        assert_eq!(all[1].id, "codex");
        assert_eq!(all[2].id, "grok");
        for s in &all {
            assert!(s.used_percent.is_some());
            assert!(s.resets_at.is_some());
        }
    }

    #[test]
    fn find_string_walks_nested_json() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"tokens":{"access_token":"tok123"}}"#).unwrap();
        assert_eq!(find_string(&v, &["access_token"]), Some("tok123"));
    }
}
