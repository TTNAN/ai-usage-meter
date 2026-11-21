//! The parts of AI Usage Meter and AI Usage Meter Dock that never touch Tauri:
//! the usage providers, the low-quota alert rules, the menu-bar / tray icon and
//! the screen-edge docking maths.
//!
//! Each app keeps its own copy of this crate (`<app>/src-tauri/crates/ai-usage-core`)
//! because each project is distributed as a standalone zip that has to build on
//! its own. The copies are byte-identical; `sync-core.sh` in either project
//! compares them and puts one back in step with the other.
//!
//! What stays in an app: `main.rs` (tray, window, commands, poller) and
//! `web.rs` (the in-app claude.ai / grok.com sign-in), which are Tauri-shaped.

pub mod edge;
pub mod notify;
pub mod providers;
pub mod tray_icon;

/// The `User-Agent` both apps send. One shared core means one shared name; it
/// carries the core's version, which moves with the apps.
pub const USER_AGENT: &str = concat!("ai-usage-meter/", env!("CARGO_PKG_VERSION"));
