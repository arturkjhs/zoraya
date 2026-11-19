//! The network connection a base-system command gets (ADR-0224 items 2 and
//! 5; N3): none, the status of the network (`needs net-status`), or `reach`
//! of the host and port of a `url` operand, as `(rights, scope)` for
//! `narrow`. Pure, so every rule is tested on the host.
//!
//! Only a *literal* `url` operand designates, as only a literal path does
//! (ADR-0153 rule 4): one that came from a variable, a substitution or a
//! glob would let data choose what the command reaches. Such an operand
//! designates only inside a `+net` rule on the line or inside the words of
//! the room it runs in (ADR-0462), and is refused before the line runs
//! otherwise. A room's words only bound a line: it gets a connection when its
//! own words or its description ask for one, inside them, and no status. A `+net` word is the person's, typed literally
//! (the token; its one argument may be expanded, ADR-0153 rule 3).

use alloc::vec::Vec;

use alloc::string::String;
use zoraya_abi::net::{
    ListenRule, MAX_RULES, Ports, Protocols, ReachRule, Scope, Target, encode, rights, valid_name,
};
use zoraya_command::Outcome;
use zoraya_describe::{Description, Kind};

use crate::grants::{Line, Refused, value_of};

/// Whether `w` is `any`, which is never saved (ADR-0397 item 3).
pub fn is_any(w: &NetWord) -> bool {
    w.target == Named::Any
}

/// A `+net` word: what it names, owned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetWord {
    target: Named,
    ports: Ports,
    protocols: Protocols,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Named {
    Name(String),
    Wildcard(String),
    Address([u8; 4]),
    Prefix([u8; 4], u8),
    Any,
    /// The web rule of a description (ADR-0324), never a `+net` word.
    Public,
    /// A URL operand's name under the web rule, on a port the web rule does
    /// not have: its public answers only (ADR-0328), never a `+net` word.
    PublicName(String),
}

impl NetWord {
    fn rule(&self) -> ReachRule<'_> {
        ReachRule {
            target: match &self.target {
                Named::Name(n) => Target::Name(n),
                Named::Wildcard(n) => Target::Wildcard(n),
                Named::Address(a) => Target::Address(*a),
                Named::Prefix(a, l) => Target::Prefix(*a, *l),
                Named::Any => Target::Any,
                Named::Public => Target::Public,
                Named::PublicName(n) => Target::PublicName(n),
            },
            ports: self.ports,
            protocols: self.protocols,
        }
    }
}

/// Reads the word after `+net` (ADR-0224 items 1 and 2): `any`, a host name,
/// `*.SUFFIX` (the names under it, not the suffix), a dotted address or
/// `ADDRESS/BITS`, each with an optional `:PORT` (without one, every port, as
/// the architect decided; one word covers TCP and UDP). Total: any text gives
/// a word or the reason.
pub fn parse_word(text: &str) -> Result<NetWord, String> {
    let (host, ports) = match text.rsplit_once(':') {
        Some((host, port)) => {
            let n: u32 = port
                .parse()
                .ok()
                .filter(|n| (1..=65535).contains(n))
                .ok_or_else(|| String::from("the port is not a number from 1 to 65535"))?;
            (host, Ports::one(n as u16))
        }
        None => (text, Ports::ANY),
    };
    let host = host.to_ascii_lowercase();
    let target = if host == "any" {
        Named::Any
    } else if let Some((address, bits)) = host.split_once('/') {
        let a = zoraya_http::parse_ipv4(address)
            .ok_or_else(|| String::from("not an address with a prefix length"))?;
        let bits: u8 = bits
            .parse()
            .ok()
            .filter(|b| *b <= 32)
            .ok_or_else(|| String::from("the prefix length is from 0 to 32"))?;
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - u32::from(bits))
        };
        if u32::from_be_bytes(a) & !mask != 0 {
            return Err(String::from("the address has bits past its prefix length"));
        }
        Named::Prefix(a, bits)
    } else if let Some(a) = zoraya_http::parse_ipv4(&host) {
        Named::Address(a)
    } else if let Some(suffix) = host.strip_prefix("*.") {
        if !valid_name(suffix) {
            return Err(String::from("not a host name after *."));
        }
        Named::Wildcard(String::from(suffix))
    } else if valid_name(&host) {
        Named::Name(host)
    } else {
        return Err(String::from("not a host name, an address or any"));
    };
    Ok(NetWord {
        target,
        ports,
        protocols: Protocols::ALL,
    })
}

