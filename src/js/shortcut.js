// AI Usage Meter — the global shortcut: parsing and labelling an accelerator
// string, the segmented control it is picked with, and the recorder behind the
// "Custom" pill. Rust holds the actual combination; this side only chooses it
// and explains what happened.

import { settings, store, PREVIEW, invoke } from "./state.js";
import { t, lang } from "./i18n.js";
import { paintDockShortcut } from "./views/dock.js";

// The two chords Settings offers in one click; anything else there is the
// user's own recording. All of them are stored the way the plugin spells them,
// so Rust can take the string as it is.
const SHORTCUTS = ["CommandOrControl+Shift+U", "CommandOrControl+Shift+A"];

// The modifiers a chord may carry. `CommandOrControl` is ⌘ on a Mac and Ctrl
// elsewhere, which is what a recorded ⌘ or Ctrl turns into.
const MOD_CANON = {
  control: "Control",
  ctrl: "Control",
  shift: "Shift",
  alt: "Alt",
  option: "Alt",
  super: "Super",
  cmd: "Super",
  command: "Super",
  meta: "Super",
  commandorcontrol: "CommandOrControl",
  commandorctrl: "CommandOrControl",
  cmdorctrl: "CommandOrControl",
  cmdorcontrol: "CommandOrControl",
};

// Every key a chord may end on. The recorder produces the first group; the
// rest are names the plugin also accepts, so a stored chord is read back
// rather than quietly thrown away.
const KEY_CANON = (() => {
  const names = [
    ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    ..."0123456789",
    ...Array.from({ length: 24 }, (_, i) => `F${i + 1}`),
    ...Array.from({ length: 10 }, (_, i) => `Numpad${i}`),
    "Space", "Tab", "Enter", "Escape", "Backspace", "Delete", "Insert",
    "Home", "End", "PageUp", "PageDown", "Up", "Down", "Left", "Right",
    "Backquote", "Minus", "Equal", "BracketLeft", "BracketRight", "Backslash",
    "Semicolon", "Quote", "Comma", "Period", "Slash",
    "NumpadAdd", "NumpadSubtract", "NumpadMultiply", "NumpadDivide",
    "NumpadDecimal", "NumpadEnter",
  ];
  const map = new Map();
  for (const n of names) map.set(n.toLowerCase(), n);
  return map;
})();

/** A chord split into its modifiers and the key it ends on; null when the
 *  string is not one this window can show and hand to Rust. */
function parseShortcut(keys) {
  if (typeof keys !== "string" || !keys) return null;
  const cut = keys.lastIndexOf("+");
  const mods = [];
  for (const token of (cut === -1 ? "" : keys.slice(0, cut)).split("+")) {
    if (!token.trim()) continue;
    const mod = MOD_CANON[token.trim().toLowerCase()];
    if (!mod) return null;
    if (!mods.includes(mod)) mods.push(mod);
  }
  const key = KEY_CANON.get((cut === -1 ? keys : keys.slice(cut + 1)).trim().toLowerCase());
  if (!key) return null;
  // Tab is never a chord, with or without modifiers: the recorder refuses it
  // (it moves the focus on, or switches tabs), so a stored one is not read
  // back as a shortcut this window would hold.
  if (key === "Tab") return null;
  // A chord needs a modifier on it: a lone letter would fire while the user
  // types. The recorder makes the same one exception — a function key alone.
  // Read back without this rule, a stored `U` would pass as a global shortcut
  // this window can hold but must never fire.
  if (!mods.length && !/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) return null;
  return { mods, key };
}

function normalizeShortcut(v) {
  if (v === null) return SHORTCUTS[0];
  return v && parseShortcut(v) ? v : "";
}
/** The chord the switches come back to: never empty, so turning the shortcut
 *  on again restores what was chosen — preset or recorded — not the first one. */
function normalizeLastShortcut(v) {
  return v && parseShortcut(v) ? v : SHORTCUTS[0];
}

// ------------------------------------------------------------- shortcut

