中文 | [中文](./README.md)

# AI Usage Meter

> A small menu-bar / system-tray window, styled like a black Dock, that shows your Claude, Codex, and Grok usage at a glance.

![AI Usage Meter](docs/screenshot.png)

## What it is

AI Usage Meter is a Tauri v2 app (Rust + plain HTML/CSS/JS, no bundler). It lives in the menu bar on macOS and in the system tray on Windows.

The window holds three cards:

| Service | Icon colour | Shows |
| --- | --- | --- |
| Claude | Orange `#D97757` | % used, % left, progress bar, reset countdown |
| Codex | Blue `#3B82F6` | % used, % left, progress bar, reset countdown |
| Grok | Purple `#A855F7` | % used, % left, progress bar, reset countdown |

- The reset countdown ticks every second.
- The tray tooltip shows the usage summary at a glance, so you can see it without opening the window.
- Click the tray icon to toggle the window.

## Sync: auto-sync only

v1 only does auto-sync: it reads the already-logged-in state of the three official CLIs on your machine and queries each provider's API for usage.

There is no manual-entry main flow. If you have not logged in to a CLI, the matching card shows "Not connected" (this is what Grok shows when the Grok CLI is not logged in); if none of the three is logged in, the UI shows a DEMO DATA badge, using the post's 37 / 42 / 57% figures only as demo data, not real usage.

## How each service is checked

| Service | Endpoint | Credential source | Poll interval |
| --- | --- | --- | --- |
| Claude | `GET https://api.anthropic.com/api/oauth/usage` | Claude Code OAuth token from `~/.claude/.credentials.json`, header `anthropic-beta: oauth-2025-04-20` | >= 5 min |
| Codex | `GET https://chatgpt.com/backend-api/wham/usage` | Codex OAuth token from `~/.codex/auth.json`, header `chatgpt-account-id` | 60s |
| Grok | `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits` | Token from `~/.grok/auth.json`, header `x-xai-token-auth: xai-grok-cli` | 5–10 min |

- Claude returns `five_hour.utilization`, `resets_at`, and `seven_day`.
- Codex returns `primary_window` (5h) and `secondary_window` (7d) `used_percent` plus `reset_at`.
- Grok returns `config.creditUsagePercent` (used) and `currentPeriod` (weekly billing period).

> ⚠️ The Codex card shows **Codex CLI quota, NOT ChatGPT web-chat quota**. They are not the same quota — do not confuse them.

## Auth and privacy

- v1 ONLY reuses the official CLIs' logged-in state: install Claude Code, Codex CLI, and Grok CLI, and log in once each.
- No passwords, no pasted cookies.
- Tokens are read from disk at call time, never written anywhere by the app, and never uploaded.
- Read-only GET requests only. If an endpoint fails, the card shows "Unavailable" — never fake numbers.
- Local only: no server, no analytics, no upload.

## Risks

Please understand these before using it:

- All three are unofficial / internal endpoints; no official subscription-quota API exists. They may change at any time and may require re-login.
- Data on each provider's settings page is authoritative.
- Low-frequency read-only polling is lower risk, but not zero risk. No public case was found of accounts banned for usage polling alone — but we do NOT promise you will never be banned.

## Install

### macOS

Download the `.dmg` from Releases. v1 is unsigned (no paid Apple Developer notarization), so on first launch: right-click the app > Open, or run:

```bash
xattr -d com.apple.quarantine /Applications/AI\ Usage\ Meter.app
```

### Windows

Download the `.exe` from Releases and double-click it.

### Prerequisites

Install and log in to Claude Code, Codex CLI, and Grok CLI first (see "Auth and privacy" above).

## Build from source

Node is not needed; there are no npm dependencies. Install Rust stable, then:

```bash
cargo install tauri-cli --version "^2"
cargo tauri build
```

Smoke test:

```bash
AI_USAGE_METER_MOCK=1 ./src-tauri/target/debug/ai-usage-meter --smoke-test
```

This prints JSON and exits 0.

## FAQ

**Is the Codex figure my ChatGPT web-chat quota?**

No. It is the Codex CLI quota, not the ChatGPT web-chat quota.

**What if a card shows "Unavailable"?**

That means the request to that endpoint failed — the endpoint may have changed, or the login may have expired. First confirm you are logged in via the official CLI, then check usage on the provider's settings page. The app will not substitute fake numbers.

**What if a card shows "Not connected"?**

That means no login credential for that CLI was found on this machine. Install and log in to the corresponding official CLI.

**Will my tokens be uploaded?**

No. Tokens are read from disk at call time only, never written anywhere, and never uploaded. The app runs locally with no server.

**What if the usage looks wrong?**

Trust the data on each provider's settings page. This project uses unofficial endpoints, which may change at any time.

## License

MIT

## Acknowledgements

- [@Litx4i](https://x.com/Litx4i) — the inspiration for this project. Original post: https://x.com/Litx4i/status/2123400444458488080
- The Quota project (https://github.com/josephbergvinson/quota) — the pattern for reusing CLI login state.

## Trademark

Claude is a trademark of Anthropic. ChatGPT / OpenAI are trademarks of OpenAI. Grok / xAI are trademarks of xAI. All brand icons and trademarks used in this project belong to their respective owners, are used only to identify the service each card refers to, and do not imply endorsement by any of them.
