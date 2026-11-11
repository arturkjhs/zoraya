use super::*;
use std::string::ToString;
use std::vec;
use std::vec::Vec;

fn run(limits: Limits, pieces: &[&[u8]]) -> (Result<(), Error>, Option<Head>, Vec<u8>) {
    let mut p = Parser::new(limits);
    let mut body = Vec::new();
    let mut result = Ok(());
    for piece in pieces {
        match p.feed(piece, &mut |_, b| body.extend_from_slice(b)) {
            Ok(_) => {}
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    if result.is_ok() {
        result = p.finish();
    }
    (result, p.head().copied(), body)
}

fn all(bytes: &[u8]) -> (Result<(), Error>, Option<Head>, Vec<u8>) {
    run(Limits::DEFAULT, &[bytes])
}

#[test]
fn urls_are_parsed_and_the_odd_ones_refused() {
    let u = parse_url("http://10.0.2.2:8080/a/b?c=d#frag").unwrap();
    assert_eq!((u.host, u.port, u.path), ("10.0.2.2", 8080, "/a/b?c=d"));
    let u = parse_url("HTTP://Example.org").unwrap();
    assert_eq!((u.host, u.port, u.path), ("Example.org", 80, "/"));
    assert_eq!(parse_url("https://a.b/").unwrap_err(), UrlError::Scheme);
    assert_eq!(parse_url("ftp://a.b/").unwrap_err(), UrlError::Scheme);
    assert_eq!(parse_url("a.b/").unwrap_err(), UrlError::Scheme);
    assert_eq!(parse_url("http:///x").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://a b/").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://a..b/").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://-a.b/").unwrap_err(), UrlError::Host);
    // A host that ends in a number is an address or nothing.
    assert_eq!(parse_url("http://1.2.3/").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://1.2.3.4.5/").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://256.1.1.1/").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://0x7f.1/").unwrap_err(), UrlError::Host);
    assert_eq!(parse_url("http://a.b:0/").unwrap_err(), UrlError::Port);
    assert_eq!(parse_url("http://a.b:65536/").unwrap_err(), UrlError::Port);
    assert_eq!(parse_url("http://a.b:/").unwrap_err(), UrlError::Port);
    assert_eq!(parse_url("http://a.b:80x/").unwrap_err(), UrlError::Port);
    assert_eq!(
        parse_url("http://u:p@a.b/").unwrap_err(),
        UrlError::Userinfo
    );
    assert_eq!(parse_url("http://a.b/x y").unwrap_err(), UrlError::Path);
    assert_eq!(
        parse_url("http://a.b/x\r\nHost: evil").unwrap_err(),
        UrlError::Path
    );
    assert_eq!(parse_url("http://a.b?x").unwrap_err(), UrlError::Path);
    let long = "http://a.b/".to_string() + &"x".repeat(MAX_URL);
    assert_eq!(parse_url(&long).unwrap_err(), UrlError::Path);
}

#[test]
fn the_request_is_the_one_line_of_http_one_one() {
    let u = parse_url("http://10.0.2.2:8080/p?q=1").unwrap();
    let r = request(&u);
    assert_eq!(
        r,
        b"GET /p?q=1 HTTP/1.1\r\nHost: 10.0.2.2:8080\r\nUser-Agent: zoraya-fetch\r\nAccept: */*\r\nConnection: close\r\n\r\n"
    );
    let r = request(&parse_url("http://test.zoraya/").unwrap());
    assert!(r.starts_with(b"GET / HTTP/1.1\r\nHost: test.zoraya\r\n"));
}

#[test]
fn a_length_delimited_body() {
    let (r, head, body) = all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX: y\r\n\r\nhello");
    assert_eq!(r, Ok(()));
    let head = head.unwrap();
    assert_eq!(
        (head.status, head.body, head.headers),
        (200, Body::Length(5), 2)
    );
    assert_eq!(body, b"hello");
}

#[test]
fn bytes_after_the_end_are_not_the_body() {
    let mut p = Parser::new(Limits::DEFAULT);
    let mut body = Vec::new();
    let input = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhiEXTRA";
    let n = p
        .feed(input, &mut |_, b| body.extend_from_slice(b))
        .unwrap();
    assert_eq!(body, b"hi");
    assert!(p.is_done());
    assert_eq!(n, input.len() - 5);
}

#[test]
fn a_chunked_body_with_extensions_and_trailers() {
    let (r, head, body) = all(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;ext=1\r\nhello\r\nA\r\n0123456789\r\n0\r\nX-Trailer: 1\r\n\r\n",
    );
    assert_eq!(r, Ok(()));
    assert_eq!(head.unwrap().body, Body::Chunked);
    assert_eq!(body, b"hello0123456789");
}

#[test]
fn a_body_until_close() {
    let (r, head, body) = all(b"HTTP/1.0 200 OK\r\nServer: x\r\n\r\nabc");
    assert_eq!(r, Ok(()));
    assert_eq!(head.unwrap().body, Body::UntilClose);
    assert_eq!(body, b"abc");
}

#[test]
fn no_body_statuses_and_zero_length() {
    for s in ["204 No Content", "304 Not Modified"] {
        let text = std::format!("HTTP/1.1 {s}\r\nContent-Length: 9\r\n\r\n");
        let (r, head, body) = all(text.as_bytes());
        assert_eq!(r, Ok(()));
        assert_eq!(head.unwrap().body, Body::None);
        assert!(body.is_empty());
    }
    let (r, _, body) = all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
    assert_eq!((r, body.len()), (Ok(()), 0));
}

#[test]
fn an_interim_response_is_skipped() {
    let (r, head, body) =
        all(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 200 OK\r\nContent-Length: 1\r\n\r\nx");
    assert_eq!(r, Ok(()));
    assert_eq!(head.unwrap().status, 200);
    assert_eq!(body, b"x");
    let many = b"HTTP/1.1 100 Continue\r\n\r\n".repeat(MAX_INTERIM + 1);
    assert_eq!(all(&many).0, Err(Error::TooManyInterim));
}

#[test]
fn a_redirect_is_seen_not_followed() {
    let (_, head, _) =
        all(b"HTTP/1.1 302 Found\r\nLocation: http://a.b/\r\nContent-Length: 0\r\n\r\n");
    let head = head.unwrap();
    assert_eq!(head.status, 302);
    assert!(head.redirect);
}

#[test]
fn a_cut_connection_is_truncated_unless_the_body_runs_to_the_close() {
    assert_eq!(
        all(b"HTTP/1.1 200 OK\r\nContent-Length: 10\r\n\r\nabc").0,
        Err(Error::Truncated)
    );
    assert_eq!(
        all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhel").0,
        Err(Error::Truncated)
    );
    assert_eq!(
        all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n").0,
        Err(Error::Truncated)
    );
    assert_eq!(
        all(b"HTTP/1.1 200 OK\r\nContent-Le").0,
        Err(Error::Truncated)
    );
    assert_eq!(all(b"").0, Err(Error::Truncated));
}

#[test]
fn framing_that_could_hide_a_second_message_is_refused() {
    let both = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nTransfer-Encoding: chunked\r\n\r\n";
    assert_eq!(all(both).0, Err(Error::BadFraming));
    let two = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Length: 4\r\n\r\nabc";
    assert_eq!(all(two).0, Err(Error::BadLength));
    // The same twice is harmless.
    let same = b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nContent-Length: 3\r\n\r\nabc";
    assert_eq!(all(same).0, Ok(()));
    for bad in ["+3", "-3", "3 3", "0x3", "", "99999999999999999999", "3,3"] {
        let text = std::format!("HTTP/1.1 200 OK\r\nContent-Length: {bad}\r\n\r\nabc");
        assert_eq!(all(text.as_bytes()).0, Err(Error::BadLength), "{bad:?}");
    }
    for coding in ["gzip", "gzip, chunked", "chunked, chunked", "identity"] {
        let text = std::format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: {coding}\r\n\r\n0\r\n\r\n");
        assert_eq!(all(text.as_bytes()).0, Err(Error::BadFraming), "{coding}");
    }
}

#[test]
fn malformed_status_lines_and_headers_are_refused() {
    for line in [
        "",
        "HTTP/1.1",
        "HTTP/2.0 200 OK",
        "HTTP/1.1 20 OK",
        "HTTP/1.1 2000 OK",
        "HTTP/1.1 abc OK",
        "HTTP/1.1 099 Low",
        "HTTP/1.1 600 High",
        "HTTP/1.1 200OK",
        "HTTP/1.1 200 O\x01K",
        "http/1.1 200 OK",
    ] {
        let text = std::format!("{line}\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(
            all(text.as_bytes()).0,
            Err(Error::BadStatusLine),
            "{line:?}"
        );
    }
    for header in [
        "NoColon",
        ": empty",
        "Bad Name: x",
        " Folded: x",
        "X: a\x01b",
        "X: a\rb",
        "X\t: y",
    ] {
        let text = std::format!("HTTP/1.1 200 OK\r\n{header}\r\nContent-Length: 0\r\n\r\n");
        assert_eq!(all(text.as_bytes()).0, Err(Error::BadHeader), "{header:?}");
    }
    // A bare LF is not a line end.
    let (r, _, _) = all(b"HTTP/1.1 200 OK\nContent-Length: 0\n\n");
    assert_eq!(r, Err(Error::Truncated));
}

#[test]
fn the_caps_hold() {
    let small = Limits {
        max_head: 64,
        max_headers: 3,
        max_body: 10,
    };
    let long = std::format!("HTTP/1.1 200 OK\r\nX: {}\r\n\r\n", "a".repeat(100));
    assert_eq!(run(small, &[long.as_bytes()]).0, Err(Error::HeadTooLarge));
    let many = b"HTTP/1.1 200 OK\r\nA: 1\r\nB: 2\r\nC: 3\r\nD: 4\r\n\r\n";
    assert_eq!(run(small, &[many]).0, Err(Error::TooManyHeaders));
    let big = b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\n\r\n01234567890";
    assert_eq!(run(small, &[big]).0, Err(Error::BodyTooLarge));
    let until = b"HTTP/1.0 200 OK\r\n\r\n0123456789A";
    assert_eq!(run(small, &[until]).0, Err(Error::BodyTooLarge));
    let chunks = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n6\r\n012345\r\n6\r\n012345\r\n0\r\n\r\n";
    assert_eq!(run(small, &[chunks]).0, Err(Error::BodyTooLarge));
    // A chunk size that does not fit in memory is a size, not an allocation.
    let huge = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffff\r\n";
    assert_eq!(
        run(Limits::DEFAULT, &[huge]).0,
        Err(Error::Truncated),
        "the size is accepted; the body cap stops the data"
    );
    let bytes = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nfffffffffffffffff\r\n";
    assert_eq!(all(bytes).0, Err(Error::BadChunk));
}

#[test]
fn bad_chunk_framing_is_refused() {
    let head = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
    for tail in [
        &b"x\r\n"[..],
        b"-1\r\n",
        b"\r\n",
        b"5\nhello\r\n",
        b"5\r\nhelloXX0\r\n\r\n",
        b"5\r\nhello\n0\r\n\r\n",
        b"0\r\nbad trailer\r\n\r\n",
        b"0\r\n X: folded\r\n\r\n",
    ] {
        let mut m = head.to_vec();
        m.extend_from_slice(tail);
        assert_eq!(all(&m).0, Err(Error::BadChunk), "{tail:?}");
    }
    // A line without an end is capped.
    let mut m = head.to_vec();
    m.extend_from_slice(&vec![b'1'; MAX_LINE + 10]);
    assert_eq!(all(&m).0, Err(Error::BadChunk));
    // Endless trailers are capped by the head's bound.
    let mut m = head.to_vec();
    m.extend_from_slice(b"0\r\n");
    for _ in 0..2000 {
        m.extend_from_slice(b"X-Trailer: aaaaaaaaaaaaaaaaaaaaaaaa\r\n");
    }
    assert_eq!(all(&m).0, Err(Error::BadChunk));
}

#[test]
fn an_error_is_final() {
    let mut p = Parser::new(Limits::DEFAULT);
    assert!(p.feed(b"garbage\r\n\r\n", &mut |_, _| {}).is_err());
    assert!(p.feed(b"HTTP/1.1 200 OK\r\n\r\n", &mut |_, _| {}).is_err());
    assert!(p.finish().is_err());
}

fn sample_messages() -> Vec<Vec<u8>> {
    vec![
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nX: y\r\n\r\nhello".to_vec(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5;e\r\nhello\r\n3\r\nabc\r\n0\r\nT: 1\r\n\r\n".to_vec(),
        b"HTTP/1.0 200 OK\r\n\r\nuntil close".to_vec(),
        b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 204 No\r\n\r\n".to_vec(),
    ]
}

#[test]
fn any_split_of_the_input_gives_the_same_result() {
    for m in sample_messages() {
        let (r0, h0, b0) = all(&m);
        for cut in 0..=m.len() {
            let (r, h, b) = run(Limits::DEFAULT, &[&m[..cut], &m[cut..]]);
            assert_eq!((r, h, &b), (r0, h0, &b0), "cut at {cut}");
        }
        // Byte by byte.
        let pieces: Vec<&[u8]> = m.chunks(1).collect();
        let (r, h, b) = run(Limits::DEFAULT, &pieces);
        assert_eq!((r, h, &b), (r0, h0, &b0));
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn hostile_bytes_never_panic_and_never_exceed_the_caps() {
    let limits = Limits {
        max_head: 512,
        max_headers: 8,
        max_body: 300,
    };
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let samples = sample_messages();
    for round in 0..60_000 {
        let mut m = if round % 4 == 0 {
            let n = (rng.next() % 200) as usize;
            (0..n).map(|_| rng.next() as u8).collect()
        } else {
            samples[(rng.next() % samples.len() as u64) as usize].clone()
        };
        for _ in 0..rng.next() % 5 {
            if m.is_empty() {
                break;
            }
            let at = (rng.next() as usize) % m.len();
            match rng.next() % 3 {
                0 => m[at] = rng.next() as u8,
                1 => {
                    m.remove(at);
                }
                _ => m.insert(at, b"\r\n:; 0fF"[(rng.next() % 8) as usize]),
            }
        }
        let cut = (rng.next() as usize) % (m.len() + 1);
        let mut p = Parser::new(limits);
        let mut got = 0u64;
        for piece in [&m[..cut], &m[cut..]] {
            if p.feed(piece, &mut |_, b| got += b.len() as u64).is_err() {
                break;
            }
        }
        assert!(got <= limits.max_body);
        assert_eq!(got, p.body_bytes());
    }
}
