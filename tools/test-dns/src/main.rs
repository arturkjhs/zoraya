//! A test DNS server for `make test-net-lookup`, and the check of that step's
//! capture. Std only.
//!
//! The server: the guest's `net.dns` is 10.0.2.2, which QEMU's user network
//! maps to the host's 127.0.0.1, where this listens on UDP, on the port it is
//! given (0: a free one, written to `--ready-file`). Port 53 needs root on
//! Linux, and on macOS for 127.0.0.1, so the step's image is the test image,
//! whose `netd` takes the port from `/config/net-test-dns-port` (feature
//! `net-test`; every other `netd` asks port 53). Each name it knows has an address and
//! a delay; a query for it is answered after the delay, every query on its
//! own (a retry gets its own late answer). A `--silent` name is never
//! answered, any other name gets NXDOMAIN at once. Bounded as the test web
//! server is: 127.0.0.1 only, at most `MAX_PENDING` late answers held, and it
//! exits by itself after `--lifetime` seconds. Every query and answer is a
//! line on standard output, with the milliseconds since the start.
//!
//! ```text
//! zoraya-test-dns --port N [--lifetime S] [--ready-file F]
//!                 [--answer NAME=A.B.C.D[@MS]]... [--silent NAME]...
//! zoraya-test-dns --check-pcap FILE --server A.B.C.D [--server-port N] --first NAME
//!                 [--then-echo NAME]... [--then-syn NAME]...
//! ```
//!
//! The check reads the step's capture (QEMU's `filter-dump`, a pcap file of
//! Ethernet frames) and fails unless: the first DNS query of the capture is
//! for `--first`, and before it the guest asked for the server with exactly
//! one ARP request, which was answered (the query did not wait for its own
//! ARP or a retry); every name given was answered; and after the first
//! answer for a `--then-echo` name an echo request to its address, after one
//! for a `--then-syn` name a TCP SYN to its address, left within
//! `FOLLOW_MS` (`netd` acted on the answer at once, not on its next wake-up).
//! It prints the frames that matter, with times relative to the first query.

use std::collections::HashMap;
use std::net::UdpSocket;
use std::time::{Duration, Instant};

/// Late answers held at once; queries beyond them are dropped.
const MAX_PENDING: usize = 64;
/// How soon after its answer the echo request or SYN must leave.
const FOLLOW_MS: u64 = 500;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Rule {
    Answer([u8; 4], u64),
    Silent,
}

/// The question of a query: (identifier, lower-case name, end of question).
fn question(m: &[u8]) -> Option<(u16, String, usize)> {
    if m.len() < 12 || m[2] & 0x80 != 0 || u16::from_be_bytes([m[4], m[5]]) != 1 {
        return None;
    }
    let mut at = 12;
    let mut labels = Vec::new();
    loop {
        let n = *m.get(at)? as usize;
        at += 1;
        if n == 0 {
            break;
        }
        if n > 63 {
            return None;
        }
        labels.push(String::from_utf8(m.get(at..at + n)?.to_vec()).ok()?);
        at += n;
    }
    // QTYPE and QCLASS.
    m.get(at..at + 4)?;
    Some((
        u16::from_be_bytes([m[0], m[1]]),
        labels.join(".").to_ascii_lowercase(),
        at + 4,
    ))
}

/// The answer to query `q` whose question ends at `end`: an A record, or
/// NXDOMAIN for `None`.
fn answer(q: &[u8], end: usize, address: Option<[u8; 4]>) -> Vec<u8> {
    let mut m = q[..end].to_vec();
    m[2] = 0x81 | (q[2] & 0x01);
    m[3] = if address.is_some() { 0x80 } else { 0x83 };
    m[6..12].copy_from_slice(&[0, u8::from(address.is_some()), 0, 0, 0, 0]);
    if let Some(a) = address {
        m.extend_from_slice(&[0xc0, 12, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4]);
        m.extend_from_slice(&a);
    }
    m
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let parts: Vec<u8> = s
        .split('.')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    parts.try_into().ok()
}