/** What a chord reads on this platform; Off is a word too. */
const IS_MAC = /Mac/.test(navigator.platform);

/** A key as a reader expects it, not as the plugin spells it. */
function keyLabel(key) {
  const named = {
    Space: lang() === "zh" ? "空格" : "Space",
    Tab: "Tab", Enter: "↩", Escape: "Esc", Backspace: "⌫", Delete: "⌦",
    Insert: "Ins", PageUp: "PgUp", PageDown: "PgDn", Home: "Home", End: "End",
    Up: "↑", Down: "↓", Left: "←", Right: "→",
    Backquote: "`", Minus: "-", Equal: "=", BracketLeft: "[",
    BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'",
    Comma: ",", Period: ".", Slash: "/",
    NumpadAdd: "+", NumpadSubtract: "-", NumpadMultiply: "*",
    NumpadDivide: "/", NumpadDecimal: ".", NumpadEnter: "↩",
  };
  return named[key] || (key.startsWith("Numpad") ? key.slice(6) : key);
}

/** ⌘⌥K on a Mac, Ctrl+Alt+K elsewhere — any chord, not just the presets. */
function chordLabel(mods, key) {
  const tail = key ? keyLabel(key) : "…";
  if (IS_MAC) {
    const sym = { Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘", CommandOrControl: "⌘" };
    let out = "";
    for (const m of ["CommandOrControl", "Super", "Control", "Alt", "Shift"]) {
      if (mods.includes(m) && !out.includes(sym[m])) out += sym[m];
    }
    return out + tail;
  }
  const word = { CommandOrControl: "Ctrl", Control: "Ctrl", Shift: "Shift", Alt: "Alt", Super: "Win" };
  const out = [];
  for (const m of ["CommandOrControl", "Control", "Shift", "Alt", "Super"]) {
    if (mods.includes(m) && !out.includes(word[m])) out.push(word[m]);
  }
  return [...out, tail].join("+");
}

function shortcutLabel(keys) {
  if (!keys) return t("scOff");
  const p = parseShortcut(keys);
  return p ? chordLabel(p.mods, p.key) : keys;
}

/** A chord that is not one of the two presets is the Custom pill's. */
function isCustomShortcut(keys = settings.shortcut) {
  return !!keys && !SHORTCUTS.includes(keys);
}

/** Remember a chord as the one the switches turn back on. */
function rememberShortcut(keys) {
  if (!keys) return;
  settings.lastShortcut = normalizeLastShortcut(keys);
  localStorage.setItem("last-shortcut", settings.lastShortcut);
}

/** Turn the global shortcut on (a preset or a recorded chord) or off ("").
 *  Turning it off keeps the chord, so the next On is that one, not ⌘⇧U. */
function setShortcut(keys) {
  settings.shortcut = normalizeShortcut(keys);
  localStorage.setItem("shortcut", settings.shortcut);
  if (settings.shortcut) rememberShortcut(settings.shortcut);
}

function paintShortcutSeg() {
  document.querySelectorAll('[data-setting="shortcut"] button[data-value]').forEach((b) => {
    b.textContent = shortcutLabel(b.dataset.value);
  });
  const custom = document.getElementById("sc-custom");
  if (!custom) return;
  if (recording) {
    custom.textContent = t("scPress");
    custom.classList.add("rec");
    custom.title = t("scHint");
    return;
  }
  custom.classList.remove("rec");
  custom.textContent = isCustomShortcut() ? shortcutLabel(settings.shortcut) : t("scCustom");
  custom.title = t("scCustomTitle");
  custom.setAttribute(
    "aria-label",
    isCustomShortcut() ? `${t("scCustomTitle")} — ${shortcutLabel(settings.shortcut)}` : t("scCustomTitle")
  );
}

/**
 * Hand the chosen shortcut to Rust, which is the only place that knows whether
 * it could actually be held: a combination another app owns comes back as an
 * error, and then the switch shows Off rather than a key that does nothing.
 */
function applyShortcut() {
  paintShortcutSeg();
  paintDockShortcut();
  const note = document.getElementById("shortcut-note");
  note.hidden = true;
  if (PREVIEW) return;
  const wanted = settings.shortcut;
  invoke("set_global_shortcut", { keys: wanted })
    .then((held) => {
      if (held === wanted) return;
      settings.shortcut = held || "";
      localStorage.setItem("shortcut", settings.shortcut);
      rememberShortcut(settings.shortcut);
      paintShortcutSeg();
      showShortcutState();
      paintDockShortcut();
    })
    .catch(() => {
      // Nothing is held now, so the switch has to say so.
      settings.shortcut = "";
      localStorage.setItem("shortcut", "");
      paintShortcutSeg();
      showShortcutState();
      note.textContent = t(store.wayland ? "scWayland" : "scTaken");
      note.hidden = false;
      paintDockShortcut();
    });
}

/** Reflect the stored choice in the segmented control (i18n repaints it too). */
function showShortcutState() {
  document.querySelectorAll('.seg[data-setting="shortcut"] button[data-value]').forEach((b) =>
    b.classList.toggle("on", b.dataset.value === settings.shortcut)
  );
  const custom = document.getElementById("sc-custom");
  if (!custom) return;
  custom.classList.toggle("on", isCustomShortcut());
}

// -------------------------------------------------- recording a shortcut

// While the Custom pill listens, the chord it would fall back to, and a pair
// of document listeners that end the listening. Capture phase, so a press is
// taken before the page's own key handling (hide on Esc, Ctrl+R…) sees it.
let recording = false;
let recordingPrior = "";
let recordingLast = "";
const MOD_CODES = new Set([
  "ShiftLeft", "ShiftRight", "ControlLeft", "ControlRight",
  "AltLeft", "AltRight", "MetaLeft", "MetaRight",
]);

/** The modifiers held right now, in the order the platform writes them. */
function modsFromEvent(e) {
  const mods = [];
  if (IS_MAC) {
    if (e.metaKey) mods.push(e.ctrlKey ? "Super" : "CommandOrControl");
    if (e.ctrlKey) mods.push("Control");
    if (e.altKey) mods.push("Alt");
    if (e.shiftKey) mods.push("Shift");
  } else {
    if (e.ctrlKey) mods.push("CommandOrControl");
    if (e.shiftKey) mods.push("Shift");
    if (e.altKey) mods.push("Alt");
    if (e.metaKey) mods.push("Super");
  }
  return mods;
}

/** The key a keydown ends on, as the plugin spells it; null for keys a global
 *  shortcut may not end on (dead keys, media keys, the modifiers themselves). */
function keyFromCode(code) {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^Numpad[0-9]$/.test(code)) return code;
  const named = {
    Space: "Space", Backquote: "Backquote", Minus: "Minus", Equal: "Equal",
    BracketLeft: "BracketLeft", BracketRight: "BracketRight",
    Backslash: "Backslash", Semicolon: "Semicolon", Quote: "Quote",
    Comma: "Comma", Period: "Period", Slash: "Slash",
    ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
    Insert: "Insert", Delete: "Delete", Enter: "Enter", Tab: "Tab",
    Backspace: "Backspace",
  };
  if (named[code]) return named[code];
  if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code;
  return null;
}

