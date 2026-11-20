// AI Usage Meter — frontend. Plain JS, no bundler.
// Talks to Rust via window.__TAURI__.core.invoke (withGlobalTauri: true).

const COLORS = { claude: "#d97757", codex: "#3b82f6", grok: "#a855f7" };
const SUB_LABELS = {
  claude: "7-day",
  codex: "7-day",
  grok: "weekly pool",
};
const CARD_NOTES = {
  // The Codex card is Codex quota, NOT ChatGPT web-chat quota. Say so on the card.
  codex: "Codex quota — not web ChatGPT",
  claude: "5-hour window",
  grok: "Weekly pool",
};

let status = [];
let timer = null;

async function invoke(cmd, args) {
  if (window.__TAURI__ && window.__TAURI__.core) {
    return window.__TAURI__.core.invoke(cmd, args || {});
  }
  throw new Error("Tauri bridge not available (open this page inside the app)");
}

function fmtCountdown(seconds) {
  if (seconds == null || seconds < 0) return "—";
  if (seconds === 0) return "resetting…";
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = Math.floor(seconds % 60);
  const pad = (n) => String(n).padStart(2, "0");
  return h > 0 ? `${pad(h)}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

function fmtReset(seconds) {
  if (seconds == null) return "";
  if (seconds <= 0) return "resets now";
  if (seconds < 3600) return `resets in ${Math.ceil(seconds / 60)}m`;
  if (seconds < 86400) return `resets in ${(seconds / 3600).toFixed(1)}h`;
  return `resets in ${Math.ceil(seconds / 86400)}d`;
}

function render() {
  const dock = document.getElementById("dock");
  const now = Date.now() / 1000;
  dock.innerHTML = status
    .map((s) => {
      const color = COLORS[s.id] || "#888";
      const used = s.usedPercent;
      const remaining = used == null ? null : Math.max(0, 100 - used);
      const resetIn = s.resetsAt == null ? null : s.resetsAt - now;
      const sub =
        s.secondaryUsedPercent != null
          ? `${SUB_LABELS[s.id]} ${s.secondaryUsedPercent.toFixed(0)}% used`
          : "";
      let stateHtml = "";
      if (s.state === "not_connected") {
        stateHtml = `<div class="state warn">${escapeHtml(s.note || "Not connected")}</div>`;
      } else if (s.state === "unavailable") {
        stateHtml = `<div class="state warn">${escapeHtml(s.note || "Unavailable")}</div>`;
      } else if (s.state === "mock") {
        stateHtml = `<div><span class="mock-badge">DEMO DATA</span></div>`;
      }
      const pctHtml =
        used == null
          ? `<div class="pct">—</div>`
          : `<div class="pct">${used.toFixed(0)}% <small>used · ${remaining.toFixed(0)}% left</small></div>`;
      const barHtml =
        used == null
          ? `<div class="bar"><div style="width:0%;background:${color}"></div></div>`
          : `<div class="bar"><div style="width:${used}%;background:${color}"></div></div>`;
      const cdHtml =
        s.state === "connected" || s.state === "mock"
          ? `<div class="countdown" style="color:${color}">${fmtCountdown(resetIn)} <span style="color:#999;font-weight:400">· ${fmtReset(resetIn)}</span></div>`
          : `<div class="countdown" style="color:#888">—</div>`;
      return `<div class="card ${remaining != null && remaining <= 15 ? "low" : ""}">
        <img class="icon" src="${s.id}.png" alt="${escapeHtml(s.label)}" />
        <div class="name">${escapeHtml(s.label)}</div>
        <div class="note-line">${escapeHtml(CARD_NOTES[s.id] || "")}</div>
        ${pctHtml}
        ${barHtml}
        ${cdHtml}
        <div class="sub">${escapeHtml(sub)}</div>
        ${stateHtml}
      </div>`;
    })
    .join("");
  const el = document.getElementById("updated");
  if (el) el.textContent = "Updated " + new Date().toLocaleTimeString();
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[c]);
}

async function load(force) {
  try {
    status = force
      ? await invoke("refresh_status")
      : await invoke("get_status");
    render();
  } catch (e) {
    document.getElementById("dock").innerHTML =
      `<div class="loading">Could not load usage: ${escapeHtml(e.message || e)}<br/>Try Refresh, or check the README.</div>`;
  }
}

document.getElementById("refresh").addEventListener("click", () => load(true));
document.getElementById("about").addEventListener("click", () => {
  invoke("open_url", { url: "https://github.com/TTNAN/ai-usage-meter" }).catch(() => {});
});

// Live countdown: re-render every second (cheap; numbers come from resets_at).
timer = setInterval(render, 1000);
load(false);
// Auto-refresh from the backend every 60s (the backend enforces per-provider
// poll intervals: Claude 5 min, Codex 60 s, Grok 5 min).
setInterval(() => load(false), 60000);
