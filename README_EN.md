<div align="center">

<img src="assets/icons/app-icon-512.png" width="112" alt="AI Usage Meter" />

# AI Usage Meter

**Your Claude, ChatGPT (Codex), Grok, Gemini and GitHub Copilot limits — live in the menu bar or system tray.**

One app, two shapes: the **rings popover** for the full picture, the **dock panel** that puts all five services and their reset times in one place — switch anytime, with the same settings and colours on both.

[中文](./README.md) · English

[![Build](https://github.com/TTNAN/ai-usage-meter/actions/workflows/build.yml/badge.svg)](https://github.com/TTNAN/ai-usage-meter/actions/workflows/build.yml)
[![Release](https://img.shields.io/github/v/release/TTNAN/ai-usage-meter?include_prereleases&color=6e9bff)](https://github.com/TTNAN/ai-usage-meter/releases/latest)
![Platforms](https://img.shields.io/badge/macOS%20%7C%20Windows%20%7C%20Linux-111?logo=tauri&logoColor=white)
[![License](https://img.shields.io/badge/license-MIT-b38cff)](./LICENSE)

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/hero-light.png" />
  <img src="docs/hero.png" alt="The AI Usage Meter rings popover" width="880" />
</picture>

<picture>
  <source media="(prefers-color-scheme: light)" srcset="docs/dock-light.png" />
  <img src="docs/dock.png" alt="The dock view: five brand squircles, the names, status chips and the colours drawer" width="880" />
</picture>

</div>

Tired of hitting "usage limit reached" mid-task? AI Usage Meter puts every 5-hour window, weekly limit, per-model allowance and reset countdown for all five services in your menu bar or system tray. **No browser tabs, no pasted cookies, no servers.**

## Highlights

- **Two shapes, one app** — two pill switches sit side by side above the dock, **Used / Left** and **Rings / Dock**, and either view is one click away. The rings popover spreads every window out; the dock panel is the original dock design — five brand-coloured squircles (ChatGPT white, Claude terracotta, Grok obsidian, Gemini pale, Copilot deep slate) rising a little out of one dark tray, one per service: the official mark, `37 % used`, a hairline bar and `3h 08m to reset`, with a 30×8 trend line and its `~2h52m` ETA under each tile. The names sit under the tray, then a row of `• ChatGPT · Connected` chips and four quiet buttons — `Colors / Edge / Refresh / Quit`; Colors opens the service-colour drawer right there and the window grows to fit it. 788 px wide with five services, narrowing as services drop out ([496 px with three](docs/dock-fewer.png)), with the same settings and colours on both views.
- **See the tightest limit first** — concentric rings and one big number tell you which quota runs out first and when it resets. Connect three services and you get the classic three rings; connect four or five and the gauge adds rings and thins the stroke, so no service is left out.
- **The icon is the gauge** — three tiny battery bars show the three tightest services (three is what actually reads at 16 px); on macOS you can show the percentage right next to it.
- **Every window, not just one** — Claude 5-hour / weekly (plus separate Opus and Sonnet limits), Codex 5-hour / weekly in ChatGPT, Grok's billing period, Gemini's Pro / Flash allowances, Copilot's premium requests and chat, each with its own countdown.
- **See where a number is going** — every row keeps a short local history and draws it as a 36×12 sparkline; at the recent rate it estimates "~1h 42m left", but only when that actually lands before the reset. A quiet service adds nothing.
- **Warned before the wall** — a native notification when a service drops to 15 % left, and one more when it goes under 5 %; each step is announced once per reset window, and "Low-quota alerts" in Settings turns the whole thing off. The alert speaks the interface language (English or Chinese).
- **One global shortcut** — `⌘⇧U` / `Ctrl+Shift+U` brings the popover up, or puts it away, from any app. Settings can switch it to `⌘⇧A`, record any combination of its own (press Custom, then the keys — `Esc` cancels), or turn it off, and says so plainly when another app already owns the combination.
- **Plan aware** — picks up Max, Pro, Plus, SuperGrok, Ultra, Individual and friends.
- **Zero setup** — reuses the logins of the ChatGPT desktop app (Codex), Claude Code, Grok Build, Gemini CLI / Antigravity and the GitHub CLI / Copilot CLI. Only use Claude or Grok on the web or in their apps? Sign in once in Settings. Gemini's access token expires after an hour: the app renews it with the refresh token and writes it back to `~/.gemini/oauth_creds.json`, so you never have to run `gemini` again.
- **Live** — re-reads every 1 to 5 minutes in the background, so usage from the web, the apps or the CLI shows up as it happens.
- **Feels native** — real macOS popover material, follows light and dark mode, opens from the icon and tucks away when you click elsewhere; a tray flyout on Windows, the tray menu on Linux (X11).
- **Drops where you drop it, and docks to the edge** — drag it anywhere by the header or any empty spot; drop it against the left or right screen edge and it snaps there, slides off-screen when it loses focus leaving a 12 px grab handle, and slides back when you click that handle (or the menu-bar icon). "Tuck at screen edge" in Settings turns it off. (A Wayland session places windows itself, so the switch is greyed out there and says why.)
- **Your colours** — Settings → Colors has one row per service: click the swatch for the system colour picker, or one of the presets, and the concentric rings, progress bars and accents follow at once; the Windows and Linux tray bars use the same colours. ↺ or "Reset all" restores each service's own accent. The dock's own Colors button opens the same set, right under the tray ([what it looks like](docs/dock-colors.png)).
- **English and 中文** — follows your system language, switchable in Settings.
- **Calm and honest** — gentle background polling, rate-limited manual refresh; one failed request keeps the last reading (clearly marked) and it never makes numbers up.
- **Tiny** — Tauri v2 and plain HTML/CSS/JS. No Node, no bundler.

<p align="center"><img src="docs/states.png" alt="Chinese UI, service colours, first run and light mode" width="880" /></p>

## Install

Grab the latest from [Releases](https://github.com/TTNAN/ai-usage-meter/releases/latest) — [v0.7.1](https://github.com/TTNAN/ai-usage-meter/releases/tag/v0.7.1) today, with the macOS `.dmg`, the Windows `.exe` and `.msi`, and the Linux `.deb` and `.AppImage`:

| Platform | File | Notes |
| --- | --- | --- |
| macOS (Apple silicon + Intel) | `.dmg` | Drag to Applications. Not notarised yet: right-click → Open the first time, or run the command below |
| Windows 10 / 11 | `.exe` or `.msi` | Per-user install, no admin needed |
| Linux x64 (Ubuntu / Debian) | `.deb` or `.AppImage` | Needs `libwebkit2gtk-4.1` and an AppIndicator extension for the tray (Ubuntu ships it; stock GNOME needs the AppIndicator extension) |

```bash
xattr -cr "/Applications/AI Usage Meter.app"
```

> [!TIP]
> The app is not notarised yet, so if macOS blocks the first launch, right-click → Open (or run the command above) once and it opens normally from then on.
>
> Linux: install the `.deb` with `sudo apt install ./AI.Usage.Meter_0.7.1_amd64.deb`, or `chmod +x` the `.AppImage` and run it (AppImage needs FUSE, or use `--appimage-extract-and-run`). A Wayland session grants no global shortcut and lets no app place its own window, so neither the shortcut nor edge tucking is available there — Settings says so.

Then sign in to whichever tools you use (the rest just show "Not connected"):

| Service | Easiest | Also works |
| --- | --- | --- |
| Claude | Click "Sign in" on the Claude row (or Settings → Claude account) and sign in to claude.ai | Picked up automatically if [Claude Code](https://docs.anthropic.com/en/docs/claude-code) is signed in |
| ChatGPT (Codex quota) | Open Codex in the [ChatGPT desktop app](https://openai.com/chatgpt/download/) and sign in | [Codex CLI](https://github.com/openai/codex) `codex login` |
| Grok | Click "Sign in" on the Grok row (or Settings → Grok account) and sign in to grok.com | Picked up automatically if [Grok Build](https://docs.x.ai/build/overview) is signed in |
| Gemini | Sign in to [Gemini CLI](https://github.com/google-gemini/gemini-cli) once (run `gemini`) | An account already signed in to Google Antigravity works too |
| GitHub Copilot | Run `gh auth login` (GitHub CLI) | Picked up automatically if you've signed in with the Copilot CLI |

> Claude on the web, desktop, mobile and Claude Code share one usage pool, and Grok's web app, mobile apps and Grok Build share one weekly pool, so any of them shows the same number. If Google sign-in is blocked in the sign-in window, use email instead.
>
> Gemini signed in with an API key or Vertex AI has no quota endpoint; the row says so and points at a Google-account sign-in. Copilot reads the Copilot subscription on your GitHub account (individual or business), and says so when the subscription has ended.

## Shortcuts

| Keys | Action |
| --- | --- |
| `⌘⇧U` / `Ctrl+Shift+U` | Global: show or put away the popover from any app (`⌘⇧A`, any combination you record — `⌘⌥K`, even `F1` — or off in Settings) |
| `⌘R` / `Ctrl R` | Refresh |
| `⌘,` | Settings |
| `Esc` | Close the popover (or tuck it back against the edge) |
| `⌘Q` / `Ctrl Q` | Quit |

## How it works

None of these services offers a public "subscription quota" API, so AI Usage Meter reads the same endpoints their own apps use: either with the login their CLI / desktop app already saved on your machine, or with the web account you sign in to inside the app.

| Service | Endpoint | Credential | Polling |
| --- | --- | --- | --- |
| Claude | `GET api.anthropic.com/api/oauth/usage` | macOS Keychain `Claude Code-credentials`, or `~/.claude/.credentials.json` | 5 min |
| ChatGPT (Codex) | `GET chatgpt.com/backend-api/wham/usage`; if no login is readable or the request fails, the limits recorded in your newest session under `~/.codex/sessions` | `~/.codex/auth.json` (shared by the ChatGPT app and Codex CLI, `$CODEX_HOME` respected) | 1 min |
| Grok | `GET cli-chat-proxy.grok.com/v1/billing` | `~/.grok/auth.json` (`$GROK_HOME` respected) | 5 min |
| Gemini | `POST cloudcode-pa.googleapis.com/v1internal:loadCodeAssist` for the plan, `:retrieveUserQuota` for each model's remaining allowance; an expired token is renewed with `POST oauth2.googleapis.com/token` and the call retried once | `~/.gemini/oauth_creds.json` (`$GEMINI_HOME` respected, shared by Gemini CLI and Antigravity) | 5 min |
| GitHub Copilot | `GET api.github.com/copilot_internal/user` | GitHub OAuth token: `$GH_TOKEN` / `$GITHUB_TOKEN`, `gh`'s `hosts.yml`, the Copilot CLI's `hosts.json`, falling back to `gh auth token` | 5 min |
| Claude web account | `GET claude.ai/api/organizations/{org}/usage` | the claude.ai session from the in-app sign-in window | 3 min |
| Grok web account | the `GetGrokCreditsConfig` call grok.com's own usage page makes | the grok.com session from the in-app sign-in window | 5 min |

Web-account requests run in a hidden window, on the service's own page (just like opening its usage page in a browser), and the result is handed back to the app. Session cookies stay in the app's own browser storage: never read out, copied or sent anywhere else. Click "Signed in" in Settings to sign out and clear them.

> [!NOTE]
> Since July 2026 the Codex app is part of the ChatGPT desktop app, so the row is called ChatGPT. It shows your **Codex** quota, not ChatGPT chat limits; they are counted separately.
>
> For Claude and Grok, both a CLI login and a web account work. With both, the CLI is used first and the web account fills in when it can't answer.

## Privacy

- Credentials are read at request time and sent only to the service they belong to. Never stored, never uploaded.
- Usage queries only (Gemini's are two POST queries the official CLI makes too). Your accounts and CLI config files are never modified. The one thing written back is `access_token` and `expiry_date` in `~/.gemini/oauth_creds.json` after a token renewal; every other field is left as it was.
- Gemini renewals use Gemini CLI's own OAuth client. It belongs to the CLI, not to this project, so this repository does not carry it: the pair is read from the `@google/gemini-cli` package installed on your machine (or from `oauth_creds.json` when it names one), and `GEMINI_CLI_OAUTH_CLIENT_ID` / `GEMINI_CLI_OAUTH_CLIENT_SECRET` override it. When neither is found the row asks you to sign in again instead of guessing.
- The usage trend lives in this app's own `localStorage` and never touches the network.
- No servers, no analytics, no telemetry. A few thousand lines — read [`crates/ai-usage-core/src/providers.rs`](src-tauri/crates/ai-usage-core/src/providers.rs) and [`web.rs`](src-tauri/src/web.rs).

## Caveats

These are unofficial, internal endpoints and can change at any time. If one does, the row says which: "can't reach it" when the request fails, "API changed" when the service's answer no longer parses. Either way it never shows a wrong number; each service's own settings page is the source of truth. Low-frequency read-only polling is low risk and there are no public reports of bans from it, but we can't promise there never will be.

## Contributing

You can work on the UI without Rust: serve `src/` with any static server and it runs as a preview with demo data.

```bash
python3 -m http.server -d src 8000
# http://localhost:8000/                     demo data (with trends and ETAs)
# http://localhost:8000/?view=dock           the dock view, five tiles
# http://localhost:8000/?view=dock&colors=1  the dock view with the colours drawer open
# http://localhost:8000/?view=dock&state=three&hide=1   a three-service dock
# http://localhost:8000/?state=trend         the rings view, to look at the sparklines and ETAs
# http://localhost:8000/?state=empty          first run
# http://localhost:8000/?state=mixed          mixed (not connected / expired)
# http://localhost:8000/?state=down           two unreachable (timed out / HTTP 502)
# http://localhost:8000/?state=changed        a changed API (Grok's answer no longer parses)
# http://localhost:8000/?state=four           four connected (four rings)
# http://localhost:8000/?view=settings        settings
# add &wayland=1 to preview the greyed-out edge switch of a Wayland session
# add &lang=zh or &lang=en to force the UI language
```

The desktop app needs Rust stable:

```bash
cargo install tauri-cli --version "^2"
cargo tauri dev                    # develop
cargo tauri build                  # package
AI_USAGE_METER_MOCK=1 cargo tauri dev   # run with demo data
```

On Linux, install the system libraries first (Ubuntu / Debian):

```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
cargo tauri build --bundles deb,appimage   # Linux ships these two (rpm needs rpmbuild)
```

The backend comes in two pieces: what is this app's alone in `src-tauri/src/` (tray, window, commands, poller, web sign-in), and the Tauri-free core in `src-tauri/crates/ai-usage-core/` (the five providers, low-quota rules, the tray icon, the edge maths). Both views share the one backend, and the window takes its size from the page (`set_window_size`), which is how the rings popover and the dock bar live in the same native window.

```bash
cd src-tauri/crates/ai-usage-core && cargo test   # core unit tests
cd src-tauri && cargo test                        # app layer (needs Tauri's system deps)
```

Want another service (Cursor, Windsurf…)? Open an [issue](https://github.com/TTNAN/ai-usage-meter/issues) or a PR — each service is a single function in `ai-usage-core/src/providers.rs`.

## Roadmap

- [x] Notifications when a limit is about to run out
- [x] More services: Gemini CLI / Antigravity, GitHub Copilot
- [x] Usage trend sparkline and a burn-rate ETA
- [x] A global shortcut (`⌘⇧U` / `Ctrl+Shift+U`, or one you record)
- [x] The Dock edition folded into one app: rings and dock views (0.7.0)
- [x] A Linux build: `.deb` and `.AppImage`, with a tray icon
- [ ] More services: Cursor, Windsurf
- [ ] Signed and notarised macOS builds

## Thanks

- [@Litx4i](https://x.com/Litx4i/status/2123400444458488080) — the original idea
- [Quota](https://github.com/josephbergvinson/quota) — the reuse-the-CLI-login approach

If it saved you from one "limit reached" surprise, a ⭐️ helps it keep getting better.

## Buy me a coffee

If this project saved you some time, a coffee is always appreciated. ☕

| WeChat Pay |
| -------- |
| <img src="assets/wechat-pay.png" width="220" alt="WeChat Pay tip code"> |

## License and trademarks

[MIT](./LICENSE). Claude is a trademark of Anthropic; OpenAI / ChatGPT / Codex are trademarks of OpenAI; Grok / xAI are trademarks of xAI; Gemini / Google are trademarks of Google; GitHub Copilot is a trademark of GitHub. Brand marks only identify each service and imply no endorsement: Gemini uses Google's official four-colour spark (Wikimedia Commons `Google_Gemini_icon_2025.svg`), Copilot uses `copilot-24.svg` from GitHub's Primer Octicons, and the rest are listed in the icon notes. Screenshots use demo data.
