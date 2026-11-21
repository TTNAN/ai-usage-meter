//! Low-quota alerts: decides when a service's remaining quota deserves one
//! native notification, and remembers what has already been announced so a
//! reset window is never announced twice.
//!
//! This module knows nothing about Tauri: `main.rs` reduces the fetched rows
//! to [`Reading`]s, this module decides, and `main.rs` shows the result. That
//! keeps the threshold and deduplication rules testable on their own.
//!
//! A service is judged by the window that binds first — the most used one,
//! which is the same number the row shows in bold.

use std::collections::HashSet;

/// Percent still left at or below which a service warns once.
pub const WARN_LEFT: f64 = 15.0;
/// …and at or below which it says the quota is nearly out.
pub const CRIT_LEFT: f64 = 5.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    Warning,
    Critical,
}

/// One service, reduced to the window that binds first.
#[derive(Debug, Clone, Copy)]
pub struct Reading<'a> {
    pub id: &'a str,
    pub label: &'a str,
    /// Stable window key the row localises ("session", "weekly", …).
    pub window: &'static str,
    /// Percent still left in that window, 0–100.
    pub left: f64,
    /// Unix seconds when the window resets, when the service says.
    pub resets_at: Option<i64>,
}

/// The text of one alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub title: String,
    pub body: String,
}

/// Whether alerts should be written in Chinese. `main.rs` keeps the interface
/// language the popover resolved ("auto" included) and passes it in here.
pub fn is_zh(lang: &str) -> bool {
    lang.to_ascii_lowercase().starts_with("zh")
}

/// Which tier a remaining percentage falls in, if any.
pub fn tier_for_left(left: f64) -> Option<Tier> {
    if left <= CRIT_LEFT {
        Some(Tier::Critical)
    } else if left <= WARN_LEFT {
        Some(Tier::Warning)
    } else {
        None
    }
}

/// What was already announced: `(service, window, reset, tier)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    id: String,
    window: &'static str,
    resets_at: Option<i64>,
    tier: Tier,
}

/// Remembers the announcements of the current quota windows.
#[derive(Default)]
pub struct Alerts {
    seen: HashSet<Key>,
}

impl Alerts {
    /// The alerts this pass should show, and nothing more.
    ///
    /// One per service per [`Tier`] per reset window: a warning fires once,
    /// escalates to "nearly out" when the number drops further, and either is
    /// forgotten once the window resets or the quota climbs back above
    /// [`WARN_LEFT`], so the next drop can speak again.
    pub fn due(&mut self, now: i64, readings: &[Reading], lang: &str) -> Vec<Alert> {
        // Windows that already reset start over.
        self.seen
            .retain(|k| k.resets_at.map_or(true, |at| at > now));

        let mut out = Vec::new();
        for r in readings {
            let Some(tier) = tier_for_left(r.left) else {
                // Above the line again (a fresh period, or a rollover we have
                // no reset time for): re-arm this window.
                self.seen
                    .retain(|k| !(k.id == r.id && k.window == r.window));
                continue;
            };
            let key = Key {
                id: r.id.to_string(),
                window: r.window,
                resets_at: r.resets_at,
                tier,
            };
            if self.seen.contains(&key) {
                continue;
            }
            self.seen.insert(key.clone());
            if tier == Tier::Critical {
                // "Nearly out" supersedes "running low": no late warning.
                self.seen.insert(Key {
                    tier: Tier::Warning,
                    ..key
                });
            }
            out.push(format_alert(r, tier, now, lang));
        }
        out
    }

    /// Forget everything — used when the user switches alerts back on, so the
    /// window they are in can speak once more.
    pub fn clear(&mut self) {
        self.seen.clear();
    }
}

/// The title and body of one alert, in `lang` ("zh" for Chinese, anything
/// else — including the default "en" — for English).
pub fn format_alert(r: &Reading, tier: Tier, now: i64, lang: &str) -> Alert {
    Alert {
        title: title(r, tier, lang),
        body: body(r, now, lang),
    }
}

fn title(r: &Reading, tier: Tier, lang: &str) -> String {
    if is_zh(lang) {
        match tier {
            Tier::Critical => format!("{} 额度即将用尽", r.label),
            Tier::Warning => format!("{} 额度偏低", r.label),
        }
    } else {
        match tier {
            Tier::Critical => format!("{} is nearly out", r.label),
            Tier::Warning => format!("{} is running low", r.label),
        }
    }
}

