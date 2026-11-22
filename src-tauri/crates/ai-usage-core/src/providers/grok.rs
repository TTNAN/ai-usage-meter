//! Grok: the Grok Build CLI token, the billing request, grok.com reports
//! and their parse.

use super::grpc::{base64_decode, grpc_trailer_status, scan_proto, ProtoScan};
use super::*;

// ------------------------------------------------------------------- Grok

fn grok_token() -> Option<String> {
    if let Some(t) = env_nonempty("GROK_ACCESS_TOKEN") {
        return Some(t);
    }
    let dir = env_nonempty("GROK_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".grok")))?;
    let auth = read_json_file(&dir.join("auth.json"))?;
    find_string(
        &auth,
        &["access_token", "accessToken", "token", "api_key", "apiKey"],
    )
    .map(str::to_string)
}

/// What the row says when Grok answered, but in a shape this app cannot read.
/// The wording matters: it tells the user the app is behind, not the login.
const GROK_SHAPE_NOTE: &str =
    "Grok answered in a format this app doesn't recognise — its API has probably changed.";

pub fn fetch_grok() -> ServiceStatus {
    let base = ServiceStatus::new("grok", "Grok", State::NotConnected);
    let Some(token) = grok_token() else {
        return base.with_note("Sign in to Grok Build once (run `grok login`).");
    };
    let v = match http_get_json(
        "https://cli-chat-proxy.grok.com/v1/billing?format=credits",
        &[
            ("Authorization", format!("Bearer {token}")),
            ("x-xai-token-auth", "xai-grok-cli".to_string()),
        ],
    ) {
        Ok(v) => v,
        Err(HttpError::Auth(_)) => {
            return ServiceStatus {
                state: State::Expired,
                ..base
            }
            .with_note("Login expired — run `grok login` again.")
        }
        Err(e) => {
            return ServiceStatus {
                state: State::Unavailable,
                ..base
            }
            .with_note(format!("Couldn't reach Grok ({}).", e.describe()))
        }
    };
    let windows = parse_grok(&v);
    if windows.is_empty() {
        // A 200 with no `creditUsagePercent` in it: the endpoint answered, but
        // not in a shape this app knows. That is not the same as "not logged
        // in", so it must not read as one.
        return ServiceStatus {
            state: State::Changed,
            ..base
        }
        .with_note(GROK_SHAPE_NOTE);
    }
    ServiceStatus {
        state: State::Connected,
        windows,
        ..base
    }
}

fn parse_grok(v: &serde_json::Value) -> Vec<UsageWindow> {
    let used = as_f64(v.get("config").and_then(|c| c.get("creditUsagePercent")))
        .or_else(|| as_f64(v.get("creditUsagePercent")));
    let Some(used) = used else { return Vec::new() };
    let period = v.get("currentPeriod");
    let start = as_timestamp(period.and_then(|p| p.get("start")));
    let end = as_timestamp(period.and_then(|p| p.get("end")));
    let secs = match (start, end) {
        (Some(s), Some(e)) if e > s => Some(e - s),
        _ => None,
    };
    vec![UsageWindow {
        kind: kind_for_seconds(secs, "weekly"),
        used_percent: used.clamp(0.0, 100.0),
        resets_at: end,
        window_seconds: secs,
    }]
}

// ----------------------------------------------------------------- Gemini
//
// Gemini CLI and Google Antigravity both sign in with a Google account and
// store the OAuth token at `~/.gemini/oauth_creds.json`. Their quota lives in
// Google's Cloud Code Assist backend, per model rather than per time window:
// one rolling allowance, each bucket with its own remaining fraction and
// reset. `loadCodeAssist` names the plan, `retrieveUserQuota` the buckets.

//   connected".
const GROK_SIGNED_OUT_NOTE: &str = "grok.com sign-in expired.";
const GROK_WEB_SHAPE_NOTE: &str =
    "grok.com answered in a format this app doesn't recognise — its API has probably changed.";
const GROK_WEB_EMPTY_NOTE: &str = "grok.com answered with an empty body.";

/// gRPC statuses that mean "the caller is not allowed": `UNAUTHENTICATED` and
/// `PERMISSION_DENIED`. Everything else non-zero is the server's problem, not
/// the account's.
const GRPC_NOT_ALLOWED: [i64; 2] = [16, 7];

/// Report from the grok.com webview: `{b64: <GetGrokCreditsConfig gRPC-web
/// response>, grpc: "<grpc-status>"}`, `{auth: false}` or `{error}`.
pub fn grok_from_web(report: &serde_json::Value, now: i64) -> ServiceStatus {
    let base = ServiceStatus {
        source: "web",
        ..ServiceStatus::new("grok", "Grok", State::NotConnected)
    };
    if web_auth_failed(report) {
        return ServiceStatus {
            state: State::Expired,
            ..base
        }
        .with_note(GROK_SIGNED_OUT_NOTE);
    }
    let header_status = report
        .get("grpc")
        .and_then(|g| g.as_str())
        .and_then(|g| g.trim().parse::<i64>().ok())
        .unwrap_or(0);
    if GRPC_NOT_ALLOWED.contains(&header_status) {
        return ServiceStatus {
            state: State::Expired,
            ..base
        }
        .with_note(GROK_SIGNED_OUT_NOTE);
    }
    let bytes = report
        .get("b64")
        .and_then(|b| b.as_str())
        .and_then(base64_decode);
    let Some(bytes) = bytes else {
        let why = report
            .get("error")
            .and_then(|e| e.as_str())
            .unwrap_or("no data");
        return ServiceStatus {
            state: State::Unavailable,
            ..base
        }
        .with_note(format!("grok.com: {why}"));
    };
    // A webview cannot read gRPC trailers as headers, so a refused call usually
    // says so only inside the body: read that before judging the message.
    if let Some(status) = grpc_trailer_status(&bytes) {
        if GRPC_NOT_ALLOWED.contains(&status) {
            return ServiceStatus {
                state: State::Expired,
                ..base
            }
            .with_note(GROK_SIGNED_OUT_NOTE);
        }
        if status != 0 {
            return ServiceStatus {
                state: State::Unavailable,
                ..base
            }
            .with_note(format!("grok.com answered with gRPC status {status}."));
        }
    }
    match read_grok_credits(&bytes, now) {
        GrokCredits::Quota(w) => ServiceStatus {
            state: State::Connected,
            plan: Some("SuperGrok".into()),
            windows: vec![w],
            ..base
        },
        GrokCredits::Empty => ServiceStatus {
            state: State::Changed,
            ..base
        }
        .with_note(GROK_WEB_EMPTY_NOTE),
        GrokCredits::Unreadable => ServiceStatus {
            state: State::Changed,
            ..base
        }
        .with_note(GROK_WEB_SHAPE_NOTE),
    }
}

#[derive(Debug)]
enum GrokCredits {
    /// The numbers, read from a body whose shape this app still knows.
    Quota(UsageWindow),
    /// A well-formed gRPC-web answer carrying no message at all.
    Empty,
    /// Bytes this app cannot read: the framing or the message shape has moved.
    Unreadable,
}

/// Reads a `GetGrokCreditsConfig` response: gRPC-web frames, or a bare
/// protobuf message (older captures). The used percent is the float at field
/// path .../1 closest to the root (normally `[1, 1]`), the reset the timestamp
/// at `[1, 5, 1]`.
fn read_grok_credits(data: &[u8], now: i64) -> GrokCredits {
    let mut payloads: Vec<&[u8]> = Vec::new();
    let mut i = 0;
    let mut framed = true;
    while i < data.len() {
        if i + 5 > data.len() {
            framed = false;
            break;
        }
        let flags = data[i];
        if flags != 0 && flags != 0x80 {
            framed = false;
            break;
        }
        let len = u32::from_be_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]) as usize;
        let (start, end) = (i + 5, i + 5 + len);
        if end > data.len() {
            framed = false;
            break;
        }
        if flags == 0 {
            payloads.push(&data[start..end]);
        }
        i = end;
    }
    if framed && payloads.is_empty() {
        // A proper envelope with no data frame in it: the call succeeded and
        // carried nothing. Not a schema change.
        return GrokCredits::Empty;
    }
    if !framed && payloads.is_empty() {
        // Not a gRPC-web body at all: try the whole thing as one message. A
        // body that stops mid-frame after good frames is different — those
        // frames were read and still carry the numbers, so they are kept and
        // only the truncated tail is dropped.
        payloads = vec![data];
    }
    if payloads.iter().all(|p| p.is_empty()) {
        return GrokCredits::Empty;
    }
    let mut scan = ProtoScan::default();
    for p in payloads {
        if !scan_proto(p, &mut Vec::new(), &mut scan, 0) {
            // Bytes that are not a protobuf message either: a JSON error body,
            // an HTML page, whatever the endpoint says now.
            return GrokCredits::Unreadable;
        }
    }
    let percent = scan
        .fixed32
        .iter()
        .filter(|(p, v)| p.last() == Some(&1) && v.is_finite() && (0.0..=100.0).contains(v))
        .min_by_key(|(p, _)| p.len())
        .map(|(_, v)| *v as f64);
    let future: Vec<(&Vec<u64>, i64)> = scan
        .varints
        .iter()
        .filter(|(_, v)| (1_700_000_000..=2_100_000_000).contains(v))
        .map(|(p, v)| (p, *v as i64))
        .filter(|(_, t)| *t > now)
        .collect();
    let reset = future
        .iter()
        .filter(|(p, _)| p.as_slice() == [1, 5, 1])
        .map(|(_, t)| *t)
        .min()
        .or_else(|| future.iter().map(|(_, t)| *t).min());
    let used = match (percent, reset) {
        (Some(p), _) => p,
        // A current period but no usage scalar: nothing used yet (proto3 omits 0).
        (None, Some(_)) if scan.fixed32.is_empty() => 0.0,
        // A message, but with neither number where they used to be: the shape
        // moved under us.
        _ => return GrokCredits::Unreadable,
    };
    let secs = reset.map(|r| r - now);
    let kind = match secs {
        Some(s) if s <= 8 * 86_400 => "weekly",
        _ => "period",
    };
    GrokCredits::Quota(UsageWindow {
        kind,
        used_percent: used.clamp(0.0, 100.0),
        resets_at: reset,
        window_seconds: if kind == "weekly" {
            Some(7 * 86_400)
        } else {
            None
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::test_bytes::*;
    use serde_json::json;

    #[test]
    fn grok_web_reads_a_real_credits_response() {
        let body = grok_credits_response(37.5, Some(1_795_672_800));
        let st = grok_from_web(
            &json!({"b64": base64(&body), "grpc": "0", "status": 200}),
            1_795_200_000,
        );
        assert_eq!(st.state, State::Connected);
        assert_eq!(st.source, "web");
        assert_eq!(st.plan.as_deref(), Some("SuperGrok"));
        assert_eq!(st.windows.len(), 1);
        assert_eq!(st.windows[0].used_percent, 37.5);
        assert_eq!(st.windows[0].resets_at, Some(1_795_672_800));
        assert_eq!(st.windows[0].kind, "weekly");
        assert_eq!(st.note, None);
    }

    #[test]
    fn grok_web_reads_a_response_with_nothing_used_yet() {
        // proto3 leaves a 0.0 float out of the message; the period is still
        // there, so this is "nothing used", not "cannot read".
        let mut config = Vec::new();
        config.extend(field_len(5, &field_varint(1, 1_795_672_800)));
        let body = frame(0, &field_len(1, &config));
        let st = grok_from_web(&json!({"b64": base64(&body)}), 1_795_200_000);
        assert_eq!(st.state, State::Connected);
        assert_eq!(st.windows[0].used_percent, 0.0);
    }

    #[test]
    fn grok_web_tells_a_signed_out_session_from_a_changed_api() {
        // The page itself saw the 401/403: signed out.
        let st = grok_from_web(&json!({"auth": false}), 0);
        assert_eq!(st.state, State::Expired);

        // The status only inside the trailer frame — which is how a webview
        // learns it, since trailers are not readable as headers.
        let mut refused = Vec::new();
        refused.extend(frame(0x80, b"grpc-status:16\r\n"));
        let st = grok_from_web(&json!({"b64": base64(&refused), "grpc": "0"}), 0);
        assert_eq!(st.state, State::Expired);
        assert!(st.note.unwrap().contains("sign-in"));

        // A server-side failure is not the account's either: retried shortly.
        let mut broken = Vec::new();
        broken.extend(frame(0x80, b"grpc-status:13\r\n"));
        let st = grok_from_web(&json!({"b64": base64(&broken), "grpc": "0"}), 0);
        assert_eq!(st.state, State::Unavailable);

        // JSON where protobuf used to be: the API moved, and that must never
        // read as "not connected".
        let body = br#"{"error":{"code":404,"message":"Not Found"}}"#;
        let st = grok_from_web(&json!({"b64": base64(body), "grpc": "0"}), 0);
        assert_eq!(st.state, State::Changed);
        assert!(st.note.unwrap().contains("API has probably changed"));

        // A 200 carrying nothing at all.
        let st = grok_from_web(&json!({"b64": "", "grpc": "0"}), 0);
        assert_eq!(st.state, State::Changed);

        // The request never got an answer.
        let st = grok_from_web(&json!({"error": "HTTP 502"}), 0);
        assert_eq!(st.state, State::Unavailable);
    }

    #[test]
    fn grok_web_calls_moved_fields_a_changed_api() {
        // A message that parses, but with the numbers in other fields: the
        // shape changed under us.
        let config = field_float(3, 42.0);
        let body = frame(0, &field_len(1, &config));
        let st = grok_from_web(&json!({"b64": base64(&body)}), 1_795_200_000);
        assert_eq!(st.state, State::Changed);

        // Bytes that are not protobuf at all.
        let st = grok_from_web(&json!({"b64": base64(b"<html>nope</html>")}), 0);
        assert_eq!(st.state, State::Changed);
    }

    #[test]
    fn grok_credits_keeps_good_frames_before_a_truncated_one() {
        // A body cut off mid-frame after a complete data frame: what was read
        // still holds the numbers, so the read must not fall back to scanning
        // the raw bytes as one message.
        let mut body = grok_credits_response(37.5, Some(1_795_672_800));
        body.extend([0x00, 0x00, 0x00, 0x00, 0x28]);
        match read_grok_credits(&body, 1_795_200_000) {
            GrokCredits::Quota(w) => {
                assert_eq!(w.used_percent, 37.5);
                assert_eq!(w.resets_at, Some(1_795_672_800));
            }
            other => panic!("a truncated tail must not cost the data frame: {other:?}"),
        }

        // The same body with nothing complete in it is still unreadable, not
        // silently empty: the first bytes are a cut frame header.
        assert!(matches!(
            read_grok_credits(&[0x00, 0x00, 0x00, 0x00, 0x28], 0),
            GrokCredits::Unreadable
        ));
    }

    #[test]
    fn grok_credits_reads_a_bare_message_and_an_empty_envelope() {
        // Older captures are a bare message with no gRPC-web frames.
        let mut config = field_float(1, 12.5);
        config.extend(field_len(5, &field_varint(1, 1_795_672_800)));
        let bare = field_len(1, &config);
        match read_grok_credits(&bare, 1_795_200_000) {
            GrokCredits::Quota(w) => assert_eq!(w.used_percent, 12.5),
            other => panic!("a bare message should read: {other:?}"),
        }
        // A framed answer with no data frame in it.
        match read_grok_credits(&frame(0x80, b"grpc-status:0\r\n"), 0) {
            GrokCredits::Empty => {}
            other => panic!("an empty envelope is empty, not unreadable: {other:?}"),
        }
    }

    #[test]
    fn grok_payload() {
        let v = json!({
            "config": {"creditUsagePercent": 57.5},
            "currentPeriod": {"start": "2026-02-01T00:00:00Z", "end": "2026-02-08T00:00:00Z"}
        });
        let w = parse_grok(&v);
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].kind, "weekly");
        assert_eq!(w[0].used_percent, 57.5);
        assert!(parse_grok(&json!({})).is_empty());
    }
}
