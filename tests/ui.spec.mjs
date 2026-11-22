// UI regression suite for the popover, run against the plain sources served
// as-is (`python3 -m http.server 4173 -d src`) — the app itself has no build
// step, so neither does this suite: Playwright loads `src/` directly.
//
// Covers the three views × zh/en × dark/light × the demo states, the global
// shortcut recorder's rules, and the reasons a Dock tile carries in its title.
// `UI_PORT` picks the port the page is served on (default 4173).
import { test, expect } from "@playwright/test";

const PORT = process.env.UI_PORT || "4173";
const url = (q = "") => `http://127.0.0.1:${PORT}/${q}`;

// Frozen time so countdowns and sparkline samples read the same on every run,
// and an empty `localStorage` at the start of every test — but only on the
// test's first page. The app keeps its choices there, and a test that walks
// from Settings to the Dock has to find what Settings stored; `sessionStorage`
// survives those navigations and remembers the wipe, while a new test gets a
// fresh context and so starts empty again.
const NOW = 1766366400000;
test.beforeEach(async ({ context }) => {
  await context.addInitScript(`(() => {
    Date.now = () => ${NOW};
    Date.prototype.getTime = () => ${NOW};
    try {
      if (sessionStorage.getItem("ui-spec-fresh")) return;
      sessionStorage.setItem("ui-spec-fresh", "1");
      localStorage.clear();
    } catch (_) {}
  })()`);
});

// The shortcut control: the Custom pill, and the preset that stands for
// ⌘⇧U / Ctrl+Shift+U. `localStorage` holds no `shortcut` until the user picks
// one — the preset is only the fallback the page reads, so "nothing was
// overwritten" means that preset is still the one switched on.
const customPill = (page) => page.locator("#sc-custom");
const presetU = (page) =>
  page.locator(
    '.seg[data-setting="shortcut"] button[data-value="CommandOrControl+Shift+U"]'
  );
const storedShortcut = (page, key = "shortcut") =>
  page.evaluate((k) => localStorage.getItem(k), key);

// A computed style, and the alpha channel of a colour string (`rgba(…)` or the
// `color(srgb … / a)` a color-mix() comes back as): how the suite asserts what
// was actually painted rather than what was asked for.
const computed = (page, sel, prop) =>
  page.evaluate(
    ([s, p]) => getComputedStyle(document.querySelector(s))[p],
    [sel, prop]
  );
const alphaOf = (color) => {
  const nums = (String(color).match(/-?[\d.]+/g) || []).map(Number);
  return nums.length > 3 ? nums[3] : 1;
};
// The r/g/b a colour string carries, in 0–255, whichever way it is written.
const channelsOf = (color) => {
  const s = String(color).trim();
  const nums = (s.match(/-?[\d.]+/g) || []).map(Number);
  // `color(srgb …)` carries 0–1 channels; `rgb()` carries 0–255.
  return nums.slice(0, 3).map((v) => Math.round(s.startsWith("color(") ? v * 255 : v));
};

async function expectPresetChordHeld(page) {
  await expect(customPill(page)).toHaveText(/^(Custom|自定义)$/);
  await expect(customPill(page)).not.toHaveClass(/\bon\b/);
  await expect(presetU(page)).toHaveClass(/\bon\b/);
  await expect(presetU(page)).toHaveText(/Ctrl\+Shift\+U|⌘⇧U/);
  expect(await storedShortcut(page)).toBeNull();
  expect(await storedShortcut(page, "last-shortcut")).toBe(
    "CommandOrControl+Shift+U"
  );
}

