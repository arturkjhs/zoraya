//! INV-0220-3 and INV-0220-4: the scope's decoder, round trip, matcher and
//! subset relation.

use std::vec;
use std::vec::Vec;

use super::net::*;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

const NAMES: [&str; 6] = [
    "example.org",
    "www.example.org",
    "a.b.example.org",
    "other.example",
    "org",
    "x",
];
const SUFFIXES: [&str; 4] = ["example.org", "b.example.org", "org", "example"];
const ADDRESSES: [[u8; 4]; 6] = [
    [10, 0, 2, 2],
    [10, 0, 2, 15],
    [10, 1, 0, 1],
    [192, 168, 0, 1],
    [203, 0, 113, 10],
    [93, 184, 215, 14],
];

fn random_target(rng: &mut Rng) -> Target<'static> {
    match rng.below(6) {
        0 => Target::Name(NAMES[rng.below(6) as usize]),
        1 => Target::Wildcard(SUFFIXES[rng.below(4) as usize]),
        2 => Target::Address(ADDRESSES[rng.below(6) as usize]),
        3 => {
            let len = [0u8, 8, 16, 24, 30, 32][rng.below(6) as usize];
            let a = ADDRESSES[rng.below(6) as usize];
            let m = u32::from_be_bytes(a) & if len == 0 { 0 } else { u32::MAX << (32 - len) };
            Target::Prefix(m.to_be_bytes(), len)
        }
        4 => Target::Public,
        _ => Target::Any,
    }
}

fn random_ports(rng: &mut Rng) -> Ports {
    match rng.below(4) {
        0 => Ports::ANY,
        1 => Ports::one([22, 80, 443, 8080][rng.below(4) as usize]),
        2 => Ports::ranges(&[(1, 1023)]).unwrap(),
        _ => Ports::ranges(&[(80, 90), (85, 100), (443, 443)]).unwrap(),
    }
}

fn random_protocols(rng: &mut Rng) -> Protocols {
    Protocols::from_raw(1 + rng.below(3) as u8).unwrap()
}

fn random_scope(rng: &mut Rng) -> Vec<u8> {
    let reach: Vec<ReachRule<'static>> = (0..rng.below(4))
        .map(|_| {
            let target = random_target(rng);
            let ports = random_ports(rng);
            let protocols = random_protocols(rng);
            // A web rule is TCP alone (INV-0324-1); the decoder refuses others.
            let protocols = if target == Target::Public {
                Protocols::TCP
            } else {
                protocols
            };
            ReachRule {
                target,
                ports,
                protocols,
            }
        })
        .collect();
    let listen: Vec<ListenRule> = (0..rng.below(3))
        .map(|_| ListenRule {
            ports: random_ports(rng),
            protocols: random_protocols(rng),
        })
        .collect();
    let mut out = vec![0u8; 4096];
    let n = encode(&reach, &listen, &mut out).expect("a valid scope encodes");
    out.truncate(n);
    out
}

fn re_encode(scope: &Scope<'_>) -> Vec<u8> {
    let reach: Vec<_> = scope.reach_rules().collect();
    let listen: Vec<_> = scope.listen_rules().collect();
    let mut out = vec![0u8; 4096];
    let n = encode(&reach, &listen, &mut out).expect("a decoded scope encodes");
    out.truncate(n);
    out
}

#[test]
fn a_scope_that_encodes_then_decodes_is_equal() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    for _ in 0..500 {
        let bytes = random_scope(&mut rng);
        let scope = Scope::decode(&bytes).expect("decodes");
        assert_eq!(re_encode(&scope), bytes);
        assert_eq!(Scope::decode(&re_encode(&scope)), Some(scope));
    }
}