/// Reads the word after `+listen` (ADR-0224 item 1): a port, or `LOW-HIGH`
/// ports, 1 to 65535, for TCP and UDP. Total: any text gives ports or the
/// reason. No `any`: a listener on every port is not one word of the line.
pub fn parse_listen(text: &str) -> Result<Ports, String> {
    let port = |t: &str| {
        t.parse::<u32>()
            .ok()
            .filter(|n| (1..=65535).contains(n))
            .map(|n| n as u16)
            .ok_or_else(|| String::from("a port is a number from 1 to 65535"))
    };
    match text.split_once('-') {
        Some((low, high)) => {
            let (low, high) = (port(low)?, port(high)?);
            if low > high {
                return Err(String::from(
                    "the range runs from the lower port to the higher",
                ));
            }
            Ports::ranges(&[(low, high)]).ok_or_else(|| String::from("not a range of ports"))
        }
        None => Ok(Ports::one(port(text)?)),
    }
}

/// The rights and scope to `narrow` the shell's connection to, if the
/// command gets a connection at all: the line's `+net` words (`nets`, their
/// values as written after the token), then the `url` operand of a
/// base-system command. A URL that does not parse gives none of its own: the
/// command says what is wrong with it.
///
/// Under the description's `net web` (ADR-0328, INV-0328-1), a literal URL
/// whose host is a name gives no rule that names the name: on the web rule's
/// ports (80, 443) it gives none (the web rule admits the name through its
/// public answers), on another port a public-only name rule
/// ([`Target::PublicName`]). For a POSIX program of the image without
/// `net web` (curl, wget) the URL's name gives a public-only name rule on its
/// port, 80 and 443 included (the architect, 2026-11-14). A dotted address keeps its own rule (the person
/// typed the address, which does not rebind), and a `+net` word that covers
/// the URL keeps ADR-0223's rule (the person named the host).
pub fn net_for(line: &Line<'_>) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    let d = line.description;
    let mut words: Vec<NetWord> = Vec::new();
    // The room's words (ADR-0462): the bound, never a grant of their own. A
    // word of the room that does not parse is the room's line's to report
    // (the reader checks the form); it bounds nothing here.
    let room_words: Vec<NetWord> = line
        .room
        .map(|r| r.words.iter().filter_map(|w| parse_word(w).ok()).collect())
        .unwrap_or_default();
    for v in line.nets {
        match parse_word(&v.text) {
            Ok(w) => words.push(w),
            Err(problem) => {
                return Err(Refused {
                    subject: alloc::format!("+net {}", v.text),
                    problem,
                    hint: Some(String::from(
                        "write a host, *.suffix, an address, ADDRESS/BITS or any, with :PORT if one port",
                    )),
                });
            }
        }
    }
    let operand = if line.base_system {
        line.parsed
            .operands
            .iter()
            .find(|(i, _)| matches!(d.operands[*i].kind, Kind::Url | Kind::Host))
            .map(|(i, text)| (d.operands[*i].kind, *text))
    } else {
        None
    };
    if let Some((Kind::Host, text)) = operand {
        // `reach` (ADR-0340 item 4): TCP to the host on the description's
        // port or the one its option gives, instead of an echo.
        let tcp = d
            .reach
            .filter(|_| line.base_system)
            .map(|r| reach_port(line, &r));
        host_operand(line, text, &mut words, tcp, &room_words)?;
    }
    let operand = operand
        .filter(|(kind, _)| *kind == Kind::Url)
        .map(|(_, t)| t);
    if let Some(text) = operand {
        let literal = value_of(line.args, text).is_some_and(|v| v.literal);
        if let Ok(url) = zoraya_http::parse_url(text) {
            let host = url.host.to_ascii_lowercase();
            let named = match zoraya_http::parse_ipv4(&host) {
                Some(a) => Named::Address(a),
                None => Named::Name(host.clone()),
            };
            let word = NetWord {
                target: named,
                ports: Ports::one(url.port),
                protocols: Protocols::TCP,
            };
            let covered = words.iter().any(|outer| word.rule().within(&outer.rule()));
            let in_room = room_words
                .iter()
                .any(|outer| word.rule().within(&outer.rule()));
            // A value designates only inside a `+net` rule of the line or of
            // its room (ADR-0224 item 2); a literal one adds its own.
            if !literal && !covered && !in_room {
                return Err(Refused {
                    subject: (*text).into(),
                    problem: "from a value, outside the line's network grants".into(),
                    hint: Some(alloc::format!(
                        "if that is meant: +net {host}:{} COMMAND",
                        url.port
                    )),
                });
            }
            if !covered {
                let web = d.net_web && line.base_system;
                // A POSIX program of the image (curl, wget, NetSurf, the
                // test programs) takes a URL's name with its public answers
                // only, on any port: the architect's decision of 2026-11-14
                // on ADR-0328's open question. The native `fetch` keeps
                // ADR-0224's name rule.
                let posix = line.base_system && d.program.is_some();
                match word.target {
                    Named::Name(name) if web || posix => {
                        if !(web && web_port(url.port)) {
                            words.push(NetWord {
                                target: Named::PublicName(name),
                                ..word
                            });
                        }
                    }
                    _ => words.push(word),
                }
            }
        } else if !literal && words.is_empty() {
            return Err(Refused {
                subject: (*text).into(),
                problem: "from a value, outside the line's network grants".into(),
                hint: Some(
                    "write the address on the line itself, or give it with +net HOST".into(),
                ),
            });
        }
    }
    // The web rule a description of the image asks for (ADR-0324 items 1
    // and 3): public hosts on ports 80 and 443, TCP; beside the line's
    // words, never a word that would let a value designate.
    if d.net_web && line.base_system {
        words.push(web_word());
    }
    // In a room every rule the line asks for lies inside the room's words
    // (ADR-0224 item 2, ADR-0462): refused before anything runs, naming them.
    if let Some(room) = line.room {
        for w in &words {
            // The loopback is the room's own (`netd` keys it by room, ADR-0442): a
            // rule on 127.0.0.0/8 asks nothing of the room's words.
            if is_loopback(w) {
                continue;
            }
            if !room_words.iter().any(|r| w.rule().within(&r.rule())) {
                let held = if room.words.is_empty() {
                    alloc::format!("room {} has no +net words", room.name)
                } else {
                    alloc::format!(
                        "room {} holds {}",
                        room.name,
                        room.words
                            .iter()
                            .map(|w| alloc::format!("+net {w}"))
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                return Err(Refused {
                    subject: describe_word(w),
                    problem: alloc::format!("outside the network words of room {}", room.name),
                    hint: Some(held),
                });
            }
        }
    }
    let rules: Vec<ReachRule<'_>> = words.iter().map(NetWord::rule).collect();
    // `+listen` words (ADR-0224 item 1): the person's own, a literal token and
    // its one word; they make listen rules and the `listen` right, and
    // nothing else (a listener needs no reach).
    let mut listens: Vec<ListenRule> = Vec::new();
    for v in line.listens {
        match parse_listen(&v.text) {
            Ok(ports) => listens.push(ListenRule {
                ports,
                protocols: Protocols::BOTH,
            }),
            Err(problem) => {
                return Err(Refused {
                    subject: alloc::format!("+listen {}", v.text),
                    problem,
                    hint: Some(String::from("write a port, or LOW-HIGH ports")),
                });
            }
        }
    }
    if rules.is_empty() && listens.is_empty() {
        if d.needs_net_status && line.base_system {
            // ADR-0462 item 5: no status in a room's terminal (a window's
            // connection has none, and the first terminal's would be
            // `init`'s): refused, not run with the wrong network.
            if line.room.is_some() {
                return Err(Refused {
                    subject: d.name.clone(),
                    problem: "the network's status is not given in a room".into(),
                    hint: Some("room --leave first, or use the first terminal at home".into()),
                });
            }
            return Ok(Some((rights::STATUS, Scope::EMPTY.as_bytes().to_vec())));
        }
        return Ok(None);
    }
    if rules.len() + listens.len() > MAX_RULES {
        return Err(Refused {
            subject: "+net".into(),
            problem: "too many network rules on one line".into(),
            hint: None,
        });
    }
    let mut out = alloc::vec![0u8; 512 + 128 * (rules.len() + listens.len())];
    let granted = (if rules.is_empty() { 0 } else { rights::REACH })
        | (if listens.is_empty() {
            0
        } else {
            rights::LISTEN
        });
    Ok(encode(&rules, &listens, &mut out).map(|n| {
        out.truncate(n);
        (granted, out)
    }))
}

/// Whether a word names the loopback only (an address or a prefix inside 127.0.0.0/8).
fn is_loopback(w: &NetWord) -> bool {
    match w.target {
        Named::Address(a) => a[0] == 127,
        Named::Prefix(a, l) => a[0] == 127 && l >= 8,
        _ => false,
    }
}

/// What a refused connection of a line is told (ADR-0462, the carved-account
/// limit): the problem and the hint, for `netd`'s `OVER_QUOTA` or another
/// refusal (a scope wider than the room's connection).
pub fn connection_refusal(over_quota: bool) -> (&'static str, &'static str) {
    if over_quota {
        (
            "the network has no room for another program's connection",
            "programs that use the network hold one each, a few at once: wait for one to end (jobs)",
        )
    } else {
        (
            "the network service refused this program's connection",
            "the room's +net words (room) bound what a line may reach",
        )
    }
}

/// The scope a room's terminal is given: the words that parse, none of the
/// others (a hand edit of `/config/rooms` is said once by the shell and does
/// not take the good words with it). `None` for no good word.
pub fn room_scope(words: &[String]) -> Option<(u32, Vec<u8>)> {
    let good: Vec<String> = words
        .iter()
        .filter(|w| parse_word(w).is_ok())
        .cloned()
        .collect();
    words_scope(&good).ok().flatten()
}

/// Whether `word` (a `+net` argument) lies inside one of `outer`: what a room
/// made from a room may be given (INV-0172-4), by the rules' `within`, not by
/// the text (a room with `*.example.org` gives `api.example.org:22`). A word
/// that does not parse is inside nothing.
pub fn word_within(word: &str, outer: &[String]) -> bool {
    let Ok(w) = parse_word(word) else {
        return false;
    };
    outer
        .iter()
        .filter_map(|o| parse_word(o).ok())
        .any(|o| w.rule().within(&o.rule()))
}

/// A word as the person writes it, for a refusal: `HOST:PORT`, `any`, a prefix.
fn describe_word(w: &NetWord) -> String {
    let host = match &w.target {
        Named::Name(n) | Named::PublicName(n) => n.clone(),
        Named::Wildcard(n) => alloc::format!("*.{n}"),
        Named::Address(a) => alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]),
        Named::Prefix(a, l) => alloc::format!("{}.{}.{}.{}/{l}", a[0], a[1], a[2], a[3]),
        Named::Any => String::from("any"),
        Named::Public => String::from("web"),
    };
    if w.ports.is_any() {
        return host;
    }
    let ports: Vec<String> = w
        .ports
        .list()
        .iter()
        .map(|&(l, h)| {
            if l == h {
                alloc::format!("{l}")
            } else {
                alloc::format!("{l}-{h}")
            }
        })
        .collect();
    alloc::format!("{host}:{}", ports.join(","))
}

