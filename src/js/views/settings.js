// AI Usage Meter — the settings drawer: the color rows, the account pills and
// the switches, each bound once here so the entry file stays a wiring diagram.

import {
  store, settings, ORDER, LABEL, PRESETS, serviceColors, effectiveColor,
  setColor, resetColor, resetAllColors, tileGlyph, esc, invoke, PREVIEW, applyTheme,
  normalizeOpacity, applyOpacity,
} from "../state.js";
import { t, syncLang } from "../i18n.js";
import {
  recording, beginShortcutRecording, cancelShortcutRecording, setShortcut,
  applyShortcut,
} from "../shortcut.js";
import { syncDockEdge } from "./dock.js";
import { manualCheck, paintUpdate } from "../update.js";
import { applyI18n, render, signIn, loadLogins } from "../../main.js";

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

/** Say "there is more below" only while a settings group really is cut off. */
function syncSettingsScroll() {
  const el = document.querySelector(".settings-body");
  if (!el) return;
  const more = el.scrollHeight - el.clientHeight > 1;
  el.classList.toggle("overflowing", more && el.scrollTop + el.clientHeight < el.scrollHeight - 2);
}

function renderAccounts() {
  document.querySelectorAll("[data-acct]").forEach((b) => {
    const on = store.webLogins.includes(b.dataset.acct);
    b.classList.toggle("on", on);
    b.textContent = t(on ? "signOut" : "signIn");
    b.title = on ? t("signOutTitle") : "";
  });
}

// -------------------------------------------------------- settings bindings

function bindSettings() {
  // Settings controls.
  document.querySelectorAll(".seg[data-setting]").forEach((seg) =>
    seg.addEventListener("click", (e) => {
      const b = e.target.closest("button");
      if (!b) return;
      const key = seg.dataset.setting;
      // The Custom pill records instead of storing a value, and clicking it
      // again while it listens is the way out of listening.
      if (b.hasAttribute("data-custom")) {
        beginShortcutRecording();
        return;
      }
      if (recording) cancelShortcutRecording();
      // The shortcut has its own setter, so the last chosen chord is kept.
      if (key === "shortcut") setShortcut(b.dataset.value);
      else {
        settings[key] = b.dataset.value;
        localStorage.setItem(key, settings[key]);
      }
      if (key === "tray") invoke("set_tray_mode", { mode: settings.tray }).catch(() => {});
      if (key === "shortcut") applyShortcut();
      if (key === "lang") syncLang();
      // 外观 repaints the whole page from the new palette, before the views
      // below are laid out again.
      if (key === "theme") applyTheme();
      applyI18n();
      render();
    })
  );

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

  // 面板透明度: a slider that thins the panel while it is dragged. Only the
  // panel's surfaces take the alpha; the text and the tiles never fade.
  const opacity = document.getElementById("set-opacity");
  opacity.value = String(settings.opacity);
  opacity.addEventListener("input", () => {
    settings.opacity = normalizeOpacity(opacity.value);
    localStorage.setItem("opacity", String(settings.opacity));
    applyOpacity();
  });

  const edgeSwitch = document.getElementById("edge-hide");
  // Wayland lets no app move its own window, so tucking has nothing to do
  // there: the switch is off and says why instead of pretending.
  if (store.wayland) settings.edgeTuck = false;
  edgeSwitch.checked = settings.edgeTuck;
  edgeSwitch.disabled = store.wayland;
  if (store.wayland) edgeSwitch.closest(".setting").title = t("edgeWayland");
  edgeSwitch.addEventListener("change", () => {
    settings.edgeTuck = edgeSwitch.checked;
    localStorage.setItem("edge-hide", settings.edgeTuck ? "1" : "0");
    syncDockEdge();
    if (!PREVIEW) invoke("set_edge_enabled", { enabled: settings.edgeTuck }).catch(() => {});
  });
  if (!PREVIEW) invoke("set_edge_enabled", { enabled: settings.edgeTuck }).catch(() => {});

  // "Check for updates": a manual check answers whatever the 24h throttle
  // says, and the note under the button repeats the answer in words.
  const check = document.getElementById("check-update");
  check.addEventListener("click", async () => {
    check.disabled = true;
    const note = document.getElementById("update-note");
    note.hidden = true;
    const res = await manualCheck();
    check.disabled = false;
    if (res.ok && res.update) {
      paintUpdate();
      note.textContent = t("updateAvailable")(res.update.version);
    } else if (res.ok) {
      note.textContent = t("upToDate");
    } else {
      note.textContent = t("updateUnreachable");
    }
    note.hidden = false;
  });
}

function bindColorRows() {
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
}

function bindAccounts() {
  document.querySelectorAll("[data-acct]").forEach((b) =>
    b.addEventListener("click", () => {
      const id = b.dataset.acct;
      if (store.webLogins.includes(id)) {
        invoke("web_logout", { id }).then(loadLogins).catch(() => {});
      } else signIn(id);
    })
  );
}

export {
  renderColorRows, syncColorRows, renderAccounts, syncSettingsScroll,
  bindSettings, bindColorRows, bindAccounts,
};
