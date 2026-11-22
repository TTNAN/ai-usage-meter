//! The lightweight update check: one GET against the GitHub Releases API,
//! compared against this build's own version.
//!
//! Deliberately minimal — never blocks startup, never opens anything on its
//! own. When the check fails (offline, rate limited, an unparseable answer)
//! it reports "unreachable" and that is all; the next manual check or the
//! next launch tries again. The semver maths itself lives in `version.rs`.

use serde::Serialize;

/// The Releases API entry the check reads. `html_url` of the answer is what
/// the UI opens, so this is only the fallback.
const RELEASES_LATEST: &str = "https://api.github.com/repos/TTNAN/ai-usage-meter/releases/latest";
/// The check runs while the user is doing something else; eight seconds is
/// more than the API needs and less than anyone waits for a button.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// What one check found. `reachable: false` means the check itself failed —
/// the footer pill stays hidden, and only Settings says "try again later".
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub has_update: bool,
    pub release_url: Option<String>,
    pub published_at: Option<String>,
    pub reachable: bool,
}

impl UpdateInfo {
    /// The quiet answer: nothing known, nothing to show anywhere.
    fn unreachable() -> Self {
        UpdateInfo {
            current_version: env!("CARGO_PKG_VERSION").to_string(),
            latest_version: None,
            has_update: false,
            release_url: None,
            published_at: None,
            reachable: false,
        }
    }
}

/// Fetch the latest release and compare it with this build. Any failure —
/// offline, HTTP error, rate limit, a body this app cannot read — degrades
/// to the unreachable answer instead of an error the UI would have to catch.
#[tauri::command]
pub async fn check_for_update() -> UpdateInfo {
    match fetch_latest().await {
        Ok(Some(r)) => {
            let current = env!("CARGO_PKG_VERSION");
            let tag = r.tag;
            UpdateInfo {
                current_version: current.to_string(),
                latest_version: Some(tag.clone()),
                has_update: crate::version::update_available(current, &tag),
                release_url: Some(r.url),
                published_at: r.published_at,
                reachable: true,
            }
        }
        _ => UpdateInfo::unreachable(),
    }
}

struct LatestRelease {
    tag: String,
    url: String,
    published_at: Option<String>,
}

async fn fetch_latest() -> Result<Option<LatestRelease>, ()> {
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(concat!("ai-usage-meter/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| ())?;
    let body: serde_json::Value = client
        .get(RELEASES_LATEST)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?
        .json()
        .await
        .map_err(|_| ())?;
    let tag = body
        .get("tag_name")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or(())?;
    Ok(Some(LatestRelease {
        // The tag carries the `v`; the version comparison and the UI text do not.
        tag: tag.trim_start_matches('v').to_string(),
        url: body
            .get("html_url")
            .and_then(serde_json::Value::as_str)
            .filter(|u| u.starts_with("https://"))
            .unwrap_or(RELEASES_LATEST)
            .to_string(),
        published_at: body
            .get("published_at")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string),
    }))
}