/// The rights and scope of a room's `+net` words (ADR-0462): what the network
/// connection of the room's terminals reaches, for `narrow` from the root.
/// `None` for no word; an `Err` names the first word that is no `+net`
/// argument. Names have local answers too, as a `+net` word's do on a line
/// (ADR-0223: the person named the host).
pub fn words_scope(words: &[String]) -> Result<Option<(u32, Vec<u8>)>, String> {
    let mut parsed: Vec<NetWord> = Vec::new();
    for w in words {
        parsed.push(parse_word(w).map_err(|why| alloc::format!("+net {w}: {why}"))?);
    }
    let rules: Vec<ReachRule<'_>> = parsed.iter().map(NetWord::rule).collect();
    if rules.is_empty() {
        return Ok(None);
    }
    if rules.len() > MAX_RULES {
        return Err(String::from("too many network rules"));
    }
    let mut out = alloc::vec![0u8; 512 + 128 * rules.len()];
    Ok(encode(&rules, &[], &mut out).map(|n| {
        out.truncate(n);
        (rights::REACH, out)
    }))
}

/// The web rule's word (ADR-0324 items 1 and 3): public hosts on ports 80 and
/// 443, TCP.
fn web_word() -> NetWord {
    NetWord {
        target: Named::Public,
        ports: Ports::ranges(&WEB_PORTS).unwrap_or(Ports::one(443)),
        protocols: Protocols::TCP,
    }
}

