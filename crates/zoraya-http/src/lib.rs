//! A small HTTP/1.1 client for `fetch` (N3): the URL, the request, and an
//! incremental decoder of the response.
//!
//! The decoder reads what a server sends, which is hostile input. It is
//! total and bounded: bytes come in pieces of any size (a split between any
//! two bytes gives the same result), the head is at most
//! [`Limits::max_head`] bytes with at most [`Limits::max_headers`] fields, a
//! chunk-size line at most [`MAX_LINE`] bytes, trailers are skipped within the
//! same bounds, and the body at most [`Limits::max_body`] bytes whatever the
//! framing says. Framing follows RFC 9112 section 6.3, strictly: a response
//! with both `Content-Length` and `Transfer-Encoding`, two different
//! lengths, a length that is not plain digits, or any transfer coding but
//! `chunked` is refused (request smuggling shapes; a client that refuses them
//! loses nothing). Lines end in CRLF; a bare LF is an error. Obsolete line
//! folding is an error. Nothing here sends or opens anything.
//!
//! The request is `GET` only, `Connection: close`, no cookies, no
//! redirects (the caller sees the `Location` header's presence through
//! [`Head::redirect`] and decides).

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

#[cfg(test)]
extern crate std;

/// The longest chunk-size line or trailer line.
pub const MAX_LINE: usize = 256;
/// Informational responses (1xx) skipped before the real one.
pub const MAX_INTERIM: usize = 4;

/// Why [`parse_url`] refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlError {
    /// Not `http://` or `https://`.
    Scheme,
    /// No host, or a host that is neither a valid name nor a dotted address.
    Host,
    /// A port that is not 1 to 65535.
    Port,
    /// Credentials in the URL, which a client must not send and a person
    /// should not type.
    Userinfo,
    /// A byte in the path that is not printable ASCII.
    Path,
}

/// A parsed `http://` or `https://` URL; it borrows from the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Url<'a> {
    /// `https://`: the connection is TLS (ADR-0228), port 443 unless given.
    pub tls: bool,
    pub host: &'a str,
    pub port: u16,
    /// Starts with `/`; includes the query; no fragment.
    pub path: &'a str,
}

/// The longest URL accepted.
pub const MAX_URL: usize = 2048;

/// Whether `host` is a dotted IPv4 address (four decimal numbers 0..=255).
pub fn parse_ipv4(host: &str) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = host.split('.');
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || !p.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *slot = p.parse::<u16>().ok().and_then(|v| u8::try_from(v).ok())?;
    }
    parts.next().is_none().then_some(out)
}

fn valid_host(host: &str) -> bool {
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    // A name never ends in a number (a spelling of an address that is not
    // a dotted quad is refused, not resolved).
    let last_numeric = host
        .rsplit('.')
        .next()
        .is_some_and(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()));
    if last_numeric {
        return parse_ipv4(host).is_some();
    }
    host.split('.').all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && !l.starts_with('-')
            && !l.ends_with('-')
            && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// `http://` or `https://` `HOST[:PORT][/PATH[?QUERY]][#FRAGMENT]`.
pub fn parse_url(text: &str) -> Result<Url<'_>, UrlError> {
    if text.len() > MAX_URL {
        return Err(UrlError::Path);
    }
    let (tls, rest) = if let Some(rest) = text
        .get(..7)
        .filter(|s| s.eq_ignore_ascii_case("http://"))
        .map(|_| &text[7..])
    {
        (false, rest)
    } else {
        text.get(..8)
            .filter(|s| s.eq_ignore_ascii_case("https://"))
            .map(|_| (true, &text[8..]))
            .ok_or(UrlError::Scheme)?
    };
    let rest = rest.split('#').next().unwrap_or("");
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => match rest.find('?') {
            // `http://host?x` has the path `/?x`; keep the borrow by
            // refusing it: a person writes the slash.
            Some(_) => return Err(UrlError::Path),
            None => (rest, "/"),
        },
    };
    if authority.contains('@') {
        return Err(UrlError::Userinfo);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            if p.is_empty() || p.len() > 5 || !p.bytes().all(|b| b.is_ascii_digit()) {
                return Err(UrlError::Port);
            }
            match p.parse::<u32>() {
                Ok(n) if (1..=65535).contains(&n) => (h, n as u16),
                _ => return Err(UrlError::Port),
            }
        }
        None => (authority, if tls { 443 } else { 80 }),
    };
    if !valid_host(host) {
        return Err(UrlError::Host);
    }
    if !path.bytes().all(|b| (0x21..0x7f).contains(&b)) {
        return Err(UrlError::Path);
    }
    Ok(Url {
        tls,
        host,
        port,
        path,
    })
}

