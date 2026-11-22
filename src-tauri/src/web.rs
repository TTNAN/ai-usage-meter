//! In-app sign-in for claude.ai and grok.com.
//!
//! Claude's and Grok's web / desktop / mobile apps draw from the same usage
//! pool as their CLIs, so a person who never installed Claude Code or Grok
//! Build can sign in once here instead:
//!
//! * "Sign in" opens a normal window on the service's own login page.
//! * After that, a hidden window on the same site asks the site's own
//!   usage endpoint (same-origin, so the browser attaches the session and
//!   passes Cloudflare like any tab would) every few minutes.
//! * The page hands the result back by navigating to a dummy
//!   `https://aum-report.invalid/…` URL, which we intercept and cancel. The
//!   remote pages get no access to the app's IPC at all.
//!
//! Session cookies stay inside the app's own webview storage. We never read
//! them out, copy them or send them anywhere else.

use crate::lock::lock;
use crate::AppState;
use ai_usage_core::providers;
use std::collections::HashSet;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const REPORT_HOST: &str = "aum-report.invalid";
/// The bare page we load to ask a site for its numbers: a static text file,
/// so a poll costs one small request instead of a whole app.
const LIGHT_PATH: &str = "/robots.txt";

// ------------------------------------------------- Grok's credits call
//
// grok.com's own web bundle asks for its credit balance with one Connect-ES
// (gRPC-web) unary call. The four pieces of that request are pinned here, in
// one place, so that a change in grok.com's front end is a one-line change
// here instead of a hunt through the page scripts.

/// The method, on the page's own origin so the browser attaches the session
/// cookie (and passes Cloudflare) like any tab would.
const GROK_CREDITS_PATH: &str = "/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig";
/// The last segment of `GROK_CREDITS_PATH`: grok.com's own scripts ask for the
/// method by this name, which is how their answer is recognised when the full
/// page is loaded.
const GROK_CREDITS_METHOD: &str = "GetGrokCreditsConfig";
/// What grok.com's bundle announces itself as. Connect-ES puts its protocol
/// version here, and the server answers in kind.
const GROK_CLIENT_AGENT: &str = "connect-es/2.1.1";
/// `content-type` of a Connect-ES unary call in its binary encoding.
const GROK_CONTENT_TYPE: &str = "application/grpc-web+proto";
/// The request body, byte for byte: an empty `GetGrokCreditsConfigRequest`,
/// already wrapped in the gRPC-web envelope. Five bytes of framing — a flag
/// byte (0x00: a data frame, not a trailer) and the message length as a
/// big-endian u32 (0x00 0x00 0x00 0x02) — then the message itself: `0x08 0x00`
/// is field 1, varint, value 0 (the request has no fields set, and proto3
/// still writes the empty message out).
const GROK_CREDITS_REQUEST: [u8; 7] = [0x00, 0x00, 0x00, 0x00, 0x02, 0x08, 0x00];

struct Site {
    id: &'static str,
    name: &'static str,
    host: &'static str,
    login_url: &'static str,
    poll_url: &'static str,
    /// Cookie names that mean "signed in".
    session_cookies: &'static [&'static str],
    every_secs: i64,
}

const SITES: [Site; 2] = [
    Site {
        id: "claude",
        name: "Claude",
        host: "claude.ai",
        login_url: "https://claude.ai/login",
        poll_url: "https://claude.ai/robots.txt",
        session_cookies: &["sessionKey"],
        every_secs: 180,
    },
    Site {
        id: "grok",
        name: "Grok",
        host: "grok.com",
        login_url: "https://accounts.x.ai/sign-in?redirect=grok-com",
        poll_url: "https://grok.com/robots.txt",
        session_cookies: &["sso", "sso-rw"],
        every_secs: 300,
    },
];

fn site(id: &str) -> Option<&'static Site> {
    SITES.iter().find(|s| s.id == id)
}