/// What a program's saved network words give (ADR-0397 item 5).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SavedNet {
    /// For `narrow` from the shell's own connection, `None` for no word that
    /// holds here.
    pub scope: Option<(u32, Vec<u8>)>,
    /// The words that were left out, as written (`+net host:443`, `+web`):
    /// one that does not parse, `any`, or one outside the room's words.
    pub dropped: Vec<String>,
}

/// The scope of an installed program's saved `+net` words and `+web`
/// (ADR-0397 items 1 and 5), the words the shell holds here only: in a room
/// (`room` is the room's `+net` words) each lies inside them, as a line's
/// do (INV-0172-4, ADR-0462); outside a room the shell's own connection is
/// the bound and `narrow` refuses what is wider. `+net any` is never kept.
pub fn saved_scope(words: &[String], web: bool, room: Option<&[String]>) -> SavedNet {
    let inside = |w: &NetWord| {
        room.is_none_or(|outer| {
            is_loopback(w)
                || outer
                    .iter()
                    .filter_map(|o| parse_word(o).ok())
                    .any(|o| w.rule().within(&o.rule()))
        })
    };
    let mut out = SavedNet::default();
    let mut parsed: Vec<NetWord> = Vec::new();
    for w in words {
        match parse_word(w) {
            Ok(n) if n.target != Named::Any && inside(&n) => parsed.push(n),
            _ => out.dropped.push(alloc::format!("+net {w}")),
        }
    }
    if web {
        let n = web_word();
        if inside(&n) {
            parsed.push(n);
        } else {
            out.dropped.push(String::from("+web"));
        }
    }
    let rules: Vec<ReachRule<'_>> = parsed.iter().map(NetWord::rule).collect();
    if rules.is_empty() {
        return out;
    }
    if rules.len() > MAX_RULES {
        out.dropped.push(String::from("too many network rules"));
        return out;
    }
    let mut buf = alloc::vec![0u8; 512 + 128 * rules.len()];
    out.scope = encode(&rules, &[], &mut buf).map(|n| {
        buf.truncate(n);
        (rights::REACH, buf)
    });
    out
}

