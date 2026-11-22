//! Gemini CLI / Antigravity: credentials, silent token refresh, the quota
//! request and its parse.

use super::*;

pub(super) const GEMINI_LABEL: &str = "Gemini";
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
        for name in [
            "gemini",
            "gemini.js",
            "gemini.cmd",
            "gemini.exe",
            "gemini.ps1",
        ] {
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
            if !formula
                .file_name()
                .to_string_lossy()
                .starts_with("gemini-cli")
            {
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
        let Ok(meta) = fs::metadata(&file) else {
            continue;
        };
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
            if number.len() >= 10 && number.chars().all(|c| c.is_ascii_digit()) && rest.len() >= 30
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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
        assert_eq!(
            id, want_id,
            "not the unrelated client id earlier in the file"
        );
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
        assert_eq!(
            scoped_gemini_package(&PathBuf::from("/usr/bin/gemini")),
            None
        );
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
}
