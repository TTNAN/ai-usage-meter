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

use crate::providers;
use crate::AppState;
use std::collections::HashSet;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const REPORT_HOST: &str = "aum-report.invalid";

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
        poll_url: "https://grok.com/?_s=usage",
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
    } else {
        None // WebView2 already identifies as Edge.
    }
}

// --------------------------------------------------------- persistence

fn logins_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_config_dir().ok().map(|d| d.join("web-logins.json"))
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
    let mut v: Vec<String> = state.web_logins.lock().unwrap().iter().cloned().collect();
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
    format!("if (location.hostname === 'grok.com') {{ {} ", report_js("grok"))
        + r#"
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
      if (url.includes('GetGrokCreditsConfig')) handle(res);
    } catch (e) {}
    return res;
  };
  window.__aumFallback = async () => {
    try {
      const res = await origFetch('/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig', {
        method: 'POST', credentials: 'include',
        headers: { 'content-type': 'application/grpc-web+proto', 'x-grpc-web': '1', 'x-user-agent': 'connect-es/2.1.1' },
        body: new Uint8Array([0, 0, 0, 0, 2, 8, 0]),
      });
      handle(res);
    } catch (e) { __aumSend({ error: String(e) }); }
  };
}
"#
}

// ------------------------------------------------------------ reports

fn handle_report(app: &AppHandle, url: &Url) {
    let id = url.path().trim_start_matches('/').to_string();
    let Some(site) = site(&id) else { return };
    let state = app.state::<AppState>();
    if !state.web_logins.lock().unwrap().contains(site.id) {
        return; // signed out meanwhile
    }
    let data = url
        .query_pairs()
        .find(|(k, _)| k == "d")
        .map(|(_, v)| v.into_owned())
        .unwrap_or_default();
    let report: serde_json::Value = serde_json::from_str(&data).unwrap_or(serde_json::Value::Null);
    if report.get("cf").and_then(|c| c.as_bool()) == Some(true) {
        // Cloudflare wants a full page first; next round loads the home page.
        state.web_use_home.lock().unwrap().insert(site.id);
    }
    let status = match site.id {
        "claude" => providers::claude_from_web(&report),
        _ => providers::grok_from_web(&report, providers::now_unix()),
    };
    state.web_last_report.lock().unwrap().insert(site.id, providers::now_unix());
    state.web.lock().unwrap().insert(site.id, status);
    crate::apply_web_results(app);
}

// ------------------------------------------------------------- windows

fn worker_label(id: &str) -> String {
    format!("web-{id}")
}

fn worker(app: &AppHandle, site: &'static Site) -> Option<WebviewWindow> {
    let label = worker_label(site.id);
    if let Some(w) = app.get_webview_window(&label) {
        return Some(w);
    }
    let url = Url::parse(site.poll_url).ok()?;
    let nav_app = app.clone();
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
                handle_report(&nav_app, u);
                return false;
            }
            // Bounced to a sign-in page: the session is gone.
            let h = u.host_str().unwrap_or("");
            let p = u.path();
            if h == "accounts.x.ai" || (h == host && (p.starts_with("/login") || p.starts_with("/sign-in"))) {
                let report = format!("https://{REPORT_HOST}/{site_id}?d=%7B%22auth%22%3Afalse%7D");
                if let Ok(r) = Url::parse(&report) {
                    handle_report(&nav_app, &r);
                }
                return false;
            }
            true
        })
        .on_page_load(move |w, payload| {
            if payload.event() != tauri::webview::PageLoadEvent::Finished {
                return;
            }
            if payload.url().host_str() != Some(host) {
                return;
            }
            match site_id {
                "claude" => {
                    let _ = w.eval(claude_js().as_str());
                }
                _ => {
                    // The usage page asks for its own numbers; if it hasn't
                    // within a few seconds, ask directly.
                    let app = w.app_handle().clone();
                    std::thread::spawn(move || {
                        let started = providers::now_unix();
                        std::thread::sleep(std::time::Duration::from_secs(12));
                        let state = app.state::<AppState>();
                        let last = state.web_last_report.lock().unwrap().get("grok").copied().unwrap_or(0);
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
    b.build().ok()
}

/// Called from the poller: re-read every signed-in site that is due.
pub fn poll_due(app: &AppHandle, force: bool) {
    let now = providers::now_unix();
    let state = app.state::<AppState>();
    let ids: Vec<String> = state.web_logins.lock().unwrap().iter().cloned().collect();
    for id in ids {
        let Some(site) = site(&id) else { continue };
        let due = {
            let mut last = state.web_last_poll.lock().unwrap();
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
        let home = state.web_use_home.lock().unwrap().remove(site.id);
        let target = if home { format!("https://{}/", site.host) } else { site.poll_url.to_string() };
        let fresh = app.get_webview_window(&worker_label(site.id)).is_none();
        let Some(w) = worker(app, site) else { continue };
        if !fresh {
            if let Ok(u) = Url::parse(&target) {
                let _ = w.navigate(u);
            }
        }
    }
}

fn has_session(w: &WebviewWindow, site: &Site) -> bool {
    let Ok(base) = Url::parse(&format!("https://{}/", site.host)) else { return false };
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
            let Some(w) = app.get_webview_window(&label) else { return };
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
                    let mut logins = state.web_logins.lock().unwrap();
                    logins.insert(site.id.to_string());
                    save_logins(&app, &logins);
                }
                state.web_last_poll.lock().unwrap().remove(site.id);
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
        let mut logins = state.web_logins.lock().unwrap();
        logins.remove(site.id);
        save_logins(app, &logins);
    }
    state.web.lock().unwrap().remove(site.id);
    let w = worker(app, site);
    if let Some(w) = &w {
        let mut hosts = vec![format!("https://{}/", site.host)];
        if site.id == "grok" {
            hosts.push("https://accounts.x.ai/".into());
            hosts.push("https://x.ai/".into());
        } else {
            hosts.push("https://anthropic.com/".into());
        }
        for h in hosts {
            if let Ok(u) = Url::parse(&h) {
                for c in w.cookies_for_url(u).unwrap_or_default() {
                    let _ = w.delete_cookie(c);
                }
            }
        }
        let _ = w.close();
    }
    crate::apply_web_results(app);
    let _ = app.emit("web-logins", signed_in(app));
    Ok(())
}
