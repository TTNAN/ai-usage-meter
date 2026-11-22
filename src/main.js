// AI Usage Meter — entry: initialises the panel, wires the events both views
// share, talks to the Rust side through the Tauri bridge, and dispatches every
// update that comes back. Plain ESM, no bundler.

import {
  REPO, store, settings, TAURI, PREVIEW, params, invoke, demoStatus,
  recordHistory, seedDemoHistory, applyColors, applyTheme, THEMES, fmtReset, fmtDur, fmtAgo,
  normalizeOpacity, applyOpacity, fitWindow,
} from "./js/state.js";
import { STRINGS, lang, t, syncLang } from "./js/i18n.js";
import { applyShortcut, paintShortcutSeg, showShortcutState } from "./js/shortcut.js";
import {
  buildRings, ringIds, syncRings, renderRings, renderHero, renderServices,
  syncServicesScroll, resetSentence,
} from "./js/views/rings.js";
import {
  renderDock, syncDockWidth, paintDockShortcut, showDockColorbar, bindDock,
  dockReset,
} from "./js/views/dock.js";
import { initUpdateCheck, paintUpdate, getUpdate, updateUrl } from "./js/update.js";
import {
  renderAccounts, renderColorRows, syncSettingsScroll, bindSettings,
  bindColorRows, bindAccounts,
} from "./js/views/settings.js";

function loadLogins() {
  if (PREVIEW) { store.webLogins = params.get("state") === "mixed" ? ["claude"] : []; renderAccounts(); return; }
  invoke("web_logins").then((v) => { store.webLogins = v || []; renderAccounts(); }).catch(() => {});
}

function signIn(id) {
  if (PREVIEW) return;
  invoke("web_login", { id }).catch(() => {});
}

let refreshing = false;

// --------------------------------------------------------------- actions

function setStatus(next) {
  if (!Array.isArray(next) || !next.length) return;
  store.status = next;
  store.lastUpdate = Date.now();
  recordHistory(next);
  render();
}

async function refresh() {
  if (refreshing) return;
  refreshing = true;
  const buttons = [document.getElementById("refresh"), document.getElementById("dock-refresh")];
  buttons.forEach((b) => b && b.classList.add("spinning"));
  const started = Date.now();
  try {
    if (PREVIEW) setStatus(demoStatus(params.get("state")));
    else setStatus(await invoke("refresh_status"));
  } catch (e) {
    console.error(e);
  } finally {
    // Let the spinner complete a turn so a fast refresh still reads as one.
    setTimeout(() => {
      buttons.forEach((b) => b && b.classList.remove("spinning"));
      refreshing = false;
    }, Math.max(0, 600 - (Date.now() - started)));
  }
}

function openUrl(url) {
  if (PREVIEW) window.open(url, "_blank");
  else invoke("open_url", { url }).catch(() => {});
}

function render() {
  renderHero();
  syncRings();
  renderRings();
  renderServices();
  renderDock();
  document.getElementById("demo-chip").hidden = params.has("clean") || !store.status.some((s) => s.state === "mock");
  tick();
}

/** Once a second: countdowns and "updated" only — no re-layout. */
function tick() {
  document.querySelectorAll("[data-reset]").forEach((el) => {
    const v = el.dataset.reset === "" ? null : parseFloat(el.dataset.reset);
    el.textContent = el.dataset.resetFmt === "sentence" ? resetSentence(v)
      : el.dataset.resetFmt === "timer" ? dockReset(v)
      : fmtReset(v);
  });
  document.querySelectorAll("[data-ago]").forEach((el) => {
    el.textContent = t("fromSession")(fmtAgo(parseFloat(el.dataset.ago)));
  });
  paintUpdate();
  const up = document.getElementById("updated");
  if (!store.lastUpdate) { up.textContent = ""; return; }
  const secs = (Date.now() - store.lastUpdate) / 1000;
  up.textContent = secs < 45 ? t("justNow") : t("ago")(fmtDur(secs));
}

function applyI18n() {
  document.documentElement.lang = lang() === "zh" ? "zh-CN" : "en";
  document.querySelectorAll("[data-i18n]").forEach((el) => (el.textContent = t(el.dataset.i18n)));
  document.querySelectorAll("[data-i18n-title]").forEach((el) => (el.title = t(el.dataset.i18nTitle)));
  document.querySelectorAll(".seg[data-setting]").forEach((seg) => {
    seg.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.value === settings[seg.dataset.setting]));
  });
  renderAccounts();
  // Row labels and pickers are localized too, and so is Off / ⌘⇧U.
  renderColorRows();
  paintShortcutSeg();
  showShortcutState();
  paintDockShortcut();
  syncViewSegs();
  syncSettingsScroll();
  // The dock spells its own labels out in the same language.
  renderDock();
}