// A page run with zero console errors and zero page errors is the baseline for
// every view: the module split must not so much as warn.
test.describe("no errors, no overflow, across views, languages and states", () => {
  const cases = [
    ["rings", "/"],
    ["rings, down", "/?state=down"],
    ["rings, changed", "/?state=changed"],
    ["rings, mixed", "/?state=mixed"],
    ["rings, empty", "/?state=empty"],
    ["rings, zh", "/?lang=zh"],
    ["rings, zh, down", "/?lang=zh&state=down"],
    ["rings, zh, changed", "/?lang=zh&state=changed"],
    ["rings, used mode", "/?mode=used"],
    ["rings, wayland", "/?wayland=1"],
    ["dock", "/?view=dock"],
    ["dock, three, hidden", "/?view=dock&state=three&hide=1"],
    ["dock, wayland", "/?view=dock&wayland=1"],
    ["dock, zh", "/?view=dock&lang=zh"],
    ["dock, zh, down", "/?view=dock&lang=zh&state=down"],
    ["dock, changed", "/?view=dock&state=changed"],
    ["settings", "/?view=settings"],
    ["settings, zh", "/?view=settings&lang=zh"],
    // The appearance switch: a forced face has to paint on the other kind of
    // desktop too, and its row has to fit beside the language one.
    ["settings, zh, forced light", "/?view=settings&lang=zh&theme=light"],
    ["rings, forced dark", "/?theme=dark"],
    ["dock, forced light", "/?view=dock&theme=light"],
    ["dock, zh, forced dark", "/?view=dock&lang=zh&theme=dark"],
    // Panel opacity: the thinner surfaces have to keep the text on them.
    ["rings, opacity 60", "/?opacity=60"],
    ["dock, opacity 60", "/?view=dock&opacity=60"],
    ["settings, zh, opacity 60", "/?view=settings&lang=zh&opacity=60"],
  ];

  for (const dark of [true, false]) {
    test.describe(dark ? "dark" : "light", () => {
      test.use({ colorScheme: dark ? "dark" : "light" });
      for (const [name, path] of cases) {
        test(`${name} (${dark ? "dark" : "light"})`, async ({ page }) => {
          const consoleErrors = [];
          const pageErrors = [];
          page.on("console", (m) => {
            if (m.type() === "error") consoleErrors.push(m.text());
          });
          page.on("pageerror", (e) => pageErrors.push(String(e)));

          await page.goto(url(path));
          await page.waitForTimeout(600);

          expect(pageErrors, "page errors").toEqual([]);
          expect(consoleErrors, "console errors").toEqual([]);

          // Nothing may stick out of the window: the popover sizes itself.
          const overflow = await page.evaluate(
            () =>
              document.documentElement.scrollWidth -
              document.documentElement.clientWidth
          );
          expect(overflow, "horizontal overflow").toBeLessThanOrEqual(0);
        });
      }
    });
  }
});

test.describe("global shortcut recorder", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto(url("/?view=settings"));
    await page.waitForTimeout(400);
  });

  test("rejects a bare letter", async ({ page }) => {
    await page.click("#sc-custom");
    await page.keyboard.press("b");
    // It says what a chord needs instead of taking the letter, and keeps
    // listening: the pill still asks for keys, the note explains.
    const note = page.locator("#shortcut-note");
    await expect(note).toBeVisible();
    await expect(note).toContainText(/Hold .*with a key|按住 .*再加一个键/);
    await expect(customPill(page)).toHaveText(/Press keys…|请按组合键…/);
    await expect(customPill(page)).toHaveClass(/\brec\b/);
    // The chord the settings came with is untouched, in the controls and in
    // what is stored: nothing was recorded.
    await expect(presetU(page)).toHaveClass(/\bon\b/);
    expect(await storedShortcut(page)).toBeNull();
    // Esc is the way out, and it leaves the chord where it was.
    await page.keyboard.press("Escape");
    await expectPresetChordHeld(page);
    await expect(note).toBeHidden();
  });

  test("accepts a bare function key", async ({ page }) => {
    await page.click("#sc-custom");
    await page.keyboard.press("F7");
    await expect(page.locator("#sc-custom")).toHaveText(/^F7$/);
    await expect(page.locator("#shortcut-note")).toBeHidden();
  });

  test("records a modifier chord", async ({ page }) => {
    await page.click("#sc-custom");
    await page.keyboard.down("Control");
    await page.keyboard.down("Shift");
    await page.keyboard.press("k");
    await page.keyboard.up("Shift");
    await page.keyboard.up("Control");
    await expect(page.locator("#sc-custom")).toHaveText(/^Ctrl\+Shift\+K$/);
  });

  test("Escape cancels without overwriting", async ({ page }) => {
    await page.click("#sc-custom");
    await expect(customPill(page)).toHaveText(/Press keys…|请按组合键…/);
    await page.keyboard.press("Escape");
    // The pill goes back to its idle word; the preset is still the chord.
    await expectPresetChordHeld(page);
  });

  test("Tab leaves the recorder and keeps the chord", async ({ page }) => {
    await page.click("#sc-custom");
    await page.keyboard.press("Tab");
    await expectPresetChordHeld(page);
  });

  test("Ctrl+Tab switches away instead of being recorded", async ({ page }) => {
    await page.click("#sc-custom");
    await page.keyboard.down("Control");
    await page.keyboard.press("Tab");
    await page.keyboard.up("Control");
    await expectPresetChordHeld(page);
  });
});