#[test]
fn the_decoder_is_total_over_random_bytes_truncations_and_mutations() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..300 {
        let bytes = random_scope(&mut rng);
        // Every proper prefix is refused: the counts say how much follows.
        for end in 0..bytes.len() {
            assert!(Scope::decode(&bytes[..end]).is_none(), "prefix {end}");
        }
        // A byte changed: no panic; if it decodes, it round-trips and its
        // rules iterate in full.
        for _ in 0..40 {
            let mut m = bytes.clone();
            let at = rng.below(m.len() as u64) as usize;
            m[at] = rng.next() as u8;
            if let Some(s) = Scope::decode(&m) {
                assert_eq!(re_encode(&s), m);
                assert_eq!(s.reach_rules().count(), s.reach_count());
                assert_eq!(s.listen_rules().count(), s.listen_count());
            }
        }
        // Trailing bytes are refused.
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(Scope::decode(&longer).is_none());
    }
    for _ in 0..5000 {
        let n = rng.below(80) as usize;
        let junk: Vec<u8> = (0..n).map(|_| rng.next() as u8).collect();
        let _ = Scope::decode(&junk);
        let mut head = vec![1, rng.below(3) as u8, rng.below(3) as u8, 0];
        head.extend(junk);
        let _ = Scope::decode(&head);
    }
    assert!(Scope::decode(&[]).is_none());
    assert!(Scope::decode(&vec![0u8; MAX_SCOPE_BYTES + 1]).is_none());
}

#[test]
fn the_empty_scope_decodes_and_allows_nothing() {
    let s = Scope::decode(Scope::EMPTY.as_bytes()).unwrap();
    assert!(s.is_empty());
    assert!(!s.allows_name("example.org", 80, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
    assert!(!s.allows_listen(80, Protocols::TCP));
    assert!(s.within(&s));
}

fn one(target: Target<'_>, ports: Ports, protocols: Protocols) -> Vec<u8> {
    let mut out = vec![0u8; 512];
    let rule = ReachRule {
        target,
        ports,
        protocols,
    };
    let n = encode(&[rule], &[], &mut out).unwrap();
    out.truncate(n);
    out
}

#[test]
fn names_are_ascii_lower_case_and_within_dns_limits() {
    for bad in [
        "Example.org",
        "exämple.org",
        "",
        ".",
        "a..b",
        ".a",
        "a.",
        "a b",
        "a/b",
        "10.0.2.2",
        "example.123",
        "1",
    ] {
        assert!(!valid_name(bad), "{bad:?}");
        let mut out = vec![0u8; 512];
        let rule = ReachRule {
            target: Target::Name(bad),
            ports: Ports::ANY,
            protocols: Protocols::TCP,
        };
        assert!(encode(&[rule], &[], &mut out).is_none(), "{bad:?}");
    }
    assert!(valid_name("a-b_c.example.org"));
    assert!(valid_name("1password.com"));
    let label = "a".repeat(63);
    assert!(valid_name(&label));
    assert!(!valid_name(&"a".repeat(64)));
    let long = std::format!("{label}.{label}.{label}.{}", "b".repeat(61));
    assert_eq!(long.len(), 253);
    assert!(valid_name(&long));
    assert!(!valid_name(&std::format!("{long}c")));
    // The decoder refuses a hand-written upper-case name too.
    let mut bytes = one(Target::Name("example.org"), Ports::ANY, Protocols::TCP);
    bytes[6] = b'E';
    assert!(Scope::decode(&bytes).is_none());
}

#[test]
fn a_prefix_has_zero_host_bits_and_a_length_to_32() {
    let mut out = vec![0u8; 64];
    let bad = ReachRule {
        target: Target::Prefix([10, 0, 2, 1], 24),
        ports: Ports::ANY,
        protocols: Protocols::TCP,
    };
    assert!(encode(&[bad], &[], &mut out).is_none());
    let bad = ReachRule {
        target: Target::Prefix([0, 0, 0, 0], 33),
        ports: Ports::ANY,
        protocols: Protocols::TCP,
    };
    assert!(encode(&[bad], &[], &mut out).is_none());
}

#[test]
fn ports_and_protocols_are_checked() {
    assert!(Ports::ranges(&[]).is_none());
    assert!(Ports::ranges(&[(0, 5)]).is_none());
    assert!(Ports::ranges(&[(6, 5)]).is_none());
    assert!(Ports::ranges(&[(1, 1); 9]).is_none());
    assert!(Protocols::from_raw(0).is_none());
    assert!(Protocols::from_raw(8).is_none());
    assert_eq!(Protocols::from_raw(4), Some(Protocols::ECHO));
    assert_eq!(Protocols::from_raw(7), Some(Protocols::ALL));
    let mut bytes = one(Target::Any, Ports::one(80), Protocols::TCP);
    let last = bytes.len() - 1;
    bytes[last] = 0;
    assert!(Scope::decode(&bytes).is_none());
    // Nonzero reserved byte, wrong version, too many rules.
    let good = one(Target::Any, Ports::ANY, Protocols::BOTH);
    for (i, v) in [(0, 2), (3, 1), (1, 65), (2, 64)] {
        let mut b = good.clone();
        b[i] = v;
        assert!(Scope::decode(&b).is_none(), "byte {i}");
    }
}

#[test]
fn a_wildcard_covers_the_names_under_it_and_not_the_suffix() {
    let bytes = one(Target::Wildcard("example.org"), Ports::ANY, Protocols::TCP);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_name("a.example.org", 1, Protocols::TCP));
    assert!(s.allows_name("a.b.example.org", 1, Protocols::TCP));
    assert!(!s.allows_name("example.org", 1, Protocols::TCP));
    assert!(!s.allows_name("notexample.org", 1, Protocols::TCP));
    assert!(!s.allows_name("a.example.org", 1, Protocols::UDP));
}

#[test]
fn a_name_never_matches_an_address_rule_or_the_reverse() {
    // INV-0220-4.
    let by_address = one(Target::Address([10, 0, 2, 2]), Ports::ANY, Protocols::BOTH);
    let s = Scope::decode(&by_address).unwrap();
    for name in ["10.0.2.2", "10-0-2-2", "gateway"] {
        assert!(!s.allows_name(name, 80, Protocols::TCP));
    }
    assert!(s.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 3], 80, Protocols::TCP));
    let by_name = one(Target::Name("gateway"), Ports::ANY, Protocols::BOTH);
    let s = Scope::decode(&by_name).unwrap();
    assert!(!s.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
    assert!(!s.reaches_address([10, 0, 2, 2]));
    let any = one(Target::Any, Ports::ANY, Protocols::BOTH);
    assert!(Scope::decode(&any).unwrap().reaches_address([1, 2, 3, 4]));
    // Types: a name is not an address and a wildcard not an exact name.
    assert!(!Target::Name("a.example.org").within(&Target::Address([1, 2, 3, 4])));
    assert!(!Target::Address([1, 2, 3, 4]).within(&Target::Name("a.example.org")));
    assert!(!Target::Wildcard("example.org").within(&Target::Name("example.org")));
    assert!(!Target::Any.within(&Target::Prefix([0; 4], 0)));
    assert!(Target::Address([1, 2, 3, 4]).within(&Target::Prefix([0; 4], 0)));
}