/// The rule a `host` operand adds (ADR-0330): the host, a name or a dotted
/// address, for ICMP echo requests and nothing else: no TCP, no UDP, no
/// port. Designation as for a `url`: a literal operand adds its own rule, one
/// that came from a value only designates inside a `+net` word of the line.
/// A `+net HOST:PORT` word does not cover this rule (it has a port, the echo
/// none): the person writes `+net HOST` (ADR-0330, known limits).
fn host_operand(
    line: &Line<'_>,
    text: &str,
    words: &mut Vec<NetWord>,
    tcp: Option<Option<u16>>,
    room_words: &[NetWord],
) -> Result<(), Refused> {
    let literal = value_of(line.args, text).is_some_and(|v| v.literal);
    // OpenSSH also reads `ssh://[USER@]HOST[:PORT]`, a form that takes user
    // and port from the word itself; the grant is made from what is written
    // as a host, so the form is a usage mistake (ADR-0340 item 4).
    if tcp.is_some() && text.starts_with("ssh://") {
        return Err(Refused {
            subject: text.into(),
            problem: "a URI is not a host: ssh takes HOST or USER@HOST".into(),
            hint: Some("write the host, with -l USER and -p PORT".into()),
        });
    }
    // `USER@HOST`: the host is what follows the last `@` (ADR-0340 item 5).
    let host = if tcp.is_some() {
        text.rsplit_once('@').map_or(text, |(_, h)| h)
    } else {
        text
    }
    .to_ascii_lowercase();
    // A port that is no number from 1 to 65535 gives nothing: the program
    // says what is wrong with it.
    let port = match tcp {
        Some(None) => return Ok(()),
        other => other.flatten(),
    };
    let target = match zoraya_http::parse_ipv4(&host) {
        Some(a) => Named::Address(a),
        None if valid_name(&host) => Named::Name(host.clone()),
        // The command says what is wrong with it.
        None => return Ok(()),
    };
    let word = match port {
        Some(port) => NetWord {
            target,
            ports: Ports::one(port),
            protocols: Protocols::TCP,
        },
        None => NetWord {
            target,
            ports: Ports::ANY,
            protocols: Protocols::ECHO,
        },
    };
    let covered = words.iter().any(|outer| word.rule().within(&outer.rule()));
    if covered {
        return Ok(());
    }
    let in_room = room_words
        .iter()
        .any(|outer| word.rule().within(&outer.rule()));
    if !literal && !in_room {
        let hint = match port {
            Some(port) => alloc::format!("if that is meant: +net {host}:{port} COMMAND"),
            None => alloc::format!("if that is meant: +net {host} COMMAND"),
        };
        return Err(Refused {
            subject: text.into(),
            problem: "from a value, outside the line's network grants".into(),
            hint: Some(hint),
        });
    }
    // A program of the image takes a name with its public answers only, as
    // for a URL (the architect's decision of 2026-11-14); an address keeps
    // its own rule.
    let word = match word.target {
        Named::Name(name) if port.is_some() && line.description.program.is_some() => NetWord {
            target: Named::PublicName(name),
            ..word
        },
        _ => word,
    };
    words.push(word);
    Ok(())
}