test.describe("the Dock view's controls", () => {
  test("toggle remembers the last shortcut, not the first preset", async ({
    page,
  }) => {
    // Record F7 in settings first: the chord is stored under `shortcut`, and
    // under `last-shortcut` as the one the switches come back to.
    await page.goto(url("/?view=settings"));
    await page.waitForTimeout(400);
    await page.click("#sc-custom");
    await page.keyboard.press("F7");
    await expect(customPill(page)).toHaveText(/^F7$/);
    expect(await storedShortcut(page)).toBe("F7");

    // Dock's switch turns it off and back on to the recorded chord — the page
    // reads `shortcut` / `last-shortcut` afresh, so this walks the storage.
    await page.goto(url("/?view=dock&colors=1"));
    await page.waitForTimeout(500);
    await expect(page.locator("#dock-shortcut")).toHaveText(/^F7$/);
    await page.click("#dock-shortcut");
    await expect(page.locator("#dock-shortcut")).toHaveText(/Off|关闭/);
    expect(await storedShortcut(page)).toBe("");
    expect(await storedShortcut(page, "last-shortcut")).toBe("F7");
    await page.click("#dock-shortcut");
    await expect(page.locator("#dock-shortcut")).toHaveText(/^F7$/);
    expect(await storedShortcut(page)).toBe("F7");
  });

  test("colours drawer opens, paints and resets", async ({ page }) => {
    await page.goto(url("/?view=dock&colors=1"));
    await page.waitForTimeout(500);
    await expect(page.locator(".cb-item")).toHaveCount(5);
    await page
      .locator('.cb-item[data-color="claude"] [data-color-input]')
      .evaluate((el) => {
        el.value = "#ff0000";
        el.dispatchEvent(new Event("input", { bubbles: true }));
      });
    await expect(
      page.locator('.cb-item[data-color="claude"] [data-hex]')
    ).toHaveText("#FF0000");
    await page.locator('.cb-item[data-color="claude"] [data-reset-color]').click();
    await expect(
      page.locator('.cb-item[data-color="claude"] [data-hex]')
    ).toHaveText("#FFFFFF");
  });

  test("window width follows the tiles", async ({ page }) => {
    // `state=three` leaves two services unconnected; `hide=1` is what takes
    // their tiles out of the Dock, so three are left to size the window.
    await page.goto(url("/?view=dock&state=three&hide=1"));
    await page.waitForTimeout(500);
    await expect(page.locator("#dock .cell")).toHaveCount(3);
    expect(await page.evaluate(() => document.getElementById("panel").style.width)).toBe(
      "496px"
    );
    await page.goto(url("/?view=dock"));
    await page.waitForTimeout(500);
    await expect(page.locator("#dock .cell")).toHaveCount(5);
    expect(await page.evaluate(() => document.getElementById("panel").style.width)).toBe(
      "788px"
    );
  });
});