#[test]
fn ports_within_handles_overlapping_and_unordered_ranges() {
    let outer = Ports::ranges(&[(100, 200), (50, 120), (201, 300)]).unwrap();
    assert!(Ports::ranges(&[(60, 290)]).unwrap().within(&outer));
    assert!(Ports::one(300).within(&outer));
    assert!(!Ports::one(301).within(&outer));
    assert!(!Ports::one(49).within(&outer));
    assert!(!Ports::ANY.within(&outer));
    assert!(outer.within(&Ports::ANY));
    assert!(Ports::ANY.within(&Ports::ANY));
    assert!(Ports::one(65535).within(&Ports::ranges(&[(1, 65535)]).unwrap()));
}

/// INV-0220-3: `within` is the subset relation, so narrowing never gives a
/// request a path that the parent did not allow.
#[test]
fn a_scope_within_another_allows_nothing_the_other_does_not() {
    let mut rng = Rng(0xdead_beef_cafe_f00d);
    let mut subsets = 0;
    for _ in 0..4000 {
        let (a, b) = (random_scope(&mut rng), random_scope(&mut rng));
        let (a, b) = (Scope::decode(&a).unwrap(), Scope::decode(&b).unwrap());
        assert!(a.within(&a));
        if !a.within(&b) {
            continue;
        }
        subsets += 1;
        // ADR-0324: a web grant of the child is a web grant or a named one
        // of the parent; a named one stays named (INV-0324-2).
        let rank = |g: NameGrant| match g {
            NameGrant::No => 0,
            NameGrant::PublicOnly => 1,
            NameGrant::Named => 2,
        };
        for name in NAMES.iter().chain(SUFFIXES.iter()) {
            for port in [22, 80, 85, 443, 1000, 8080] {
                for p in [Protocols::TCP, Protocols::UDP] {
                    if a.allows_name(name, port, p) {
                        assert!(b.allows_name(name, port, p), "{name} {port}");
                    }
                    let (ga, gb) = (a.name_grant(name, port, p), b.name_grant(name, port, p));
                    assert!(rank(ga) <= rank(gb), "{name} {port} {ga:?} {gb:?}");
                }
            }
            assert!(rank(a.resolve_grant(name)) <= rank(b.resolve_grant(name)));
        }
        for address in ADDRESSES {
            for port in [22, 80, 85, 443, 1000, 8080] {
                for p in [Protocols::TCP, Protocols::UDP] {
                    if a.allows_address(address, port, p) {
                        assert!(b.allows_address(address, port, p));
                    }
                    if a.names_address(address, port, p) {
                        assert!(b.names_address(address, port, p));
                    }
                }
            }
            if a.reaches_address(address) {
                assert!(b.reaches_address(address));
            }
        }
        for port in [22, 80, 85, 443, 1000, 8080] {
            for p in [Protocols::TCP, Protocols::UDP] {
                if a.allows_listen(port, p) {
                    assert!(b.allows_listen(port, p));
                }
            }
        }
    }
    assert!(subsets > 100, "the generator finds subsets: {subsets}");
}

