// AI Usage Meter — popover UI. Plain JS, no bundler, no dependencies.
// Inside the app it talks to Rust through window.__TAURI__ (withGlobalTauri).
// Opened directly in a browser it runs as a self-contained preview with demo
// data, which is handy for design work and screenshots.

"use strict";

const REPO = "https://github.com/TTNAN/ai-usage-meter";
const ORDER = ["claude", "codex", "grok"];
const ACCENT_VAR = { claude: "--c-claude", codex: "--c-codex", grok: "--c-grok" };
const CLI = { claude: "claude", codex: "codex login", grok: "grok login" };
const LABEL = { claude: "Claude", codex: "ChatGPT", grok: "Grok" };
const WEB_IDS = ["claude", "grok"];
let webLogins = [];

const TAURI = window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__ : null;
const PREVIEW = !TAURI;
const params = new URLSearchParams(location.search);

// ------------------------------------------------------------------ i18n

const STRINGS = {
  en: {
    title: "AI Usage",
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
    launchAtLogin: "Launch at login",
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
    noneLine: "Sign in to Claude or Grok here, or use ChatGPT (Codex), Claude Code or Grok Build — then it just works.",
    loading: "Reading your quotas…",
    win: { session: "5-hour", weekly: "Weekly", weekly_opus: "Opus", weekly_sonnet: "Sonnet", period: "Period" },
    notConnected: (id) =>
      id === "codex"
        ? "Not connected. Open Codex in the ChatGPT desktop app and sign in, or run `codex login`."
        : `Not connected. Sign in to your account, or run \`${CLI[id]}\` once.`,
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
    expired: (id) => `Login expired. Run \`${CLI[id]}\` once to refresh it.`,
    unavailable: (name) => `Couldn't reach ${name}. Retrying shortly.`,
    stale: "last reading",
    justNow: "just now",
    ago: (t) => `${t} ago`,
    dur: (d, h, m) => (d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${Math.max(m, 1)}m`),
  },
  zh: {
    title: "AI 用量",
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
    launchAtLogin: "开机启动",
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
    noneLine: "在这里登录 Claude 或 Grok 账号，或用 ChatGPT（Codex）、Claude Code、Grok Build，登录后自动显示。",
    loading: "正在读取额度…",
    win: { session: "5 小时", weekly: "本周", weekly_opus: "Opus", weekly_sonnet: "Sonnet", period: "周期" },
    notConnected: (id) =>
      id === "codex"
        ? "未连接。在 ChatGPT 桌面版里打开 Codex 并登录，或运行 `codex login`。"
        : `未连接。登录账号，或运行一次 \`${CLI[id]}\`。`,
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
    expired: (id) => `登录已过期。运行一次 \`${CLI[id]}\` 刷新登录。`,
    unavailable: (name) => `暂时连不上 ${name}，稍后自动重试。`,
    stale: "上次读数",
    justNow: "刚刚更新",
    ago: (t) => `${t}前`,
    dur: (d, h, m) => (d ? `${d} 天 ${h} 小时` : h ? `${h} 小时 ${m} 分` : `${Math.max(m, 1)} 分钟`),
  },
};

// -------------------------------------------------------------- settings

const settings = {
  mode: localStorage.getItem("mode") || "left",
  tray: localStorage.getItem("tray") || "icon",
  lang: localStorage.getItem("lang") || "auto",
};
function lang() {
  if (settings.lang !== "auto") return settings.lang;
  return (navigator.language || "en").toLowerCase().startsWith("zh") ? "zh" : "en";
}
const t = (key) => STRINGS[lang()][key];

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
  return [
    { id: "claude", label: "Claude", state: "mock", plan: "Max", fetchedAt: now,
      windows: [w("session", 42, 2 * 3600 + 14 * 60, 18000), w("weekly", 26, 3 * 86400 + 5 * 3600, 604800)] },
    { id: "codex", label: "ChatGPT", state: "mock", plan: "Plus",
      ...(kind === "mixed" ? { source: "session", fetchedAt: now - 12 * 60 } : { fetchedAt: now }),
      windows: [w("session", 37, 58 * 60, 18000), w("weekly", 18, 5 * 86400, 604800)] },
    kind === "mixed"
      ? { id: "grok", label: "Grok", state: "not_connected", plan: null, windows: [], fetchedAt: now }
      : { id: "grok", label: "Grok", state: "mock", plan: "SuperGrok", fetchedAt: now,
          windows: [w("weekly", 87, 4 * 86400 + 2 * 3600, 604800)] },
  ];
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

// ---------------------------------------------------------------- render

const RINGS = [
  { id: "claude", r: 53 },
  { id: "codex", r: 40 },
  { id: "grok", r: 27 },
];
const RING_W = 10;

function buildRings() {
  const svg = document.getElementById("rings");
  svg.innerHTML = RINGS.map(({ id, r }) => {
    const c = 2 * Math.PI * r;
    return `<circle class="track" cx="60" cy="60" r="${r}" stroke-width="${RING_W}"/>
      <circle class="arc off" data-ring="${id}" cx="60" cy="60" r="${r}" stroke-width="${RING_W}"
        stroke="var(${ACCENT_VAR[id]})" stroke-dasharray="${c}" stroke-dashoffset="${c}"/>`;
  }).join("");
}

function renderRings() {
  for (const { id, r } of RINGS) {
    const arc = document.querySelector(`[data-ring="${id}"]`);
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

  return `<li class="${cls.join(" ")}" style="--accent: var(${ACCENT_VAR[s.id]})">
    <div class="svc-head">
      <span class="tile ${s.id}"><i></i></span>
      <span class="svc-name">${esc(s.label)}</span>${plan}${value}
    </div>${body}</li>`;
}

function renderServices() {
  const ul = document.getElementById("services");
  if (!status.length) {
    ul.innerHTML = ORDER.map(
      (id) => `<li class="svc skeleton"><div class="svc-head"><span class="tile ${id}"><i></i></span>
        <span class="shimmer" style="width:90px"></span></div>
        <div class="windows"><div class="shimmer" style="width:100%;height:4px"></div></div></li>`
    ).join("");
    return;
  }
  ul.innerHTML = ORDER.map((id) => status.find((s) => s.id === id)).filter(Boolean).map(serviceRow).join("");
  requestAnimationFrame(() =>
    requestAnimationFrame(() =>
      ul.querySelectorAll(".bar > i").forEach((i) => {
        const f = parseFloat(i.dataset.frac);
        i.style.transform = `scaleX(${f <= 0 ? 0 : Math.max(f, 0.02)})`;
      })
    )
  );
}

function render() {
  renderHero();
  renderRings();
  renderServices();
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
    el.textContent = el.dataset.resetFmt === "sentence" ? resetSentence(v) : fmtReset(v);
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
  document.querySelectorAll(".seg").forEach((seg) => {
    seg.querySelectorAll("button").forEach((b) => b.classList.toggle("on", b.dataset.value === settings[seg.dataset.setting]));
  });
  renderAccounts();
}

// --------------------------------------------------------------- actions

function setStatus(next) {
  if (!Array.isArray(next) || !next.length) return;
  status = next;
  lastUpdate = Date.now();
  render();
}

async function refresh() {
  if (refreshing) return;
  refreshing = true;
  const btn = document.getElementById("refresh");
  btn.classList.add("spinning");
  const started = Date.now();
  try {
    if (PREVIEW) setStatus(demoStatus(params.get("state")));
    else setStatus(await invoke("refresh_status"));
  } catch (e) {
    console.error(e);
  } finally {
    // Let the spinner complete a turn so a fast refresh still reads as one.
    setTimeout(() => { btn.classList.remove("spinning"); refreshing = false; }, Math.max(0, 600 - (Date.now() - started)));
  }
}

function openUrl(url) {
  if (PREVIEW) window.open(url, "_blank");
  else invoke("open_url", { url }).catch(() => {});
}

function showView(name) {
  document.getElementById("view-usage").hidden = name !== "usage";
  document.getElementById("view-settings").hidden = name !== "settings";
}

function fitWindow() {
  if (PREVIEW) return;
  const h = Math.ceil(document.getElementById("panel").getBoundingClientRect().height);
  invoke("set_height", { height: h }).catch(() => {});
}

// ------------------------------------------------------------------ boot

async function boot() {
  const root = document.documentElement;
  let platform = /Mac/.test(navigator.platform) ? "macos" : "windows";

  if (PREVIEW) {
    root.classList.add("preview");
    if (params.has("bare")) root.classList.add("bare");
  } else {
    try {
      const info = await invoke("app_info");
      platform = info.platform;
      document.getElementById("version").textContent = "v" + info.version;
    } catch (_) {}
    if (platform === "macos") root.classList.add("vibrancy");
  }
  if (PREVIEW) document.getElementById("version").textContent = "v0.3.0";
  root.classList.add("platform-" + platform);
  if (platform !== "macos") {
    document.getElementById("setting-tray").hidden = true;
    document.querySelector("#quit .kbd").textContent = "Ctrl Q";
    STRINGS.en.refresh = "Refresh (Ctrl R)";
    STRINGS.zh.refresh = "刷新 (Ctrl R)";
  }

  buildRings();
  applyI18n();
  render();

  // Size the native window to the panel, whatever its content.
  new ResizeObserver(fitWindow).observe(document.getElementById("panel"));

  // Settings controls.
  document.querySelectorAll(".seg").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      const key = seg.dataset.setting;
      settings[key] = b.dataset.value;
      localStorage.setItem(key, settings[key]);
      if (key === "tray") invoke("set_tray_mode", { mode: settings.tray }).catch(() => {});
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

  document.getElementById("refresh").addEventListener("click", refresh);
  document.getElementById("services").addEventListener("click", (e) => {
    const b = e.target.closest("[data-login]");
    if (b) signIn(b.dataset.login);
  });
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
    setTimeout(() => setStatus(demoStatus(params.get("state"))), params.has("instant") ? 0 : 250);
    return;
  }

  invoke("set_tray_mode", { mode: settings.tray }).catch(() => {});
  TAURI.event.listen("usage", (e) => setStatus(e.payload));
  TAURI.event.listen("web-logins", (e) => { webLogins = e.payload || []; renderAccounts(); render(); });
  TAURI.event.listen("popover-shown", () => { showView("usage"); renderRings(); });
  setStatus(await invoke("get_status").catch(() => []));
}

boot();