test.describe("a Dock tile says why it is quiet", () => {
  const cases = [
    ["down", "codex", /Couldn't reach ChatGPT \(timed out\)\./],
    ["changed", "grok", /answered in a format this app doesn't recognise/],
    ["mixed", "copilot", /^Login expired$/],
  ];
  for (const [state, id, re] of cases) {
    test(`state=${state} — ${id}`, async ({ page }) => {
      await page.goto(url(`/?view=dock&state=${state}`));
      await page.waitForTimeout(500);
      await expect(page.locator(`.cell[data-id="${id}"]`)).toHaveAttribute(
        "title",
        re
      );
    });
  }
  test("a connected tile carries no title", async ({ page }) => {
    await page.goto(url("/?view=dock"));
    await page.waitForTimeout(500);
    expect(await page.locator('.cell[data-id="claude"]').getAttribute("title")).toBeNull();
  });
});

test.describe("the rings popover says why a row is quiet", () => {
  test("changed API names the app, not the login", async ({ page }) => {
    await page.goto(url("/?state=changed"));
    await page.waitForTimeout(500);
    await expect(page.locator(".svc.grok .svc-msg")).toContainText(
      /its API has probably changed/
    );
    await expect(page.locator(".svc.grok .inline-btn")).toHaveCount(0);
  });
  test("a timed-out row says what happened, in this language", async ({
    page,
  }) => {
    await page.goto(url("/?lang=zh&state=down"));
    await page.waitForTimeout(500);
    await expect(page.locator(".svc.codex .svc-msg")).toContainText(/暂时连不上|超时/);
  });
  test("the hero reads the tightest window", async ({ page }) => {
    await page.goto(url("/?lang=zh"));
    await page.waitForTimeout(500);
    await expect(page.locator(".hero-num")).toContainText(/^[0-9]+%$/);
  });
});

test.describe("view switching and the language switch", () => {
  test("footer seg switches to the Dock and back", async ({ page }) => {
    await page.goto(url("/"));
    await page.waitForTimeout(400);
    await page.click('.bottom .view-seg [data-view="dock"]');
    await expect(page.locator("#panel")).toHaveAttribute("data-view", "dock", {
      timeout: 2000,
    });
    await expect(page.locator("#view-dock")).toBeVisible();
    await page.click('.dock-bar .view-seg [data-view="concentric"]');
    await expect(page.locator("#panel")).toHaveAttribute("data-view", "concentric");
    // The choice survives a reload.
    await page.reload();
    await page.waitForTimeout(400);
    await expect(page.locator("#panel")).toHaveAttribute("data-view", "concentric");
  });

  test("settings language switch repaints everything", async ({ page }) => {
    await page.goto(url("/?view=settings"));
    await page.waitForTimeout(400);
    await page.click('[data-setting="lang"] [data-value="zh"]');
    await expect(page.locator(".brand-name")).toHaveText("AI 用量");
    await expect(page.locator("html")).toHaveAttribute("lang", "zh-CN");
  });
});

// --------------------------------------------------------------- appearance
test.describe("the appearance switch", () => {
  // The two inks the palette itself sets, so a test can tell the faces apart
  // without trusting any one surface's colour.
  const INK = { dark: "rgb(244, 244, 245)", light: "rgb(24, 24, 27)" };
  const themeButton = (v) => `.seg[data-setting="theme"] button[data-value="${v}"]`;
  const savedTheme = (page) => page.evaluate(() => localStorage.getItem("theme"));

  test("Light wins over a dark desktop, and Auto hands it back", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto(url("/?view=settings"));
    await page.waitForTimeout(400);
    // Auto on a dark desktop paints dark, and says so.
    await expect(page.locator("html")).toHaveAttribute("data-theme", "auto");
    expect(await computed(page, "body", "color")).toBe(INK.dark);

    await page.click(themeButton("light"));
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await expect(page.locator(themeButton("light"))).toHaveClass(/\bon\b/);
    // The panel goes pale although the desktop is still dark.
    expect(await computed(page, "body", "color")).toBe(INK.light);
    expect(channelsOf(await computed(page, "#panel", "backgroundColor"))).toEqual([250, 250, 250]);
    expect(await savedTheme(page)).toBe("light");

    await page.click(themeButton("auto"));
    await expect(page.locator("html")).toHaveAttribute("data-theme", "auto");
    expect(await computed(page, "body", "color")).toBe(INK.dark);
    expect(await savedTheme(page)).toBe("auto");
  });

  test("the Dock's tray takes the same face", async ({ page }) => {
    // The Dock has its own palette (surface, tray, ink) — it has to follow, not
    // just the popover's.
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto(url("/?view=dock&state=empty"));
    await page.waitForTimeout(500);
    expect(channelsOf(await computed(page, "#panel", "backgroundColor"))).toEqual([11, 11, 19]);

    // A dock panel has no Settings button of its own: a quiet squircle opens it.
    await page.click('#dock-logins [data-login="copilot"]');
    await expect(page.locator("#view-settings")).toBeVisible();
    await page.click(themeButton("light"));
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    await page.click("#back");
    await expect(page.locator("#view-dock")).toBeVisible();
    expect(channelsOf(await computed(page, "#panel", "backgroundColor"))).toEqual([247, 247, 249]);
    expect(await computed(page, ".view-dock .names span", "color")).toBe(INK.light);
  });

  test("the choice survives a reload", async ({ page }) => {
    await page.goto(url("/"));
    await page.waitForTimeout(400);
    await page.click("#settings-btn");
    await page.click(themeButton("dark"));
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    expect(await savedTheme(page)).toBe("dark");

    await page.reload();
    await page.waitForTimeout(400);
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
    expect(await computed(page, "body", "color")).toBe(INK.dark);
  });

  test("Auto follows a system flip, down to the default colours", async ({ page }) => {
    await page.goto(url("/?view=settings"));
    await page.waitForTimeout(400);
    const hex = page.locator('.color-row[data-color="claude"] [data-hex]');
    await expect(hex).toHaveText("#D0704F"); // Claude on a light desktop
    await page.emulateMedia({ colorScheme: "dark" });
    await expect(page.locator("html")).toHaveAttribute("data-theme", "auto");
    expect(await computed(page, "body", "color")).toBe(INK.dark);
    // The rows read their defaults from the palette, so they follow along.
    await expect(hex).toHaveText("#E08A6B");
  });

  test("?theme= previews a face without saving it", async ({ page }) => {
    await page.emulateMedia({ colorScheme: "dark" });
    await page.goto(url("/?theme=light"));
    await page.waitForTimeout(400);
    await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
    expect(await computed(page, "body", "color")).toBe(INK.light);
    // A preview, not a choice: nothing was stored…
    expect(await savedTheme(page)).toBeNull();
    // …and the control shows the face being previewed.
    await page.click("#settings-btn");
    await expect(page.locator(themeButton("light"))).toHaveClass(/\bon\b/);
  });

  test("names the three choices in both languages", async ({ page }) => {
    await page.goto(url("/?view=settings&lang=zh"));
    await page.waitForTimeout(400);
    await expect(page.locator(themeButton("auto"))).toHaveText("跟随系统");
    await expect(page.locator(themeButton("dark"))).toHaveText("深色");
    await expect(page.locator(themeButton("light"))).toHaveText("浅色");
    await page.click(themeButton("dark"));
    await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  });
});

// ------------------------------------------------------------ panel opacity
test.describe("panel opacity", () => {
  const alpha = (page) =>
    page.evaluate(() =>
      document.documentElement.style.getPropertyValue("--panel-alpha").trim()
    );
  const saved = (page) => page.evaluate(() => localStorage.getItem("opacity"));
  const panelBg = (page) => computed(page, "#panel", "backgroundColor");

  test("the slider thins the panel, never the ink on it", async ({ page }) => {
    await page.goto(url("/?view=settings"));
    await page.waitForTimeout(400);
    // 100 % is the panel exactly as it always was.
    expect(await alpha(page)).toBe("1");
    await expect(page.locator("#opacity-value")).toHaveText("100%");
    const solid = await panelBg(page);
    const ink = await computed(page, "body", "color");
    const label = await computed(page, "#opacity-label", "color");

    await page.locator("#set-opacity").fill("75");
    await expect(page.locator("#opacity-value")).toHaveText("75%");
    expect(await alpha(page)).toBe("0.75");
    expect(await saved(page)).toBe("75");

    // The surface gives way: the light panel's 0.9 × 0.75.
    const thin = await panelBg(page);
    expect(alphaOf(thin)).toBeCloseTo(0.675, 2);
    expect(alphaOf(thin)).toBeLessThan(alphaOf(solid));
    // …while the text, its colour and the panel's own opacity are untouched.
    expect(await computed(page, "body", "color")).toBe(ink);
    expect(await computed(page, "#opacity-label", "color")).toBe(label);
    expect(await computed(page, "#panel", "opacity")).toBe("1");

    // The choice survives a restart.
    await page.reload();
    await page.waitForTimeout(400);
    expect(await alpha(page)).toBe("0.75");
    await expect(page.locator("#opacity-value")).toHaveText("75%");
    expect(await page.locator("#set-opacity").inputValue()).toBe("75");
  });

  test("?opacity=60 previews it in Rings and Dock, and stores nothing", async ({
    page,
  }) => {
    const errors = [];
    page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
    page.on("pageerror", (e) => errors.push(String(e)));

    await page.goto(url("/?opacity=60"));
    await page.waitForTimeout(500);
    expect(await alpha(page)).toBe("0.6");
    expect(await saved(page)).toBeNull();
    expect(alphaOf(await panelBg(page))).toBeCloseTo(0.54, 2); // 0.9 × 0.6

    // The Dock's own surfaces are thinned the same way: panel and tray.
    await page.goto(url("/?view=dock&opacity=60"));
    await page.waitForTimeout(500);
    expect(await alpha(page)).toBe("0.6");
    expect(alphaOf(await panelBg(page))).toBeCloseTo(0.6, 2);
    const tray = await computed(page, "#dock", "backgroundColor");
    expect(alphaOf(tray)).toBeCloseTo(0.6, 2);
    expect(errors, "console and page errors").toEqual([]);
  });

  test("clamps what it is given", async ({ page }) => {
    await page.goto(url("/?opacity=5"));
    await page.waitForTimeout(300);
    expect(await alpha(page)).toBe("0.4");
    await page.goto(url("/?opacity=oops"));
    await page.waitForTimeout(300);
    expect(await alpha(page)).toBe("1");
  });
});