fn parse_rule(s: &str) -> Option<(String, Rule)> {
    let (name, rest) = s.split_once('=')?;
    let (address, delay) = match rest.split_once('@') {
        Some((a, d)) => (a, d.parse().ok()?),
        None => (rest, 0),
    };
    Some((
        name.to_ascii_lowercase(),
        Rule::Answer(parse_ipv4(address)?, delay),
    ))
}

fn serve(port: u16, lifetime: u64, ready: Option<String>, rules: HashMap<String, Rule>) -> i32 {
    // The address may be held for a moment by another run's server on this
    // host: try again for a while before giving up.
    let started = Instant::now();
    let socket = loop {
        match UdpSocket::bind(("127.0.0.1", port)) {
            Ok(s) => break s,
            Err(e)
                if e.kind() == std::io::ErrorKind::AddrInUse
                    && started.elapsed().as_secs() < 60 =>
            {
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(e) => {
                println!("bind failed: 127.0.0.1:{port}: {e}");
                return 2;
            }
        }
    };
    let port = match socket.local_addr() {
        Ok(a) => a.port(),
        Err(e) => {
            println!("no local address: {e}");
            return 2;
        }
    };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(10)));
    let start = Instant::now();
    let deadline = start + Duration::from_secs(lifetime);
    if let Some(f) = ready
        && std::fs::write(&f, format!("{port}\n")).is_err()
    {
        println!("cannot write {f}");
        return 2;
    }
    println!("listening on 127.0.0.1:{port}");
    let ms = |at: Instant| at.duration_since(start).as_millis();
    let mut pending: Vec<(Instant, Vec<u8>, std::net::SocketAddr, String)> = Vec::new();
    let mut buf = [0u8; 600];
    while Instant::now() < deadline {
        if let Ok((n, from)) = socket.recv_from(&mut buf) {
            let now = Instant::now();
            match question(&buf[..n]) {
                None => println!("{} ms: not a query from {from}", ms(now)),
                Some((id, name, end)) => {
                    let rule = rules.get(&name).copied();
                    println!("{} ms: query {name} id {id:#06x} from {from}", ms(now));
                    match rule {
                        Some(Rule::Silent) => {}
                        Some(Rule::Answer(a, delay)) if pending.len() < MAX_PENDING => pending
                            .push((
                                now + Duration::from_millis(delay),
                                answer(&buf[..n], end, Some(a)),
                                from,
                                name,
                            )),
                        Some(Rule::Answer(..)) => {
                            println!("{} ms: dropped, {MAX_PENDING} held", ms(now))
                        }
                        None => pending.push((now, answer(&buf[..n], end, None), from, name)),
                    }
                }
            }
        }
        let now = Instant::now();
        let mut i = 0;
        while i < pending.len() {
            if pending[i].0 <= now {
                let (_, reply, to, name) = pending.swap_remove(i);
                let _ = socket.send_to(&reply, to);
                let id = u16::from_be_bytes([reply[0], reply[1]]);
                let what = if reply[3] & 0x0f == 3 {
                    "no such name"
                } else {
                    "answer"
                };
                println!("{} ms: {what} {name} id {id:#06x}", ms(now));
            } else {
                i += 1;
            }
        }
    }
    println!("lifetime over");
    0
}

/// One frame of the capture, as far as the check needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Frame {
    ArpRequest {
        from: [u8; 4],
        target: [u8; 4],
    },
    ArpReply {
        from: [u8; 4],
    },
    DnsQuery {
        to: [u8; 4],
        name: String,
    },
    DnsAnswer {
        from: [u8; 4],
        name: String,
        addresses: Vec<[u8; 4]>,
    },
    EchoRequest {
        to: [u8; 4],
    },
    Syn {
        to: [u8; 4],
        port: u16,
    },
    Other,
}

fn ip(b: &[u8]) -> [u8; 4] {
    [b[0], b[1], b[2], b[3]]
}

