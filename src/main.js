// AI Usage Meter — popover UI. Plain JS, no bundler, no dependencies.
// Inside the app it talks to Rust through window.__TAURI__ (withGlobalTauri).
// Opened directly in a browser it runs as a self-contained preview with demo
// data, which is handy for design work and screenshots.

"use strict";

const REPO = "https://github.com/TTNAN/ai-usage-meter";
// The rows, in the order they are shown. A new provider joins this list.
const ORDER = ["claude", "codex", "grok", "gemini", "copilot"];
// The hero gauge is concentric rings, one per connected service, outermost
// first. Three or fewer keep the classic geometry; four and five get their own
// radii and a finer stroke so every service still fits the same 120×120 box.
const RING_RADII = [53, 40, 27];
const RING_W = 10;
// Slots left over when fewer than three services report numbers.
const RING_FALLBACK = ["claude", "codex", "grok"];
// Radii/stroke for the wider sets. Every ring keeps ~3 px of air between it and
// its neighbour, and nothing pokes past the 60 px half-box.
const RING_SETS = {
  4: { width: 7.6, radii: [54.5, 43.4, 32.3, 21.2] },
  5: { width: 6.8, radii: [55, 45, 35, 25, 15] },
};
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
const WEB_IDS = ["claude", "grok"];
let webLogins = [];

const TAURI = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__ : null;
const PREVIEW = !TAURI;
const params = new URLSearchParams(location.search);

// ------------------------------------------------------------------ i18n

