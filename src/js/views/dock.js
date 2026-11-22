// AI Usage Meter — the Dock view: brand squircles rising out of one dark tray,
// the names under them, the status chips, the colours drawer and the four
// quiet controls. The window is exactly as wide as its tiles.

import {
  store, settings, LABEL, WEB_IDS, TILE_BG, inkFor, serviceColors, hasNumbers,
  peakUsed, shown, headline, esc, sparkline, etaSeconds, fitWindow, invoke,
  PREVIEW, setColor, resetColor, resetAllColors,
} from "../state.js";
import { t } from "../i18n.js";
import { shortcutLabel, setShortcut, applyShortcut } from "../shortcut.js";
import { visibleRows, noteReason } from "./rings.js";
import { refresh, signIn, showView, openUrl } from "../../main.js";
import { getUpdate, updateUrl } from "../update.js";

// --------------------------------------------------------------- dock
// The Dock view is the original ai-usage-meter-dock design, ported whole —
// the same tray, the same squircles and the same four controls. The order is
// the original's too: ChatGPT, Claude, Grok, Gemini, Copilot.

const DOCK_ORDER = ["codex", "claude", "grok", "gemini", "copilot"];
const DOCK_TILE = {
  codex: "tile-chatgpt",
  claude: "tile-claude",
  grok: "tile-grok",
  gemini: "tile-gemini",
  copilot: "tile-copilot",
};
// What a squircle's number, bar and countdown fall back to: one accent per
// brand surface, exactly as the original dock painted them.
const DOCK_ACCENT = {
  codex: "#2f6fed",
  claude: "#ffffff",
  grok: "#a78bfa",
  gemini: "#38bdf8",
  copilot: "#34d399",
};
const DOCK_PRESETS = ["#2f6fed", "#e08a6b", "#f5b04c", "#b38cff", "#34d399"];
// A cell, the gap between cells and the frame around them (the tray's 24 px
// on each side plus the panel's 12): the width the page asks Rust for, so the
// bar narrows with the number of services and nothing is ever clipped.
const CELL_W = 132;
const CELL_GAP = 14;
const FRAME_W = 72;
const MIN_W = 300;

function dockRows() {
  if (!store.status.length)
    return DOCK_ORDER.map((id) => ({ id, label: LABEL[id], state: "loading", windows: [] }));
  return visibleRows()
    .slice()
    .sort((a, b) => DOCK_ORDER.indexOf(a.id) - DOCK_ORDER.indexOf(b.id));
}

function dockLabel(s) {
  if (!hasNumbers(s)) {
    if (s.state === "loading") return "…";
    if (s.state === "unavailable") return t("offlineShort");
    if (s.state === "changed") return t("changedShort");
    return s.state === "expired" ? t("expiredShort") : t("notConnectedShort");
  }
  return t(settings.mode === "used" ? "used" : "left");
}

/** `~2h52m` — what fits in 8.5 px under a squircle. */
function etaShort(secs) {
  const total = Math.max(1, Math.round(secs / 60));
  const h = Math.floor(total / 60);
  const m = total % 60;
  return h ? `~${h}h${String(m).padStart(2, "0")}m` : `~${m}m`;
}

/** The mark: a masked glyph, or Gemini's own four-colour spark. */
function dockMark(id) {
  return id === "gemini" ? `<img src="icons/gemini.svg" alt="">` : "<i></i>";
}

/** Under the squircle, on the tray: the trend line and the ETA, or nothing. */
function dockTrend(s) {
  const spark = sparkline(s.id, 30, 8);
  const eta = etaSeconds(s);
  if (!spark && eta == null) return "";
  const text = eta == null ? "" : `<span class="eta">${etaShort(eta)}</span>`;
  return `${spark}${text}`;
}

function dockTrendTitle(s) {
  const eta = etaSeconds(s);
  if (eta == null) return t("trendTitle");
  const total = Math.max(1, Math.round(eta / 60));
  return t("etaTitle")(t("etaDur")(Math.floor(total / 60), total % 60));
}

