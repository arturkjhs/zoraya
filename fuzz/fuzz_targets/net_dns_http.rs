//! The two parsers of bytes that a server controls (N3): the resolver's
//! decoder of a DNS answer (`zoraya-dns`) and `fetch`'s decoder of an HTTP
//! response (`zoraya-http`). The first byte picks what the rest is:
//!
//! 0. a DNS message as the answer to a query for `test.zoraya`;
//! 1. the same with our header and question put in front, so that the
//!    records are what is hostile;
//! 2. an HTTP response, in two pieces split where the second byte says;
//! 3. a URL for `parse_url`, and the request built from it.
//!
//! Nothing may panic, a decoder may not deliver more body than its cap, and
//! an accepted DNS answer holds at most `MAX_ADDRESSES` addresses.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zoraya_dns::{MAX_ADDRESSES, MAX_QUERY, Outcome, encode_query, parse_response};
use zoraya_http::{Limits, Parser, parse_url, request};

fuzz_target!(|data: &[u8]| {
    let Some((&kind, rest)) = data.split_first() else {
        return;
    };
    match kind % 4 {
        0 => check_dns(rest),
        1 => {
            let mut q = [0u8; MAX_QUERY];
            let n = encode_query(7, "test.zoraya", &mut q).expect("a good name");
            let mut m = q[..n].to_vec();
            m[2] = 0x81;
            m[3] = 0x80;
            m[7] = 3;
            m.extend_from_slice(rest);
            check_dns(&m);
        }
        2 => {
            let limits = Limits {
                max_head: 1024,
                max_headers: 16,
                max_body: 4096,
            };
            let Some((&cut, bytes)) = rest.split_first() else {
                return;
            };
            let cut = usize::from(cut) % (bytes.len() + 1);
            let mut p = Parser::new(limits);
            let mut got = 0u64;
            for piece in [&bytes[..cut], &bytes[cut..]] {
                if p.feed(piece, &mut |b| got += b.len() as u64).is_err() {
                    break;
                }
            }
            assert!(got <= limits.max_body);
            let _ = p.finish();
        }
        _ => {
            if let Ok(text) = core::str::from_utf8(rest)
                && let Ok(url) = parse_url(text)
            {
                let r = request(&url);
                // One request, no extra header lines from the URL.
                assert_eq!(r.windows(4).filter(|w| *w == b"\r\n\r\n").count(), 1);
            }
        }
    }
});

fn check_dns(m: &[u8]) {
    for id in [7u16, 0] {
        if let Ok(Outcome::Addresses(a)) = parse_response(m, id, "test.zoraya") {
            assert!(a.count >= 1 && a.count <= MAX_ADDRESSES);
        }
    }
}
