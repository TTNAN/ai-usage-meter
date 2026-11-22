//! gRPC-web framing, trailer status and protobuf field scanning, as used by
//! grok.com and the Grok Build CLI.

pub(super) fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[derive(Default)]
pub(super) struct ProtoScan {
    // fields are set by scan_proto and read by grok's credit parser
    pub(super) fixed32: Vec<(Vec<u64>, f32)>,
    pub(super) varints: Vec<(Vec<u64>, u64)>,
}

pub(super) fn read_varint(b: &[u8], i: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in 0..10 {
        let byte = *b.get(*i)?;
        *i += 1;
        v |= ((byte & 0x7f) as u64) << (shift * 7);
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Walk a protobuf message without a schema. Returns false when `b` is not
/// a well-formed message (so the caller treats it as a plain byte string).
pub(super) fn scan_proto(b: &[u8], path: &mut Vec<u64>, out: &mut ProtoScan, depth: u32) -> bool {
    if depth > 8 {
        return false;
    }
    let mut local = ProtoScan::default();
    let mut i = 0;
    while i < b.len() {
        let Some(key) = read_varint(b, &mut i) else {
            return false;
        };
        let (field, wire) = (key >> 3, key & 7);
        if field == 0 {
            return false;
        }
        path.push(field);
        match wire {
            0 => match read_varint(b, &mut i) {
                Some(v) => local.varints.push((path.clone(), v)),
                None => {
                    path.pop();
                    return false;
                }
            },
            1 => {
                if i + 8 > b.len() {
                    path.pop();
                    return false;
                }
                i += 8;
            }
            2 => {
                let Some(len) = read_varint(b, &mut i) else {
                    path.pop();
                    return false;
                };
                let len = len as usize;
                if i + len > b.len() {
                    path.pop();
                    return false;
                }
                let mut sub = ProtoScan::default();
                if len > 0 && scan_proto(&b[i..i + len], path, &mut sub, depth + 1) {
                    local.fixed32.extend(sub.fixed32);
                    local.varints.extend(sub.varints);
                }
                i += len;
            }
            5 => {
                if i + 4 > b.len() {
                    path.pop();
                    return false;
                }
                let v = f32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
                local.fixed32.push((path.clone(), v));
                i += 4;
            }
            _ => {
                path.pop();
                return false;
            }
        }
        path.pop();
    }
    out.fixed32.extend(local.fixed32);
    out.varints.extend(local.varints);
    true
}

/// The gRPC status carried by a response's trailer frame, when it has one.
/// A webview cannot read trailers as headers, so `grpc-status` reaches this app
/// only inside the body: a 0x80-flagged frame holding `grpc-status: 16` text.
pub(super) fn grpc_trailer_status(data: &[u8]) -> Option<i64> {
    let mut i = 0;
    while i + 5 <= data.len() {
        let flags = data[i];
        let len = u32::from_be_bytes([data[i + 1], data[i + 2], data[i + 3], data[i + 4]]) as usize;
        let (start, end) = (i + 5, i + 5 + len);
        if end > data.len() {
            return None;
        }
        if flags == 0x80 {
            let text = String::from_utf8_lossy(&data[start..end]);
            for line in text.lines() {
                if let Some(v) = line.trim().strip_prefix("grpc-status:") {
                    // A line this app cannot read as a number (a proxy's error
                    // page, a half-cut frame) is not the answer: keep looking,
                    // since a later line or trailer frame may carry the real
                    // status. Returning on the first bad line used to turn a
                    // signed-out account into an unreadable API.
                    if let Ok(code) = v.trim().parse() {
                        return Some(code);
                    }
                }
            }
        }
        i = end;
    }
    None
}

/// What a `GetGrokCreditsConfig` body turned out to be. The three cases are
/// deliberately not folded into an `Option`: "nothing came back" and "this app
/// cannot read what came back" are different news for the user.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::test_bytes::*;

    #[test]
    fn grpc_trailer_status_skips_a_line_it_cannot_read() {
        // A first line that is not a number must not swallow the real status
        // that follows it.
        let trailer = b"grpc-status: abc\r\ngrpc-status: 16\r\n";
        assert_eq!(grpc_trailer_status(&frame(0x80, trailer)), Some(16));

        // Nor may it hide a valid status in a later trailer frame.
        let mut both = frame(0x80, b"grpc-status: \xff\r\n");
        both.extend(frame(0x80, b"grpc-status: 0\r\n"));
        assert_eq!(grpc_trailer_status(&both), Some(0));

        // With nothing readable anywhere there is still no status.
        assert_eq!(
            grpc_trailer_status(&frame(0x80, b"grpc-status: abc\r\n")),
            None
        );
        assert_eq!(
            grpc_trailer_status(&frame(0x80, b"grpc-message: hi\r\n")),
            None
        );
    }
}
