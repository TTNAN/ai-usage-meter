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