function dockCell(s) {
  const live = hasNumbers(s);
  const w = live ? headline(s) : null;
  const value = live ? Math.max(0, Math.min(100, Math.round(shown(peakUsed(s))))) : 0;
  const tag = `<span class="tag-mode">${esc(dockLabel(s))}</span>`;
  const inner = live
    ? `<div class="mark">${dockMark(s.id)}</div>
       <div class="pct-line"><span class="num">${value}</span><span class="pct-sign">%</span>${tag}</div>
       <div class="bar"><i style="width:${value}%"></i></div>
       <div class="reset${dockReset(w.resetsAt).length > 12 ? " long" : ""}" data-reset="${
         w.resetsAt ?? ""
       }" data-reset-fmt="timer">${esc(dockReset(w.resetsAt))}</div>`
    : `<div class="mark">${dockMark(s.id)}</div>
       <div class="pct-line"><span class="num">--</span>${tag}</div>
       <div class="bar"><i style="width:0%"></i></div>
       <div class="reset">${esc(
         // Before the first reading there is nothing to do yet: say nothing.
         // An unreachable service is signed in already — its line carries why.
         s.state === "loading" ? ""
           : s.state === "unavailable" ? noteReason(s.note) || t("offlineShort")
           : s.state === "changed" ? (getUpdate() ? t("updateOpen")(getUpdate().version) : t("changedHint"))
           : WEB_IDS.includes(s.id) ? t("dockSignIn") : t("howToConnect")
       )}</div>`;
  const trend = live ? dockTrend(s) : "";
  // A quiet squircle is a button in all but name: give it the keyboard too.
  const a11y = live ? "" : ` role="button" tabindex="0" aria-label="${esc(s.label)} · ${esc(dockLabel(s))}"`;
  // The reason travels with the tile: hovering it says exactly what the row's
  // note says, so an unreachable, changed or expired service explains itself
  // here as it does in the popover.
  const title = s.note ? ` title="${esc(s.note)}"` : "";
  return `<div class="cell${live ? "" : " off"}" data-id="${s.id}"${title}>
    <div class="tile ${DOCK_TILE[s.id]}${live ? "" : " off"}"${a11y}>${inner}</div>
    <div class="micro"${trend ? ` title="${esc(dockTrendTitle(s))}"` : ""}>${trend}</div>
  </div>`;
}

function renderDock() {
  const dock = document.getElementById("dock");
  if (!dock) return;
  const rows = dockRows();
  dock.innerHTML = rows.map(dockCell).join("");
  const names = document.getElementById("dock-names");
  if (names) names.innerHTML = rows.map((s) => `<span>${esc(s.label)}</span>`).join("");
  const panel = document.getElementById("panel");
  panel.dataset.count = String(rows.length);
  panel.style.setProperty("--cells", String(Math.max(1, rows.length)));
  syncDockWidth();
  syncDockColors();
  renderDockChips();
  renderDockColorbar();
}

/** How each service is connected: through a web sign-in, through its CLI, or not. */
function dockChipState(id) {
  const s = store.status.find((x) => x.id === id);
  // An answer this app cannot read is the one thing worth saying about the
  // service here: signing in again would not fix it.
  if (s && s.state === "changed") return "changed";
  if (store.webLogins.includes(id)) return "signed-in";
  return s && hasNumbers(s) ? "connected" : "not";
}

function renderDockChips() {
  const box = document.getElementById("dock-logins");
  if (!box) return;
  box.innerHTML = dockRows()
    .filter((s) => s.state !== "loading")
    .map((s) => {
      const st = dockChipState(s.id);
      const state =
        st === "signed-in" ? t("signOut")
        : st === "connected" ? t("dockConnected")
        : st === "changed" ? t("changedShort")
        : t("notConnectedShort");
      return `<button class="chip${st === "connected" || st === "signed-in" ? " on" : ""}" type="button" data-login="${s.id}" title="${esc(
        LABEL[s.id]
      )}"><span class="dot"></span>${esc(LABEL[s.id])} · ${esc(state)}</button>`;
    })
    .join("");
}