/// The A records of an answer whose question ends at `end`.
fn a_records(m: &[u8], end: usize) -> Vec<[u8; 4]> {
    let count = u16::from_be_bytes([m[6], m[7]]);
    let mut at = end;
    let mut out = Vec::new();
    for _ in 0..count {
        // A compressed name (two bytes) or labels.
        if m.get(at).is_some_and(|b| b & 0xc0 == 0xc0) {
            at += 2;
        } else {
            while let Some(&n) = m.get(at) {
                at += 1 + n as usize;
                if n == 0 {
                    break;
                }
            }
        }
        let Some(rr) = m.get(at..at + 10) else { break };
        let len = u16::from_be_bytes([rr[8], rr[9]]) as usize;
        if rr[0..4] == [0, 1, 0, 1]
            && len == 4
            && let Some(a) = m.get(at + 10..at + 14)
        {
            out.push(ip(a));
        }
        at += 10 + len;
    }
    out
}

fn classify(f: &[u8], dns_port: u16) -> Frame {
    if f.len() < 14 {
        return Frame::Other;
    }
    match [f[12], f[13]] {
        [0x08, 0x06] if f.len() >= 42 => match [f[20], f[21]] {
            [0, 1] => Frame::ArpRequest {
                from: ip(&f[28..32]),
                target: ip(&f[38..42]),
            },
            [0, 2] => Frame::ArpReply {
                from: ip(&f[28..32]),
            },
            _ => Frame::Other,
        },
        [0x08, 0x00] if f.len() >= 34 => {
            let ihl = usize::from(f[14] & 0x0f) * 4;
            let (src, dst, proto) = (ip(&f[26..30]), ip(&f[30..34]), f[23]);
            let l4 = 14 + ihl;
            match proto {
                1 if f.get(l4) == Some(&8) => Frame::EchoRequest { to: dst },
                6 if f.len() >= l4 + 14 => {
                    let flags = f[l4 + 13];
                    if flags & 0x02 != 0 && flags & 0x10 == 0 {
                        Frame::Syn {
                            to: dst,
                            port: u16::from_be_bytes([f[l4 + 2], f[l4 + 3]]),
                        }
                    } else {
                        Frame::Other
                    }
                }
                17 if f.len() >= l4 + 8 + 12 => {
                    let sport = u16::from_be_bytes([f[l4], f[l4 + 1]]);
                    let dport = u16::from_be_bytes([f[l4 + 2], f[l4 + 3]]);
                    let m = &f[l4 + 8..];
                    // The question, read as a query's (the QR bit aside).
                    let mut q = m.to_vec();
                    q[2] &= 0x7f;
                    match (question(&q), dport == dns_port, sport == dns_port) {
                        (Some((_, name, _)), true, _) if m[2] & 0x80 == 0 => {
                            Frame::DnsQuery { to: dst, name }
                        }
                        (Some((_, name, end)), _, true) if m[2] & 0x80 != 0 => Frame::DnsAnswer {
                            from: src,
                            name,
                            addresses: a_records(m, end),
                        },
                        _ => Frame::Other,
                    }
                }
                _ => Frame::Other,
            }
        }
        _ => Frame::Other,
    }
}

/// The frames of a pcap file with their times in microseconds.
fn read_pcap(bytes: &[u8], dns_port: u16) -> Result<Vec<(u64, Frame)>, String> {
    let header = bytes.get(..24).ok_or("not a pcap file: too short")?;
    let le = match header[..4] {
        [0xd4, 0xc3, 0xb2, 0xa1] => true,
        [0xa1, 0xb2, 0xc3, 0xd4] => false,
        _ => return Err("not a pcap file (microsecond magic)".into()),
    };
    let word = |b: &[u8]| {
        let w = [b[0], b[1], b[2], b[3]];
        u64::from(if le {
            u32::from_le_bytes(w)
        } else {
            u32::from_be_bytes(w)
        })
    };
    let mut at = 24;
    let mut out = Vec::new();
    while let Some(rec) = bytes.get(at..at + 16) {
        let t = word(&rec[0..4]) * 1_000_000 + word(&rec[4..8]);
        let len = word(&rec[8..12]) as usize;
        let frame = bytes
            .get(at + 16..at + 16 + len)
            .ok_or("a record past the end")?;
        out.push((t, classify(frame, dns_port)));
        at += 16 + len;
    }
    Ok(out)
}

