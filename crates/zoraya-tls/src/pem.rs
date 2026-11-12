//! PEM certificates from a file (ADR-0228 item 3): `-----BEGIN
//! CERTIFICATE-----`, base64, `-----END CERTIFICATE-----`. Text outside the
//! blocks is ignored. A block that is not base64 is skipped.

use alloc::vec::Vec;

const BEGIN: &[u8] = b"-----BEGIN CERTIFICATE-----";
const END: &[u8] = b"-----END CERTIFICATE-----";

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn base64(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut pad) = (0u32, 0, 0);
    for &c in text {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                pad += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return None,
        };
        if pad > 0 {
            return None;
        }
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    (pad <= 2 && !out.is_empty()).then_some(out)
}

/// The DER of every certificate block in `text`.
pub fn pem_certificates(text: &[u8]) -> Vec<Vec<u8>> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(start) = find(rest, BEGIN) {
        let body = &rest[start + BEGIN.len()..];
        let Some(end) = find(body, END) else { break };
        if let Some(der) = base64(&body[..end]) {
            found.push(der);
        }
        rest = &body[end + END.len()..];
    }
    found
}
