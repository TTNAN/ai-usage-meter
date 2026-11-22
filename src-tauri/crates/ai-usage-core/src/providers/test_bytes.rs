//! Byte fixtures shared by the Grok and gRPC tests: a protobuf encoder, a
//! gRPC-web framer and base64, the way grok.com's own web bundle builds them.

#![allow(dead_code)]

// ------------------------------------------------ Grok credits fixtures
//
// Bytes shaped the way grok.com's own web bundle sends them: a
// `GetGrokCreditsConfigResponse` in one data frame, then a trailer frame.
// Written out here rather than copied from a live session: they are public
// traffic shapes, and a test fixture should not carry anyone's account.
/// A protobuf varint.
pub(crate) fn varint(mut v: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return out;
        }
        out.push(b | 0x80);
    }
}

/// A length-delimited field (wire type 2).
pub(crate) fn field_len(field: u32, body: &[u8]) -> Vec<u8> {
    let mut out = varint(((field as u64) << 3) | 2);
    out.extend(varint(body.len() as u64));
    out.extend_from_slice(body);
    out
}

/// A varint field (wire type 0) — a timestamp.
pub(crate) fn field_varint(field: u32, v: u64) -> Vec<u8> {
    let mut out = varint((field as u64) << 3);
    out.extend(varint(v));
    out
}

/// A float field (wire type 5): where the usage percentage lives today.
pub(crate) fn field_float(field: u32, v: f32) -> Vec<u8> {
    let mut out = varint(((field as u64) << 3) | 5);
    out.extend_from_slice(&v.to_le_bytes());
    out
}

/// One gRPC-web frame: a flag byte (0x00 data, 0x80 trailer) and the
/// message length as a big-endian u32.
pub(crate) fn frame(flags: u8, body: &[u8]) -> Vec<u8> {
    let mut out = vec![flags];
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(body);
    out
}

/// `GetGrokCreditsConfigResponse { config: { usage_percent, current_period
/// { reset_at } } }`, framed like a real answer.
pub(crate) fn grok_credits_response(percent: f32, reset_at: Option<u64>) -> Vec<u8> {
    let mut config = field_float(1, percent);
    if let Some(t) = reset_at {
        config.extend(field_len(5, &field_varint(1, t)));
    }
    let mut body = frame(0, &field_len(1, &config));
    body.extend(frame(0x80, b"grpc-status:0\r\n"));
    body
}

/// Standard base64, the way the page encodes the bytes it sends back.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16
            | (*c.get(1).unwrap_or(&0) as u32) << 8
            | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            if k <= c.len() {
                s.push(T[(n >> (18 - 6 * k) & 63) as usize] as char)
            } else {
                s.push('=')
            }
        }
    }
    s
}
