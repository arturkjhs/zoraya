#![no_std]
#![no_main]

//! `fetch URL [-o FILE]` (N3, N5): an HTTP/1.1 GET over a connection `zosh`
//! made for exactly this host and port (the `url` operand, ADR-0224 item 2).
//! The request and the decoder of the response are `zoraya-http`; the bytes
//! travel on the two streams this program made for `netd` (ADR-0222's note of
//! 2026-11-10), so a slow page costs this program's room and nobody else's.
//! With `https://` the bytes are TLS (`zoraya-tls`, ADR-0228) over the same
//! streams: the roots come from `/system/tls/roots`, which the shell adds to
//! the table of a program with a network grant, the date from the wall clock
//! of ADR-0226 and the randomness from the connection ADR-0227 gives every
//! program; `netd` sees ciphertext only. The name checked against the
//! certificate is the host of the address, which is also the name granted.

extern crate alloc;

use alloc::string::String;
use alloc::vec;

use zoraya_abi::{Handle, Mode, Packet, Status, stream_signals};
use zoraya_http::{Error as HttpError, Limits, Parser, parse_ipv4, parse_url, request};
use zoraya_proto_file::{open_flags, rights};
use zoraya_proto_net::{reason, status};
use zoraya_tls::{Config, Connection as Tls, Transport, TransportError};
use zoraya_user::files::File;
use zoraya_user::netconn::{CONNECT_WAIT_NS, ConnectError, Connection, connect};
use zoraya_user::{
    clock_monotonic, command, handle_close, namespace, port_create, port_subscribe, port_wait,
    stream_read, stream_write,
};

zoraya_user::describe!("commands", "fetch");

/// How long the connection may be silent before the fetch gives up.
const IDLE_NS: u64 = 30_000_000_000;

fn fail(c: &command::Command, subject: Option<&str>, problem: &str) -> i32 {
    c.error(subject, problem);
    1
}

/// Waits until `handle` shows one of `signals`, or the deadline.
fn wait(port: Handle, handle: Handle, signals: u32, until: u64) -> bool {
    let key = 7;
    if port_subscribe(port, handle, signals, key, Mode::Level).is_err() {
        return false;
    }
    let mut packets = [Packet::default()];
    let ok = port_wait(port, until, &mut packets).is_ok();
    let _ = zoraya_user::port_unsubscribe(port, key);
    ok
}