/// The request for `url`: `GET`, HTTP/1.1, `Host`, `Connection: close`.
pub fn request(url: &Url<'_>) -> Vec<u8> {
    let mut s = String::new();
    s.push_str("GET ");
    s.push_str(url.path);
    s.push_str(" HTTP/1.1\r\nHost: ");
    s.push_str(url.host);
    if url.port != if url.tls { 443 } else { 80 } {
        s.push(':');
        push_number(&mut s, u64::from(url.port));
    }
    s.push_str("\r\nUser-Agent: zoraya-fetch\r\nAccept: */*\r\nConnection: close\r\n\r\n");
    s.into_bytes()
}

fn push_number(s: &mut String, mut n: u64) {
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    for d in &digits[i..] {
        s.push(char::from(*d));
    }
}

/// How the body ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Body {
    /// No body (1xx handled, 204, 304).
    None,
    Length(u64),
    Chunked,
    /// Until the server closes the connection.
    UntilClose,
}

/// What the head said that `fetch` needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub status: u16,
    pub body: Body,
    /// A `Location` header was present (3xx).
    pub redirect: bool,
    pub headers: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_head: usize,
    pub max_headers: usize,
    pub max_body: u64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        max_head: 8192,
        max_headers: 64,
        max_body: 64 << 20,
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The head is longer than [`Limits::max_head`].
    HeadTooLarge,
    TooManyHeaders,
    BadStatusLine,
    BadHeader,
    /// A `Content-Length` that is not digits, or two that differ.
    BadLength,
    /// Both framings, or a transfer coding that is not `chunked`.
    BadFraming,
    BadChunk,
    BodyTooLarge,
    /// The connection ended before the body did.
    Truncated,
    /// Too many 1xx responses.
    TooManyInterim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Head,
    /// `left` bytes of a length-delimited body.
    Length(u64),
    ChunkSize,
    ChunkData(u64),
    ChunkEnd,
    Trailers,
    UntilClose,
    Done,
    Failed,
}

/// The decoder. Feed it what the connection delivers, in any pieces.
pub struct Parser {
    limits: Limits,
    state: State,
    /// The head, or the current line of the chunked framing.
    line: Vec<u8>,
    head: Option<Head>,
    interim: usize,
    body_bytes: u64,
    trailer_bytes: usize,
}

impl Parser {
    pub fn new(limits: Limits) -> Parser {
        Parser {
            limits,
            state: State::Head,
            line: Vec::new(),
            head: None,
            interim: 0,
            body_bytes: 0,
            trailer_bytes: 0,
        }
    }

    pub fn head(&self) -> Option<&Head> {
        self.head.as_ref()
    }

    /// The body is complete (a connection that ends now is a good end).
    pub fn is_done(&self) -> bool {
        self.state == State::Done
    }

    /// Body bytes delivered so far.
    pub fn body_bytes(&self) -> u64 {
        self.body_bytes
    }

    /// The connection ended. `Ok` if the response was complete, or ended by
    /// the close; a short body or a head not finished is `Truncated`.
    pub fn finish(&self) -> Result<(), Error> {
        match self.state {
            State::Done | State::UntilClose => Ok(()),
            _ => Err(Error::Truncated),
        }
    }

    /// Takes the next bytes. Body bytes go to `body` with the head, in order,
    /// in pieces that never exceed the input. Returns how many bytes it took; fewer
    /// than `input.len()` only when the response ended (what is left is
    /// beyond it, and ignored by the caller) or on an error (and then the
    /// parser accepts nothing more).
    pub fn feed(
        &mut self,
        input: &[u8],
        body: &mut dyn FnMut(&Head, &[u8]),
    ) -> Result<usize, Error> {
        match self.step(input, body) {
            Ok(n) => Ok(n),
            Err(e) => {
                self.state = State::Failed;
                Err(e)
            }
        }
    }