/// The port a `reach` line gives this line: the option's value when the line
/// has the option, else the description's. `None` when the option's value is
/// no number from 1 to 65535 (the command refuses it itself).
fn reach_port(line: &Line<'_>, reach: &zoraya_describe::Reach) -> Option<u16> {
    let given = reach.option.and_then(|o| {
        line.parsed
            .options
            .iter()
            // The first, as OpenSSH's own option reading takes it.
            .find(|(i, _)| *i == o)
            .and_then(|(_, v)| *v)
    });
    match given {
        None => Some(reach.port),
        Some(v) => v.parse::<u16>().ok().filter(|p| *p != 0),
    }
}

/// Whether a line for a POSIX program may run (ADR-0317 item 6, INV-0312-1).
/// A program with a window (its description has `window`) runs only a line its
/// description parses: an option it does not give (`netsurf --ca_bundle=FILE`)
/// is the program's own and could replace the roots, and a line that does not
/// parse would fall back to no options and still keep the network of the
/// `+net` words. `--help` alone is the program's to answer. A program without
/// a window keeps the default-settings rule for a line that does not parse.
pub fn line_may_run(d: &Description, args: &[&str]) -> bool {
    if d.window.is_none() {
        return true;
    }
    match zoraya_command::parse(d, args) {
        Ok(Outcome::Run(_)) => true,
        Ok(Outcome::Help) => args.len() == 1,
        Err(_) => false,
    }
}

