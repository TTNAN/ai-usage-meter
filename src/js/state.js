// AI Usage Meter — shared state: constants, the saved settings, the readings,
// the usage history and the per-service colours. Plain ESM, no bundler.
//
// Views import what they render; the entry (`main.js`) wires events. The only
// top-level work here is reading `localStorage` into `settings`, the colours
// and the history — everything else waits for a call.

import {
  parseShortcut, normalizeShortcut, normalizeLastShortcut, rememberShortcut,
} from "./shortcut.js";
import { t, lang } from "./i18n.js";
import { syncColorRows } from "./views/settings.js";
import { syncDockColors, syncDockColorbar } from "./views/dock.js";

const REPO = "https://github.com/TTNAN/ai-usage-meter";

// The rows, in the order they are shown. A new provider joins this list.
const ORDER = ["claude", "codex", "grok", "gemini", "copilot"];

const ACCENT_VAR = {
  claude: "--c-claude",
  codex: "--c-codex",
  grok: "--c-grok",
  gemini: "--c-gemini",
  copilot: "--c-copilot",
};
const ACCENT_FALLBACK = "--c-neutral";
const accent = (id) => `var(${ACCENT_VAR[id] || ACCENT_FALLBACK})`;

// Brand marks that carry their own colours (Gemini's four-colour spark) are
// drawn as images; every other mark is a white glyph cut out with a CSS mask.
const TILE_IMG = { gemini: "icons/gemini.svg" };
const tileGlyph = (id) => (TILE_IMG[id] ? `<img src="${TILE_IMG[id]}" alt="">` : "<i></i>");

// The flat surface each service's squircle is painted in the Dock view — the
// brand's own colour, so a tile reads as the app it measures. The two light
// ones need dark ink, the three dark ones white; `inkFor` sorts out a picked
// colour that would otherwise vanish.
const TILE_BG = {
  codex: "#ffffff",
  claude: "#d97757",
  grok: "#0d0d10",
  gemini: "#eff3fd",
  copilot: "#17252a",
};

// Quick colours offered on every row of the Colors section.
const PRESETS = ["#e08a6b", "#f5b04c", "#6e9bff", "#b38cff", "#5fc27e"];
const HEX = /^#[0-9a-f]{6}$/i;

const CLI = {
  claude: "claude",
  codex: "codex login",
  grok: "grok login",
  gemini: "gemini",
  copilot: "gh auth login",
};

const LABEL = { claude: "Claude", codex: "ChatGPT", grok: "Grok", gemini: "Gemini", copilot: "Copilot" };

// 外观 — 跟随系统 / 深色 / 浅色. `theme` is kept in `localStorage`; `data-theme`
// on <html> is what the stylesheet reads, and "auto" leaves the OS in charge
// (which is also the face that paints before the page has booted).
const THEMES = ["auto", "dark", "light"];
const normalizeTheme = (v) => (THEMES.includes(v) ? v : "auto");

// 面板透明度 — 40–100 %. It only thins the panel's own surfaces: the ink, the
// icons and the dock's brand tiles stay solid however far it is pulled down.
const OPACITY_MIN = 40;
const OPACITY_MAX = 100;

function normalizeOpacity(v) {
  if (v == null || v === "") return OPACITY_MAX;
  const n = Math.round(Number(v));
  if (!Number.isFinite(n)) return OPACITY_MAX;
  return Math.min(OPACITY_MAX, Math.max(OPACITY_MIN, n));
}

const WEB_IDS = ["claude", "grok"];

const TAURI = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__ : null;
const PREVIEW = !TAURI;
const params = new URLSearchParams(location.search);

// Shared mutable state. `settings` holds the choices themselves; everything
// the whole app reads and one module writes lives here, so the views below
// can import one live object instead of passing values around.

const store = {
  status: [],        // what the views render, updated by `setStatus`
  lastUpdate: null,  // when the latest reading landed (ms)
  webLogins: [],     // services signed in through this app
  wayland: false,    // Linux on Wayland: no global shortcut, no window moving
};

