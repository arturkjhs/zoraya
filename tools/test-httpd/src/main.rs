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
//! With `--tls-dir DIR` (the test PKI of `tests/net/tls`) three more listeners
//! run in front of the same server, one per certificate: `valid` (for
//! `test.zoraya`), `expired` (for `expired.zoraya`, whose validity ended in
//! 2020) and `wrong` (a certificate for `other.zoraya`, to be asked as
//! `wrong.zoraya`). Each terminates TLS and passes the request to the plain
//! server on this host, then the answer back and a close_notify (ADR-0228,
//! ADR-0230). Their ports are written to `--tls-ready-file`, one line:
//! `valid expired wrong`.
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

fn crc32(data: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in data {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xedb8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A solid-colour RGB PNG of `w x h` (stored deflate blocks: no compressor).
fn solid_png(w: u32, h: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut raw = Vec::new();
    for _ in 0..h {
        raw.extend_from_slice(&[0]);
        for _ in 0..w {
            raw.extend_from_slice(&rgb);
        }
    }
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &raw {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    let mut z = vec![0x78, 0x01];
    let mut chunks = raw.chunks(65535).peekable();
    while let Some(c) = chunks.next() {
        z.push(u8::from(chunks.peek().is_none()));
        z.extend_from_slice(&(c.len() as u16).to_le_bytes());
        z.extend_from_slice(&(!(c.len() as u16)).to_le_bytes());
        z.extend_from_slice(c);
    }
    z.extend_from_slice(&((b << 16) | a).to_be_bytes());
    let mut out = vec![0x89, b'P', b'N', b'G', 13, 10, 26, 10];
    let mut chunk = |kind: &[u8; 4], data: &[u8]| {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend_from_slice(data);
        out.extend_from_slice(&body);
        out.extend_from_slice(&crc32(&body).to_be_bytes());
    };
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(b"IHDR", &ihdr);
    chunk(b"IDAT", &z);
    chunk(b"IEND", &[]);
    out
}

/// The pages of the NetSurf test (`make test-netsurf`, TASK-27 9d): HTML with
/// a style sheet and an image of known colours, a form that submits and a
/// result page. Returns the content type and the body.
fn ns_page(path: &str) -> Option<(&'static str, Vec<u8>)> {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    match route {
        "/ns/page" => Some((
            "text/html",
            br#"<html><head><title>NS page</title><link rel="stylesheet" type="text/css" href="/ns/style.css"></head>
<body style="margin:0;background:#ffffff"><div class="blue"></div>
<img src="/ns/green.png" width="100" height="60" alt="green">
<form action="/ns/result" method="get"><input type="text" name="q" size="12"><input type="submit" value="Go"></form>
</body></html>
"#
            .to_vec(),
        )),
        // A page taller than the window: blue for 1200 pixels, then magenta,
        // for the wheel (proposed ADR-0322).
        "/ns/tall" => Some((
            "text/html",
            br#"<html><head><title>NS tall</title></head>
<body style="margin:0;background:#ffffff"><div style="height:1200px;background:#0000ff"></div>
<div style="height:1500px;background:#ff00ff"></div>
</body></html>
"#
            .to_vec(),
        )),
        // Russian and Ukrainian text in the system's fonts (ADR-0326,
        // 9f-text): sizes in points, so that the page is twice the pixels at
        // scale 2 (24 pt is 30 pixels at NetSurf's 90 dpi). Line 1 is ten і,
        // line 2 ten ж (і is a third as wide in a proportional font; the box
        // and the old 8-pixel cells are one width for both), line 3 the
        // Ukrainian letters, line 4 a Russian phrase.
        "/ns/cyrillic" => Some((
            "text/html; charset=utf-8",
            "<html><head><meta charset=\"utf-8\"><title>NS cyrillic</title></head>
<body style=\"margin:0;background:#ffffff;color:#000000\">
<div style=\"font-family:sans-serif;font-size:24pt;line-height:36pt;white-space:nowrap;margin:0\">
<div>\u{0456}\u{0456}\u{0456}\u{0456}\u{0456}\u{0456}\u{0456}\u{0456}\u{0456}\u{0456}</div>
<div>\u{0436}\u{0436}\u{0436}\u{0436}\u{0436}\u{0436}\u{0436}\u{0436}\u{0436}\u{0436}</div>
<div>\u{0490}\u{0491} \u{0404}\u{0454} \u{0406}\u{0456} \u{0407}\u{0457}</div>
<div>\u{0421}\u{044a}\u{0435}\u{0448}\u{044c} \u{0436}\u{0435} \u{0435}\u{0449}\u{0451}</div>
</div></body></html>
"
            .as_bytes()
            .to_vec(),
        )),
        "/ns/style.css" => Some((
            "text/css",
            b".blue{width:200px;height:60px;background:#0000ff}\n".to_vec(),
        )),
        "/ns/green.png" => Some(("image/png", solid_png(100, 60, [0, 255, 0]))),
        "/ns/result" => {
            let q: String = query
                .strip_prefix("q=")
                .unwrap_or("")
                .chars()
                .filter(|c| c.is_ascii_alphanumeric())
                .collect();
            Some((
                "text/html",
                format!(
                    "<html><head><title>NS result</title></head><body style=\"margin:0;background:#ffff00\"><h1>Result: {q}</h1></body></html>\n"
                )
                .into_bytes(),
            ))
        }
        _ => None,
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
    if first.contains("/ns/") {
        eprintln!("http: {first}");
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
    let ns = ns_page(path);
    let result = match path.split('/').collect::<Vec<_>>().as_slice() {
        _ if ns.is_some() => {
            let (kind, bytes) = ns.unwrap_or(("text/plain", Vec::new()));
            let h = head(
                200,
                "OK",
                &[
                    ("Content-Type", kind.into()),
                    ("Content-Length", bytes.len().to_string()),
                ],
            );
            stream.write_all(&h).and_then(|()| stream.write_all(&bytes))
        }
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

/// The certificate chain and key of `DIR/NAME.pem` and `DIR/NAME.key`.
fn tls_config(dir: &str, name: &str) -> Result<Arc<rustls::ServerConfig>, String> {
    let pem = |ext: &str| {
        std::fs::read(format!("{dir}/{name}.{ext}")).map_err(|e| format!("{dir}/{name}.{ext}: {e}"))
    };
    let chain: Vec<rustls::pki_types::CertificateDer<'static>> =
        rustls::pki_types::pem::PemObject::pem_slice_iter(&pem("pem")?)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("{name}.pem: {e}"))?;
    let key = rustls::pki_types::pem::PemObject::from_pem_slice(&pem("key")?)
        .map_err(|e| format!("{name}.key: {e}"))?;
    rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .map(Arc::new)
        .map_err(|e| format!("{name}: {e}"))
}

/// One TLS connection: the request head from the client, to the plain server,
/// its whole answer back, a close_notify.
fn tls_serve(name: &str, config: Arc<rustls::ServerConfig>, backend: u16, mut tcp: TcpStream) {
    let _ = tcp.set_read_timeout(Some(TIMEOUT));
    let _ = tcp.set_write_timeout(Some(TIMEOUT));
    let Ok(mut conn) = rustls::ServerConnection::new(config) else {
        return;
    };
    let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.ends_with(b"\r\n\r\n") && request.len() < MAX_HEAD {
        match tls.read(&mut chunk) {
            Ok(0) | Err(_) => {
                // The test script's checker reads these lines: a refused
                // handshake must leave no `request` line (INV-0228-1).
                eprintln!("tls {name}: ended after {} request bytes", request.len());
                return;
            }
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    eprintln!("tls {name}: request of {} bytes", request.len());
    let Ok(mut plain) = TcpStream::connect(("127.0.0.1", backend)) else {
        return;
    };
    let _ = plain.set_read_timeout(Some(TIMEOUT));
    if plain.write_all(&request).is_err() {
        return;
    }
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        match plain.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if tls.write_all(&buf[..n]).is_err() {
                    return;
                }
            }
        }
    }
    tls.conn.send_close_notify();
    let _ = tls.flush();
    let _ = tcp.shutdown(Shutdown::Write);
    let mut sink = [0u8; 256];
    let _ = tcp.set_read_timeout(Some(Duration::from_millis(200)));
    let _ = tcp.read(&mut sink);
}

/// Starts the three TLS listeners; gives their ports.
fn start_tls(dir: &str, backend: u16, deadline: Instant) -> Result<[u16; 3], String> {
    let mut ports = [0u16; 3];
    for (i, name) in ["valid", "expired", "wrong"].into_iter().enumerate() {
        let config = tls_config(dir, name)?;
        let listener = TcpListener::bind(("127.0.0.1", 0)).map_err(|e| e.to_string())?;
        ports[i] = listener.local_addr().map_err(|e| e.to_string())?.port();
        listener.set_nonblocking(true).map_err(|e| e.to_string())?;
        thread::spawn(move || {
            let active = Arc::new(AtomicUsize::new(0));
            while Instant::now() < deadline {
                if active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    thread::sleep(Duration::from_millis(20));
                    continue;
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let _ = stream.set_nonblocking(false);
                        active.fetch_add(1, Ordering::SeqCst);
                        let (active, config) = (active.clone(), config.clone());
                        thread::spawn(move || {
                            tls_serve(name, config, backend, stream);
                            active.fetch_sub(1, Ordering::SeqCst);
                        });
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => thread::sleep(Duration::from_millis(100)),
                }
            }
        });
    }
    Ok(ports)
}

fn main() {
    let mut port: Option<u16> = None;
    let mut lifetime = 300u64;
    let mut ready: Option<String> = None;
    let (mut tls_dir, mut tls_ready): (Option<String>, Option<String>) = (None, None);
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--port" => port = args.next().and_then(|v| v.parse().ok()),
            "--lifetime" => lifetime = args.next().and_then(|v| v.parse().ok()).unwrap_or(300),
            "--ready-file" => ready = args.next(),
            "--tls-dir" => tls_dir = args.next(),
            "--tls-ready-file" => tls_ready = args.next(),
            "--checksum" => {
                // The expected FNV-1a of /pattern/N, for the test script.
                let n = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);
                println!("{:016x}", checksum(n));
                return;
            }
            _ => {
                eprintln!(
                    "usage: zoraya-test-httpd --port N [--lifetime SECONDS] [--ready-file FILE] [--tls-dir DIR --tls-ready-file FILE] | --checksum N"
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
    if let Some(dir) = tls_dir {
        let backend = listener.local_addr().map_or(port, |a| a.port());
        match start_tls(&dir, backend, deadline) {
            Ok(ports) => {
                if let Some(path) = tls_ready {
                    let _ =
                        std::fs::write(path, format!("{} {} {}\n", ports[0], ports[1], ports[2]));
                }
            }
            Err(e) => {
                eprintln!("zoraya-test-httpd: tls: {e}");
                exit(1);
            }
        }
    }
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