/// The window's pills for a scope (ADR-0317 item 3): `+net TARGET` for each
/// rule, with the target as the first word of its `describe` line after
/// `net: `, and `web` for the web rule (ADR-0324 item 6; not `+net web`,
/// which would be a host named `web`).
pub fn pills(scope: &[u8]) -> Vec<String> {
    let Some(decoded) = Scope::decode(scope) else {
        return Vec::new();
    };
    decoded
        .reach_rules()
        .zip(describe(scope))
        .filter_map(|(rule, line)| {
            if rule.target == Target::Public {
                return Some(String::from(WEB_PILL));
            }
            let target = line.trim_start_matches("net: ").split_whitespace().next()?;
            // A public-only name (ADR-0328) carries the mark `public`: a
            // bare `+net HOST:PORT` would read as the word that takes local
            // answers. The panel takes a pill that starts with `+net ` and
            // `web` alone (ADR-0324 item 6), so the mark follows the target.
            if matches!(rule.target, Target::PublicName(_)) {
                return Some(alloc::format!("+net {target} {PUBLIC_MARK}"));
            }
            Some(alloc::format!("+net {target}"))
        })
        .collect()
}

/// The pill of the web rule.
pub const WEB_PILL: &str = "web";

/// The mark after a public-only name's pill (ADR-0328).
pub const PUBLIC_MARK: &str = "public";

/// The ports of the web rule (ADR-0324 item 3).
pub const WEB_PORTS: [(u16, u16); 2] = [(80, 80), (443, 443)];

/// Whether the web rule has `port` (ADR-0328: a URL's name on it needs no
/// rule of its own).
fn web_port(port: u16) -> bool {
    WEB_PORTS.iter().any(|&(l, h)| l <= port && port <= h)
}

/// Whether a line may have the web rule its description asks for
/// (ADR-0324 item 7, INV-0324-3): never in an untrusted room. This refusal
/// is the web rule's own and stays after the gate's second stage opens; the
/// shell calls it wherever it makes a POSIX program's or a command's network
/// scope (`untrusted-gate: web-refusal-call`).
pub fn web_refused(d: &Description, untrusted: bool) -> Result<(), Refused> {
    if d.net_web && untrusted {
        // untrusted-gate: web-refusal (ADR-0324 item 7, `make check-untrusted-gate`)
        return Err(Refused {
            subject: d.name.clone(),
            problem: "its description gives it the web (public hosts on ports 80 and 443), which an untrusted room does not give".into(),
            hint: Some(String::from(
                "ADR-0324: in an untrusted room a program reaches only the room's own network words",
            )),
        });
    }
    Ok(())
}