function showView(name) {
  // "usage" (and "main") mean whichever view this session is in; the settings
  // drawer always takes the popover's own 372 px width, even from Dock view.
  const settingsOpen = name === "settings";
  const dock = settings.view === "dock";
  document.getElementById("panel").classList.toggle("settings-open", settingsOpen);
  document.getElementById("view-usage").hidden = settingsOpen || dock;
  document.getElementById("view-dock").hidden = settingsOpen || !dock;
  document.getElementById("view-settings").hidden = !settingsOpen;
  syncDockWidth();
  if (settingsOpen) syncSettingsScroll();
  fitWindow();
}

/** Switch between the rings popover and the dock panel. */
function applyView() {
  document.getElementById("panel").dataset.view = settings.view === "dock" ? "dock" : "concentric";
  syncViewSegs();
  showView(document.getElementById("view-settings").hidden ? "usage" : "settings");
}

function setView(view) {
  settings.view = view === "dock" ? "dock" : "concentric";
  localStorage.setItem("view", settings.view);
  applyView();
}

function syncViewSegs() {
  document.querySelectorAll(".view-seg button").forEach((b) =>
    b.classList.toggle("on", b.dataset.view === settings.view)
  );
}

/**
 * Edge docking, as reported by Rust: the card slides almost off the screen
 * when it loses focus (leaving a 12 px handle), and that handle brings it
 * back. `edge` is "left" / "right" / "none".
 */
function setEdgeState(edge, hidden) {
  const root = document.documentElement;
  root.classList.toggle("edge-left", edge === "left");
  root.classList.toggle("edge-right", edge === "right");
  root.classList.toggle("edge-hidden", !!hidden && (edge === "left" || edge === "right"));
  // The grab handle exists only while the card is actually tucked away.
  const docked = edge === "left" || edge === "right";
  document.getElementById("edge-handle").hidden = !(docked && hidden);
}