/// A desktop-browser user agent, so sign-in pages (and Google / Apple
/// sign-in) treat the window like a normal browser tab.
fn user_agent() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Safari/605.1.15")
    } else if cfg!(target_os = "linux") {
        // WebKitGTK introduces itself as "Safari on Linux" — a combination
        // that does not exist, which sign-in pages and Cloudflare read as a
        // bot. A Linux Chrome UA is what a real browser on the same machine
        // sends.
        Some("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36")
    } else {
        None // WebView2 already identifies as Edge.
    }
}

// --------------------------------------------------------- persistence

fn logins_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("web-logins.json"))
}

pub fn load_logins(app: &AppHandle) -> HashSet<String> {
    logins_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str::<Vec<String>>(&t).ok())
        .map(|v| v.into_iter().filter(|id| site(id).is_some()).collect())
        .unwrap_or_default()
}

fn save_logins(app: &AppHandle, logins: &HashSet<String>) {
    let Some(p) = logins_path(app) else { return };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut v: Vec<&String> = logins.iter().collect();
    v.sort();
    let _ = std::fs::write(p, serde_json::to_string(&v).unwrap_or_else(|_| "[]".into()));
}

pub fn signed_in(app: &AppHandle) -> Vec<String> {
    let state = app.state::<AppState>();
    let mut v: Vec<String> = lock(&state.web_logins).iter().cloned().collect();
    v.sort();
    v
}

// ------------------------------------------------------- page scripts

fn report_js(id: &str) -> String {
    format!(
        "const __aumSend=(o)=>{{try{{location.href='https://{REPORT_HOST}/{id}?d='+encodeURIComponent(JSON.stringify(o));}}catch(e){{}}}};"
    )
}

fn claude_js() -> String {
    report_js("claude")
        + r#"
(async () => {
  try {
    const get = async (u) => {
      const r = await fetch(u, { credentials: 'include', headers: { accept: 'application/json' } });
      if (r.status === 401 || r.status === 403) {
        const t = await r.text();
        return /cloudflare|cf-chl|challenge-platform|just a moment/i.test(t) ? { cf: true } : { auth: false };
      }
      const ct = r.headers.get('content-type') || '';
      if (!ct.includes('json')) return { cf: true };
      return { json: await r.json() };
    };
    const orgs = await get('/api/organizations');
    if (orgs.cf) return __aumSend({ error: 'cloudflare', cf: true });
    if (orgs.auth === false) return __aumSend({ auth: false });
    const list = Array.isArray(orgs.json) ? orgs.json : [];
    const org = list.find((o) => (o.capabilities || []).includes('chat')) || list[0];
    if (!org) return __aumSend({ error: 'no organization' });
    const usage = await get('/api/organizations/' + org.uuid + '/usage');
    if (usage.cf) return __aumSend({ error: 'cloudflare', cf: true });
    if (usage.auth === false) return __aumSend({ auth: false });
    __aumSend({ usage: usage.json, capabilities: org.capabilities || [], tier: org.rate_limit_tier || null });
  } catch (e) {
    __aumSend({ error: String(e) });
  }
})();
"#
}

/// Runs before grok.com's own scripts: when the usage page asks for its
/// credits config, keep a copy of the answer.
fn grok_init_js() -> String {
    format!(
        "if (location.hostname === 'grok.com') {{ {send}\n{intercept}\n  window.__aumFallback = () => {{ {ask} }};\n}}\n",
        send = report_js("grok"),
        intercept = GROK_INTERCEPT_JS.replace("{METHOD}", GROK_CREDITS_METHOD),
        ask = grok_ask_js(),
    )
}

/// Watching fetch for the page's own credits request (full grok.com page).
const GROK_INTERCEPT_JS: &str = r#"
  const b64 = (buf) => { const a = new Uint8Array(buf); let s = ''; for (let i = 0; i < a.length; i++) s += String.fromCharCode(a[i]); return btoa(s); };
  const origFetch = window.fetch.bind(window);
  const handle = (res) => {
    res.clone().arrayBuffer().then((buf) => {
      if (res.status === 401 || res.status === 403) return __aumSend({ auth: false });
      __aumSend({ b64: b64(buf), status: res.status, grpc: res.headers.get('grpc-status') || '0' });
    }).catch(() => {});
  };
  window.fetch = async function (input, init) {
    const res = await origFetch(input, init);
    try {
      const url = typeof input === 'string' ? input : (input && input.url) || '';
      if (url.includes('{METHOD}')) handle(res);
    } catch (e) {}
    return res;
  };