const settings = {
  // Which shape the window takes: the rings popover, or the dock panel.
  // Switched from the footer of either view and remembered across launches.
  view: localStorage.getItem("view") || "concentric",
  mode: localStorage.getItem("mode") || "left",
  tray: localStorage.getItem("tray") || "icon",
  lang: localStorage.getItem("lang") || "auto",
  // 外观: which face the app paints. "auto" follows the OS's own preference.
  theme: normalizeTheme(localStorage.getItem("theme")),
  // 面板透明度: 100 keeps the panel exactly as it always was.
  opacity: normalizeOpacity(localStorage.getItem("opacity")),
  // On unless the user turned it off; the tray has to know before the UI opens.
  notify: localStorage.getItem("notify") !== "0",
  hideDisconnected: localStorage.getItem("hide-disconnected") === "1",
  // Dragging the window to a screen edge tucks it there; on by default.
  edgeTuck: localStorage.getItem("edge-hide") !== "0",
  // The global shortcut: an accelerator string, or "" for off.
  shortcut: normalizeShortcut(localStorage.getItem("shortcut")),
  // The chord to turn back on after Off, so the choice isn't forgotten.
  lastShortcut: normalizeLastShortcut(localStorage.getItem("last-shortcut")),
};
// A shortcut that survived a restart is the one to come back to as well.
if (settings.shortcut) rememberShortcut(settings.shortcut);

// ------------------------------------------------------------------ theme
//
// Anything the page works out in JS from the palette — the default service
// colours among them — asks `effectiveTheme()` what is actually painted, and
// `applyTheme()` puts the saved choice where both the stylesheet and the
// native window read it.

/** The face actually shown: a forced Dark or Light, otherwise the OS's. */
function effectiveTheme() {
  const chosen = document.documentElement.dataset.theme || settings.theme;
  if (chosen === "dark" || chosen === "light") return chosen;
  return matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Paint the saved choice, and re-read everything that came from the palette. */
function applyTheme() {
  document.documentElement.dataset.theme = settings.theme;
  // Each theme brings its own default accents: the hex labels and swatches in
  // Settings are read from the stylesheet, so they have to be read again.
  syncColorRows();
  syncDockColors();
  syncDockColorbar();
  // The window paints its own back as well — macOS's vibrancy material, the
  // frame Windows and Linux draw — so hand the choice down to Rust too.
  if (!PREVIEW) invoke("set_theme", { theme: settings.theme }).catch(() => {});
}

// ---------------------------------------------------------- panel opacity

/** Hand the chosen transparency to the stylesheet, and show it as a percentage. */
function applyOpacity() {
  document.documentElement.style.setProperty("--panel-alpha", String(settings.opacity / 100));
  const value = document.getElementById("opacity-value");
  if (value) value.textContent = `${settings.opacity}%`;
  const slider = document.getElementById("set-opacity");
  // Don't fight the thumb while it is being dragged.
  if (slider && document.activeElement !== slider) slider.value = String(settings.opacity);
}

// ------------------------------------------------------- usage history
//
// Rows can say which way a number is going instead of only where it is: a few
// samples per service, kept in `localStorage` as `{ t: ms, left: percent }`,
// cost nothing and make a burn rate possible. Nothing leaves the machine.
const HISTORY_KEY = "usageHistory";
const HISTORY_MAX = 24; // entries per service
const HISTORY_SPAN_MS = 6 * 3600e3; // nothing older than six hours
const HISTORY_GAP_MS = 60e3; // at most one sample a minute…
const BURN_SPAN_MS = 5 * 60e3; // …and at least five minutes to judge a slope

function loadHistory() {
  try {
    const raw = JSON.parse(localStorage.getItem(HISTORY_KEY) || "{}");
    return raw && typeof raw === "object" ? raw : {};
  } catch (_) {
    return {};
  }
}

let history = loadHistory();
// Preview mode seeds a plausible history once; recording over it would flatten
// the demo numbers while a screenshot is being taken.
let demoHistory = false;

function saveHistory() {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
  } catch (_) {}
}

/** What a row is about to show as "left": the tightest window. */
function leftPercent(s) {
  return hasNumbers(s) ? Math.max(0, 100 - peakUsed(s)) : null;
}

/** Keep the history current: a sample when a number changed, or once a minute. */
function recordHistory(next, now = Date.now()) {
  if (demoHistory) return;
  for (const s of next) {
    const left = leftPercent(s);
    if (left == null) continue;
    const value = Math.round(left * 10) / 10;
    const list = history[s.id] || (history[s.id] = []);
    const last = list[list.length - 1];
    if (last && last.left === value && now - last.t < HISTORY_GAP_MS) continue;
    list.push({ t: now, left: value });
    const cutoff = now - HISTORY_SPAN_MS;
    while (list.length > HISTORY_MAX) list.shift();
    while (list.length > 2 && list[0].t < cutoff) list.shift();
  }
  saveHistory();
}