function dockColorItem(s) {
  const id = s.id;
  return `<div class="cb-item" data-color="${id}">
      <div class="cb-top">
        <span class="cb-name">${esc(LABEL[id])}</span>
        <button class="cb-reset" type="button" data-reset-color title="${esc(t("reset"))}">
          <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M3.2 8a4.8 4.8 0 1 0 1.4-3.4M3.2 2.2v2.9h2.9" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"/></svg>
        </button>
      </div>
      <div class="cb-mid">
        <label class="cb-swatch" title="${esc(t("pick"))}"><input type="color" data-color-input aria-label="${esc(
          LABEL[id]
        )} ${esc(t("pick"))}"></label>
        <span class="cb-hex" data-hex></span>
      </div>
      <div class="cb-presets">${DOCK_PRESETS.map(
        (p) => `<button type="button" data-preset="${p}" style="--p:${p}" title="${p.toUpperCase()}"></button>`
      ).join("")}</div>
    </div>`;
}

function renderDockColorbar() {
  const grid = document.getElementById("cb-grid");
  if (!grid) return;
  const ids = dockRows().map((s) => s.id);
  const have = Array.from(grid.querySelectorAll(".cb-item")).map((el) => el.dataset.color);
  // Rebuild only when the set of services changed: a rebuild while the system
  // colour picker is open would close it.
  if (ids.join() !== have.join()) grid.innerHTML = dockRows().map(dockColorItem).join("");
  syncDockColorbar();
}

function syncDockColorbar() {
  document.querySelectorAll("#cb-grid .cb-item").forEach((item) => {
    const id = item.dataset.color;
    const hex = serviceColors[id] || DOCK_ACCENT[id] || "#9a9aa2";
    item.querySelector("[data-hex]").textContent = hex.toUpperCase();
    item.querySelector(".cb-swatch").style.setProperty("--sw", hex);
    const input = item.querySelector("[data-color-input]");
    // Don't fight the open picker while the user drags in it.
    if (document.activeElement !== input) input.value = hex;
    item.querySelector("[data-reset-color]").disabled = !serviceColors[id];
    item.querySelectorAll("[data-preset]").forEach((b) => b.classList.toggle("on", b.dataset.preset === hex));
  });
}

/** Paint a picked colour onto its squircle, through the contrast-safe ink. */
function syncDockColors() {
  for (const id of DOCK_ORDER) {
    const cell = document.querySelector(`#dock .cell[data-id="${id}"]`);
    if (!cell) continue;
    const pick = serviceColors[id];
    cell.classList.toggle("custom", !!pick);
    if (pick) cell.style.setProperty("--ink", inkFor(pick, TILE_BG[id]));
    else cell.style.removeProperty("--ink");
  }
}

/** The window is the dock: 788 px for five cells, less for fewer, never clipped. */
function syncDockWidth() {
  const panel = document.getElementById("panel");
  if (!panel) return;
  const n = Math.max(1, Number(panel.dataset.count || DOCK_ORDER.length));
  if (settings.view !== "dock" || panel.classList.contains("settings-open")) {
    panel.style.width = "";
    return;
  }
  // Five cells give 788 px; fewer narrow the bar — but never past the two
  // pills above it, which are the narrowest thing the frame has to hold.
  panel.style.width = `${Math.max(MIN_W, n * CELL_W + (n - 1) * CELL_GAP + FRAME_W)}px`;
}

/** The shortcut switch in the drawer's foot: on shows the keys, off says so.
 *  Both read the way the settings view spells them, so the dock shows the
 *  chord the user actually picked — recorded or preset. */
function paintDockShortcut() {
  const btn = document.getElementById("dock-shortcut");
  if (!btn) return;
  const on = !!settings.shortcut;
  btn.classList.toggle("on", on);
  btn.textContent = shortcutLabel(settings.shortcut);
  btn.setAttribute("aria-pressed", on ? "true" : "false");
  btn.title = t(on ? "dockShortcutOn" : "dockShortcutOff");
}