"#;

/// POSTs to grok.com's credits endpoint (gRPC-web) and reports whatever comes
/// back. Self-contained on purpose: it is the page's late fallback, and the
/// only script that runs on the bare robots.txt document, where none of
/// grok.com's own code exists. `{PATH}`, `{CTYPE}`, `{AGENT}`, `{BODY}` and
/// `{HOST}` are filled in from the constants above by `grok_ask_js`.
const GROK_ASK_JS: &str = r#"(async () => {
  const send = (o) => { try { location.href = 'https://{HOST}/grok?d=' + encodeURIComponent(JSON.stringify(o)); } catch (e) {} };
  const b64 = (buf) => { const a = new Uint8Array(buf); let s = ''; for (let i = 0; i < a.length; i++) s += String.fromCharCode(a[i]); return btoa(s); };
  try {
    const res = await fetch('{PATH}', {
      method: 'POST', credentials: 'include',
      headers: { 'content-type': '{CTYPE}', 'x-grpc-web': '1', 'x-user-agent': '{AGENT}' },
      body: new Uint8Array({BODY}),
    });
    if (res.status === 401 || res.status === 403) return send({ auth: false });
    if (!res.ok) return send({ error: 'HTTP ' + res.status });
    send({ b64: b64(await res.arrayBuffer()), status: res.status, grpc: res.headers.get('grpc-status') || '0' });
  } catch (e) { send({ error: String(e) }); }
})()"#;

fn grok_ask_js() -> String {
    GROK_ASK_JS
        .replace("{HOST}", REPORT_HOST)
        .replace("{PATH}", GROK_CREDITS_PATH)
        .replace("{CTYPE}", GROK_CONTENT_TYPE)
        .replace("{AGENT}", GROK_CLIENT_AGENT)
        .replace("{BODY}", &format!("{:?}", GROK_CREDITS_REQUEST))
}

// ------------------------------------------------------------ reports

/// Which site a report may speak for. The URL path names the site, but a page
/// in one service's window can navigate anywhere: a script on claude.ai (or a
/// third-party one it loads) could jump to `aum-report.invalid/grok?d=…` and
/// write Grok's row. So a report is accepted only from that site's own hidden
/// worker — never from the popover, a sign-in window, or the other service.
fn report_site(label: &str, url: &Url) -> Option<&'static Site> {
    let s = site(url.path().trim_start_matches('/'))?;
    if label != worker_label(s.id) {
        return None;
    }
    Some(s)
}

fn handle_report(app: &AppHandle, label: &str, url: &Url) {
    let Some(site) = report_site(label, url) else {
        return;
    };
    let state = app.state::<AppState>();
    if !lock(&state.web_logins).contains(site.id) {
        // Signed out while the page was answering: drop its report rather than
        // bring a row back for an account the user just left.
        return;
    }
    let data = url
        .query_pairs()
        .find(|(k, _)| k == "d")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let report: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
    if report.get("cf").and_then(|c| c.as_bool()) == Some(true) {
        // Cloudflare wants a full page first; next round loads the home page.
        lock(&state.web_use_home).insert(site.id);
    }
    let status = match site.id {
        "claude" => providers::claude_from_web(&report),
        _ => providers::grok_from_web(&report, providers::now_unix()),
    };
    lock(&state.web_last_report).insert(site.id, providers::now_unix());
    lock(&state.web).insert(site.id, status);
    crate::poller::apply_web_results(app);
    // The report is the last thing this hidden window was for: let go of it
    // instead of keeping a whole webview resident until the next poll. The
    // session lives in the app's shared webview storage, so the next poll
    // opens a fresh worker and picks it up again.
    close_worker_after_report(app, site.id);
}