struct Check {
    server: [u8; 4],
    first: String,
    echo: Vec<String>,
    syn: Vec<String>,
}

/// The check of the module's comment: the lines to print, or the failure.
fn check(frames: &[(u64, Frame)], c: &Check) -> Result<Vec<String>, String> {
    let dotted = |a: [u8; 4]| format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]);
    let first_query = frames
        .iter()
        .position(|(_, f)| matches!(f, Frame::DnsQuery { .. }))
        .ok_or("no DNS query in the capture")?;
    let t0 = frames[first_query].0;
    let rel = |t: u64| (t as i64 - t0 as i64) as f64 / 1000.0;
    let mut lines = Vec::new();
    match &frames[first_query].1 {
        Frame::DnsQuery { to, name } if *to == c.server && *name == c.first => {}
        other => {
            return Err(format!(
                "the first DNS query is {other:?}, not {} to {}",
                c.first,
                dotted(c.server)
            ));
        }
    }
    let before = &frames[..first_query];
    let asked = before
        .iter()
        .filter(|(_, f)| matches!(f, Frame::ArpRequest { target, .. } if *target == c.server))
        .count();
    let answered = before
        .iter()
        .any(|(_, f)| matches!(f, Frame::ArpReply { from } if *from == c.server));
    for (t, f) in before {
        if matches!(f, Frame::ArpRequest { target, .. } if *target == c.server)
            || matches!(f, Frame::ArpReply { from } if *from == c.server)
        {
            lines.push(format!("{:+10.3} ms  {f:?}", rel(*t)));
        }
    }
    if asked != 1 || !answered {
        return Err(format!(
            "{asked} ARP request(s) for {} before the first query (one, answered, wanted; answered: {answered})",
            dotted(c.server)
        ));
    }
    let mut names: Vec<&String> = vec![&c.first];
    for n in c.echo.iter().chain(c.syn.iter()) {
        if !names.contains(&n) {
            names.push(n);
        }
    }
    for name in names {
        let queries: Vec<u64> = frames
            .iter()
            .filter(|(_, f)| matches!(f, Frame::DnsQuery { name: n, .. } if n == name))
            .map(|(t, _)| *t)
            .collect();
        let Some(at) = frames
            .iter()
            .position(|(_, f)| matches!(f, Frame::DnsAnswer { name: n, from, .. } if n == name && *from == c.server))
        else {
            return Err(format!("no answer for {name} in the capture"));
        };
        let (ta, Frame::DnsAnswer { addresses, .. }) = &frames[at] else {
            unreachable!()
        };
        lines.push(format!(
            "{name}: queries at {:?} ms, first answer at {:+.3} ms ({addresses:?})",
            queries.iter().map(|t| rel(*t)).collect::<Vec<_>>(),
            rel(*ta)
        ));
        let echo = c.echo.contains(name);
        let syn = c.syn.contains(name);
        if !echo && !syn {
            continue;
        }
        let follow = frames[at..].iter().find(|(_, f)| match f {
            Frame::EchoRequest { to } => echo && addresses.contains(to),
            Frame::Syn { to, .. } => syn && addresses.contains(to),
            _ => false,
        });
        let Some((tf, f)) = follow else {
            return Err(format!("nothing went to {name}'s address after its answer"));
        };
        let gap_ms = (tf - ta) / 1000;
        lines.push(format!(
            "{:+10.3} ms  {f:?}, {:.3} ms after the answer",
            rel(*tf),
            (tf - ta) as f64 / 1000.0
        ));
        if gap_ms > FOLLOW_MS {
            return Err(format!(
                "{name}: {f:?} left {gap_ms} ms after the answer (at most {FOLLOW_MS} wanted)"
            ));
        }
    }
    Ok(lines)
}