fn window_name(window: &str, lang: &str) -> &'static str {
    if is_zh(lang) {
        match window {
            "session" => "5 小时",
            "weekly" => "本周",
            "weekly_opus" => "Opus",
            "weekly_sonnet" => "Sonnet",
            "gemini_pro" => "Pro",
            "gemini_flash" => "Flash",
            "premium" => "高级请求",
            "chat" => "对话",
            "period" => "周期",
            _ => "额度",
        }
    } else {
        match window {
            "session" => "5-hour window",
            "weekly" => "weekly limit",
            "weekly_opus" => "Opus weekly limit",
            "weekly_sonnet" => "Sonnet weekly limit",
            "gemini_pro" => "Pro limit",
            "gemini_flash" => "Flash limit",
            "premium" => "Premium request limit",
            "chat" => "Chat limit",
            "period" => "billing period",
            _ => "limit",
        }
    }
}

fn body(r: &Reading, now: i64, lang: &str) -> String {
    let left = r.left.max(0.0);
    if is_zh(lang) {
        // "5 小时窗口剩余 12%，2 小时 13 分后重置"
        let mut s = format!("{}窗口剩余 {left:.0}%", window_name(r.window, lang));
        if let Some(at) = r.resets_at {
            if at > now {
                s.push_str(&format!("，{}后重置", fmt_dur(at - now, lang)));
            } else {
                // The reset moment has arrived: the row says "resetting now".
                s.push_str("，正在重置");
            }
        }
        return s;
    }
    let mut s = format!("{left:.0}% left of the {}", window_name(r.window, lang));
    if let Some(at) = r.resets_at.filter(|at| *at > now) {
        s.push_str(&format!(", resets in {}", fmt_dur(at - now, lang)));
    }
    s
}