/** Percent per hour a service is losing, or null when the samples can't say. */
function burnRate(id) {
  const list = history[id] || [];
  if (list.length < 2) return null;
  const first = list[0];
  const last = list[list.length - 1];
  const span = last.t - first.t;
  const drop = first.left - last.left;
  if (span < BURN_SPAN_MS || drop <= 0.5) return null;
  return (drop / span) * 3_600_000;
}

/**
 * Seconds until the service runs out at the current rate — only when that is
 * soon enough to matter, i.e. before the window would reset anyway. Silence is
 * the right answer for a quiet service.
 */
function etaSeconds(s) {
  const rate = burnRate(s.id);
  const left = leftPercent(s);
  if (!rate || left == null || left <= 0) return null;
  const secs = (left / rate) * 3600;
  const reset = headline(s).resetsAt;
  const resetIn = reset ? reset - Date.now() / 1000 : Infinity;
  if (!(secs >= 60) || secs > 24 * 3600 || secs >= resetIn) return null;
  return secs;
}

/** `~1h 42m left` / `约 1 小时 42 分用完`. */
function etaText(secs) {
  const total = Math.max(1, Math.round(secs / 60));
  const h = Math.floor(total / 60);
  const m = total % 60;
  return t("eta")(t("etaDur")(h, m));
}

const SPARK_W = 36;
const SPARK_H = 12;

/** A 36×12 SVG of the last samples, or "" when there is nothing to draw. */
function sparkline(id, w = SPARK_W, h = SPARK_H) {
  const list = (history[id] || []).slice(-12);
  if (list.length < 2) return "";
  const values = list.map((p) => p.left);
  const min = Math.min(...values);
  const max = Math.max(...values);
  // A flat line says nothing; leave the space empty instead.
  if (max - min < 0.5) return "";
  const pad = 1.5;
  const x = (i) => pad + (i / (list.length - 1)) * (w - pad * 2);
  const y = (v) => pad + (1 - (v - min) / (max - min)) * (h - pad * 2);
  const points = list.map((p, i) => `${x(i).toFixed(1)},${y(p.left).toFixed(1)}`).join(" ");
  return `<svg class="spark" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}" aria-hidden="true"><polyline points="${points}"/></svg>`;
}

/** Demo history: ten samples five minutes apart, so the sparkline and ETA have
 *  something real to draw from. Copilot's monthly window barely moves, so it
 *  stays without a trend — exactly what a quiet service looks like. */
function seedDemoHistory(kind) {
  const BURN = { claude: 34, codex: 22, grok: 6, gemini: 28 }; // % per hour
  const now = Date.now();
  history = {};
  for (const s of demoStatus(kind)) {
    const rate = BURN[s.id];
    if (!hasNumbers(s) || !rate) continue;
    const left = 100 - peakUsed(s);
    history[s.id] = Array.from({ length: 10 }, (_, i) => {
      const ago = (9 - i) * 5; // minutes before now, oldest first
      return { t: now - ago * 60e3, left: Math.round((left + (rate * ago) / 60) * 10) / 10 };
    });
  }
  demoHistory = true;
  saveHistory();
}

