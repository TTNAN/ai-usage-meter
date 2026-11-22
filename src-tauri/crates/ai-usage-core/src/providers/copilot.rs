//! GitHub Copilot: the GitHub OAuth token, the subscription request and its
//! parse.

use super::*;

// matters is `premium_interactions`; `chat` only exists on some plans.

pub(super) const COPILOT_LABEL: &str = "Copilot";

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