    fn step(&mut self, input: &[u8], body: &mut dyn FnMut(&Head, &[u8])) -> Result<usize, Error> {
        let mut at = 0;
        while at < input.len() {
            match self.state {
                State::Done => return Ok(at),
                State::Failed => return Err(Error::BadFraming),
                State::Head => {
                    at += self.take_head(&input[at..])?;
                }
                State::Length(left) => {
                    let n = (input.len() - at).min(left.min(usize::MAX as u64) as usize);
                    self.deliver(&input[at..at + n], body)?;
                    at += n;
                    let left = left - n as u64;
                    self.state = if left == 0 {
                        State::Done
                    } else {
                        State::Length(left)
                    };
                }
                State::UntilClose => {
                    self.deliver(&input[at..], body)?;
                    at = input.len();
                }
                State::ChunkSize => {
                    let Some(line) = self.take_line(input, &mut at)? else {
                        continue;
                    };
                    let size = chunk_size(&line)?;
                    self.state = if size == 0 {
                        self.trailer_bytes = 0;
                        State::Trailers
                    } else {
                        State::ChunkData(size)
                    };
                }
                State::ChunkData(left) => {
                    let n = (input.len() - at).min(left.min(usize::MAX as u64) as usize);
                    self.deliver(&input[at..at + n], body)?;
                    at += n;
                    let left = left - n as u64;
                    self.state = if left == 0 {
                        State::ChunkEnd
                    } else {
                        State::ChunkData(left)
                    };
                }
                State::ChunkEnd => {
                    // The CRLF after the data, byte by byte through `line`.
                    let Some(line) = self.take_line(input, &mut at)? else {
                        continue;
                    };
                    if !line.is_empty() {
                        return Err(Error::BadChunk);
                    }
                    self.state = State::ChunkSize;
                }
                State::Trailers => {
                    let Some(line) = self.take_line(input, &mut at)? else {
                        continue;
                    };
                    if line.is_empty() {
                        self.state = State::Done;
                    } else {
                        self.trailer_bytes += line.len() + 2;
                        if self.trailer_bytes > self.limits.max_head || !valid_field(&line) {
                            return Err(Error::BadChunk);
                        }
                    }
                }
            }
        }
        // A body of length zero is complete without a further byte.
        if self.state == State::Length(0) {
            self.state = State::Done;
        }
        Ok(at)
    }

    fn deliver(&mut self, bytes: &[u8], body: &mut dyn FnMut(&Head, &[u8])) -> Result<(), Error> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.body_bytes += bytes.len() as u64;
        if self.body_bytes > self.limits.max_body {
            return Err(Error::BodyTooLarge);
        }
        // The head is known whenever a body byte is: no state with a body
        // comes before it.
        if let Some(head) = &self.head {
            body(head, bytes);
        }
        Ok(())
    }

    /// Collects bytes of a CRLF-ended line of at most [`MAX_LINE`] bytes;
    /// `Some(line)` (without the CRLF) when complete.
    fn take_line(&mut self, input: &[u8], at: &mut usize) -> Result<Option<Vec<u8>>, Error> {
        while *at < input.len() {
            let b = input[*at];
            *at += 1;
            self.line.push(b);
            let n = self.line.len();
            if b == b'\n' {
                if n >= 2 && self.line[n - 2] == b'\r' {
                    self.line.truncate(n - 2);
                    return Ok(Some(core::mem::take(&mut self.line)));
                }
                return Err(Error::BadChunk);
            }
            if n > MAX_LINE {
                return Err(Error::BadChunk);
            }
        }
        Ok(None)
    }

    /// Collects the head; when it ends, parses it. Returns bytes taken.
    fn take_head(&mut self, input: &[u8]) -> Result<usize, Error> {
        let mut taken = 0;
        for &b in input {
            taken += 1;
            self.line.push(b);
            if self.line.len() > self.limits.max_head {
                return Err(Error::HeadTooLarge);
            }
            if self.line.ends_with(b"\r\n\r\n") {
                let head = core::mem::take(&mut self.line);
                self.finish_head(&head)?;
                return Ok(taken);
            }
        }
        Ok(taken)
    }

    fn finish_head(&mut self, head: &[u8]) -> Result<(), Error> {
        // "STATUS\r\nH1\r\nH2\r\n" (the empty line dropped), taken apart at
        // each CRLF; a bare CR or LF inside a line fails the checks below.
        let mut rest = &head[..head.len() - 2];
        let end = find_crlf(rest).ok_or(Error::BadStatusLine)?;
        let status = status_code(&rest[..end])?;
        rest = &rest[end + 2..];
        let mut length: Option<u64> = None;
        let mut chunked = false;
        let mut other_coding = false;
        let mut redirect = false;
        let mut headers = 0usize;
        while !rest.is_empty() {
            let end = find_crlf(rest).ok_or(Error::BadHeader)?;
            let line = &rest[..end];
            rest = &rest[end + 2..];
            headers += 1;
            if headers > self.limits.max_headers {
                return Err(Error::TooManyHeaders);
            }
            if !valid_field(line) {
                return Err(Error::BadHeader);
            }
            let colon = line
                .iter()
                .position(|b| *b == b':')
                .ok_or(Error::BadHeader)?;
            let (name, value) = (&line[..colon], trim(&line[colon + 1..]));
            if name.eq_ignore_ascii_case(b"content-length") {
                if value.is_empty() || value.len() > 19 || !value.iter().all(u8::is_ascii_digit) {
                    return Err(Error::BadLength);
                }
                let n = value.iter().fold(0u64, |a, d| a * 10 + u64::from(d - b'0'));
                if length.is_some_and(|l| l != n) {
                    return Err(Error::BadLength);
                }
                length = Some(n);
            } else if name.eq_ignore_ascii_case(b"transfer-encoding") {
                if value.eq_ignore_ascii_case(b"chunked") && !chunked {
                    chunked = true;
                } else {
                    other_coding = true;
                }
            } else if name.eq_ignore_ascii_case(b"location") {
                redirect = true;
            }
        }
        if other_coding || (chunked && length.is_some()) {
            return Err(Error::BadFraming);
        }
        if (100..200).contains(&status) && status != 101 {
            self.interim += 1;
            if self.interim > MAX_INTERIM {
                return Err(Error::TooManyInterim);
            }
            // The real response follows.
            return Ok(());
        }
        let (kind, state) = if status == 204 || status == 304 || status == 101 {
            (Body::None, State::Done)
        } else if chunked {
            (Body::Chunked, State::ChunkSize)
        } else if let Some(n) = length {
            if n > self.limits.max_body {
                return Err(Error::BodyTooLarge);
            }
            (
                Body::Length(n),
                if n == 0 {
                    State::Done
                } else {
                    State::Length(n)
                },
            )
        } else {
            (Body::UntilClose, State::UntilClose)
        };
        self.head = Some(Head {
            status,
            body: kind,
            redirect,
            headers,
        });
        self.state = state;
        Ok(())
    }
}