function demoStatus(kind) {
  const now = Date.now() / 1000;
  const w = (k, used, inSecs, secs) => ({ kind: k, usedPercent: used, resetsAt: now + inSecs, windowSeconds: secs });
  if (kind === "empty") {
    return ORDER.map((id) => ({ id, label: LABEL[id], state: "not_connected", plan: null, windows: [], fetchedAt: now }));
  }
  const all = [
    { id: "claude", label: "Claude", state: "mock", plan: "Max", fetchedAt: now,
      windows: [w("session", 42, 2 * 3600 + 14 * 60, 18000), w("weekly", 26, 3 * 86400 + 5 * 3600, 604800)] },
    { id: "codex", label: "ChatGPT", state: "mock", plan: "Plus",
      ...(kind === "mixed" ? { source: "session", fetchedAt: now - 12 * 60 } : { fetchedAt: now }),
      windows: [w("session", 37, 58 * 60, 18000), w("weekly", 18, 5 * 86400, 604800)] },
    kind === "mixed" || kind === "three"
      ? { id: "grok", label: "Grok", state: "not_connected", plan: null, windows: [], fetchedAt: now }
      : { id: "grok", label: "Grok", state: "mock", plan: "SuperGrok", fetchedAt: now,
          windows: [w("weekly", 87, 4 * 86400 + 2 * 3600, 604800)] },
    { id: "gemini", label: "Gemini", state: "mock", plan: "Ultra", fetchedAt: now,
      windows: [w("gemini_pro", 34, 5 * 3600 + 20 * 60, 0), w("gemini_flash", 12, 5 * 3600 + 20 * 60, 0)] },
    kind === "mixed"
      ? { id: "copilot", label: "Copilot", state: "expired", plan: "Individual", windows: [], fetchedAt: now,
          note: "Login expired" }
      : kind === "four" || kind === "three"
        ? { id: "copilot", label: "Copilot", state: "not_connected", plan: null, windows: [], fetchedAt: now }
        : { id: "copilot", label: "Copilot", state: "mock", plan: "Individual", fetchedAt: now,
            windows: [w("premium", 61, 11 * 86400, 0), w("chat", 23, 11 * 86400, 0)] },
  ];
  if (kind === "down") {
    // Two providers that could not be reached, with the kind of reason the
    // real ones put in `note`: the row should say which one it was.
    const down = (id, note) => ({
      id, label: LABEL[id], state: "unavailable", plan: null, windows: [], fetchedAt: now, note,
    });
    all[1] = down("codex", "Couldn't reach ChatGPT (timed out).");
    all[3] = down("gemini", "Couldn't reach Gemini (HTTP 502).");
  }
  if (kind === "changed") {
    // The service answered, but in a shape this app cannot read: the row and
    // the dock tile have to say so, and must not fall back to "not connected".
    all[2] = {
      id: "grok", label: "Grok", state: "changed", plan: null, windows: [], fetchedAt: now,
      note: "grok.com answered in a format this app doesn't recognise — its API has probably changed.",
    };
  }
  return all;
}

async function invoke(cmd, args) {
  if (!TAURI) throw new Error("preview");
  return TAURI.core.invoke(cmd, args || {});
}

const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);
const hasNumbers = (s) => (s.state === "connected" || s.state === "mock") && s.windows && s.windows.length;
const peakUsed = (s) => Math.max(...s.windows.map((w) => w.usedPercent));
const shown = (used) => (settings.mode === "used" ? used : Math.max(0, 100 - used));
/** The window that binds first: highest usage. */
const headline = (s) => s.windows.reduce((a, b) => (b.usedPercent > a.usedPercent ? b : a));

function fmtDur(secs) {
  secs = Math.max(0, Math.round(secs));
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.floor((secs % 3600) / 60);
  return t("dur")(d, h, m);
}
function fmtReset(resetsAt) {
  if (resetsAt == null) return "—";
  const left = resetsAt - Date.now() / 1000;
  return left <= 0 ? t("resetsNow") : fmtDur(left);
}
function fmtAbs(resetsAt) {
  if (resetsAt == null) return "";
  return new Date(resetsAt * 1000).toLocaleString(lang() === "zh" ? "zh-CN" : undefined, {
    weekday: "short", month: "short", day: "numeric", hour: "2-digit", minute: "2-digit",
  });
}

const esc = (s) =>
  String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