fn main() {
    let mut args = std::env::args().skip(1);
    let (mut port, mut lifetime, mut ready) = (None, 300u64, None);
    let mut rules = HashMap::new();
    let (mut pcap, mut server, mut first) = (None, None, None);
    let (mut echo, mut syn) = (Vec::new(), Vec::new());
    let mut dns_port = 53u16;
    let usage = "usage: zoraya-test-dns --port N [--lifetime S] [--ready-file F] [--answer NAME=A.B.C.D[@MS]]... [--silent NAME]... | --check-pcap FILE --server A.B.C.D [--server-port N] --first NAME [--then-echo NAME]... [--then-syn NAME]...";
    while let Some(a) = args.next() {
        let mut value = || args.next().unwrap_or_default();
        match a.as_str() {
            "--port" => port = value().parse::<u16>().ok(),
            "--lifetime" => lifetime = value().parse().unwrap_or(300),
            "--ready-file" => ready = Some(value()),
            "--answer" => match parse_rule(&value()) {
                Some((n, r)) => {
                    rules.insert(n, r);
                }
                None => {
                    eprintln!("{usage}");
                    std::process::exit(2);
                }
            },
            "--silent" => {
                rules.insert(value().to_ascii_lowercase(), Rule::Silent);
            }
            "--check-pcap" => pcap = Some(value()),
            "--server" => server = parse_ipv4(&value()),
            "--server-port" => dns_port = value().parse().unwrap_or(0),
            "--first" => first = Some(value().to_ascii_lowercase()),
            "--then-echo" => echo.push(value().to_ascii_lowercase()),
            "--then-syn" => syn.push(value().to_ascii_lowercase()),
            _ => {
                eprintln!("{usage}");
                std::process::exit(2);
            }
        }
    }
    if let Some(file) = pcap {
        let (Some(server), Some(first)) = (server, first) else {
            eprintln!("{usage}");
            std::process::exit(2);
        };
        let frames = match std::fs::read(&file)
            .map_err(|e| e.to_string())
            .and_then(|b| read_pcap(&b, dns_port))
        {
            Ok(f) => f,
            Err(e) => {
                println!("FAIL: {file}: {e}");
                std::process::exit(1);
            }
        };
        match check(
            &frames,
            &Check {
                server,
                first,
                echo,
                syn,
            },
        ) {
            Ok(lines) => {
                for l in lines {
                    println!("{l}");
                }
            }
            Err(e) => {
                println!("FAIL: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    let Some(port) = port else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    std::process::exit(serve(port, lifetime, ready, rules));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(id: u16, name: &str) -> Vec<u8> {
        let mut m = vec![(id >> 8) as u8, id as u8, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0];
        for l in name.split('.') {
            m.push(l.len() as u8);
            m.extend_from_slice(l.as_bytes());
        }
        m.extend_from_slice(&[0, 0, 1, 0, 1]);
        m
    }

    #[test]
    fn a_query_is_answered_with_its_question_and_one_a_record() {
        let q = query(0x1234, "Slow.Zoraya");
        let (id, name, end) = question(&q).unwrap();
        assert_eq!((id, name.as_str(), end), (0x1234, "slow.zoraya", q.len()));
        let a = answer(&q, end, Some([10, 0, 2, 2]));
        assert_eq!(&a[..2], &[0x12, 0x34]);
        assert_eq!(a[2] & 0x80, 0x80);
        assert_eq!(a_records(&a, end), vec![[10, 0, 2, 2]]);
        let nx = answer(&q, end, None);
        assert_eq!(nx[3] & 0x0f, 3);
        assert!(a_records(&nx, end).is_empty());
        // An answer is not a query; garbage is nothing.
        assert!(question(&a).is_none());
        assert!(question(&[0; 5]).is_none());
        assert!(question(&q[..q.len() - 1]).is_none());
    }

    #[test]
    fn rules_parse_with_and_without_a_delay() {
        assert_eq!(
            parse_rule("Slow.zoraya=10.0.2.2@3000"),
            Some(("slow.zoraya".into(), Rule::Answer([10, 0, 2, 2], 3000)))
        );
        assert_eq!(
            parse_rule("a=1.2.3.4"),
            Some(("a".into(), Rule::Answer([1, 2, 3, 4], 0)))
        );
        assert_eq!(parse_rule("a=1.2.3"), None);
        assert_eq!(parse_rule("a"), None);
    }

    fn c(first: &str) -> Check {
        Check {
            server: [10, 0, 2, 2],
            first: first.into(),
            echo: vec![first.into()],
            syn: Vec::new(),
        }
    }

    fn answer_frame(name: &str) -> Frame {
        Frame::DnsAnswer {
            from: [10, 0, 2, 2],
            name: name.into(),
            addresses: vec![[10, 0, 2, 2]],
        }
    }

    #[test]
    fn the_check_wants_one_arp_then_the_query_and_a_prompt_echo_request() {
        let server = [10, 0, 2, 2];
        let me = [10, 0, 2, 15];
        let good = vec![
            (
                0,
                Frame::ArpRequest {
                    from: me,
                    target: server,
                },
            ),
            (10, Frame::ArpReply { from: server }),
            (
                2_000_000,
                Frame::DnsQuery {
                    to: server,
                    name: "slow".into(),
                },
            ),
            (5_000_000, answer_frame("slow")),
            (5_000_300, Frame::EchoRequest { to: server }),
        ];
        assert!(check(&good, &c("slow")).is_ok());
        // The run waited for something else to wake netd.
        let mut late = good.clone();
        late[4].0 = 17_000_000;
        assert!(
            check(&late, &c("slow"))
                .unwrap_err()
                .contains("after the answer")
        );
        // Two ARP requests: the query waited for its own.
        let mut twice = good.clone();
        twice.insert(
            2,
            (
                1_000_000,
                Frame::ArpRequest {
                    from: me,
                    target: server,
                },
            ),
        );
        assert!(check(&twice, &c("slow")).unwrap_err().contains("2 ARP"));
        // Not the first lookup of the boot.
        let mut other = good.clone();
        other.insert(
            2,
            (
                1_500_000,
                Frame::DnsQuery {
                    to: server,
                    name: "x".into(),
                },
            ),
        );
        assert!(check(&other, &c("slow")).is_err());
        // Never answered.
        let unanswered: Vec<_> = good[..3].to_vec();
        assert!(
            check(&unanswered, &c("slow"))
                .unwrap_err()
                .contains("no answer")
        );
    }

    #[test]
    fn frames_are_classified_from_their_bytes() {
        let mut arp = vec![0xff; 6];
        arp.extend_from_slice(&[0x52, 0x54, 0, 0x12, 0x34, 0x56, 0x08, 0x06]);
        arp.extend_from_slice(&[0, 1, 8, 0, 6, 4, 0, 1]);
        arp.extend_from_slice(&[0x52, 0x54, 0, 0x12, 0x34, 0x56, 10, 0, 2, 15]);
        arp.extend_from_slice(&[0, 0, 0, 0, 0, 0, 10, 0, 2, 2]);
        assert_eq!(
            classify(&arp, 53),
            Frame::ArpRequest {
                from: [10, 0, 2, 15],
                target: [10, 0, 2, 2]
            }
        );
        let q = query(7, "slow.zoraya");
        let mut udp = vec![0; 12];
        udp.extend_from_slice(&[0x08, 0x00, 0x45, 0, 0, 0, 0, 0, 0, 0, 64, 17, 0, 0]);
        udp.extend_from_slice(&[10, 0, 2, 15, 10, 0, 2, 2]);
        udp.extend_from_slice(&[0x80, 0x00, 0, 53, 0, 0, 0, 0]);
        udp.extend_from_slice(&q);
        assert_eq!(
            classify(&udp, 53),
            Frame::DnsQuery {
                to: [10, 0, 2, 2],
                name: "slow.zoraya".into()
            }
        );
        assert_eq!(classify(&[0; 10], 53), Frame::Other);
    }
}