fn find_crlf(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|w| w == b"\r\n")
}

fn trim(mut v: &[u8]) -> &[u8] {
    while let [b' ' | b'\t', rest @ ..] = v {
        v = rest;
    }
    while let [rest @ .., b' ' | b'\t'] = v {
        v = rest;
    }
    v
}

/// A header line (or a trailer): a token, a colon, a value without control
/// characters; no folding.
fn valid_field(line: &[u8]) -> bool {
    let Some(colon) = line.iter().position(|b| *b == b':') else {
        return false;
    };
    let name = &line[..colon];
    !name.is_empty()
        && name.iter().all(|b| is_token(*b))
        && line[colon + 1..]
            .iter()
            .all(|b| *b == b'\t' || (0x20..0x7f).contains(b) || *b >= 0x80)
}

fn is_token(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b)
}

fn status_code(line: &[u8]) -> Result<u16, Error> {
    // "HTTP/1.x SSS" then end or a space and a reason (printable).
    if line.len() < 12 || !(line.starts_with(b"HTTP/1.1 ") || line.starts_with(b"HTTP/1.0 ")) {
        return Err(Error::BadStatusLine);
    }
    let digits = &line[9..12];
    if !digits.iter().all(u8::is_ascii_digit) {
        return Err(Error::BadStatusLine);
    }
    if line.len() > 12 && (line[12] != b' ' || !line[13..].iter().all(|b| (0x20..0x7f).contains(b)))
    {
        return Err(Error::BadStatusLine);
    }
    let n = digits
        .iter()
        .fold(0u16, |a, d| a * 10 + u16::from(d - b'0'));
    if !(100..=599).contains(&n) {
        return Err(Error::BadStatusLine);
    }
    Ok(n)
}

fn chunk_size(line: &[u8]) -> Result<u64, Error> {
    // Hex digits, then optionally `;` and extensions we do not read.
    let end = line.iter().position(|b| *b == b';').unwrap_or(line.len());
    let digits = &line[..end];
    let digits = {
        // Whitespace before an extension is allowed by old servers.
        let mut d = digits;
        while let [rest @ .., b' ' | b'\t'] = d {
            d = rest;
        }
        d
    };
    if digits.is_empty() || digits.len() > 15 {
        return Err(Error::BadChunk);
    }
    let mut n = 0u64;
    for b in digits {
        let v = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return Err(Error::BadChunk),
        };
        n = n << 4 | u64::from(v);
    }
    Ok(n)
}

#[cfg(test)]
zoraya_test_alloc::cap_memory!();

#[cfg(test)]
mod tests;
