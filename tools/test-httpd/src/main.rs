//! A test web server for `make test-net` (N3): the guest reaches it through
//! QEMU's user network at 10.0.2.2, which SLIRP maps to the host's
//! 127.0.0.1. Std only, and bounded in every way a test server can be left
//! running by mistake:
//!
//! - it listens on 127.0.0.1 only, on the port it is given (0: a free one,
//!   written to `--ready-file`);
//! - at most `MAX_CONNECTIONS` connections at once (the others wait in the
//!   kernel's backlog), each with a read and a write timeout, a request head
//!   of at most 8 KiB, and at most `MAX_REQUESTS` requests;
//! - it exits by itself after `--lifetime` seconds (default 300), whatever
//!   happens to the process that started it;
//! - bodies are generated, never read from disk, at most `MAX_BODY` bytes.
//!
//! Pages (GET only, `Connection: close` always):
//!
//! ```text
//! /                  a short text page (PAGE)
//! /pattern/N         N bytes of a fixed pseudo-random pattern, with Content-Length
//! /chunked/N         the same bytes in chunks of 1000 with a trailer
//! /close/N           the same bytes, no length, ended by closing the connection
//! /status/CODE       status CODE (100..=599, not 1xx/204/304 bodies) and a short body
//! /redirect          302 to /
//! /slow/N            the pattern, 1 KiB per 10 ms (for backpressure)
//! /reset             closes the connection with a reset after the head
//! ```
//!
//! `pattern(n)` is the one place the bytes are defined; the guest side
//! compares sizes and a checksum printed by `tests/shell/net-fetch.script`
//! (`make test-net` computes the expected line from this same function with
//! `--describe pattern N`).

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::process::exit;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

const MAX_CONNECTIONS: usize = 8;
const MAX_REQUESTS: usize = 10_000;
const MAX_BODY: u64 = 64 << 20;
const MAX_HEAD: usize = 8 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
const PAGE: &str = "Zoraya test page\nfetched over HTTP, byte for byte\n";

/// The bytes of `/pattern/N`: xorshift64, low byte, from a fixed seed.
fn pattern(n: u64) -> impl Iterator<Item = u8> {
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    (0..n).map(move |_| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x >> 24) as u8
    })
}

