#![no_std]
#![no_main]

//! `fetch URL [-o FILE]` (N3): an HTTP/1.1 GET over a connection `zosh`
//! made for exactly this host and port (the `url` operand, ADR-0224 item 2).
//! The request and the decoder of the response are `zoraya-http`; the bytes
//! travel on the two streams this program made for `netd` (ADR-0222's note of
//! 2026-11-10), so a slow page costs this program's room and nobody else's.

extern crate alloc;

use alloc::string::String;
use alloc::vec;

use zoraya_abi::{Handle, Mode, Packet, Status, stream_signals};
use zoraya_http::{Error as HttpError, Limits, Parser, parse_ipv4, parse_url, request};
use zoraya_proto_file::{open_flags, rights};
use zoraya_proto_net::{reason, status};
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
                zoraya_http::UrlError::Scheme => "only http:// addresses are understood",
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
    if !write_all(waiter, outbound, &request(&url)) {
        return fail(&c, Some(text), "the request could not be sent");
    }
    let mut parser = Parser::new(Limits::DEFAULT);
    let mut buf = vec![0u8; 16 * 1024];
    let mut offset = 0u64;
    let mut write_failed = false;
    let mut result = Ok(());
    let sum = p.flag(&c.d, "sum");
    let started = clock_monotonic();
    let (mut size, mut hash, mut hashing) = (0u64, 0xcbf2_9ce4_8422_2325u64, 0u64);
    loop {
        let n = match stream_read(conn.inbound, &mut buf) {
            Ok(0) => break,
            Ok(n) => n.min(buf.len()),
            Err(Status::WouldBlock) => {
                let until = clock_monotonic() + IDLE_NS;
                let signals = stream_signals::READABLE | stream_signals::PEER_CLOSED;
                if !wait(waiter, conn.inbound, signals, until) {
                    result = Err(HttpError::Truncated);
                    break;
                }
                continue;
            }
            // A read error is a stream that is not ours any more: the
            // connection is cut (the check below says so).
            Err(_) => {
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