const STRINGS = {
  en: {
    title: "AI Usage",
    viewRings: "Rings",
    viewDock: "Dock",
    dockEdge: "Tuck at screen edge",
    dockReset: (d) => `resets in ${d}`,
    notConnectedShort: "Not connected",
    expiredShort: "Expired",
    refresh: "Refresh (⌘R)",
    settings: "Settings",
    showAs: "Show numbers as",
    optLeft: "Left",
    optUsed: "Used",
    menuBar: "Menu bar",
    optIcon: "Icon",
    optPercent: "Icon + %",
    language: "Language",
    optAuto: "Auto",
    shortcut: "Shortcut",
    scOff: "Off",
    scTaken: "Another app already uses that shortcut. Pick the other one, or leave it off.",
    trendTitle: "Recent movement",
    etaDur: (h, m) => (h ? (m ? `${h}h ${m}m` : `${h}h`) : `${m}m`),
    eta: (d) => `~${d}`,
    etaTitle: (d) => `~${d} left`,
    launchAtLogin: "Launch at login",
    lowQuota: "Low-quota alerts",
    hideDisconnected: "Hide disconnected",
    edgeTuck: "Tuck at screen edge",
    colors: "Colors",
    resetAll: "Reset all",
    pick: "Pick a color",
    reset: "Back to default",
    expand: "Expand",
    star: "Star on GitHub",
    report: "Report an issue",
    quit: "Quit AI Usage Meter",
    privacy:
      "Reads the logins your official apps and CLIs already saved, or the accounts you sign in to here (the session stays inside this app). Read-only requests, straight to each service. No servers, no analytics.",
    local: "On-device · read-only",
    demo: "Demo data",
    left: "left",
    used: "used",
    tightest: "Tightest limit",
    busiest: "Most used",
    allClear: "All clear",
    resetsIn: (t) => `resets in ${t}`,
    resetsNow: "resetting now",
    noneTitle: "Nothing connected",
    noneNum: "Sign in to start",
    noneLine:
      "Sign in to Claude or Grok here, or use Claude Code, Codex, Grok Build, Gemini CLI or GitHub Copilot — then it just works.",
    loading: "Reading your quotas…",
    win: {
      session: "5-hour",
      weekly: "Weekly",
      weekly_opus: "Opus",
      weekly_sonnet: "Sonnet",
      gemini_pro: "Pro",
      gemini_flash: "Flash",
      premium: "Premium",
      chat: "Chat",
      period: "Period",
    },
    notConnected: (id) => {
      switch (id) {
        case "codex":
          return "Not connected. Open Codex in the ChatGPT desktop app and sign in, or run `codex login`.";
        case "gemini":
          return "Not connected. Sign in to Gemini CLI once (run `gemini`).";
        case "copilot":
          return "Not connected. Run `gh auth login`, or sign in to the Copilot CLI.";
        default:
          return `Not connected. Sign in to your account, or run \`${CLI[id]}\` once.`;
      }
    },
    webExpired: "Your sign-in expired.",
    signInRow: "Sign in",
    signIn: "Sign in",
    signOut: "Signed in",
    signOutTitle: "Click to sign out",
    acctClaude: "Claude account",
    acctClaudeSub: "claude.ai · same pool as the desktop app",
    acctGrok: "Grok account",
    acctGrokSub: "grok.com · same pool as the Grok app",
    fromSession: (t) => `from your latest Codex session · ${t}`,
    expired: (id) => {
      if (id === "gemini") return "Login expired. Run `gemini` once to refresh it.";
      if (id === "copilot") return "Login expired. Run `gh auth login` again.";
      return `Login expired. Run \`${CLI[id]}\` once to refresh it.`;
    },
    unavailable: (name) => `Couldn't reach ${name}. Retrying shortly.`,
    stale: "last reading",
    justNow: "just now",
    ago: (t) => `${t} ago`,
    dur: (d, h, m) => (d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${Math.max(m, 1)}m`),
  },
  zh: {
    title: "AI 用量",
    viewRings: "环图",
    viewDock: "Dock",
    dockEdge: "贴边自动收起",
    dockReset: (d) => `${d} 后重置`,
    notConnectedShort: "未连接",
    expiredShort: "已过期",
    refresh: "刷新 (⌘R)",
    settings: "设置",
    showAs: "数字显示为",
    optLeft: "剩余",
    optUsed: "已用",
    menuBar: "菜单栏",
    optIcon: "图标",
    optPercent: "图标 + %",
    language: "语言",
    optAuto: "自动",
    shortcut: "全局快捷键",
    scOff: "关闭",
    scTaken: "这个快捷键已被其他应用占用，换一个或选择关闭。",
    trendTitle: "最近用量走势",
    etaDur: (h, m) => (h ? (m ? `${h} 小时 ${m} 分` : `${h} 小时`) : `${m} 分钟`),
    eta: (d) => `约 ${d}`,
    etaTitle: (d) => `约 ${d}用完`,
    launchAtLogin: "开机启动",
    lowQuota: "低额度提醒",
    hideDisconnected: "隐藏未连接服务",
    edgeTuck: "贴边自动收起",
    colors: "服务配色",
    resetAll: "全部恢复",
    pick: "选择颜色",
    reset: "恢复默认",
    expand: "展开",
    star: "在 GitHub 上点个 Star",
    report: "反馈问题",
    quit: "退出 AI Usage Meter",
    privacy: "只读取官方应用和 CLI 已保存的登录，或你在这里登录的账号（登录状态只留在本应用里）。只发只读请求、直连各家服务。没有服务器，没有统计。",
    local: "本地运行 · 只读",
    demo: "演示数据",
    left: "剩余",
    used: "已用",
    tightest: "最紧张的额度",
    busiest: "用得最多",
    allClear: "额度充足",
    resetsIn: (t) => `${t}后重置`,
    resetsNow: "正在重置",
    noneTitle: "尚未连接",
    noneNum: "登录一个账号",
    noneLine:
      "在这里登录 Claude 或 Grok 账号，或用 Claude Code、Codex、Grok Build、Gemini CLI、GitHub Copilot，登录后自动显示。",
    loading: "正在读取额度…",
    win: {
      session: "5 小时",
      weekly: "本周",
      weekly_opus: "Opus",
      weekly_sonnet: "Sonnet",
      gemini_pro: "Pro",
      gemini_flash: "Flash",
      premium: "高级请求",
      chat: "对话",
      period: "周期",
    },
    notConnected: (id) => {
      switch (id) {
        case "codex":
          return "未连接。在 ChatGPT 桌面版里打开 Codex 并登录，或运行 `codex login`。";
        case "gemini":
          return "未连接。运行一次 `gemini` 登录 Gemini CLI。";
        case "copilot":
          return "未连接。运行 `gh auth login`，或登录 Copilot CLI。";
        default:
          return `未连接。登录账号，或运行一次 \`${CLI[id]}\`。`;
      }
    },
    webExpired: "登录已过期。",
    signInRow: "登录账号",
    signIn: "登录",
    signOut: "已登录",
    signOutTitle: "点击退出登录",
    acctClaude: "Claude 账号",
    acctClaudeSub: "claude.ai，与桌面版同一额度",
    acctGrok: "Grok 账号",
    acctGrokSub: "grok.com，与 Grok App 同一额度",
    fromSession: (t) => `来自最近一次 Codex 会话 · ${t}`,
    expired: (id) => {
      if (id === "gemini") return "登录已过期。运行一次 `gemini` 刷新登录。";
      if (id === "copilot") return "登录已过期。重新运行 `gh auth login`。";
      return `登录已过期。运行一次 \`${CLI[id]}\` 刷新登录。`;
    },
    unavailable: (name) => `暂时连不上 ${name}，稍后自动重试。`,
    stale: "上次读数",
    justNow: "刚刚更新",
    ago: (t) => `${t}前`,
    dur: (d, h, m) => (d ? `${d} 天 ${h} 小时` : h ? `${h} 小时 ${m} 分` : `${Math.max(m, 1)} 分钟`),
  },
};

// -------------------------------------------------------------- settings

// The shortcuts Settings offers: two presets and Off. Stored as the plugin
// spells them, so Rust can take the string as-is.
const SHORTCUTS = ["CommandOrControl+Shift+U", "CommandOrControl+Shift+A"];
function normalizeShortcut(v) {
  if (v === null) return SHORTCUTS[0];
  return SHORTCUTS.includes(v) ? v : "";
}