/// Closes the hidden worker once the report it carried is in. Deferred a
/// moment (this runs inside the window's own navigation callback) and guarded
/// by the worker epoch, so a poll that has already taken the window over is
/// never closed out from under itself.
fn close_worker_after_report(app: &AppHandle, id: &'static str) {
    let epoch = lock(&app.state::<AppState>().web_worker_epoch)
        .get(id)
        .copied()
        .unwrap_or(0);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let state = app.state::<AppState>();
        if lock(&state.web_worker_epoch).get(id).copied().unwrap_or(0) != epoch {
            return;
        }
        if let Some(w) = app.get_webview_window(&worker_label(id)) {
            let _ = w.close();
        }
    });
}

// ------------------------------------------------------------- windows

fn worker_label(id: &str) -> String {
    format!("web-{id}")
}

/// Counts the workers a site has been given, so a deferred close can tell
/// whether the window it meant is still the current one.
fn bump_worker_epoch(app: &AppHandle, id: &'static str) -> u64 {
    let state = app.state::<AppState>();
    let mut epochs = lock(&state.web_worker_epoch);
    let n = epochs.entry(id).or_insert(0);
    *n += 1;
    *n
}

fn worker(app: &AppHandle, site: &'static Site) -> Option<WebviewWindow> {
    let label = worker_label(site.id);
    if let Some(w) = app.get_webview_window(&label) {
        return Some(w);
    }
    let url = Url::parse(site.poll_url).ok()?;
    let nav_app = app.clone();
    // The label this window is created with, carried into its own navigation
    // handler: `handle_report` refuses a report whose URL names a different
    // site, so this window can only ever speak for the site it was built for.
    let nav_label = label.clone();
    let site_id = site.id;
    let host = site.host;
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(format!("{} usage", site.name))
        .visible(false)
        .skip_taskbar(true)
        .focused(false)
        .inner_size(480.0, 640.0)
        .on_navigation(move |u| {
            if u.host_str() == Some(REPORT_HOST) {
                handle_report(&nav_app, &nav_label, u);
                return false;
            }
            // Bounced to a sign-in page: the session is gone.
            let h = u.host_str().unwrap_or("");
            let p = u.path();
            if h == "accounts.x.ai"
                || (h == host && (p.starts_with("/login") || p.starts_with("/sign-in")))
            {
                let report = format!("https://{REPORT_HOST}/{site_id}?d=%7B%22auth%22%3Afalse%7D");
                if let Ok(r) = Url::parse(&report) {
                    handle_report(&nav_app, &nav_label, &r);
                }
                return false;
            }
            true
        })
        .on_page_load(move |w, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }
            let url = payload.url();
            if url.host_str() != Some(host) {
                return;
            }
            match site_id {
                "claude" => {
                    let _ = w.eval(claude_js().as_str());
                }
                _ => {
                    if url.path() == LIGHT_PATH {
                        // A bare text document: nothing on it will ask for the
                        // numbers, so run the same ask window.__aumFallback
                        // runs, right away instead of waiting.
                        let _ = w.eval(grok_ask_js().as_str());
                        return;
                    }
                    // The full site (Cloudflare wanted a real page first): its
                    // own scripts ask for the numbers; if they haven't within
                    // a few seconds, ask directly.
                    let app = w.app_handle().clone();
                    std::thread::spawn(move || {
                        let started = providers::now_unix();
                        std::thread::sleep(std::time::Duration::from_secs(12));
                        let state = app.state::<AppState>();
                        let last = lock(&state.web_last_report)
                            .get("grok")
                            .copied()
                            .unwrap_or(0);
                        if last < started {
                            if let Some(w) = app.get_webview_window(&worker_label("grok")) {
                                let _ = w.eval("window.__aumFallback && window.__aumFallback()");
                            }
                        }
                    });
                }
            }
        });
    if site.id == "grok" {
        b = b.initialization_script(&grok_init_js());
    }
    if let Some(ua) = user_agent() {
        b = b.user_agent(ua);
    }
    b.build().ok().inspect(|_| {
        bump_worker_epoch(app, site.id);
    })
}

