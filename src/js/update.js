// AI Usage Meter — the lightweight update check. One GET to the GitHub
// Releases API, at most one automatic check a day, a quiet pill when a newer
// release exists. Rust does the fetching and the version comparison; this
// side only decides when to ask and where to show the answer.

import { PREVIEW, params, invoke } from "./state.js";
import { t } from "./i18n.js";

const LATEST_URL = "https://github.com/TTNAN/ai-usage-meter/releases/latest";
const CHECK_KEY = "update-last-check";
const CHECK_EVERY_MS = 24 * 3600e3;
// A few seconds after launch: the panel is up, the poller is running, and a
// slow answer cannot delay anything the user is looking at.
const AUTO_CHECK_DELAY_MS = 4000;

let update = null; // { version, url } once a newer release is known
let checking = false;

/** The known newer release, or null. Views read this as they render. */
export function getUpdate() {
  return update;
}

export function updateUrl() {
  return update ? update.url : LATEST_URL;
}

/** Once at boot: honour `?update=` for previews, then the 24h throttle. */
export function initUpdateCheck() {
  const want = params.get("update");
  if (want && /^v?\d+\.\d+\.\d+/.test(want)) {
    update = { version: want.replace(/^v/, ""), url: LATEST_URL };
    return;
  }
  if (PREVIEW) return;
  const last = Number(localStorage.getItem(CHECK_KEY) || 0);
  if (Date.now() - last < CHECK_EVERY_MS) return;
  setTimeout(() => checkNow(false), AUTO_CHECK_DELAY_MS);
}

async function checkNow(manual) {
  if (checking) return { reachable: false };
  checking = true;
  try {
    const info = await invoke("check_for_update");
    localStorage.setItem(CHECK_KEY, String(Date.now()));
    if (info && info.reachable && info.has_update && info.latest_version) {
      update = { version: info.latest_version, url: info.release_url || LATEST_URL };
    } else {
      update = null;
    }
    return { reachable: !!(info && info.reachable) };
  } catch (_) {
    // Offline, or the bridge is not there (preview): stay quiet, hide the pill.
    update = null;
    return { reachable: false };
  } finally {
    checking = false;
  }
}

/** The Settings button: always asks, whatever the 24h throttle says. */
export async function manualCheck() {
  if (PREVIEW) return { ok: true, update }; // the preview param answers for the API
  const res = await checkNow(true);
  return { ok: !!(res && res.reachable), update };
}

/** Reflect the answer in both views' footers. Cheap enough for every tick. */
export function paintUpdate() {
  for (const id of ["update-pill", "dock-update"]) {
    const pill = document.getElementById(id);
    if (!pill) continue;
    if (update) {
      pill.hidden = false;
      pill.textContent = t("updateAvailable")(update.version);
    } else {
      pill.hidden = true;
    }
  }
}