/// FNV-1a over the pattern, as `sum` prints it in the guest.
fn checksum(n: u64) -> u64 {
    pattern(n).fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn head(status: u16, reason: &str, extra: &[(&str, String)]) -> Vec<u8> {
    let mut out =
        format!("HTTP/1.1 {status} {reason}\r\nServer: zoraya-test-httpd\r\nConnection: close\r\n");
    for (k, v) in extra {
        out += &format!("{k}: {v}\r\n");
    }
    out += "\r\n";
    out.into_bytes()
}

fn body(out: &mut TcpStream, n: u64, slow: bool) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(64 * 1024);
    let mut it = pattern(n);
    loop {
        buf.clear();
        let want = if slow { 1024 } else { 64 * 1024 };
        buf.extend(it.by_ref().take(want));
        if buf.is_empty() {
            return Ok(());
        }
        out.write_all(&buf)?;
        if slow {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn serve(mut stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(TIMEOUT));
    let _ = stream.set_write_timeout(Some(TIMEOUT));
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    // The request head: lines until an empty one, at most MAX_HEAD bytes.
    let mut first = String::new();
    let mut total = 0;
    let mut line = String::new();
    loop {
        line.clear();
        match reader.by_ref().take(MAX_HEAD as u64).read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(n) => total += n,
        }
        if first.is_empty() {
            first = line.trim_end().to_string();
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        if total > MAX_HEAD {
            let _ = stream.write_all(&head(431, "Request Header Fields Too Large", &[]));
            return;
        }
    }
    let mut parts = first.split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    if method != "GET" {
        let _ = stream.write_all(&head(
            405,
            "Method Not Allowed",
            &[("Content-Length", "0".into())],
        ));
        return;
    }
    let size = |s: &str| s.parse::<u64>().ok().filter(|n| *n <= MAX_BODY);
    let result = match path.split('/').collect::<Vec<_>>().as_slice() {
        ["", ""] => {
            let h = head(
                200,
                "OK",
                &[
                    ("Content-Type", "text/plain".into()),
                    ("Content-Length", PAGE.len().to_string()),
                ],
            );
            stream
                .write_all(&h)
                .and_then(|()| stream.write_all(PAGE.as_bytes()))
        }
        ["", "pattern", n] | ["", "slow", n] if size(n).is_some() => {
            let n = size(n).unwrap_or(0);
            let slow = path.starts_with("/slow");
            let h = head(200, "OK", &[("Content-Length", n.to_string())]);
            stream
                .write_all(&h)
                .and_then(|()| body(&mut stream, n, slow))
        }
        ["", "close", n] if size(n).is_some() => {
            let n = size(n).unwrap_or(0);
            stream
                .write_all(&head(200, "OK", &[]))
                .and_then(|()| body(&mut stream, n, false))
        }
        ["", "chunked", n] if size(n).is_some() => {
            let n = size(n).unwrap_or(0);
            let mut result = stream.write_all(&head(
                200,
                "OK",
                &[
                    ("Transfer-Encoding", "chunked".into()),
                    ("Trailer", "X-Done".into()),
                ],
            ));
            let mut it = pattern(n);
            loop {
                let chunk: Vec<u8> = it.by_ref().take(1000).collect();
                if chunk.is_empty() || result.is_err() {
                    break;
                }
                result = stream
                    .write_all(format!("{:x};part\r\n", chunk.len()).as_bytes())
                    .and_then(|()| stream.write_all(&chunk))
                    .and_then(|()| stream.write_all(b"\r\n"));
            }
            result.and_then(|()| stream.write_all(b"0\r\nX-Done: yes\r\n\r\n"))
        }
        ["", "status", code] => {
            let code = code
                .parse::<u16>()
                .ok()
                .filter(|c| (200..=599).contains(c) && *c != 204 && *c != 304);
            match code {
                Some(c) => {
                    let text = format!("status {c}\n");
                    let h = head(c, "Test", &[("Content-Length", text.len().to_string())]);
                    stream
                        .write_all(&h)
                        .and_then(|()| stream.write_all(text.as_bytes()))
                }
                None => {
                    stream.write_all(&head(400, "Bad Request", &[("Content-Length", "0".into())]))
                }
            }
        }
        ["", "redirect"] => stream.write_all(&head(
            302,
            "Found",
            &[("Location", "/".into()), ("Content-Length", "0".into())],
        )),
        ["", "reset"] => {
            let r = stream.write_all(&head(200, "OK", &[("Content-Length", "100".into())]));
            // Closing with unread input makes the kernel send a reset.
            let _ = r;
            return;
        }
        _ => stream.write_all(&head(404, "Not Found", &[("Content-Length", "0".into())])),
    };
    let _ = result;
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
    // Let the client read what is in flight before the socket is dropped.
    let mut sink = [0u8; 256];
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = stream.read(&mut sink);
}

fn main() {
    let mut port: Option<u16> = None;
    let mut lifetime = 300u64;
    let mut ready: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|v| v.parse().ok()),
            "--lifetime" => lifetime = args.next().and_then(|v| v.parse().ok()).unwrap_or(300),
            "--ready-file" => ready = args.next(),
            "--checksum" => {
                // The expected FNV-1a of /pattern/N, for the test script.
                let n = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                println!("{:016x}", checksum(n));
                return;
            }
            _ => {
                eprintln!(
                    "usage: zoraya-test-httpd --port N [--lifetime SECONDS] [--ready-file FILE] | --checksum N"
                );
                exit(2);
            }
        }
    }
    let Some(port) = port else {
        eprintln!("zoraya-test-httpd: --port is required");
        exit(2);
    };
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("zoraya-test-httpd: cannot listen on 127.0.0.1:{port}: {e}");
            exit(1);
        }
    };
    if let Some(path) = ready {
        // The port it got: `--port 0` asks the kernel for a free one.
        let got = listener.local_addr().map_or(port, |a| a.port());
        let _ = std::fs::write(path, format!("{got}\n"));
    }
    let deadline = Instant::now() + Duration::from_secs(lifetime);
    // The accept loop wakes every 200 ms to see the deadline.
    listener.set_nonblocking(true).expect("non-blocking");
    let active = Arc::new(AtomicUsize::new(0));
    let mut served = 0;
    while Instant::now() < deadline && served < MAX_REQUESTS {
        if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
            thread::sleep(Duration::from_millis(20));
            continue;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = stream.set_nonblocking(false);
                served += 1;
                active.fetch_add(1, Ordering::SeqCst);
                let active = active.clone();
                thread::spawn(move || {
                    serve(stream);
                    active.fetch_sub(1, Ordering::SeqCst);
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(20));
            }
            Err(_) => thread::sleep(Duration::from_millis(100)),
        }
    }
}