#[test]
fn narrowing_examples() {
    let ports = Ports::ranges(&[(80, 443)]).unwrap();
    let parent = one(Target::Wildcard("example.org"), ports, Protocols::BOTH);
    let parent = Scope::decode(&parent).unwrap();
    let inside = one(
        Target::Name("www.example.org"),
        Ports::one(443),
        Protocols::TCP,
    );
    assert!(Scope::decode(&inside).unwrap().within(&parent));
    for outside in [
        one(Target::Name("example.org"), Ports::one(443), Protocols::TCP),
        one(
            Target::Name("www.example.org"),
            Ports::one(444),
            Protocols::TCP,
        ),
        one(Target::Name("www.example.org"), Ports::ANY, Protocols::TCP),
        one(Target::Any, Ports::one(443), Protocols::TCP),
        one(Target::Wildcard("org"), Ports::one(443), Protocols::TCP),
    ] {
        assert!(!Scope::decode(&outside).unwrap().within(&parent));
    }
    // The empty scope is inside everything; nothing but it is inside the empty.
    assert!(Scope::EMPTY.within(&parent));
    assert!(!parent.within(&Scope::EMPTY));
    // A listen rule is not a reach rule.
    let mut out = vec![0u8; 64];
    let rule = ListenRule {
        ports: Ports::one(80),
        protocols: Protocols::TCP,
    };
    let n = encode(&[], &[rule], &mut out).unwrap();
    let listen = Scope::decode(&out[..n]).unwrap();
    assert!(!listen.within(&parent));
    assert!(listen.allows_listen(80, Protocols::TCP));
    assert!(!listen.allows_listen(81, Protocols::TCP));
    assert!(!listen.allows_listen(80, Protocols::UDP));
}