/** 配色 opens the drawer under the row; the window grows to fit it. */
function showDockColorbar(show) {
  const bar = document.getElementById("colorbar");
  const btn = document.getElementById("dock-colors");
  if (!bar || !btn) return;
  bar.hidden = !show;
  btn.classList.toggle("on", show);
  btn.setAttribute("aria-expanded", show ? "true" : "false");
  fitWindow();
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

// ------------------------------------------------------- dock bindings

const dockEdge = document.getElementById("dock-edge");


  // The dock view carries the controls the original dock had: two pill
  // toggles, the status chips, 配色 / 贴边 / 刷新 / 退出, and the colors
  // drawer behind 配色. Same state and storage as the settings page.
function syncDockEdge() {
    dockEdge.classList.toggle("on", settings.edgeTuck);
    dockEdge.setAttribute("aria-pressed", settings.edgeTuck ? "true" : "false");
    dockEdge.disabled = store.wayland;
    if (store.wayland) dockEdge.title = t("edgeWayland");
}

function bindDock() {
  syncDockEdge();
  dockEdge.addEventListener("click", () => {
    if (store.wayland) return;
    settings.edgeTuck = !settings.edgeTuck;
    localStorage.setItem("edge-hide", settings.edgeTuck ? "1" : "0");
    document.getElementById("edge-hide").checked = settings.edgeTuck;
    syncDockEdge();
    if (!PREVIEW) invoke("set_edge_enabled", { enabled: settings.edgeTuck }).catch(() => {});
  });
  document.getElementById("dock-update").addEventListener("click", () => openUrl(updateUrl()));
  document.getElementById("dock-refresh").addEventListener("click", refresh);
  document.getElementById("dock-quit").addEventListener("click", () => invoke("quit").catch(() => {}));

  // 配色 opens the drawer under the chips row; the window grows to fit it.
  document.getElementById("dock-colors").addEventListener("click", () =>
    showDockColorbar(document.getElementById("colorbar").hidden)
  );
  const cbGrid = document.getElementById("cb-grid");
  cbGrid.addEventListener("input", (e) => {
    const input = e.target.closest("[data-color-input]");
    const item = input && input.closest(".cb-item");
    if (item) setColor(item.dataset.color, input.value);
  });
  cbGrid.addEventListener("click", (e) => {
    const item = e.target.closest(".cb-item");
    if (!item) return;
    const preset = e.target.closest("[data-preset]");
    if (preset) setColor(item.dataset.color, preset.dataset.preset);
    else if (e.target.closest("[data-reset-color]")) resetColor(item.dataset.color);
  });
  document.getElementById("dock-colors-reset-all").addEventListener("click", resetAllColors);
  document.getElementById("dock-shortcut").addEventListener("click", () => {
    // Back on goes to the chord last chosen, not to the first preset.
    setShortcut(settings.shortcut ? "" : settings.lastShortcut);
    applyShortcut();
  });

  const dock = document.getElementById("dock");
  dock.addEventListener("click", (e) => {
    const tile = e.target.closest(".tile.off");
    if (!tile) return;
    const id = tile.closest(".cell").dataset.id;
    // A quiet squircle is the way to fix it: web sign-in where the app can do
    // it, the settings page (which says how to connect) for the CLI-backed ones.
    if (dockChipState(id) === "changed" && getUpdate()) openUrl(updateUrl());
    else if (WEB_IDS.includes(id)) signIn(id);
    else showView("settings");
  });
  dock.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    const tile = e.target.closest(".tile.off");
    if (!tile) return;
    e.preventDefault();
    tile.click();
  });
  document.getElementById("dock-logins").addEventListener("click", (e) => {
    const b = e.target.closest("[data-login]");
    if (!b) return;
    const id = b.dataset.login;
    if (dockChipState(id) === "not" && WEB_IDS.includes(id)) signIn(id);
    else showView("settings");
  });
}

export {
  dockRows, dockLabel, dockCell, renderDock, dockChipState, renderDockChips,
  renderDockColorbar, syncDockColorbar, syncDockColors, syncDockWidth,
  paintDockShortcut, showDockColorbar, dockReset, syncDockEdge, bindDock,
};
