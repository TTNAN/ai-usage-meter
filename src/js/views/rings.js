// AI Usage Meter — the rings popover: the concentric hero gauge, the hero copy
// beside it, and the per-service rows with their windows and countdowns.

import {
  store, settings, ORDER, WEB_IDS, accent, tileGlyph, hasNumbers, peakUsed,
  shown, headline, fmtAbs, fmtReset, fmtDur, fmtAgo, esc, withCode,
  sparkline, etaSeconds, etaText,
} from "../state.js";
import { getUpdate } from "../update.js";
import { t } from "../i18n.js";

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
  if (!store.status.length) return RING_FALLBACK.slice(0, RING_RADII.length);
  const left = (s) => 100 - peakUsed(s);
  const live = store.status
    .filter(hasNumbers)
    .slice()
    .sort((a, b) => left(a) - left(b))
    .map((s) => s.id);
  if (live.length > RING_RADII.length) return live.slice(0, 5);
  const ids = live.slice();
  for (const id of RING_FALLBACK) {
    if (ids.length >= RING_RADII.length) break;
    if (!ids.includes(id) && store.status.some((s) => s.id === id)) ids.push(id);
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
    const s = store.status.find((x) => x.id === id);
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
  const live = store.status.filter(hasNumbers);
  hero.classList.toggle("hero-empty", !live.length);

  if (!store.status.length) {
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


/** The short reason a failed fetch puts in its note — the bit in brackets in
 *  `Couldn't reach ChatGPT (timed out).` — said in this language. A reason the
 *  table doesn't know (`HTTP 502`) already reads fine as it is. */
function noteReason(note) {
  const m = /\(([^()]+)\)/.exec(note || "");
  if (!m) return "";
  const raw = m[1].trim();
  return t("reason")[raw.toLowerCase()] || raw;
}

/** An unreachable service's line, with the provider's reason when it gave one:
 *  a timeout then reads differently from an HTTP error. */
function unavailableMsg(s) {
  const why = noteReason(s.note);
  return why ? t("unavailableWhy")(s.label, why) : t("unavailable")(s.label);
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
      : s.state === "changed" ? t("apiChanged")(s.label)
      : s.state === "unavailable" ? unavailableMsg(s)
      : t("notConnected")(s.id);
    const warn = s.state === "expired" || s.state === "unavailable" || s.state === "changed";
    // A changed API is not a sign-in problem: offering "Sign in" there would
    // send the user to a login page that cannot fix it.
    const login = WEB_IDS.includes(s.id) && s.state !== "changed" && (s.state !== "expired" || s.source === "web" || !store.webLogins.includes(s.id))
      ? ` <button class="inline-btn" type="button" data-login="${s.id}">${esc(t("signInRow"))} →</button>` : "";
    // When the fix is already out, the changed row points straight at it.
    const fix = s.state === "changed" && getUpdate()
      ? ` <button class="inline-btn" type="button" data-open-update>${esc(t("updateOpen")(getUpdate().version))} →</button>` : "";
    body = `<div class="svc-msg" title="${esc(s.note || "")}"><span class="dot ${warn ? "warn" : ""}"></span><span>${withCode(msg)}${fix}${login}</span></div>`;
  }

  return `<li class="${cls.join(" ")}" style="--accent: ${accent(s.id)}">
    <div class="svc-head">
      <span class="tile ${s.id}">${tileGlyph(s.id)}</span>
      <span class="svc-name">${esc(s.label)}</span>${plan}${trend}${value}
    </div>${body}</li>`;
}

function visibleRows() {
  const rows = ORDER.map((id) => store.status.find((s) => s.id === id)).filter(Boolean);
  if (!settings.hideDisconnected) return rows;
  // Only hide the quiet ones while something else is actually reporting: with
  // nothing connected, every row is the instructions for connecting.
  const live = rows.some((s) => s.state === "connected" || s.state === "mock");
  return live ? rows.filter((s) => s.state !== "not_connected") : rows;
}

function renderServices() {
  const ul = document.getElementById("services");
  if (!store.status.length) {
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

export {
  ringIds, ringGeometry, buildRings, syncRings, renderRings, renderHero,
  resetSentence, noteReason, unavailableMsg, serviceRow, visibleRows,
  renderServices, syncServicesScroll, trendHtml,
};