function shortcutNote(text, kind) {
  const note = document.getElementById("shortcut-note");
  if (!note) return;
  note.textContent = text || "";
  note.classList.toggle("hint", kind === "hint");
  note.hidden = !text;
  note.removeAttribute("title");
}

function endShortcutRecording() {
  recording = false;
  document.removeEventListener("keydown", onShortcutKey, true);
  document.removeEventListener("pointerdown", onShortcutPointer, true);
}

function beginShortcutRecording() {
  if (recording) return cancelShortcutRecording();
  if (store.wayland) return shortcutNote(t("scWayland"));
  recording = true;
  recordingPrior = settings.shortcut;
  recordingLast = settings.lastShortcut;
  shortcutNote(t("scHint"), "hint");
  paintShortcutSeg();
  showShortcutState();
  document.addEventListener("keydown", onShortcutKey, true);
  document.addEventListener("pointerdown", onShortcutPointer, true);
}

function cancelShortcutRecording() {
  if (!recording) return;
  endShortcutRecording();
  shortcutNote("");
  paintShortcutSeg();
  showShortcutState();
  paintDockShortcut();
}

/** A click anywhere else is a click away from the recorder, too. */
function onShortcutPointer(e) {
  const seg = document.querySelector('.seg[data-setting="shortcut"]');
  if (seg && seg.contains(e.target)) return;
  cancelShortcutRecording();
}