const withCode = (s) => esc(s).replace(/`([^`]+)`/g, "<code>$1</code>");

function fmtAgo(unixSecs) {
  const secs = Date.now() / 1000 - unixSecs;
  return secs < 45 ? t("justNow") : t("ago")(fmtDur(secs));
}

function fitWindow() {
  if (PREVIEW) return;
  // Both shapes come from the page: 372 px of rings, or as wide as the dock
  // bar's tiles. Rust clamps, resizes and keeps the window where it was.
  const r = document.getElementById("panel").getBoundingClientRect();
  invoke("set_window_size", { width: Math.ceil(r.width), height: Math.ceil(r.height) }).catch(() => {});
}

// ----------------------------------------------------------------- colors

// One accent per service, kept in `localStorage` and written to `--c-<id>` on
// `:root`, which is all it takes for the hero rings, the progress bars and the
// hover accents to follow. `{}` (nothing stored) means each service keeps its
// own default, which is also what a reset goes back to.
let serviceColors = loadColors();

function loadColors() {
  let raw = {};
  try {
    raw = JSON.parse(localStorage.getItem("serviceColors") || "{}") || {};
  } catch (_) {
    raw = {};
  }
  const out = {};
  for (const id of ORDER) {
    const v = raw[id];
    if (typeof v === "string" && HEX.test(v)) out[id] = v.toLowerCase();
  }
  return out;
}

function saveColors() {
  localStorage.setItem("serviceColors", JSON.stringify(serviceColors));
}

/** The colour a service is drawn in right now — picked, or the theme default. */
function effectiveColor(id) {
  if (serviceColors[id]) return serviceColors[id];
  const v = getComputedStyle(document.documentElement).getPropertyValue(ACCENT_VAR[id] || "").trim();
  // The stylesheet paints one neutral per theme; this is that value for the
  // face currently shown, for the case where the variable cannot be read.
  const neutral = effectiveTheme() === "light" ? "#71717a" : "#9a9aa2";
  return HEX.test(v) ? v.toLowerCase() : neutral;
}

// A colour is only useful if it can be read: WCAG relative luminance and the
// contrast ratio between two hex colours, so the Dock can keep a picked
// colour when it works on a white tile and push it darker (or lighter) on the
// same tile when it would otherwise disappear.
const rgbOf = (hex) => [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
const hexOf = (c) =>
  "#" + c.map((v) => Math.round(Math.max(0, Math.min(255, v))).toString(16).padStart(2, "0")).join("");
function luminance(hex) {
  const [r, g, b] = rgbOf(hex).map((v) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}
function contrast(a, b) {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}
function mixHex(hex, target, t) {
  const a = rgbOf(hex);
  const b = rgbOf(target);
  return hexOf(a.map((v, i) => v + (b[i] - v) * t));
}
/**
 * A colour that reads on `bg`: the picked one itself while it has enough
 * contrast, otherwise the same hue pushed toward black (light tile) or white
 * (dark tile) just far enough to read.
 */
function inkFor(accent, bg) {
  if (contrast(accent, bg) >= 3.0) return accent;
  const target = luminance(bg) > 0.4 ? "#000000" : "#ffffff";
  for (let t = 0.1; t <= 1.0001; t += 0.1) {
    const c = mixHex(accent, target, t);
    if (contrast(c, bg) >= 3.0) return c;
  }
  return target;
}

function applyColors() {
  const root = document.documentElement;
  for (const id of ORDER) {
    if (serviceColors[id]) root.style.setProperty(ACCENT_VAR[id], serviceColors[id]);
    else root.style.removeProperty(ACCENT_VAR[id]);
  }
  syncColorRows();
  syncDockColors();
  syncDockColorbar();
  syncTrayColors();
}

function setColor(id, hex) {
  if (!HEX.test(hex)) return;
  serviceColors[id] = hex.toLowerCase();
  saveColors();
  applyColors();
}

function resetColor(id) {
  delete serviceColors[id];
  saveColors();
  applyColors();
}

function resetAllColors() {
  serviceColors = {};
  saveColors();
  applyColors();
}

/** The tray bars on Windows / Linux are drawn in Rust, so hand the colours over. */
function syncTrayColors() {
  if (PREVIEW) return;
  invoke("set_custom_colors", { colors: serviceColors }).catch(() => {});
}

export {
  REPO, ORDER, ACCENT_VAR, ACCENT_FALLBACK, accent, TILE_IMG, tileGlyph, TILE_BG,
  PRESETS, HEX, CLI, LABEL, WEB_IDS, TAURI, PREVIEW, params, store, settings,
  THEMES, effectiveTheme, applyTheme, normalizeOpacity, applyOpacity,
  invoke, fitWindow, demoStatus, cap, hasNumbers, peakUsed, shown, headline,
  fmtDur, fmtReset, fmtAbs, esc, withCode, fmtAgo,
  loadHistory, saveHistory, recordHistory, leftPercent, burnRate, etaSeconds,
  etaText, sparkline, seedDemoHistory,
  serviceColors, effectiveColor, inkFor, applyColors, setColor, resetColor,
  resetAllColors, syncTrayColors,
};