/// Writes all of `bytes` to the stream, waiting for room.
fn write_all(port: Handle, end: Handle, mut bytes: &[u8]) -> bool {
    while !bytes.is_empty() {
        match stream_write(end, bytes) {
            Ok(n) if n > 0 => bytes = &bytes[n.min(bytes.len())..],
            Ok(_) | Err(Status::WouldBlock) => {
                let until = clock_monotonic() + IDLE_NS;
                let signals = stream_signals::WRITABLE | stream_signals::PEER_CLOSED;
                if !wait(port, end, signals, until) {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// FILE, made or emptied (the shell granted the one name).
fn open_output(path: &str) -> Result<File, u32> {
    let ns = namespace::get().ok_or(zoraya_proto_file::status::ACCESS_DENIED)?;
    ns.open(
        path,
        open_flags::CREATE | open_flags::TRUNCATE,
        rights::READ | rights::WRITE,
    )
}

fn why(e: ConnectError) -> &'static str {
    match e {
        ConnectError::Refused(status::NOT_GRANTED) => "the network is not granted to this command",
        ConnectError::Refused(_) | ConnectError::Unavailable => "the network is not available",
        ConnectError::TimedOut => "the network service did not answer in time",
        ConnectError::Failed(reason::UNRESOLVABLE) => "the name has no address",
        ConnectError::Failed(reason::REFUSED) => "the connection was refused",
        ConnectError::Failed(reason::UNREACHABLE) => "there is no route to the host",
        ConnectError::Failed(reason::TIMED_OUT) => "the host did not answer",
        ConnectError::Failed(reason::LOCAL) => "the name points at this machine's own network",
        ConnectError::Failed(reason::OVER_LIMIT) => {
            "the network service has no room for another connection"
        }
        ConnectError::Failed(_) => "the connection failed",
    }
}

/// The roots the shell put in this program's table (ADR-0228 item 3).
const ROOTS: &str = "/system/tls/roots";

/// The wall clock in seconds (ADR-0226 item 4), or none.
fn wall_clock() -> Option<u64> {
    namespace::realtime().map(|ns| ns / 1_000_000_000)
}

/// TLS over the two streams of the connection.
struct StreamIo {
    waiter: Handle,
    inbound: Handle,
    outbound: Handle,
}

impl Transport for StreamIo {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        loop {
            match stream_read(self.inbound, buffer) {
                Ok(n) => return Ok(n.min(buffer.len())),
                Err(Status::WouldBlock) => {
                    let until = clock_monotonic() + IDLE_NS;
                    let signals = stream_signals::READABLE | stream_signals::PEER_CLOSED;
                    if !wait(self.waiter, self.inbound, signals, until) {
                        return Err(TransportError("no answer in time"));
                    }
                }
                Err(_) => return Err(TransportError("the stream failed")),
            }
        }
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        if write_all(self.waiter, self.outbound, data) {
            Ok(())
        } else {
            Err(TransportError("the request could not be sent"))
        }
    }
}

/// The handshake with `host` over `io`: roots, date and randomness from the
/// system, or why not.
fn start_tls(host: &str, io: StreamIo) -> Result<Tls<StreamIo>, String> {
    let roots = namespace::get()
        .and_then(|ns| ns.open(ROOTS, 0, rights::READ).ok())
        .and_then(|f| zoraya_user::files::read_all(&f).ok())
        .map(|text| zoraya_tls::pem_certificates(&text))
        .unwrap_or_default();
    if roots.is_empty() {
        return Err(String::from("no trusted roots were given to this command"));
    }
    let mut seed = [0u8; 32];
    if !zoraya_user::random::random(&mut seed) {
        return Err(String::from("the system gave no random bytes"));
    }
    zoraya_tls::seed_random(&seed);
    let config = Config::new(&roots, wall_clock).map_err(|e| alloc::format!("{e}"))?;
    Tls::connect(&config, host, io).map_err(|e| alloc::format!("{e}"))
}

/// Where the page's bytes come from: the stream as it is, or TLS over it.
enum Source {
    Plain { waiter: Handle, inbound: Handle },
    Tls(alloc::boxed::Box<Tls<StreamIo>>),
}

impl Source {
    /// Reads some bytes: 0 is the end. `Err` says why not.
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, Option<String>> {
        match self {
            Source::Plain { waiter, inbound } => loop {
                match stream_read(*inbound, buf) {
                    Ok(n) => return Ok(n.min(buf.len())),
                    Err(Status::WouldBlock) => {
                        let until = clock_monotonic() + IDLE_NS;
                        let signals = stream_signals::READABLE | stream_signals::PEER_CLOSED;
                        if !wait(*waiter, *inbound, signals, until) {
                            return Err(None);
                        }
                    }
                    // A read error is a stream that is not ours any more: the
                    // connection is cut (the check below says so).
                    Err(_) => return Err(None),
                }
            },
            Source::Tls(tls) => tls.read(buf).map_err(|e| Some(alloc::format!("{e}"))),
        }
    }
}

fn http_why(e: HttpError) -> &'static str {
    match e {
        HttpError::Truncated => "the connection ended before the page did",
        HttpError::BodyTooLarge => "the page is larger than 64 MiB",
        HttpError::HeadTooLarge | HttpError::TooManyHeaders => "the response head is too large",
        _ => "the response is not valid HTTP",
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let Some(text) = p.operand(&c.d, "URL") else {
        return fail(&c, None, "no URL");
    };
    let url = match parse_url(text) {
        Ok(u) => u,
        Err(e) => {
            let problem = match e {
                zoraya_http::UrlError::Scheme => {
                    "only http:// and https:// addresses are understood"
                }
                zoraya_http::UrlError::Host => "the host is not a name or a dotted address",
                zoraya_http::UrlError::Port => "the port is not a number from 1 to 65535",
                zoraya_http::UrlError::Userinfo => "a user name or password in the address",
                zoraya_http::UrlError::Path => "the path has a character that is not allowed",
            };
            return fail(&c, Some(text), problem);
        }
    };
    let Some(net) = namespace::get().and_then(|n| n.net()) else {
        return fail(&c, None, "the network is not available to this command");
    };
    // The file is opened (and emptied) at the first byte of a 2xx page, not
    // before: a refused connection, a 404 or a name that does not resolve
    // leaves an existing FILE as it was.
    let out_path = p.value(&c.d, "output");
    if out_path.is_some() && namespace::get().is_none() {
        return fail(&c, None, "it was given no files: run it from zosh");
    }
    let mut file: Option<File> = None;
    let mut open_failed: Option<u32> = None;
    let host = String::from(url.host).to_ascii_lowercase();
    let (name, address) = match parse_ipv4(&host) {
        Some(a) => ("", a),
        None => (host.as_str(), [0; 4]),
    };
    let mut conn: Connection = match connect(net, name, address, url.port, CONNECT_WAIT_NS) {
        Ok(conn) => conn,
        Err(e) => return fail(&c, Some(text), why(e)),
    };
    let Ok(waiter) = port_create() else {
        return fail(&c, None, "no port");
    };
    let Some(outbound) = conn.outbound else {
        return fail(&c, None, "no stream");
    };
    let mut handshake_us = 0u64;
    let mut source = if url.tls {
        let io = StreamIo {
            waiter,
            inbound: conn.inbound,
            outbound,
        };
        // The handshake ends before any byte of the request is sent
        // (INV-0228-1): a name, date or root that does not fit fails here.
        let began = clock_monotonic();
        let started_tls = start_tls(&host, io);
        handshake_us = (clock_monotonic() - began) / 1000;
        match started_tls {
            Ok(mut tls) => {
                if tls.write_all(&request(&url)).is_err() {
                    return fail(&c, Some(text), "the request could not be sent");
                }
                Source::Tls(alloc::boxed::Box::new(tls))
            }
            Err(problem) => {
                conn.shutdown_write();
                return fail(&c, Some(text), &problem);
            }
        }
    } else {
        if !write_all(waiter, outbound, &request(&url)) {
            return fail(&c, Some(text), "the request could not be sent");
        }
        Source::Plain {
            waiter,
            inbound: conn.inbound,
        }
    };
    let mut tls_problem: Option<String> = None;
    let mut parser = Parser::new(Limits::DEFAULT);
    let mut buf = vec![0u8; 16 * 1024];
    let mut offset = 0u64;
    let mut write_failed = false;
    let mut result = Ok(());
    let sum = p.flag(&c.d, "sum");
    let started = clock_monotonic();
    let (mut size, mut hash, mut hashing) = (0u64, 0xcbf2_9ce4_8422_2325u64, 0u64);
    loop {
        let n = match source.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(why) => {
                tls_problem = why;
                result = Err(HttpError::Truncated);
                break;
            }
        };
        let fed = parser.feed(&buf[..n], &mut |head, bytes| {
            // Only a successful page is a page; the body of an error is not
            // printed as if it were the content.
            if !(200..300).contains(&head.status) {
                return;
            }
            if sum {
                let t = clock_monotonic();
                for b in bytes {
                    hash = (hash ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3);
                }
                hashing += clock_monotonic() - t;
                size += bytes.len() as u64;
                return;
            }
            let Some(path) = out_path else {
                zoraya_user::write_output(bytes);
                return;
            };
            if file.is_none() {
                match open_output(path) {
                    Ok(f) => file = Some(f),
                    Err(s) => {
                        open_failed = Some(s);
                        return;
                    }
                }
            }
            if let Some(f) = &file {
                match f.write(offset, 0, bytes) {
                    Ok(_) => offset += bytes.len() as u64,
                    Err(_) => write_failed = true,
                }
            }
        });
        if let Err(e) = fed {
            result = Err(e);
            break;
        }
        if write_failed || open_failed.is_some() || parser.is_done() {
            break;
        }
    }
    if result.is_ok() {
        result = parser.finish();
    }
    let reset = conn.was_reset();
    if let Source::Tls(tls) = source {
        let _ = tls.close();
    }
    conn.shutdown_write();
    let _ = handle_close(waiter);
    if let Some(status) = open_failed {
        let name = zoraya_user::files::status_name(status);
        return fail(&c, out_path, name);
    }
    if write_failed {
        return fail(&c, out_path, "the file could not be written");
    }
    // A reset before the end of the page is a cut page whatever the framing:
    // a body that ends with the connection looks complete to the decoder.
    if reset && !parser.is_done() {
        return fail(&c, Some(text), "connection reset");
    }
    if let Err(e) = result {
        if let Some(problem) = &tls_problem {
            return fail(&c, Some(text), problem);
        }
        return fail(&c, Some(text), http_why(e));
    }
    let (status_code, redirected) = parser.head().map_or((0, false), |h| (h.status, h.redirect));
    if !(200..300).contains(&status_code) {
        let problem = if redirected {
            "the page has moved; redirects are not followed"
        } else {
            "the server did not give the page"
        };
        let line = alloc::format!("{problem} (HTTP {status_code})");
        return fail(&c, Some(text), &line);
    }
    if let (Some(path), None) = (out_path, &file)
        && !sum
        && let Err(s) = open_output(path)
    {
        return fail(&c, Some(path), zoraya_user::files::status_name(s));
    }
    if sum {
        if url.tls {
            zoraya_user::println!(
                "tls handshake {}.{:03} ms",
                handshake_us / 1000,
                handshake_us % 1000
            );
        }
        let us = (clock_monotonic() - started) / 1000;
        let hashing = hashing / 1000;
        zoraya_user::println!(
            "{size} bytes fnv1a {hash:016x} in {}.{:03} ms (hashing {}.{:03} ms)",
            us / 1000,
            us % 1000,
            hashing / 1000,
            hashing % 1000
        );
    }
    0
}