/**
 * The listening pill takes the next chord. A bare modifier is only a step of
 * one, Escape gives up, and a key with no modifier (bar a function key) is not
 * a global shortcut — a lone letter would fire while the user types.
 */
function onShortcutKey(e) {
  if (!recording) return;
  const plain = !e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey;
  // Escape gives up, and Tab leaves: both move the user on rather than
  // becoming a chord, so neither is swallowed.
  if (e.code === "Escape" && plain) {
    e.preventDefault();
    e.stopPropagation();
    cancelShortcutRecording();
    return;
  }
  // Tab is never a chord, with or without modifiers: bare it moves the focus
  // on, and ⌘/Ctrl+Tab switches tabs in the browser and in the app around it.
  // Recorded, it would hijack that switch everywhere the app runs; so the
  // recorder steps aside and lets the key do its own job.
  if (e.code === "Tab" || e.key === "Tab") {
    cancelShortcutRecording();
    return;
  }
  e.preventDefault();
  e.stopPropagation();
  if (e.repeat) return;
  const custom = document.getElementById("sc-custom");
  if (MOD_CODES.has(e.code)) {
    if (custom) custom.textContent = chordLabel(modsFromEvent(e), "");
    return;
  }
  const key = keyFromCode(e.code);
  if (!key) {
    if (custom) custom.textContent = t("scPress");
    return;
  }
  const mods = modsFromEvent(e);
  if (!mods.length && !/^F([1-9]|1[0-9]|2[0-4])$/.test(key)) {
    shortcutNote(t("scNeedMod")(IS_MAC));
    if (custom) custom.textContent = t("scPress");
    return;
  }
  endShortcutRecording();
  commitShortcut([...mods, key].join("+"));
}

/**
 * Store a recorded chord, show it everywhere, and let Rust hold it. A chord
 * another app owns comes back as an error: the shortcut that was in use goes
 * back on, and the note says why, so a failed try never leaves the user with
 * none — nor with a chord that does nothing.
 */
function commitShortcut(keys) {
  const prior = recordingPrior;
  const priorLast = recordingLast;
  setShortcut(keys);
  shortcutNote("");
  paintShortcutSeg();
  showShortcutState();
  paintDockShortcut();
  if (PREVIEW) return;
  invoke("set_global_shortcut", { keys })
    .then((held) => {
      if (held === keys) return;
      throw new Error(held ? String(held) : "nothing held");
    })
    .catch((err) => {
      settings.shortcut = prior;
      settings.lastShortcut = priorLast;
      localStorage.setItem("shortcut", prior);
      localStorage.setItem("last-shortcut", priorLast);
      shortcutNote(t(store.wayland ? "scWayland" : "scTaken"));
      const note = document.getElementById("shortcut-note");
      note.title = String((err && err.message) || err || "");
      paintShortcutSeg();
      showShortcutState();
      paintDockShortcut();
      invoke("set_global_shortcut", { keys: prior }).catch(() => {});
    });
}

export {
  SHORTCUTS, parseShortcut, normalizeShortcut, normalizeLastShortcut,
  IS_MAC, keyLabel, chordLabel, shortcutLabel, isCustomShortcut,
  rememberShortcut, setShortcut, paintShortcutSeg, showShortcutState,
  applyShortcut, recording, beginShortcutRecording, cancelShortcutRecording,
};