/// The lines `grants` prints for the scope `net_for` made (ADR-0224 item 3),
/// one per reach rule: `net: example.org:443    reach (operand URL)`, or
/// `net: example.org         reach, any port (grant +net)`. A rule over TCP
/// alone is the `url` operand's; one over both protocols is a `+net` word's.
/// A public-only name says so: `net: example.org:8080    reach, public
/// answers only (operand URL)` (ADR-0328).
/// A scope that does not decode (not made here) gives none.
pub fn describe(scope: &[u8]) -> Vec<String> {
    let Some(scope) = Scope::decode(scope) else {
        return Vec::new();
    };
    let listening: Vec<String> = scope
        .listen_rules()
        .map(|rule| listen_line(&rule))
        .collect();
    let reaching: Vec<String> = scope
        .reach_rules()
        .map(|rule| {
            let host = match rule.target {
                Target::Name(n) | Target::PublicName(n) => String::from(n),
                Target::Wildcard(n) => alloc::format!("*.{n}"),
                Target::Address(a) => alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3]),
                Target::Prefix(a, l) => alloc::format!("{}.{}.{}.{}/{l}", a[0], a[1], a[2], a[3]),
                Target::Any => String::from("any"),
                Target::Public => {
                    // Every range whole, and `any port` (9f-net review,
                    // note 9): a narrowed web scope is a valid input.
                    let ports = if rule.ports.is_any() {
                        String::from("any port")
                    } else {
                        let list: Vec<String> = rule
                            .ports
                            .list()
                            .iter()
                            .map(|&(l, h)| {
                                if l == h {
                                    alloc::format!("{l}")
                                } else {
                                    alloc::format!("{l}-{h}")
                                }
                            })
                            .collect();
                        let word = if list.len() == 1 && !list[0].contains('-') {
                            "port"
                        } else {
                            "ports"
                        };
                        let joined = match list.split_last() {
                            Some((last, rest)) if !rest.is_empty() => {
                                alloc::format!("{} and {last}", rest.join(", "))
                            }
                            _ => list.concat(),
                        };
                        alloc::format!("{word} {joined}")
                    };
                    return alloc::format!(
                        "net: {:<24}reach public hosts, {ports}; not this machine or its network (description)",
                        "web",
                    );
                }
            };
            let public = matches!(rule.target, Target::PublicName(_));
            let (shown, access) = if rule.protocols == Protocols::ECHO {
                (host, "echo only")
            } else if rule.ports.is_any() {
                let access = if public {
                    "reach, any port, public answers only"
                } else {
                    "reach, any port"
                };
                (host, access)
            } else {
                let ports: Vec<String> = rule
                    .ports
                    .list()
                    .iter()
                    .map(|&(l, h)| {
                        if l == h {
                            alloc::format!("{l}")
                        } else {
                            alloc::format!("{l}-{h}")
                        }
                    })
                    .collect();
                let access = if public {
                    "reach, public answers only"
                } else {
                    "reach"
                };
                (alloc::format!("{host}:{}", ports.join(",")), access)
            };
            let source = if rule.protocols == Protocols::ECHO {
                "operand HOST"
            } else if rule.protocols == Protocols::TCP {
                "operand URL"
            } else {
                "grant +net"
            };
            alloc::format!("net: {shown:<23} {access} ({source})")
        })
        .collect();
    // The listen rules after the reach rules, so that `pills` still pairs
    // each reach rule with its line.
    reaching.into_iter().chain(listening).collect()
}

/// `net: port 8000             listen (grant +listen)`.
fn listen_line(rule: &ListenRule) -> String {
    let list: Vec<String> = rule
        .ports
        .list()
        .iter()
        .map(|&(l, h)| {
            if l == h {
                alloc::format!("{l}")
            } else {
                alloc::format!("{l}-{h}")
            }
        })
        .collect();
    let word = if list.len() == 1 && !list[0].contains('-') {
        "port"
    } else {
        "ports"
    };
    let shown = if rule.ports.is_any() {
        String::from("any port")
    } else {
        alloc::format!("{word} {}", list.join(","))
    };
    alloc::format!("net: {shown:<23} listen (grant +listen)")
}

/// The launcher's root connection, as [`available`] sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Root {
    /// None held.
    None,
    /// Its far end is closed: the service ended.
    Closed,
    Open,
}

/// Whether a line that needs the network may run (ADR-0221 items 5 and 6,
/// ADR-0296 item 3), or the refusal's problem. `told` says the launcher has
/// the services channel, so `init` runs a network for it: with no usable
/// root such a line is refused, never run without a connection (N6 review,
/// finding 7). `down` is `NetworkDown` received: no restart comes.
pub fn available(told: bool, root: Root, down: bool) -> Result<(), &'static str> {
    match root {
        Root::Open => Ok(()),
        Root::Closed if !down => Err("the network is not available (restarting)"),
        Root::Closed => Err("the network is not available"),
        Root::None if told => Err("the network is not available"),
        Root::None => Ok(()),
    }
}
