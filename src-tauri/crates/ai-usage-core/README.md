# ai-usage-core

The parts of AI Usage Meter that never touch Tauri: the usage providers
(Claude, ChatGPT/Codex, Grok, Gemini, GitHub Copilot), the low-quota alert
rules, the menu-bar / tray icon and the screen-edge docking maths. Both views
of the app — the rings popover and the dock panel — share this crate.

The app's `src-tauri/src/main.rs` (tray, window, commands, poller) and `web.rs`
(the in-app claude.ai / grok.com sign-in) stay in the app, because they are
Tauri-shaped.

`cargo test` in this directory runs the module tests: no network, no logins, no
files outside a temp directory.