/// Called from the poller: re-read every signed-in site that is due.
pub fn poll_due(app: &AppHandle, force: bool) {
    let now = providers::now_unix();
    let state = app.state::<AppState>();
    let ids: Vec<String> = lock(&state.web_logins).iter().cloned().collect();
    for id in ids {
        let Some(site) = site(&id) else { continue };
        let due = {
            let mut last = lock(&state.web_last_poll);
            let since = now - last.get(site.id).copied().unwrap_or(0);
            let ok = since >= if force { 30 } else { site.every_secs };
            if ok {
                last.insert(site.id, now);
            }
            ok
        };
        if !due {
            continue;
        }
        let home = lock(&state.web_use_home).remove(site.id);
        let target = if home {
            format!("https://{}/", site.host)
        } else {
            site.poll_url.to_string()
        };
        let fresh = app.get_webview_window(&worker_label(site.id)).is_none();
        let Some(w) = worker(app, site) else { continue };
        if !fresh {
            if let Ok(u) = Url::parse(&target) {
                let _ = w.navigate(u);
            }
            // This window is ours again: cancel any close still on its way.
            bump_worker_epoch(app, site.id);
        }
    }
}

fn has_session(w: &WebviewWindow, site: &Site) -> bool {
    let Ok(base) = Url::parse(&format!("https://{}/", site.host)) else {
        return false;
    };
    w.cookies_for_url(base)
        .map(|cs| cs.iter().any(|c| site.session_cookies.contains(&c.name())))
        .unwrap_or(false)
}

/// Open the service's login page in a normal window. When it lands back
/// on the site with a session, close it and start reading.
pub fn open_login(app: &AppHandle, id: &str) -> Result<(), String> {
    let site = site(id).ok_or("unknown service")?;
    let label = format!("login-{}", site.id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.set_focus();
        return Ok(());
    }
    let url = Url::parse(site.login_url).map_err(|e| e.to_string())?;
    let mut b = WebviewWindowBuilder::new(app, &label, WebviewUrl::External(url))
        .title(format!("Sign in to {}", site.name))
        .inner_size(480.0, 720.0)
        .center()
        .focused(true);
    if let Some(ua) = user_agent() {
        b = b.user_agent(ua);
    }
    let w = b.build().map_err(|e| e.to_string())?;
    let _ = w.set_focus();

    let app = app.clone();
    std::thread::spawn(move || {
        // Up to 15 minutes to finish signing in.
        for _ in 0..600 {
            std::thread::sleep(std::time::Duration::from_millis(1500));
            let Some(w) = app.get_webview_window(&label) else {
                return;
            };
            let on_site = w
                .url()
                .map(|u| {
                    u.host_str() == Some(site.host)
                        && !u.path().starts_with("/login")
                        && !u.path().starts_with("/sign-in")
                })
                .unwrap_or(false);
            if on_site && has_session(&w, site) {
                let _ = w.close();
                let state = app.state::<AppState>();
                {
                    let mut logins = lock(&state.web_logins);
                    logins.insert(site.id.to_string());
                    save_logins(&app, &logins);
                }
                lock(&state.web_last_poll).remove(site.id);
                if let Some(old) = app.get_webview_window(&worker_label(site.id)) {
                    let _ = old.close();
                }
                poll_due(&app, true);
                let _ = app.emit("web-logins", signed_in(&app));
                return;
            }
        }
    });
    Ok(())
}