#[test]
fn an_echo_rule_grants_echo_and_no_connection_and_no_datagram() {
    // ADR-0330: ICMP echo is a protocol of its own in a reach rule.
    let echo = one(Target::Name("example.org"), Ports::ANY, Protocols::ECHO);
    let s = Scope::decode(&echo).unwrap();
    assert!(s.allows_echo_name("example.org"));
    assert!(!s.allows_echo_name("other.org"));
    for port in [1, 80, 65535] {
        assert!(!s.allows_name("example.org", port, Protocols::TCP));
        assert!(!s.allows_name("example.org", port, Protocols::UDP));
    }
    let by_address_bytes = one(Target::Address([10, 0, 2, 2]), Ports::ANY, Protocols::ECHO);
    let by_address = Scope::decode(&by_address_bytes).unwrap();
    assert!(by_address.allows_echo_address([10, 0, 2, 2]));
    assert!(!by_address.allows_echo_address([10, 0, 2, 3]));
    assert!(!by_address.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
    assert!(!by_address.allows_address([10, 0, 2, 2], 53, Protocols::UDP));
    // A rule of TCP and UDP grants no echo.
    let tcp_bytes = one(Target::Address([10, 0, 2, 2]), Ports::ANY, Protocols::BOTH);
    let tcp = Scope::decode(&tcp_bytes).unwrap();
    assert!(!tcp.allows_echo_address([10, 0, 2, 2]));
    // An echo rule narrows only to echo: a child of TCP is not inside it.
    let tcp_child = one(Target::Address([10, 0, 2, 2]), Ports::ANY, Protocols::TCP);
    assert!(!Scope::decode(&tcp_child).unwrap().within(&by_address));
    let echo_child = one(Target::Address([10, 0, 2, 2]), Ports::ANY, Protocols::ECHO);
    assert!(Scope::decode(&echo_child).unwrap().within(&by_address));
    // A listen rule with the echo bit does not encode (and so does not decode).
    let mut out = vec![0u8; 64];
    let listen = ListenRule {
        ports: Ports::ANY,
        protocols: Protocols::ALL,
    };
    assert!(encode(&[], &[listen], &mut out).is_none());
}

/// ADR-0324 item 2: the blocks a web rule never reaches, at their edges.
#[test]
fn is_public_refuses_every_local_block_at_both_edges() {
    for (block, len) in NOT_PUBLIC {
        let first = u32::from_be_bytes(block);
        let last = first
            | if len == 0 {
                u32::MAX
            } else {
                !(u32::MAX << (32 - len))
            };
        assert!(!is_public(first.to_be_bytes()), "{block:?}/{len} first");
        assert!(!is_public(last.to_be_bytes()), "{block:?}/{len} last");
        let middle = first + (last - first) / 2;
        assert!(!is_public(middle.to_be_bytes()));
        // The neighbours are public unless another block holds them.
        for n in [first.wrapping_sub(1), last.wrapping_add(1)] {
            let inside = NOT_PUBLIC.iter().any(|&(b, l)| {
                let m = if l == 0 { 0 } else { u32::MAX << (32 - l) };
                n & m == u32::from_be_bytes(b)
            });
            assert_eq!(
                is_public(n.to_be_bytes()),
                !inside,
                "{n:#x} beside {block:?}/{len}"
            );
        }
    }
    for a in [
        [10, 0, 2, 2],
        [10, 0, 2, 15],
        [127, 0, 0, 1],
        [192, 168, 1, 1],
        [172, 31, 255, 255],
        [169, 254, 169, 254],
        [100, 64, 0, 1],
        [0, 0, 0, 0],
        [255, 255, 255, 255],
        [224, 0, 0, 1],
        [198, 19, 0, 1],
    ] {
        assert!(!is_public(a), "{a:?}");
    }
    for a in [
        [93, 184, 215, 14],
        [1, 1, 1, 1],
        [8, 8, 8, 8],
        [203, 0, 113, 10],
        [172, 32, 0, 1],
        [100, 128, 0, 1],
        [11, 0, 0, 1],
        [192, 0, 2, 1],
        [198, 51, 100, 1],
        [223, 255, 255, 254],
    ] {
        assert!(is_public(a), "{a:?}");
    }
}

fn web() -> Vec<u8> {
    one(
        Target::Public,
        Ports::ranges(&[(80, 80), (443, 443)]).unwrap(),
        Protocols::TCP,
    )
}

/// ADR-0324: a web rule reaches public addresses on its ports, admits a name
/// only through public answers, and never names a local address.
#[test]
fn a_web_rule_reaches_public_addresses_and_admits_names_through_their_answers() {
    let bytes = web();
    let s = Scope::decode(&bytes).unwrap();
    assert_eq!(re_encode(&s), bytes);
    assert_eq!(s.reach_rules().next().unwrap().target, Target::Public);
    for port in [80, 443] {
        assert!(s.allows_address([93, 184, 215, 14], port, Protocols::TCP));
        assert!(s.allows_address([203, 0, 113, 10], port, Protocols::TCP));
        for local in [
            [10, 0, 2, 2],
            [127, 0, 0, 1],
            [192, 168, 0, 1],
            [169, 254, 1, 1],
        ] {
            assert!(!s.allows_address(local, port, Protocols::TCP), "{local:?}");
        }
        // Never a name rule, never a rule that names an address.
        assert!(!s.allows_name("example.org", port, Protocols::TCP));
        assert!(!s.names_address([93, 184, 215, 14], port, Protocols::TCP));
        assert_eq!(
            s.name_grant("example.org", port, Protocols::TCP),
            NameGrant::PublicOnly
        );
    }
    for port in [22, 81, 442, 444, 8080] {
        assert!(!s.allows_address([93, 184, 215, 14], port, Protocols::TCP));
        assert_eq!(
            s.name_grant("example.org", port, Protocols::TCP),
            NameGrant::No
        );
    }
    assert!(!s.allows_address([93, 184, 215, 14], 443, Protocols::UDP));
    assert_eq!(
        s.name_grant("example.org", 443, Protocols::UDP),
        NameGrant::No
    );
    assert!(!s.allows_echo_name("example.org"));
    assert!(!s.allows_echo_address([93, 184, 215, 14]));
    assert!(!s.reaches_name("example.org"));
    assert!(!s.reaches_address([93, 184, 215, 14]));
    assert_eq!(s.resolve_grant("example.org"), NameGrant::PublicOnly);
    assert_eq!(Scope::EMPTY.resolve_grant("example.org"), NameGrant::No);
    // With a name rule beside it, that name is named.
    let mut out = vec![0u8; 128];
    let rules = [
        s.reach_rules().next().unwrap(),
        ReachRule {
            target: Target::Name("test.zoraya"),
            ports: Ports::one(8080),
            protocols: Protocols::TCP,
        },
    ];
    let n = encode(&rules, &[], &mut out).unwrap();
    let both = Scope::decode(&out[..n]).unwrap();
    assert_eq!(
        both.name_grant("test.zoraya", 8080, Protocols::TCP),
        NameGrant::Named
    );
    assert_eq!(
        both.name_grant("test.zoraya", 80, Protocols::TCP),
        NameGrant::PublicOnly
    );
    assert_eq!(both.resolve_grant("test.zoraya"), NameGrant::Named);
}

/// INV-0324-2: only a web rule is inside a web rule, and a web rule is inside
/// only a web rule or `any`.
#[test]
fn only_a_web_rule_narrows_from_a_web_rule() {
    let bytes = web();
    let parent = Scope::decode(&bytes).unwrap();
    let narrower = one(Target::Public, Ports::one(443), Protocols::TCP);
    assert!(Scope::decode(&narrower).unwrap().within(&parent));
    for outside in [
        one(
            Target::Address([93, 184, 215, 14]),
            Ports::one(443),
            Protocols::TCP,
        ),
        one(
            Target::Prefix([93, 184, 215, 0], 24),
            Ports::one(443),
            Protocols::TCP,
        ),
        one(Target::Name("example.org"), Ports::one(443), Protocols::TCP),
        one(Target::Wildcard("org"), Ports::one(443), Protocols::TCP),
        one(Target::Any, Ports::one(443), Protocols::TCP),
        one(Target::Public, Ports::one(8080), Protocols::TCP),
    ] {
        assert!(!Scope::decode(&outside).unwrap().within(&parent));
    }
    let any = one(Target::Any, Ports::ANY, Protocols::ALL);
    assert!(parent.within(&Scope::decode(&any).unwrap()));
    for not_outer in [
        one(Target::Prefix([0, 0, 0, 0], 0), Ports::ANY, Protocols::ALL),
        one(Target::Wildcard("org"), Ports::ANY, Protocols::ALL),
    ] {
        assert!(!parent.within(&Scope::decode(&not_outer).unwrap()));
    }
}

/// INV-0324-1: a web rule admits no protocol but TCP, and the decoder (so
/// `netd`) refuses one with UDP or echo whoever made the scope; `encode`,
/// which checks its output with the decoder, makes none.
#[test]
fn a_web_rule_with_udp_or_echo_does_not_decode() {
    for raw in [2u8, 3, 4, 5, 6, 7] {
        let protocols = Protocols::from_raw(raw).unwrap();
        let rule = ReachRule {
            target: Target::Public,
            ports: Ports::one(443),
            protocols,
        };
        let mut out = vec![0u8; 64];
        assert_eq!(encode(&[rule], &[], &mut out), None, "{raw}");
        // The bytes by hand: version, one reach rule, kind 5, one port, the
        // protocols.
        let bytes = [VERSION, 1, 0, 0, 5, 1, 0xbb, 0x01, 0xbb, 0x01, raw];
        assert!(Scope::decode(&bytes).is_none(), "{raw}");
    }
    let tcp = [VERSION, 1, 0, 0, 5, 1, 0xbb, 0x01, 0xbb, 0x01, 1];
    let s = Scope::decode(&tcp).expect("a web rule over TCP decodes");
    assert!(!s.allows_echo_address([93, 184, 215, 14]));
    assert!(s.allows_address([93, 184, 215, 14], 443, Protocols::TCP));
}
