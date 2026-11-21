# ai-usage-core

The parts of AI Usage Meter and AI Usage Meter Dock that never touch Tauri:
the usage providers (Claude, ChatGPT/Codex, Grok, Gemini, GitHub Copilot), the
low-quota alert rules, the menu-bar / tray icon and the screen-edge docking
maths.

Each app keeps its own copy at `src-tauri/crates/ai-usage-core` because each
project ships as a standalone zip that has to build on its own. The two copies
are byte-identical; run `./sync-core.sh` from either project after editing this
crate, or `./sync-core.sh --check` to verify without writing.

The apps' `src-tauri/src/main.rs` (tray, window, commands, poller) and `web.rs`
(the in-app claude.ai / grok.com sign-in) stay in the app, because they are
Tauri-shaped.

`cargo test` in this directory runs the module tests: no network, no logins, no
files outside a temp directory.