const settings = {
  // Which shape the window takes: the rings popover, or the compact dock bar.
  // Switched from the footer of either view and remembered across launches.
  view: localStorage.getItem("view") || "concentric",
  mode: localStorage.getItem("mode") || "left",
  tray: localStorage.getItem("tray") || "icon",
  lang: localStorage.getItem("lang") || "auto",
  // On unless the user turned it off; the tray has to know before the UI opens.
  notify: localStorage.getItem("notify") !== "0",
  hideDisconnected: localStorage.getItem("hide-disconnected") === "1",
  // Dragging the window to a screen edge tucks it there; on by default.
  edgeTuck: localStorage.getItem("edge-hide") !== "0",
  // The global shortcut: an accelerator string, or "" for off.
  shortcut: normalizeShortcut(localStorage.getItem("shortcut")),
};
function lang() {
  if (settings.lang !== "auto") return settings.lang;
  return (navigator.language || "en").toLowerCase().startsWith("zh") ? "zh" : "en";
}
const t = (key) => STRINGS[lang()][key];

// Rust writes the native low-quota notifications, so it needs the language
// this window resolved too — "auto" is settled right here, against the
// system locale, so the notification and the popover never disagree.
function syncLang() {
  if (PREVIEW) return;
  invoke("set_lang", { lang: lang() }).catch(() => {});
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
  return HEX.test(v) ? v.toLowerCase() : "#9a9aa2";
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

function renderColorRows() {
  const box = document.getElementById("color-rows");
  if (!box) return;
  box.innerHTML = ORDER.map(
    (id) => `<div class="color-row" data-color="${id}">
      <span class="tile ${id}">${tileGlyph(id)}</span>
      <span class="color-name">${esc(LABEL[id])}</span>
      <span class="color-hex" data-hex></span>
      <span class="presets">${PRESETS.map(
        (p) => `<button class="preset" type="button" data-preset="${p}" style="--p:${p}" title="${p.toUpperCase()}"></button>`
      ).join("")}</span>
      <label class="swatch" title="${esc(t("pick"))}">
        <input type="color" data-color-input aria-label="${esc(LABEL[id])} ${esc(t("pick"))}">
      </label>
      <button class="color-reset" type="button" data-reset-color title="${esc(t("reset"))}">
        <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.2 8a4.8 4.8 0 1 0 1.4-3.4M3.2 2.2v2.9h2.9" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>
      </button>
    </div>`
  ).join("");
  syncColorRows();
}

function syncColorRows() {
  document.querySelectorAll(".color-row").forEach((row) => {
    const id = row.dataset.color;
    const hex = effectiveColor(id);
    row.querySelector("[data-hex]").textContent = hex.toUpperCase();
    row.querySelector(".swatch").style.setProperty("--sw", hex);
    const input = row.querySelector("[data-color-input]");
    // Don't fight the open picker while the user drags in it.
    if (document.activeElement !== input) input.value = hex;
    row.querySelector("[data-reset-color]").disabled = !serviceColors[id];
    row.querySelectorAll(".preset").forEach((b) => b.classList.toggle("on", b.dataset.preset === hex));
  });
}

// ------------------------------------------------------------------ data

let status = [];
let lastUpdate = null;
let refreshing = false;

async function invoke(cmd, args) {
  if (!TAURI) throw new Error("preview");
  return TAURI.core.invoke(cmd, args || {});
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
  return all;
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

/** The row's trend: a sparkline, an ETA, or both — omitted when neither is known. */
function trendHtml(s) {
  if (!hasNumbers(s)) return "";
  const eta = etaSeconds(s);
  const spark = sparkline(s.id);
  if (!spark && eta == null) return "";
  const label = eta == null ? "" : `<span class="eta">${esc(etaText(eta))}</span>`;
  const title = eta == null ? t("trendTitle") : t("etaTitle")(t("etaDur")(Math.floor(Math.round(eta / 60) / 60), Math.round(eta / 60) % 60));
  return `<span class="trend" title="${esc(title)}">${spark}${label}</span>`;
}

// ---------------------------------------------------------------- render

// Which services the hero rings show: every service with numbers, closest to
// running out first, so the outermost ring is the number beside it. Three or
// fewer keep the three-slot shape (padded with Claude / Codex / Grok, exactly
// like the tray icon does in Rust), four and five show all of them.
function ringIds() {
  // Before the first reading, keep the usual three slots so the hero has its
  // shape while it loads.
  if (!status.length) return RING_FALLBACK.slice(0, RING_RADII.length);
  const left = (s) => 100 - peakUsed(s);
  const live = status
    .filter(hasNumbers)
    .slice()
    .sort((a, b) => left(a) - left(b))
    .map((s) => s.id);
  if (live.length > RING_RADII.length) return live.slice(0, 5);
  const ids = live.slice();
  for (const id of RING_FALLBACK) {
    if (ids.length >= RING_RADII.length) break;
    if (!ids.includes(id) && status.some((s) => s.id === id)) ids.push(id);
  }
  return ids;
}

/** Stroke width and radii for a given number of rings. */
function ringGeometry(n) {
  return RING_SETS[n] || { width: RING_W, radii: RING_RADII };
}

let RINGS = [];

function buildRings(ids) {
  const svg = document.getElementById("rings");
  const g = ringGeometry(ids.length);
  RINGS = ids.map((id, i) => ({ id, r: g.radii[i], w: g.width }));
  svg.innerHTML = RINGS.map(({ id, r, w }) => {
    const c = 2 * Math.PI * r;
    return `<circle class="track" cx="60" cy="60" r="${r}" stroke-width="${w}"/>
      <circle class="arc off" data-ring="${id}" cx="60" cy="60" r="${r}" stroke-width="${w}"
        stroke="${accent(id)}" stroke-dasharray="${c}" stroke-dashoffset="${c}"/>`;
  }).join("");
}

function syncRings() {
  const ids = ringIds();
  if (ids.join() !== RINGS.map((r) => r.id).join()) buildRings(ids);
}

function renderRings() {
  for (const { id, r } of RINGS) {
    const arc = document.querySelector(`[data-ring="${id}"]`);
    if (!arc) continue;
    const s = status.find((x) => x.id === id);
    const c = 2 * Math.PI * r;
    if (!s || !hasNumbers(s)) {
      arc.classList.add("off");
      arc.style.strokeDashoffset = c;
      continue;
    }
    const frac = shown(peakUsed(s)) / 100;
    arc.classList.remove("off");
    // Never draw a bare dot: keep a minimum sweep when anything is left.
    const sweep = frac <= 0 ? 0 : Math.max(frac, 0.025);
    requestAnimationFrame(() => (arc.style.strokeDashoffset = c * (1 - sweep)));
  }
}

function renderHero() {
  const el = document.getElementById("hero-copy");
  const hero = document.getElementById("hero");
  const live = status.filter(hasNumbers);
  hero.classList.toggle("hero-empty", !live.length);

  if (!status.length) {
    el.innerHTML = `<div class="eyebrow">&nbsp;</div><div class="shimmer" style="width:120px;height:30px;margin:8px 0"></div><div class="hero-line">${esc(t("loading"))}</div>`;
    return;
  }
  if (!live.length) {
    el.innerHTML = `<div class="eyebrow">${esc(t("noneTitle"))}</div>
      <div class="hero-num">${esc(t("noneNum"))}</div>
      <div class="hero-line">${esc(t("noneLine"))}</div>`;
    return;
  }
  // The binding constraint is the service with the most used.
  const top = live.reduce((a, b) => (peakUsed(b) > peakUsed(a) ? b : a));
  const used = peakUsed(top);
  const value = shown(used);
  const eyebrow = used < 50 && settings.mode === "left" ? t("allClear") : settings.mode === "used" ? t("busiest") : t("tightest");
  const w = headline(top);
  el.innerHTML = `<div class="eyebrow">${esc(eyebrow)}</div>
    <div class="hero-num">${value.toFixed(0)}<small>%</small></div>
    <div class="hero-line"><b>${esc(top.label)}</b> · ${esc(t("win")[w.kind] || "")} ·
      <span data-reset="${w.resetsAt ?? ""}" data-reset-fmt="sentence">${esc(resetSentence(w.resetsAt))}</span></div>`;
}

function resetSentence(resetsAt) {
  if (resetsAt == null || resetsAt === "") return "";
  const left = resetsAt - Date.now() / 1000;
  return left <= 0 ? t("resetsNow") : t("resetsIn")(fmtDur(left));
}

/** A countdown in the timer's own short hand — `2h 13m`, the same in every
 *  language, because a timer reads the same everywhere. */
function fmtTimer(secs) {
  const d = Math.floor(secs / 86400);
  const h = Math.floor((secs % 86400) / 3600);
  const m = Math.max(0, Math.floor((secs % 3600) / 60));
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${Math.max(1, m)}m`;
}

/** The Dock's hover line: `2h 13m 后重置` / `resets in 2h 13m`. */
function dockReset(resetsAt) {
  if (resetsAt == null || resetsAt === "") return "—";
  const left = resetsAt - Date.now() / 1000;
  return left <= 0 ? t("resetsNow") : t("dockReset")(fmtTimer(left));
}

function serviceRow(s) {
  const live = hasNumbers(s);
  const used = live ? peakUsed(s) : null;
  const left = used == null ? null : 100 - used;
  const cls = ["svc", s.id];
  if (!live) cls.push("muted");
  if (left != null && left <= 15) cls.push("low");
  if (left != null && left <= 2) cls.push("out");

  const value = live
    ? `<div class="svc-value"><strong>${shown(used).toFixed(0)}%</strong><span>${esc(t(settings.mode === "used" ? "used" : "left"))}</span></div>`
    : "";
  const plan = s.plan ? `<span class="plan">${esc(s.plan)}</span>` : "";
  const trend = live ? trendHtml(s) : "";

  let body = "";
  if (live) {
    body = `<div class="windows">${s.windows
      .map((w) => {
        const frac = shown(w.usedPercent) / 100;
        return `<div class="win" title="${esc(fmtAbs(w.resetsAt))}">
          <span class="win-label">${esc(t("win")[w.kind] || w.kind)}</span>
          <span class="bar"><i style="transform:scaleX(0)" data-frac="${frac}"></i></span>
          <span class="win-reset" data-reset="${w.resetsAt ?? ""}">${esc(fmtReset(w.resetsAt))}</span>
        </div>`;
      })
      .join("")}</div>`;
    if (s.stale) body += `<div class="svc-msg"><span class="dot warn"></span><span class="stale-tag">${esc(t("stale"))}</span></div>`;
    else if (s.source === "session")
      body += `<div class="svc-msg"><span class="dot"></span><span data-ago="${s.fetchedAt}">${esc(t("fromSession")(fmtAgo(s.fetchedAt)))}</span></div>`;
  } else {
    const msg =
      s.state === "expired" && s.source === "web" ? t("webExpired")
      : s.state === "expired" ? t("expired")(s.id)
      : s.state === "unavailable" ? t("unavailable")(s.label)
      : t("notConnected")(s.id);
    const warn = s.state === "expired" || s.state === "unavailable";
    const login = WEB_IDS.includes(s.id) && (s.state !== "expired" || s.source === "web" || !webLogins.includes(s.id))
      ? ` <button class="inline-btn" type="button" data-login="${s.id}">${esc(t("signInRow"))} →</button>` : "";
    body = `<div class="svc-msg" title="${esc(s.note || "")}"><span class="dot ${warn ? "warn" : ""}"></span><span>${withCode(msg)}${login}</span></div>`;
  }

  return `<li class="${cls.join(" ")}" style="--accent: ${accent(s.id)}">
    <div class="svc-head">
      <span class="tile ${s.id}">${tileGlyph(s.id)}</span>
      <span class="svc-name">${esc(s.label)}</span>${plan}${trend}${value}
    </div>${body}</li>`;
}

function visibleRows() {
  const rows = ORDER.map((id) => status.find((s) => s.id === id)).filter(Boolean);
  if (!settings.hideDisconnected) return rows;
  // Only hide the quiet ones while something else is actually reporting: with
  // nothing connected, every row is the instructions for connecting.
  const live = rows.some((s) => s.state === "connected" || s.state === "mock");
  return live ? rows.filter((s) => s.state !== "not_connected") : rows;
}

// --------------------------------------------------------------- dock
// The compact view: one 88 px tile per service. Everything the rings view
// spells out in a row — the binding window, when it resets, where it is going
// — stays one hover away (the tooltip and the strip under the bar), so the
// tiles themselves are only the icon, the number and its one-word label.

function dockLabel(s) {
  if (!hasNumbers(s)) {
    if (s.state === "loading") return "…";
    return s.state === "expired" ? t("expiredShort") : t("notConnectedShort");
  }
  return t(settings.mode === "used" ? "used" : "left");
}

/** The tile's tooltip: name and plan, the binding window, the reset, the ETA. */
function dockTitle(s) {
  const head = s.plan ? `${s.label} · ${s.plan}` : s.label;
  if (!hasNumbers(s)) {
    if (s.state === "loading") return `${head}\n${t("loading")}`;
    const why =
      s.state === "expired" ? t("expiredShort")
      : s.state === "unavailable" ? t("unavailable")(s.label)
      : t("notConnected")(s.id);
    return `${head}\n${why}`;
  }
  const w = headline(s);
  const lines = [
    head,
    `${dockLabel(s)} ${shown(peakUsed(s)).toFixed(0)}% · ${t("win")[w.kind] || w.kind}`,
    resetSentence(w.resetsAt),
  ];
  const eta = etaSeconds(s);
  if (eta != null) lines.push(etaText(eta));
  return lines.filter(Boolean).join("\n");
}

function dockTile(s) {
  const live = hasNumbers(s);
  const left = live ? 100 - peakUsed(s) : null;
  const cls = ["dtile"];
  if (!live) cls.push("off");
  if (left != null && left <= 2) cls.push("out");
  // A colour picked in Settings tints this tile's number and bar, through the
  // contrast-safe ink; the surface and the mark stay the brand's own.
  const pick = serviceColors[s.id];
  const ink = pick ? inkFor(pick, TILE_BG[s.id]) : null;
  if (ink) cls.push("custom");
  const value = live ? Math.max(0, Math.min(100, Math.round(shown(peakUsed(s))))) : null;
  const aria = live ? `${s.label} ${value}% ${dockLabel(s)}` : `${s.label} ${dockLabel(s)}`;
  const inner = live
    ? `<span class="dicon">${tileGlyph(s.id)}</span>
       <span class="dpct"><b>${value}</b><em>%</em><span class="dword">${esc(dockLabel(s))}</span></span>
       <span class="dbar"><i style="width:${value}%"></i></span>`
    : `<span class="dicon">${tileGlyph(s.id)}</span>
       <span class="doff">${esc(dockLabel(s))}</span>`;
  return `<button class="${cls.join(" ")}" type="button" data-id="${s.id}"
      style="--accent: ${accent(s.id)}${ink ? `; --ink: ${ink}` : ""}" title="${esc(dockTitle(s))}" aria-label="${esc(aria)}">
    <span class="dtsq">${inner}</span>
    <span class="dcap">${esc(s.label)}</span>
  </button>`;
}

function renderDock() {
  const dock = document.getElementById("dock");
  if (!dock) return;
  // Before the first reading the bar keeps its five places, so the window does
  // not grow into them once the numbers land.
  const rows = status.length
    ? visibleRows()
    : ORDER.map((id) => ({ id, label: LABEL[id], state: "loading", windows: [] }));
  dock.innerHTML = rows.map(dockTile).join("");
  document.getElementById("panel").dataset.count = String(rows.length);
  dockDetail(null);
}

/**
 * The one line of the tray's footer: the hovered tile's reset countdown and
 * its burn-rate ETA. Nothing at all when no tile is hovered — the bar stays
 * as quiet as the tin it is drawn after.
 */
function dockDetail(s) {
  const el = document.getElementById("dock-detail");
  if (!el) return;
  if (!s) {
    el.textContent = "";
    el.classList.remove("on");
    return;
  }
  if (!hasNumbers(s)) {
    el.textContent =
      s.state === "loading" ? t("loading")
      : s.state === "expired" ? t("expiredShort")
      : t("notConnectedShort");
    el.classList.add("on");
    return;
  }
  const w = headline(s);
  const eta = etaSeconds(s);
  el.innerHTML =
    `<span data-reset="${w.resetsAt ?? ""}" data-reset-fmt="timer">${esc(dockReset(w.resetsAt))}</span>` +
    (eta != null ? `<span class="dock-sep">·</span><span class="dock-eta">${esc(t("eta")(fmtTimer(eta)))}</span>` : "");
  el.classList.add("on");
  // A three-tile dock leaves the footer a narrow gap: drop the ETA rather than
  // cut a number in half. The countdown — the part that moves — always stays.
  const extra = el.querySelector(".dock-eta");
  if (extra && el.scrollWidth > el.clientWidth) {
    const sep = el.querySelector(".dock-sep");
    if (sep) sep.remove();
    extra.remove();
  }
}

function renderServices() {
  const ul = document.getElementById("services");
  if (!status.length) {
    ul.innerHTML = ORDER.map(
      (id) => `<li class="svc skeleton"><div class="svc-head"><span class="tile ${id}">${tileGlyph(id)}</span>
        <span class="shimmer" style="width:90px"></span></div>
        <div class="windows"><div class="shimmer" style="width:100%;height:4px"></div></div></li>`
    ).join("");
    return;
  }
  ul.innerHTML = visibleRows().map(serviceRow).join("");
  requestAnimationFrame(() =>
    requestAnimationFrame(() => {
      ul.querySelectorAll(".bar > i").forEach((i) => {
        const f = parseFloat(i.dataset.frac);
        i.style.transform = `scaleX(${f <= 0 ? 0 : Math.max(f, 0.02)})`;
      });
      syncServicesScroll();
    })
  );
}

/** Say "there is more below" only while a row really is cut off. */
function syncServicesScroll() {
  const el = document.getElementById("services");
  const hidden = el.scrollHeight - el.clientHeight > 1;
  el.classList.toggle("overflowing", hidden && el.scrollTop + el.clientHeight < el.scrollHeight - 2);
}

function render() {
  renderHero();
  syncRings();
  renderRings();
  renderServices();
  renderDock();
  document.getElementById("demo-chip").hidden = params.has("clean") || !status.some((s) => s.state === "mock");
  tick();
}

function renderAccounts() {
  document.querySelectorAll("[data-acct]").forEach((b) => {
    const on = webLogins.includes(b.dataset.acct);
    b.classList.toggle("on", on);
    b.textContent = t(on ? "signOut" : "signIn");
    b.title = on ? t("signOutTitle") : "";
  });
}
function loadLogins() {
  if (PREVIEW) { webLogins = params.get("state") === "mixed" ? ["claude"] : []; renderAccounts(); return; }
  invoke("web_logins").then((v) => { webLogins = v || []; renderAccounts(); }).catch(() => {});
}
function signIn(id) {
  if (PREVIEW) return;
  invoke("web_login", { id }).catch(() => {});
}

function fmtAgo(unixSecs) {
  const secs = Date.now() / 1000 - unixSecs;
  return secs < 45 ? t("justNow") : t("ago")(fmtDur(secs));
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
  const up = document.getElementById("updated");
  if (!lastUpdate) { up.textContent = ""; return; }
  const secs = (Date.now() - lastUpdate) / 1000;
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
  syncViewSegs();
  syncSettingsScroll();
}

// --------------------------------------------------------------- actions

function setStatus(next) {
  if (!Array.isArray(next) || !next.length) return;
  status = next;
  lastUpdate = Date.now();
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

/** Say "there is more below" only while a settings group really is cut off. */
function syncSettingsScroll() {
  const el = document.querySelector(".settings-body");
  if (!el) return;
  const more = el.scrollHeight - el.clientHeight > 1;
  el.classList.toggle("overflowing", more && el.scrollTop + el.clientHeight < el.scrollHeight - 2);
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
  if (settingsOpen) syncSettingsScroll();
  fitWindow();
}

/** Switch between the rings popover and the compact dock bar. */
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

function fitWindow() {
  if (PREVIEW) return;
  // Both shapes come from the page: 372 px of rings, or as wide as the dock
  // bar's tiles. Rust clamps, resizes and keeps the window where it was.
  const r = document.getElementById("panel").getBoundingClientRect();
  invoke("set_window_size", { width: Math.ceil(r.width), height: Math.ceil(r.height) }).catch(() => {});
}

// ------------------------------------------------------------- shortcut

/** What the preset reads on this platform; Off is a word too. */
const IS_MAC = /Mac/.test(navigator.platform);

function shortcutLabel(keys) {
  if (!keys) return t("scOff");
  const tail = keys.split("+").pop();
  return IS_MAC ? `⌘⇧${tail}` : `Ctrl+Shift+${tail}`;
}

function paintShortcutSeg() {
  document.querySelectorAll('[data-setting="shortcut"] button').forEach((b) => {
    b.textContent = shortcutLabel(b.dataset.value);
  });
}

/**
 * Hand the chosen shortcut to Rust, which is the only place that knows whether
 * it could actually be held: a combination another app owns comes back as an
 * error, and then the switch shows Off rather than a key that does nothing.
 */
function applyShortcut() {
  paintShortcutSeg();
  const note = document.getElementById("shortcut-note");
  note.hidden = true;
  if (PREVIEW) return;
  const wanted = settings.shortcut;
  invoke("set_global_shortcut", { keys: wanted })
    .then((held) => {
      if (held === wanted) return;
      settings.shortcut = held || "";
      localStorage.setItem("shortcut", settings.shortcut);
      paintShortcutSeg();
      showShortcutState();
    })
    .catch(() => {
      // Nothing is held now, so the switch has to say so.
      settings.shortcut = "";
      localStorage.setItem("shortcut", "");
      paintShortcutSeg();
      document.querySelectorAll('.seg[data-setting="shortcut"] button').forEach((b) =>
        b.classList.toggle("on", b.dataset.value === "")
      );
      note.textContent = t("scTaken");
      note.hidden = false;
    });
}

/** Reflect the stored choice in the segmented control (i18n repaints it too). */
function showShortcutState() {
  document.querySelectorAll('.seg[data-setting="shortcut"] button').forEach((b) =>
    b.classList.toggle("on", b.dataset.value === settings.shortcut)
  );
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

// ------------------------------------------------------------------ boot

async function boot() {
  const root = document.documentElement;
  let platform = /Mac/.test(navigator.platform) ? "macos" : "windows";

  // The saved shape, chosen before anything is painted. In the browser
  // preview `?view=dock` / `?view=settings` / `?hide=1` open it directly.
  const wantView = params.get("view");
  if (wantView === "dock" || wantView === "concentric") settings.view = wantView;
  const wantLang = params.get("lang");
  if (wantLang === "zh" || wantLang === "en") settings.lang = wantLang;
  if (params.get("hide") === "1") settings.hideDisconnected = true;
  document.getElementById("panel").dataset.view = settings.view;

  if (PREVIEW) {
    root.classList.add("preview");
    if (params.has("bare")) root.classList.add("bare");
    // `?frame=1`: the panel alone on a transparent page (README hero shots).
    if (params.has("frame")) root.classList.add("frame");
  } else {
    try {
      const info = await invoke("app_info");
      platform = info.platform;
      document.getElementById("version").textContent = "v" + info.version;
    } catch (_) {}
    if (platform === "macos") root.classList.add("vibrancy");
  }
  if (PREVIEW) document.getElementById("version").textContent = "v0.7.0";
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

  // Size the native window to the panel, whatever its content.
  new ResizeObserver(fitWindow).observe(document.getElementById("panel"));

  // Settings controls.
  document.querySelectorAll(".seg[data-setting]").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      const key = seg.dataset.setting;
      settings[key] = b.dataset.value;
      localStorage.setItem(key, settings[key]);
      if (key === "tray") invoke("set_tray_mode", { mode: settings.tray }).catch(() => {});
      if (key === "shortcut") applyShortcut();
      if (key === "lang") syncLang();
      applyI18n();
      render();
    })
  );
  loadLogins();
  const auto = document.getElementById("autostart");
  if (!PREVIEW) invoke("get_autostart").then((v) => (auto.checked = !!v)).catch(() => {});
  auto.addEventListener("change", () => {
    if (PREVIEW) return;
    invoke("set_autostart", { enabled: auto.checked }).then((v) => (auto.checked = !!v)).catch(() => (auto.checked = !auto.checked));
  });

  const notify = document.getElementById("notify");
  notify.checked = settings.notify;
  notify.addEventListener("change", () => {
    settings.notify = notify.checked;
    localStorage.setItem("notify", settings.notify ? "1" : "0");
    if (!PREVIEW) invoke("set_notify_enabled", { enabled: settings.notify }).catch(() => {});
  });
  if (!PREVIEW) invoke("set_notify_enabled", { enabled: settings.notify }).catch(() => {});

  const hide = document.getElementById("hide-disconnected");
  hide.checked = settings.hideDisconnected;
  hide.addEventListener("change", () => {
    settings.hideDisconnected = hide.checked;
    localStorage.setItem("hide-disconnected", settings.hideDisconnected ? "1" : "0");
    render();
  });

  const edgeSwitch = document.getElementById("edge-hide");
  edgeSwitch.checked = settings.edgeTuck;
  edgeSwitch.addEventListener("change", () => {
    settings.edgeTuck = edgeSwitch.checked;
    localStorage.setItem("edge-hide", settings.edgeTuck ? "1" : "0");
    syncDockEdge();
    if (!PREVIEW) invoke("set_edge_enabled", { enabled: settings.edgeTuck }).catch(() => {});
  });
  if (!PREVIEW) invoke("set_edge_enabled", { enabled: settings.edgeTuck }).catch(() => {});

  // The dock bar carries its own copies of the controls that matter there:
  // view, Used/Left, edge tuck, settings, refresh. Same state, same storage.
  const dockEdge = document.getElementById("dock-edge");
  function syncDockEdge() {
    dockEdge.classList.toggle("on", settings.edgeTuck);
    dockEdge.setAttribute("aria-pressed", settings.edgeTuck ? "true" : "false");
  }
  syncDockEdge();
  dockEdge.addEventListener("click", () => {
    settings.edgeTuck = !settings.edgeTuck;
    localStorage.setItem("edge-hide", settings.edgeTuck ? "1" : "0");
    edgeSwitch.checked = settings.edgeTuck;
    syncDockEdge();
    if (!PREVIEW) invoke("set_edge_enabled", { enabled: settings.edgeTuck }).catch(() => {});
  });
  document.getElementById("dock-settings").addEventListener("click", () => showView("settings"));
  document.getElementById("dock-refresh").addEventListener("click", refresh);
  document.querySelectorAll(".view-seg").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (b) setView(b.dataset.view);
    })
  );
  const dock = document.getElementById("dock");
  dock.addEventListener("click", (e) => {
    const tile = e.target.closest(".dtile");
    if (!tile) return;
    const s = status.find((x) => x.id === tile.dataset.id);
    if (!s || hasNumbers(s)) return;
    // A quiet tile is the way to fix it: web sign-in where the app can do it,
    // and the drawer (which says how to connect) for the CLI-backed ones.
    if (WEB_IDS.includes(s.id)) signIn(s.id);
    else showView("settings");
  });
  dock.addEventListener("mouseover", (e) => {
    const tile = e.target.closest(".dtile");
    if (!tile) return;
    dockDetail(status.find((x) => x.id === tile.dataset.id));
  });
  dock.addEventListener("mouseleave", () => dockDetail(null));

  // Colors: the native picker streams "input" while it is open, so the rings
  // and bars repaint as the user drags through it.
  const colorRows = document.getElementById("color-rows");
  colorRows.addEventListener("input", (e) => {
    const input = e.target.closest("[data-color-input]");
    const row = input && input.closest(".color-row");
    if (row) setColor(row.dataset.color, input.value);
  });
  colorRows.addEventListener("click", (e) => {
    const row = e.target.closest(".color-row");
    if (!row) return;
    const preset = e.target.closest("[data-preset]");
    if (preset) setColor(row.dataset.color, preset.dataset.preset);
    else if (e.target.closest("[data-reset-color]")) resetColor(row.dataset.color);
  });
  document.getElementById("colors-reset-all").addEventListener("click", resetAllColors);
  document.getElementById("edge-handle").addEventListener("click", () => invoke("expand_docked").catch(() => {}));

  document.getElementById("refresh").addEventListener("click", refresh);
  document.getElementById("services").addEventListener("click", (e) => {
    const b = e.target.closest("[data-login]");
    if (b) signIn(b.dataset.login);
  });
  document.getElementById("services").addEventListener("scroll", syncServicesScroll, { passive: true });
  document.querySelector(".settings-body").addEventListener("scroll", syncSettingsScroll, { passive: true });
  window.addEventListener("resize", syncServicesScroll);
  window.addEventListener("resize", syncSettingsScroll);
  document.querySelectorAll("[data-acct]").forEach((b) =>
    b.addEventListener("click", () => {
      const id = b.dataset.acct;
      if (webLogins.includes(id)) {
        invoke("web_logout", { id }).then(loadLogins).catch(() => {});
      } else signIn(id);
    })
  );
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

  if (PREVIEW) {
    if (params.get("view") === "settings") showView("settings");
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
  TAURI.event.listen("web-logins", (e) => { webLogins = e.payload || []; renderAccounts(); render(); });
  TAURI.event.listen("popover-shown", () => { showView("usage"); renderRings(); renderDock(); });
  setStatus(await invoke("get_status").catch(() => []));
}

boot();