fn fmt_dur(secs: i64, lang: &str) -> String {
    let secs = secs.max(0);
    let (d, h, m) = (secs / 86_400, (secs % 86_400) / 3_600, (secs % 3_600) / 60);
    if is_zh(lang) {
        if d > 0 {
            format!("{d} 天 {h} 小时")
        } else if h > 0 {
            format!("{h} 小时 {m} 分")
        } else {
            format!("{} 分钟", m.max(1))
        }
    } else if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m}m")
    } else {
        format!("{}m", m.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: &str = "en";
    const ZH: &str = "zh";

    fn reading(
        id: &'static str,
        window: &'static str,
        left: f64,
        resets_at: Option<i64>,
    ) -> Reading<'static> {
        let label = match id {
            "codex" => "ChatGPT",
            "grok" => "Grok",
            "gemini" => "Gemini",
            "copilot" => "Copilot",
            _ => "Claude",
        };
        Reading {
            id,
            label,
            window,
            left,
            resets_at,
        }
    }

    #[test]
    fn tiers_cover_the_boundaries() {
        assert_eq!(tier_for_left(80.0), None);
        assert_eq!(tier_for_left(15.01), None);
        assert_eq!(tier_for_left(15.0), Some(Tier::Warning));
        assert_eq!(tier_for_left(5.01), Some(Tier::Warning));
        assert_eq!(tier_for_left(5.0), Some(Tier::Critical));
        assert_eq!(tier_for_left(0.0), Some(Tier::Critical));
        assert_eq!(
            tier_for_left(-4.0),
            Some(Tier::Critical),
            "an over-run still counts as out"
        );
    }

    #[test]
    fn a_quiet_service_never_speaks() {
        let mut alerts = Alerts::default();
        assert!(alerts
            .due(
                1_000,
                &[reading("claude", "session", 62.0, Some(9_000))],
                EN
            )
            .is_empty());
    }

    #[test]
    fn a_window_is_announced_once() {
        let mut alerts = Alerts::default();
        let r = reading("claude", "session", 12.0, Some(9_000));
        let first = alerts.due(1_000, &[r], EN);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].title, "Claude is running low");
        assert_eq!(
            first[0].body,
            "12% left of the 5-hour window, resets in 2h 13m"
        );
        // Same window, still low, ten polls later: silence.
        assert!(alerts
            .due(1_600, &[reading("claude", "session", 9.0, Some(9_000))], EN)
            .is_empty());
    }

    #[test]
    fn a_warning_escalates_to_nearly_out() {
        let mut alerts = Alerts::default();
        assert_eq!(
            alerts
                .due(1_000, &[reading("codex", "weekly", 14.0, Some(90_000))], EN)
                .len(),
            1
        );
        let second = alerts.due(2_000, &[reading("codex", "weekly", 4.0, Some(90_000))], EN);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].title, "ChatGPT is nearly out");
        // …and a late warning in the same window stays quiet.
        assert!(alerts
            .due(3_000, &[reading("codex", "weekly", 6.0, Some(90_000))], EN)
            .is_empty());
        assert!(alerts
            .due(4_000, &[reading("codex", "weekly", 5.0, Some(90_000))], EN)
            .is_empty());
    }

    #[test]
    fn a_new_reset_window_speaks_again() {
        let mut alerts = Alerts::default();
        assert_eq!(
            alerts
                .due(1_000, &[reading("grok", "weekly", 4.0, Some(9_000))], EN)
                .len(),
            1
        );
        // Next period, new reset time, same low number: one fresh alert.
        assert_eq!(
            alerts
                .due(10_000, &[reading("grok", "weekly", 3.0, Some(700_000))], EN)
                .len(),
            1
        );
        assert!(alerts
            .due(10_010, &[reading("grok", "weekly", 3.0, Some(700_000))], EN)
            .is_empty());
    }

    #[test]
    fn climbing_back_above_the_line_rearms_the_window() {
        let mut alerts = Alerts::default();
        assert_eq!(
            alerts
                .due(1_000, &[reading("claude", "session", 8.0, None)], EN)
                .len(),
            1
        );
        // A window we have no reset time for: the only signal is the number.
        assert!(alerts
            .due(2_000, &[reading("claude", "session", 70.0, None)], EN)
            .is_empty());
        assert_eq!(
            alerts
                .due(3_000, &[reading("claude", "session", 12.0, None)], EN)
                .len(),
            1
        );
    }

    #[test]
    fn each_service_and_window_is_judged_on_its_own() {
        let mut alerts = Alerts::default();
        let due = alerts.due(
            1_000,
            &[
                reading("claude", "session", 10.0, Some(9_000)),
                reading("codex", "session", 40.0, Some(9_000)),
                reading("grok", "weekly", 2.0, Some(9_000)),
            ],
            EN,
        );
        assert_eq!(due.len(), 2);
        assert!(due
            .iter()
            .any(|a| a.title.starts_with("Grok is nearly out")));
        assert!(due.iter().any(|a| a.title == "Claude is running low"));
    }

    #[test]
    fn clearing_rearms_the_current_window() {
        let mut alerts = Alerts::default();
        let r = reading("claude", "weekly", 4.0, Some(9_000));
        assert_eq!(alerts.due(1_000, &[r], EN).len(), 1);
        assert!(alerts.due(1_100, &[r], EN).is_empty());
        alerts.clear();
        assert_eq!(
            alerts.due(1_200, &[r], EN).len(),
            1,
            "switching alerts back on speaks once more"
        );
    }

    #[test]
    fn past_reset_windows_are_forgotten() {
        let mut alerts = Alerts::default();
        assert_eq!(
            alerts
                .due(1_000, &[reading("claude", "session", 4.0, Some(2_000))], EN)
                .len(),
            1
        );
        // The reset has come and gone; the same reading is a new window now.
        assert_eq!(
            alerts
                .due(3_000, &[reading("claude", "session", 4.0, Some(2_000))], EN)
                .len(),
            1
        );
    }

    #[test]
    fn body_wording() {
        assert_eq!(
            body(
                &reading("claude", "weekly_opus", 0.4, Some(7 * 86_400)),
                0,
                EN
            ),
            "0% left of the Opus weekly limit, resets in 7d 0h"
        );
        assert_eq!(
            body(&reading("claude", "period", 9.6, None), 0, EN),
            "10% left of the billing period"
        );
        assert_eq!(
            body(&reading("claude", "session", 4.4, Some(60)), 0, EN),
            "4% left of the 5-hour window, resets in 1m"
        );
        assert_eq!(
            body(&reading("claude", "session", 4.4, Some(10)), 50, EN),
            "4% left of the 5-hour window"
        );
    }

    // ------------------------------------------------------------ Chinese

    #[test]
    fn chinese_titles_name_the_service_and_the_tier() {
        assert_eq!(
            format_alert(
                &reading("claude", "session", 12.0, Some(9_000)),
                Tier::Warning,
                1_000,
                ZH
            ),
            Alert {
                title: "Claude 额度偏低".into(),
                body: "5 小时窗口剩余 12%，2 小时 13 分后重置".into(),
            }
        );
        assert_eq!(
            format_alert(
                &reading("codex", "weekly", 4.0, Some(90_000)),
                Tier::Critical,
                1_000,
                ZH
            ),
            Alert {
                title: "ChatGPT 额度即将用尽".into(),
                body: "本周窗口剩余 4%，1 天 0 小时后重置".into(),
            }
        );
    }

    #[test]
    fn chinese_body_names_every_window() {
        let names = [
            ("session", "5 小时"),
            ("weekly", "本周"),
            ("weekly_opus", "Opus"),
            ("weekly_sonnet", "Sonnet"),
            ("gemini_pro", "Pro"),
            ("gemini_flash", "Flash"),
            ("premium", "高级请求"),
            ("chat", "对话"),
            ("period", "周期"),
        ];
        for (window, name) in names {
            assert_eq!(
                body(&reading("claude", window, 9.0, None), 0, ZH),
                format!("{name}窗口剩余 9%"),
                "{window}"
            );
        }
        assert_eq!(
            body(&reading("grok", "something_new", 9.0, None), 0, ZH),
            "额度窗口剩余 9%"
        );
    }

    #[test]
    fn new_services_alert_in_both_languages() {
        // Gemini's Pro bucket and Copilot's premium requests, in English.
        assert_eq!(
            format_alert(
                &reading("gemini", "gemini_pro", 4.0, Some(9_000)),
                Tier::Critical,
                1_000,
                EN
            ),
            Alert {
                title: "Gemini is nearly out".into(),
                body: "4% left of the Pro limit, resets in 2h 13m".into(),
            }
        );
        assert_eq!(
            body(&reading("copilot", "premium", 12.0, None), 0, EN),
            "12% left of the Premium request limit"
        );
        assert_eq!(
            body(&reading("copilot", "chat", 12.0, Some(61)), 0, EN),
            "12% left of the Chat limit, resets in 1m"
        );
        // …and in Chinese.
        assert_eq!(
            format_alert(
                &reading("gemini", "gemini_flash", 12.0, Some(9_000)),
                Tier::Warning,
                1_000,
                ZH
            ),
            Alert {
                title: "Gemini 额度偏低".into(),
                body: "Flash窗口剩余 12%，2 小时 13 分后重置".into(),
            }
        );
        assert_eq!(
            body(&reading("copilot", "premium", 12.0, None), 0, ZH),
            "高级请求窗口剩余 12%"
        );
    }

    #[test]
    fn chinese_durations_and_reset_states() {
        // Days win over hours, hours over minutes, and never "0 分钟".
        assert_eq!(
            body(
                &reading("claude", "weekly_opus", 0.4, Some(7 * 86_400)),
                0,
                ZH
            ),
            "Opus窗口剩余 0%，7 天 0 小时后重置"
        );
        assert_eq!(
            body(&reading("claude", "weekly", 9.0, Some(61 * 60)), 0, ZH),
            "本周窗口剩余 9%，1 小时 1 分后重置"
        );
        assert_eq!(
            body(&reading("claude", "session", 4.4, Some(60)), 0, ZH),
            "5 小时窗口剩余 4%，1 分钟后重置"
        );
        // No reset time at all: no trailing clause.
        assert_eq!(
            body(&reading("claude", "period", 9.6, None), 0, ZH),
            "周期窗口剩余 10%"
        );
        // Already at the reset moment: says so instead of "resets in 0 分钟".
        assert_eq!(
            body(&reading("claude", "session", 4.4, Some(10)), 50, ZH),
            "5 小时窗口剩余 4%，正在重置"
        );
        assert_eq!(
            body(&reading("claude", "session", 4.4, Some(50)), 50, ZH),
            "5 小时窗口剩余 4%，正在重置"
        );
    }

    #[test]
    fn chinese_alert_fires_once_per_window() {
        let mut alerts = Alerts::default();
        let first = alerts.due(
            1_000,
            &[reading("claude", "session", 12.0, Some(9_000))],
            ZH,
        );
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].title, "Claude 额度偏低");
        assert!(alerts
            .due(1_600, &[reading("claude", "session", 9.0, Some(9_000))], ZH)
            .is_empty());
        let second = alerts.due(2_000, &[reading("claude", "session", 4.0, Some(9_000))], ZH);
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].title, "Claude 额度即将用尽");
    }

    #[test]
    fn chinese_is_only_for_zh_tags() {
        assert!(is_zh("zh"));
        assert!(is_zh("zh-CN"));
        assert!(is_zh("ZH"));
        assert!(!is_zh("en"));
        assert!(!is_zh("en-GB"));
        assert!(!is_zh(""));
        assert_eq!(
            format_alert(
                &reading("grok", "weekly", 4.0, None),
                Tier::Critical,
                0,
                "en-GB"
            )
            .title,
            "Grok is nearly out"
        );
    }
}