/// Forget the in-app session: delete that site's cookies from the app's
/// webview storage and stop reading it.
pub fn sign_out(app: &AppHandle, id: &str) -> Result<(), String> {
    let site = site(id).ok_or("unknown service")?;
    let state = app.state::<AppState>();
    {
        let mut logins = lock(&state.web_logins);
        logins.remove(site.id);
        save_logins(app, &logins);
    }
    lock(&state.web).remove(site.id);
    lock(&state.web_last_poll).remove(site.id);
    lock(&state.web_use_home).remove(site.id);

    let mut hosts = vec![format!("https://{}/", site.host)];
    if site.id == "grok" {
        hosts.push("https://accounts.x.ai/".into());
        hosts.push("https://x.ai/".into());
    } else {
        hosts.push("https://anthropic.com/".into());
    }

    // Every webview in the app shares one cookie store, so whichever window is
    // already open can forget the session — the hidden worker if it is still
    // around, else a login window, else the popover. Nothing is created just
    // to sign out: signing out must never send a request to the service.
    let worker = app.get_webview_window(&worker_label(site.id));
    let login = app.get_webview_window(&format!("login-{}", site.id));
    let cleaner = worker
        .clone()
        .or_else(|| login.clone())
        .or_else(|| app.get_webview_window("main"));
    if let Some(w) = &cleaner {
        for h in hosts {
            if let Ok(u) = Url::parse(&h) {
                for c in w.cookies_for_url(u).unwrap_or_default() {
                    let _ = w.delete_cookie(c);
                }
            }
        }
    }
    if let Some(w) = worker {
        let _ = w.close();
    }
    if let Some(w) = login {
        let _ = w.close();
    }
    crate::poller::apply_web_results(app);
    let _ = app.emit("web-logins", signed_in(app));
    Ok(())
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn report_url(path: &str) -> Url {
        Url::parse(&format!(
            "https://{REPORT_HOST}/{path}?d=%7B%22auth%22%3Afalse%7D"
        ))
        .expect("a report URL")
    }

    #[test]
    fn a_report_is_only_taken_from_the_window_that_fetched_it() {
        // Each site's own hidden worker, which is what the poller creates.
        assert_eq!(
            report_site("web-claude", &report_url("claude")).map(|s| s.id),
            Some("claude")
        );
        assert_eq!(
            report_site("web-grok", &report_url("grok")).map(|s| s.id),
            Some("grok")
        );

        // The page in one service's window cannot answer for the other: a
        // script on claude.ai that navigates to the Grok report URL is not
        // Grok, and its report is dropped.
        assert!(report_site("web-claude", &report_url("grok")).is_none());
        assert!(report_site("web-grok", &report_url("claude")).is_none());

        // Nor can the popover, or a sign-in window the user is typing in.
        assert!(report_site("main", &report_url("grok")).is_none());
        assert!(report_site("login-grok", &report_url("grok")).is_none());

        // A path that names no site is nobody's report.
        assert!(report_site("web-grok", &report_url("nothing")).is_none());
        assert!(report_site("web-grok", &report_url("")).is_none());
    }

    #[test]
    fn the_ask_script_is_built_from_the_connect_es_constants() {
        let js = grok_ask_js();
        assert!(js.contains(GROK_CREDITS_PATH), "the method path is in it");
        assert!(js.contains(GROK_CONTENT_TYPE));
        assert!(js.contains(GROK_CLIENT_AGENT));
        assert!(
            js.contains("[0, 0, 0, 0, 2, 8, 0]"),
            "the enveloped request bytes are in it"
        );
        assert!(js.contains(&format!("https://{}/grok", REPORT_HOST)));
        for placeholder in ["{HOST}", "{PATH}", "{CTYPE}", "{AGENT}", "{BODY}"] {
            assert!(!js.contains(placeholder), "{placeholder} was left unfilled");
        }
    }

    #[test]
    fn the_method_name_is_the_end_of_the_path_it_is_asked_on() {
        assert!(GROK_CREDITS_PATH.ends_with(GROK_CREDITS_METHOD));
        let init = grok_init_js();
        assert!(
            init.contains(GROK_CREDITS_METHOD),
            "the fetch watcher matches the method it is watching for"
        );
        assert!(!init.contains("{METHOD}"));
    }

    #[test]
    fn the_envelope_says_two_bytes_of_message() {
        assert_eq!(GROK_CREDITS_REQUEST.len(), 7);
        let framed = &GROK_CREDITS_REQUEST[..5];
        assert_eq!(framed[0], 0x00, "a data frame, not a trailer");
        assert_eq!(
            u32::from_be_bytes([framed[1], framed[2], framed[3], framed[4]]),
            2,
            "the message length the envelope announces"
        );
    }
}