async function boot() {
  const root = document.documentElement;
  let platform = /Mac/.test(navigator.platform) ? "macos" : "windows";

  // The saved shape, chosen before anything is painted. In the browser
  // preview `?view=dock` / `?view=settings` / `?hide=1` open it directly.
  const wantView = params.get("view");
  if (wantView === "dock" || wantView === "concentric") settings.view = wantView;
  const wantLang = params.get("lang");
  if (wantLang === "zh" || wantLang === "en") settings.lang = wantLang;
  const wantMode = params.get("mode");
  if (wantMode === "used" || wantMode === "left") settings.mode = wantMode;
  // `?theme=light` / `?theme=dark` previews a face without storing it, the way
  // the README's screenshots are taken.
  const wantTheme = params.get("theme");
  if (THEMES.includes(wantTheme)) settings.theme = wantTheme;
  // `?opacity=60` previews a thinner panel the same way, and stores nothing.
  if (params.has("opacity")) settings.opacity = normalizeOpacity(params.get("opacity"));
  if (params.get("hide") === "1") settings.hideDisconnected = true;
  document.getElementById("panel").dataset.view = settings.view;
  // The face first: everything painted below reads its colours.
  applyTheme();
  applyOpacity();
  // The update check reads its `?update=` preview param before anything
  // renders, and schedules its daily background check for later.
  initUpdateCheck();

  if (PREVIEW) {
    root.classList.add("preview");
    if (params.has("bare")) root.classList.add("bare");
    // `?wayland=1`: the Linux session's window-placing limits, for previews.
    if (params.get("wayland") === "1") store.wayland = true;
    // `?frame=1`: the panel alone on a transparent page (README hero shots).
    if (params.has("frame")) root.classList.add("frame");
  } else {
    try {
      const info = await invoke("app_info");
      platform = info.platform;
      store.wayland = !!info.wayland;
      document.getElementById("version").textContent = "v" + info.version;
    } catch (_) {}
    if (platform === "macos") root.classList.add("vibrancy");
  }
  if (PREVIEW) document.getElementById("version").textContent = "v0.8.0";
  root.classList.add("platform-" + platform);
  if (platform !== "macos") {
    document.getElementById("setting-tray").hidden = true;
    document.querySelector("#quit .kbd").textContent = "Ctrl Q";
    STRINGS.en.refresh = "Refresh (Ctrl R)";
    STRINGS.zh.refresh = "刷新 (Ctrl R)";
  }

  buildRings(ringIds());
  applyI18n();
  applyColors();
  applyShortcut();
  render();
  applyView();

  bindSettings();
  bindDock();
  bindColorRows();
  bindAccounts();

  // Size the native window to the panel, whatever its content.
  new ResizeObserver(fitWindow).observe(document.getElementById("panel"));

  loadLogins();

  document.querySelectorAll(".view-seg").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (b) setView(b.dataset.view);
    })
  );

  document.getElementById("edge-handle").addEventListener("click", () => invoke("expand_docked").catch(() => {}));
  document.getElementById("update-pill").addEventListener("click", () => openUrl(updateUrl()));

  document.getElementById("refresh").addEventListener("click", refresh);
  document.getElementById("services").addEventListener("click", (e) => {
    // A changed API with a fix already out: the row's own way to it.
    const up = e.target.closest("[data-open-update]");
    if (up) { openUrl(updateUrl()); return; }
    const b = e.target.closest("[data-login]");
    if (b) signIn(b.dataset.login);
  });
  document.getElementById("services").addEventListener("scroll", syncServicesScroll, { passive: true });
  document.querySelector(".settings-body").addEventListener("scroll", syncSettingsScroll, { passive: true });
  window.addEventListener("resize", syncServicesScroll);
  window.addEventListener("resize", syncSettingsScroll);

  document.getElementById("settings-btn").addEventListener("click", () =>
    showView(document.getElementById("view-settings").hidden ? "settings" : "usage")
  );
  document.getElementById("back").addEventListener("click", () => showView("usage"));
  document.getElementById("star").addEventListener("click", () => openUrl(REPO));
  document.getElementById("issues").addEventListener("click", () => openUrl(REPO + "/issues"));
  document.getElementById("quit").addEventListener("click", () => invoke("quit").catch(() => {}));

  document.addEventListener("keydown", (e) => {
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.key.toLowerCase() === "r") { e.preventDefault(); refresh(); }
    else if (mod && e.key === ",") { e.preventDefault(); showView("settings"); }
    else if (mod && e.key.toLowerCase() === "q") { e.preventDefault(); invoke("quit").catch(() => {}); }
    else if (e.key === "Escape") {
      if (!document.getElementById("view-settings").hidden) showView("usage");
      else invoke("hide_window").catch(() => {});
    }
  });

  setInterval(tick, 1000);

  // Auto follows the OS: when the desktop flips while the window is open, the
  // stylesheet repaints by itself, and this refreshes what the page read from
  // it (the default accents shown in Settings). Older WebKit only has the
  // deprecated `addListener`, so both spellings are offered.
  const onScheme = () => {
    if (settings.theme === "auto") applyTheme();
  };
  const scheme = matchMedia("(prefers-color-scheme: light)");
  if (scheme.addEventListener) scheme.addEventListener("change", onScheme);
  else if (scheme.addListener) scheme.addListener(onScheme);

  if (PREVIEW) {
    if (params.get("view") === "settings") showView("settings");
    if (params.has("colors")) showDockColorbar(true);
    if (params.has("edge")) setEdgeState(params.get("edge"), params.get("edge-hidden") === "1");
    setTimeout(() => {
      seedDemoHistory(params.get("state"));
      setStatus(demoStatus(params.get("state")));
    }, params.has("instant") ? 0 : 250);
    return;
  }

  TAURI.event.listen("edge-state", (e) => setEdgeState(e.payload.edge, e.payload.hidden));
  invoke("edge_state").then((s) => setEdgeState(s.edge, s.hidden)).catch(() => {});

  invoke("set_tray_mode", { mode: settings.tray }).catch(() => {});
  syncLang();
  TAURI.event.listen("usage", (e) => setStatus(e.payload));
  TAURI.event.listen("web-logins", (e) => { store.store.webLogins = e.payload || []; renderAccounts(); render(); });
  TAURI.event.listen("popover-shown", () => { showView("usage"); renderRings(); renderDock(); });
  setStatus(await invoke("get_status").catch(() => []));
}

boot();

export { showView, applyView, setView, setEdgeState, refresh, setStatus, signIn, loadLogins, render, applyI18n, openUrl };
