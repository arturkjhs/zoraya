//! Host tests of the network grant a command line gives (ADR-0224 item 2;
//! INV-0224-1 for the cases that exist).

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::net::{Protocols, Scope, rights};
use zoraya_command::{Outcome, Parsed};
use zoraya_describe::Description;

use crate::eval::Value;
use crate::grants::{Line, Refused};
use crate::netgrant::net_for;

const FETCH: &str = include_str!("../../../user/describe/commands/fetch.cmd");
const PING: &str = include_str!("../../../user/describe/commands/ping.cmd");
const NET: &str = include_str!("../../../user/describe/commands/net.cmd");
const CAT: &str = include_str!("../../../user/describe/commands/cat.cmd");

fn go(text: &str, args: &[(&str, bool)], base: bool) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    go_nets(text, args, &[], base)
}

fn go_nets(
    text: &str,
    args: &[(&str, bool)],
    nets: &[&str],
    base: bool,
) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    go_all(text, args, nets, &[], base)
}

fn go_all(
    text: &str,
    args: &[(&str, bool)],
    nets: &[&str],
    listens: &[&str],
    base: bool,
) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    let listens: Vec<Value> = listens
        .iter()
        .map(|a| Value {
            text: String::from(*a),
            literal: true,
        })
        .collect();
    let d: Description = zoraya_describe::parse(text.as_bytes()).unwrap();
    let values: Vec<Value> = args
        .iter()
        .map(|(a, literal)| Value {
            text: String::from(*a),
            literal: *literal,
        })
        .collect();
    let nets: Vec<Value> = nets
        .iter()
        .map(|a| Value {
            text: String::from(*a),
            literal: true,
        })
        .collect();
    let texts: Vec<&str> = values.iter().map(|v| v.text.as_str()).collect();
    let parsed: Parsed<'_> = match zoraya_command::parse(&d, &texts) {
        Ok(Outcome::Run(p)) => p,
        _ => Parsed::default(),
    };
    net_for(&Line {
        description: &d,
        parsed: &parsed,
        args: &values,
        cwd: "/",
        grants: &[],
        nets: &nets,
        listens: &listens,
        socks: &[],
        base_system: base,
        room: None,
    })
}

fn scope_of(r: Option<(u32, Vec<u8>)>) -> (u32, Vec<u8>) {
    r.expect("a connection")
}

#[test]
fn a_literal_address_reaches_that_host_and_port_and_nothing_else() {
    let (r, bytes) = scope_of(go(FETCH, &[("http://10.0.2.2:8080/a?b=c", true)], true).unwrap());
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_address([10, 0, 2, 2], 8080, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 8081, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 3], 8080, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 8080, Protocols::UDP));
    assert!(!s.allows_name("example.org", 8080, Protocols::TCP));
}

#[test]
fn a_literal_name_is_lower_cased_and_defaults_to_port_80() {
    let (_, bytes) = scope_of(go(FETCH, &[("http://Test.Zoraya/", true)], true).unwrap());
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_name("test.zoraya", 80, Protocols::TCP));
    assert!(!s.allows_name("test.zoraya", 443, Protocols::TCP));
    assert!(!s.allows_name("other.zoraya", 80, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
}

#[test]
fn an_operand_from_a_variable_a_substitution_or_a_glob_is_refused_before_it_runs() {
    let r = go(FETCH, &[("http://10.0.2.2:8080/", false)], true).unwrap_err();
    assert_eq!(r.subject, "http://10.0.2.2:8080/");
    assert!(r.problem.contains("from a value"), "{}", r.problem);
    assert!(r.problem.contains("network grants"), "{}", r.problem);
    assert!(r.hint.is_some());
    // The option's argument is a file, not a URL: a value there is the
    // path planner's business, and the URL beside it is still literal.
    assert!(
        go(
            FETCH,
            &[("-o", true), ("f", false), ("http://a.b/", true)],
            true
        )
        .is_ok()
    );
}

#[test]
fn a_url_that_does_not_parse_gives_no_connection_and_the_command_says_why() {
    for bad in [
        "https://a.b:0/",
        "http://a.b:0/",
        "http://u:p@a.b/",
        "http://a b/",
        "ftp://a.b/",
    ] {
        assert_eq!(go(FETCH, &[(bad, true)], true).unwrap(), None, "{bad}");
    }
}

/// ADR-0228: an `https://` address designates the host and port 443 (or the
/// one written), like `http://` does with 80.
#[test]
fn an_https_url_designates_its_host_and_port_443() {
    let (_, https) = scope_of(go(FETCH, &[("https://a.b/", true)], true).unwrap());
    let (_, http443) = scope_of(go(FETCH, &[("http://a.b:443/", true)], true).unwrap());
    assert_eq!(https, http443);
}

#[test]
fn only_base_system_commands_designate() {
    assert_eq!(go(FETCH, &[("http://a.b/", true)], false).unwrap(), None);
    assert_eq!(go(FETCH, &[("http://a.b/", false)], false).unwrap(), None);
}

#[test]
fn a_command_without_a_url_operand_or_the_need_gets_nothing() {
    assert_eq!(go(CAT, &[("f", true)], true).unwrap(), None);
    let (r, bytes) = scope_of(go(NET, &[], true).unwrap());
    assert_eq!(r, rights::STATUS);
    assert!(Scope::decode(&bytes).unwrap().is_empty());
}

#[test]
fn the_local_hosts_are_reached_only_when_written_out() {
    // 127.0.0.1, 10.0.2.2: the rule names exactly what was typed; whether the
    // answer is local is `netd`'s rule, applied to names only.
    let (_, bytes) = scope_of(go(FETCH, &[("http://127.0.0.1:80/", true)], true).unwrap());
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_address([127, 0, 0, 1], 80, Protocols::TCP));
    assert!(!s.allows_name("localhost", 80, Protocols::TCP));
}

// ---- the `+net` word (ADR-0224 item 1; N6, INV-0224-1) ----

#[test]
fn a_net_word_without_a_port_reaches_every_port_over_both_protocols() {
    let (r, bytes) = scope_of(go_nets(CAT, &[], &["Example.org"], false).unwrap());
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_name("example.org", 443, Protocols::TCP));
    assert!(s.allows_name("example.org", 53, Protocols::UDP));
    assert!(!s.allows_name("other.org", 443, Protocols::TCP));
    assert!(!s.allows_name("www.example.org", 443, Protocols::TCP));
}

#[test]
fn a_net_word_with_a_port_reaches_that_port_only() {
    let (_, bytes) = scope_of(go_nets(CAT, &[], &["example.org:443"], false).unwrap());
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_name("example.org", 443, Protocols::TCP));
    assert!(!s.allows_name("example.org", 80, Protocols::TCP));
}

#[test]
fn a_wildcard_covers_the_names_under_it_and_not_itself() {
    let (_, bytes) = scope_of(go_nets(CAT, &[], &["*.example.org"], false).unwrap());
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_name("www.example.org", 443, Protocols::TCP));
    assert!(!s.allows_name("example.org", 443, Protocols::TCP));
}

#[test]
fn addresses_prefixes_and_any() {
    let (_, bytes) = scope_of(go_nets(CAT, &[], &["10.0.2.0/24:80", "192.0.2.7"], false).unwrap());
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_address([10, 0, 2, 9], 80, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 9], 81, Protocols::TCP));
    assert!(s.allows_address([192, 0, 2, 7], 1, Protocols::UDP));
    assert!(!s.allows_address([192, 0, 2, 8], 1, Protocols::UDP));
    let (_, any) = scope_of(go_nets(CAT, &[], &["any"], false).unwrap());
    assert!(
        Scope::decode(&any)
            .unwrap()
            .allows_name("x.org", 9, Protocols::TCP)
    );
}

#[test]
fn a_bad_net_word_refuses_the_line() {
    for bad in [
        "",
        "exa mple",
        "example.org:0",
        "example.org:70000",
        "example.org:x",
        "10.0.2.1/33",
        "10.0.2.1/24",
        "*.",
        "*.10",
        "1.2.3",
    ] {
        let r = go_nets(CAT, &[], &[bad], false);
        assert!(r.is_err(), "{bad:?} should be refused, got {r:?}");
    }
}

#[test]
fn a_value_url_designates_only_inside_a_net_word_of_the_line() {
    // Outside: refused, with the grant that would allow it.
    let r = go_nets(
        FETCH,
        &[("http://other.example/", false)],
        &["example.org"],
        true,
    );
    let e = r.unwrap_err();
    assert!(e.hint.unwrap().contains("+net other.example:80"));
    let r = go(FETCH, &[("http://other.example/", false)], true);
    assert!(r.is_err());
    // Inside: it runs, and the scope is the word's, nothing wider.
    let (_, bytes) = scope_of(
        go_nets(
            FETCH,
            &[("http://www.example.org/", false)],
            &["*.example.org"],
            true,
        )
        .unwrap(),
    );
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_name("www.example.org", 80, Protocols::TCP));
    assert!(s.allows_name("www.example.org", 443, Protocols::TCP));
    assert!(!s.allows_name("other.example", 80, Protocols::TCP));
    // The port matters: a rule for 443 does not cover a value on 80.
    assert!(
        go_nets(
            FETCH,
            &[("http://example.org/", false)],
            &["example.org:443"],
            true
        )
        .is_err()
    );
}

#[test]
fn a_literal_url_with_a_net_word_has_both_rules() {
    let (_, bytes) = scope_of(
        go_nets(
            FETCH,
            &[("http://10.0.2.2:8080/", true)],
            &["example.org:443"],
            true,
        )
        .unwrap(),
    );
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_address([10, 0, 2, 2], 8080, Protocols::TCP));
    assert!(s.allows_name("example.org", 443, Protocols::TCP));
}

#[test]
fn net_words_and_status_together() {
    // `net` needs the status; with a word it gets a scope as well.
    let (r, bytes) = scope_of(go_nets(NET, &[], &["example.org"], true).unwrap());
    assert_eq!(r, rights::REACH);
    assert!(
        Scope::decode(&bytes)
            .unwrap()
            .allows_name("example.org", 1, Protocols::TCP)
    );
}

#[test]
fn grants_prints_exactly_the_rules_the_connection_carries() {
    // INV-0224-2 for the scope.
    let (_, bytes) = scope_of(
        go_nets(
            FETCH,
            &[("http://10.0.2.2:8080/", true)],
            &["example.org", "*.test.zoraya:443"],
            true,
        )
        .unwrap(),
    );
    let lines = crate::netgrant::describe(&bytes);
    assert_eq!(lines.len(), Scope::decode(&bytes).unwrap().reach_count());
    assert_eq!(
        lines[0],
        "net: example.org             reach, any port (grant +net)"
    );
    assert_eq!(
        lines[1],
        format!("net: {:<24}reach (grant +net)", "*.test.zoraya:443")
    );
    assert_eq!(
        lines[2],
        format!("net: {:<24}reach (operand URL)", "10.0.2.2:8080")
    );
}

#[test]
fn a_launcher_with_no_root_refuses_a_line_that_needs_the_network() {
    use crate::netgrant::{Root, available};
    // N6 review, finding 7 (ADR-0221 items 5 and 6): a launcher that `init`
    // tells about the network (it has the services channel) runs no line
    // that needs it without a usable root, as when the root is closed.
    assert_eq!(available(true, Root::Open, false), Ok(()));
    assert_eq!(
        available(true, Root::Closed, false),
        Err("the network is not available (restarting)")
    );
    assert_eq!(
        available(true, Root::Closed, true),
        Err("the network is not available")
    );
    assert_eq!(
        available(true, Root::None, false),
        Err("the network is not available")
    );
    assert_eq!(
        available(true, Root::None, true),
        Err("the network is not available")
    );
    // No services channel: a machine with no network device; the command
    // runs without a connection and says so itself.
    assert_eq!(available(false, Root::None, false), Ok(()));
    assert_eq!(available(false, Root::Open, false), Ok(()));
    assert_eq!(
        available(false, Root::Closed, false),
        Err("the network is not available (restarting)")
    );
}

#[test]
fn a_literal_host_operand_grants_echo_to_that_host_and_nothing_else() {
    let (r, bytes) =
        scope_of(go(PING, &[("-c", true), ("2", true), ("10.0.2.2", true)], true).unwrap());
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_echo_address([10, 0, 2, 2]));
    assert!(!s.allows_echo_address([10, 0, 2, 3]));
    // ADR-0330: the rule is echo and no connection and no datagram, on no port.
    for port in [1, 53, 80, 65535] {
        assert!(!s.allows_address([10, 0, 2, 2], port, Protocols::TCP));
        assert!(!s.allows_address([10, 0, 2, 2], port, Protocols::UDP));
    }
    assert_eq!(
        crate::netgrant::describe(&bytes),
        [format!("net: {:<24}echo only (operand HOST)", "10.0.2.2")]
    );
}

#[test]
fn a_host_operand_by_name_grants_echo_to_that_name_only() {
    let (_, bytes) = scope_of(go(PING, &[("Example.ORG", true)], true).unwrap());
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_echo_name("example.org"));
    assert!(!s.allows_echo_name("www.example.org"));
    assert!(!s.allows_name("example.org", 80, Protocols::TCP));
    assert!(!s.allows_name("example.org", 53, Protocols::UDP));
    assert_eq!(
        crate::netgrant::describe(&bytes),
        [format!(
            "net: {:<24}echo only (operand HOST)",
            "example.org"
        )]
    );
}

#[test]
fn a_host_operand_from_a_value_needs_a_net_word_that_covers_it() {
    let r = go(PING, &[("10.0.2.2", false)], true).unwrap_err();
    assert!(r.problem.contains("from a value"), "{}", r.problem);
    assert_eq!(
        r.hint.as_deref(),
        Some("if that is meant: +net 10.0.2.2 COMMAND")
    );
    // A `+net` word covers echo too (TCP, UDP and echo), a value inside it designates.
    let covered = go_nets(PING, &[("10.0.2.2", false)], &["10.0.2.0/24"], true).unwrap();
    assert!(
        Scope::decode(&scope_of(covered).1)
            .unwrap()
            .allows_echo_address([10, 0, 2, 2])
    );
    // Only the base system's commands are given a network by an operand.
    assert!(go(PING, &[("10.0.2.2", true)], false).unwrap().is_none());
}

#[test]
fn a_host_operand_that_is_not_a_host_makes_no_grant() {
    for bad in ["10.0.2", "10.0.2.2.2", "a..b", "*.org", "a b"] {
        let r = go(PING, &[(bad, true)], true).unwrap();
        assert!(r.is_none(), "{bad} made a grant");
    }
}

const NETSURF: &str = include_str!("../../../user/describe/posix/netsurf.cmd");
const CURL: &str = include_str!("../../../user/describe/posix/curl.cmd");
const TCC: &str = include_str!("../../../user/describe/posix/tcc.cmd");

/// INV-0312-1, INV-0317-1: a program with a window runs only a line its
/// description parses; options that would replace or disable the roots do not
/// run. A program without a window keeps the default-settings rule.
#[test]
fn a_window_program_runs_only_a_line_its_description_parses() {
    use crate::netgrant::line_may_run;
    let ns = zoraya_describe::parse(NETSURF.as_bytes()).unwrap();
    assert!(line_may_run(&ns, &["https://h/"]));
    assert!(line_may_run(&ns, &[]));
    assert!(line_may_run(&ns, &["--help"]));
    for bad in [
        "--ca_bundle=/x.pem",
        "--ca_path=/x",
        "--ssl_verify_certificates=false",
        "--homepage_url=https://h/",
        "-k",
    ] {
        assert!(!line_may_run(&ns, &[bad]), "{bad}");
        assert!(!line_may_run(&ns, &[bad, "https://h/"]), "{bad}");
        assert!(!line_may_run(&ns, &["--help", bad]), "{bad}");
    }
    // Without a window the rule stays as it was.
    let curl = zoraya_describe::parse(CURL.as_bytes()).unwrap();
    assert!(line_may_run(&curl, &["-k", "https://h/"]));
    let tcc = zoraya_describe::parse(TCC.as_bytes()).unwrap();
    assert!(line_may_run(&tcc, &["--anything"]));
}

/// The pills' `+net` targets, as the window writes them.
#[test]
fn net_targets_are_the_scope_hosts() {
    let ok = go_nets(NET, &[], &["example.org"], true).unwrap();
    let scope = ok.expect("a scope").1;
    assert_eq!(
        crate::netgrant::pills(&scope),
        vec![String::from("+net example.org")]
    );
}

/// ADR-0324 items 1, 3 and 6 (INV-0324-3, INV-0324-4): the image's NetSurf
/// gets the web rule (public hosts, ports 80 and 443, TCP) beside its URL's
/// own rule; curl does not; a description outside the image does not; the
/// pill is `web` and `grants` says what it reaches.
#[test]
fn the_web_rule_is_the_descriptions_and_shows_as_web() {
    use zoraya_abi::net::{NameGrant, Target};
    let ok = go(NETSURF, &[], true).unwrap().expect("a scope");
    assert_eq!(ok.0, rights::REACH);
    let s = Scope::decode(&ok.1).unwrap();
    let rules: Vec<_> = s.reach_rules().collect();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].target, Target::Public);
    assert_eq!(rules[0].protocols, Protocols::TCP);
    assert_eq!(rules[0].ports.list(), &[(80, 80), (443, 443)]);
    assert!(s.allows_address([93, 184, 215, 14], 443, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
    assert!(!s.allows_address([127, 0, 0, 1], 80, Protocols::TCP));
    assert!(!s.allows_address([93, 184, 215, 14], 8080, Protocols::TCP));
    assert!(!s.allows_echo_address([93, 184, 215, 14]));
    assert_eq!(crate::netgrant::pills(&ok.1), vec![String::from("web")]);
    assert_eq!(
        crate::netgrant::describe(&ok.1),
        vec![String::from(
            "net: web                     reach public hosts, ports 80 and 443; not this machine or its network (description)"
        )]
    );
    // With a URL: its own rule too, which names its host (a private one).
    let url = go(NETSURF, &[("http://10.0.2.2:8080/ns/page", true)], true)
        .unwrap()
        .expect("a scope");
    let s = Scope::decode(&url.1).unwrap();
    assert!(s.allows_address([10, 0, 2, 2], 8080, Protocols::TCP));
    assert!(s.names_address([10, 0, 2, 2], 8080, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 80, Protocols::TCP));
    assert_eq!(
        crate::netgrant::pills(&url.1),
        vec![String::from("+net 10.0.2.2:8080"), String::from("web")]
    );
    // +net words name local hosts beside it.
    let words = go_nets(NETSURF, &[], &["192.168.1.1:80"], true)
        .unwrap()
        .expect("a scope");
    let s = Scope::decode(&words.1).unwrap();
    assert!(s.names_address([192, 168, 1, 1], 80, Protocols::TCP));
    assert_eq!(
        s.name_grant("example.org", 443, Protocols::TCP),
        NameGrant::PublicOnly
    );
    // A URL from a value still needs a +net word: the web rule does not make
    // it designate.
    assert!(go(NETSURF, &[("http://example.org/", false)], true).is_err());
    // Not for a description outside the image, and not for curl.
    assert_eq!(go(NETSURF, &[], false).unwrap(), None);
    let curl = go(CURL, &[("https://example.org/", true)], true)
        .unwrap()
        .expect("a scope");
    assert!(
        Scope::decode(&curl.1)
            .unwrap()
            .reach_rules()
            .all(|r| r.target != Target::Public)
    );
}

/// ADR-0324 item 7 (INV-0324-3): a description's web rule is refused in an
/// untrusted room, and only there; a description without it is not.
#[test]
fn the_web_rule_is_refused_in_an_untrusted_room() {
    use crate::netgrant::web_refused;
    let ns = zoraya_describe::parse(NETSURF.as_bytes()).unwrap();
    assert!(ns.net_web);
    let refused = web_refused(&ns, true).unwrap_err();
    assert_eq!(refused.subject, "netsurf");
    assert!(
        refused.problem.contains("untrusted room"),
        "{}",
        refused.problem
    );
    assert!(web_refused(&ns, false).is_ok());
    let curl = zoraya_describe::parse(CURL.as_bytes()).unwrap();
    assert!(!curl.net_web);
    assert!(web_refused(&curl, true).is_ok());
}

/// `grants` of a web rule says every range whole, and `any port` (9f-net
/// review, note 9): `describe` is total over scopes, and a narrowed or
/// hand-made web scope is a valid input.
#[test]
fn grants_says_every_port_of_a_web_rule() {
    use zoraya_abi::net::{Ports, ReachRule, Target, encode};
    let line = |ports: Ports| {
        let rule = ReachRule {
            target: Target::Public,
            ports,
            protocols: Protocols::TCP,
        };
        let mut out = vec![0u8; 128];
        let n = encode(&[rule], &[], &mut out).unwrap();
        crate::netgrant::describe(&out[..n]).concat()
    };
    let tail = "; not this machine or its network (description)";
    assert_eq!(
        line(Ports::one(443)),
        alloc::format!("net: web                     reach public hosts, port 443{tail}")
    );
    assert_eq!(
        line(Ports::ranges(&[(80, 80), (443, 443)]).unwrap()),
        alloc::format!("net: web                     reach public hosts, ports 80 and 443{tail}")
    );
    assert_eq!(
        line(Ports::ranges(&[(80, 90), (443, 443), (8000, 8080)]).unwrap()),
        alloc::format!(
            "net: web                     reach public hosts, ports 80-90, 443 and 8000-8080{tail}"
        )
    );
    assert_eq!(
        line(Ports::ranges(&[(1, 1023)]).unwrap()),
        alloc::format!("net: web                     reach public hosts, ports 1-1023{tail}")
    );
    assert_eq!(
        line(Ports::ANY),
        alloc::format!("net: web                     reach public hosts, any port{tail}")
    );
}

/// A host literally named `web` (`+net web`) is a host, beside the web rule:
/// its pill is `+net web` and its `grants` line says `any port (grant
/// +net)`, so the two `net: web` lines differ in what follows (9f-net review,
/// note 10).
#[test]
fn a_host_named_web_is_a_host_beside_the_web_rule() {
    use zoraya_abi::net::Target;
    let ok = go_nets(NETSURF, &[], &["web"], true)
        .unwrap()
        .expect("a scope");
    let s = Scope::decode(&ok.1).unwrap();
    let targets: Vec<_> = s.reach_rules().map(|r| r.target).collect();
    assert_eq!(targets, vec![Target::Name("web"), Target::Public]);
    assert_eq!(
        crate::netgrant::pills(&ok.1),
        vec![String::from("+net web"), String::from("web")]
    );
    let lines = crate::netgrant::describe(&ok.1);
    assert_eq!(
        lines[0],
        "net: web                     reach, any port (grant +net)"
    );
    assert!(lines[1].starts_with("net: web                     reach public hosts"));
    // `+net any` is every host, never a host named any.
    let any = go_nets(NETSURF, &[], &["any:80"], true)
        .unwrap()
        .expect("a scope");
    let s = Scope::decode(&any.1).unwrap();
    assert_eq!(s.reach_rules().next().unwrap().target, Target::Any);
}

const WGET: &str = include_str!("../../../user/describe/posix/wget.cmd");

/// The reach rules of a line's scope, owned.
fn targets(scope: &[u8]) -> Vec<String> {
    Scope::decode(scope)
        .unwrap()
        .reach_rules()
        .map(|r| alloc::format!("{:?} {:?} {}", r.target, r.ports.list(), r.protocols.raw()))
        .collect()
}

/// ADR-0328 (INV-0328-1): under the description's `net web`, a literal URL
/// whose host is a name on port 80 or 443 adds no rule of its own: the web
/// rule admits the name through its public answers only, so a later answer
/// in the local network is refused (the rebinding of the 9f-net review,
/// finding 2).
#[test]
fn under_web_a_url_name_on_80_or_443_adds_no_rule() {
    use zoraya_abi::net::{NameGrant, Target};
    for (url, port) in [
        ("http://evil.example/cgi-bin/reboot", 80),
        ("https://Evil.Example/", 443),
        ("http://evil.example:443/", 443),
    ] {
        let ok = go(NETSURF, &[(url, true)], true).unwrap().expect("a scope");
        let s = Scope::decode(&ok.1).unwrap();
        let rules: Vec<_> = s.reach_rules().collect();
        assert_eq!(rules.len(), 1, "{url}");
        assert_eq!(rules[0].target, Target::Public, "{url}");
        assert!(
            !s.allows_name("evil.example", port, Protocols::TCP),
            "{url}"
        );
        assert_eq!(
            s.name_grant("evil.example", port, Protocols::TCP),
            NameGrant::PublicOnly,
            "{url}"
        );
        assert_eq!(s.resolve_grant("evil.example"), NameGrant::PublicOnly);
        assert_eq!(crate::netgrant::pills(&ok.1), vec![String::from("web")]);
        assert_eq!(crate::netgrant::describe(&ok.1).len(), 1);
    }
}

/// ADR-0328 (INV-0328-1): on another port, a public-only name rule for the
/// URL's name on that port alone, TCP; its pill is `+net HOST:PORT public` and
/// `grants` says the answers are public only.
#[test]
fn under_web_a_url_name_on_another_port_adds_a_public_only_rule() {
    use zoraya_abi::net::{NameGrant, Target};
    let ok = go(
        NETSURF,
        &[("http://Two.Web.Test:8080/ns/web-port", true)],
        true,
    )
    .unwrap()
    .expect("a scope");
    let s = Scope::decode(&ok.1).unwrap();
    let rules: Vec<_> = s.reach_rules().collect();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].target, Target::PublicName("two.web.test"));
    assert_eq!(rules[0].ports.list(), &[(8080, 8080)]);
    assert_eq!(rules[0].protocols, Protocols::TCP);
    assert_eq!(rules[1].target, Target::Public);
    assert_eq!(
        s.name_grant("two.web.test", 8080, Protocols::TCP),
        NameGrant::PublicOnly
    );
    assert!(!s.allows_name("two.web.test", 8080, Protocols::TCP));
    assert_eq!(
        s.name_grant("other.test", 8080, Protocols::TCP),
        NameGrant::No
    );
    assert_eq!(
        s.name_grant("two.web.test", 8081, Protocols::TCP),
        NameGrant::No
    );
    assert_eq!(
        s.name_grant("two.web.test", 8080, Protocols::UDP),
        NameGrant::No
    );
    for a in [[10, 0, 2, 2], [192, 168, 1, 1], [203, 0, 113, 11]] {
        assert!(!s.names_address(a, 8080, Protocols::TCP));
        assert!(!s.allows_address(a, 8080, Protocols::TCP));
    }
    assert_eq!(
        crate::netgrant::pills(&ok.1),
        vec![
            String::from("+net two.web.test:8080 public"),
            String::from("web")
        ]
    );
    assert_eq!(
        crate::netgrant::describe(&ok.1)[0],
        "net: two.web.test:8080       reach, public answers only (operand URL)"
    );
}

/// ADR-0328: a dotted address keeps its own rule (it does not rebind), a
/// `+net` word that covers the URL keeps ADR-0223's rule (the person named
/// the host, local answers included), a URL from a value still needs a word,
/// and the native `fetch` keeps the URL's name rule.
#[test]
fn under_web_an_address_a_net_word_and_programs_without_web_keep_their_rules() {
    use zoraya_abi::net::{NameGrant, Target};
    let address = go(NETSURF, &[("http://192.168.1.1/", true)], true)
        .unwrap()
        .expect("a scope");
    let s = Scope::decode(&address.1).unwrap();
    assert!(s.names_address([192, 168, 1, 1], 80, Protocols::TCP));
    for word in [
        "intranet",
        "intranet:8080",
        "*.intranet:8080",
        "intranet:80",
    ] {
        let url = if word.ends_with(":80") {
            "http://intranet/"
        } else if word.starts_with('*') {
            "http://a.intranet:8080/"
        } else {
            "http://intranet:8080/"
        };
        let host = url
            .trim_start_matches("http://")
            .split([':', '/'])
            .next()
            .unwrap();
        let port = if url.contains(":8080") { 8080 } else { 80 };
        let ok = go_nets(NETSURF, &[(url, true)], &[word], true)
            .unwrap()
            .expect("a scope");
        let s = Scope::decode(&ok.1).unwrap();
        assert_eq!(
            s.name_grant(host, port, Protocols::TCP),
            NameGrant::Named,
            "{word}"
        );
        assert!(
            s.reach_rules()
                .all(|r| !matches!(r.target, Target::PublicName(_))),
            "{word}"
        );
    }
    assert!(go(NETSURF, &[("http://example.org:8080/", false)], true).is_err());
    for url in ["http://example.org/", "http://example.org:8080/"] {
        let ok = go(FETCH, &[(url, true)], true).unwrap().expect("a scope");
        let t = targets(&ok.1);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!(t[0].starts_with("Name(\"example.org\")"), "{t:?}");
    }
    // Outside the image (`base_system` false) a `url` operand gives no rule
    // at all: `net_for` reads operands only for the image's descriptions.
    let outside = go(NETSURF, &[("http://example.org:8080/", true)], false).unwrap();
    assert_eq!(outside, None);
}

/// `grants` of a public-only name on any port (a scope narrowed by hand;
/// `net_for` always gives the URL's one port) says so (9g-url review, note 5).
#[test]
fn grants_says_a_public_only_name_on_any_port() {
    use zoraya_abi::net::{Ports, ReachRule, Target, encode};
    let rule = ReachRule {
        target: Target::PublicName("example.org"),
        ports: Ports::ANY,
        protocols: Protocols::TCP,
    };
    let mut out = vec![0u8; 128];
    let n = encode(&[rule], &[], &mut out).unwrap();
    assert_eq!(
        crate::netgrant::describe(&out[..n]),
        vec![String::from(
            "net: example.org             reach, any port, public answers only (operand URL)"
        )]
    );
    assert_eq!(
        crate::netgrant::pills(&out[..n]),
        vec![String::from("+net example.org public")]
    );
}

/// The pill of a public-only name is 7 bytes longer than a `+net` word's:
/// for a host of 196 bytes on a five-digit port it passes the panel's
/// 200-byte limit, and the panel refuses the window (no window, never a cut
/// pill: 9g-url review, note 3; `zoraya-panel`'s test of the same line).
#[test]
fn a_public_only_pill_past_the_panels_limit_is_long() {
    let label = "a".repeat(62);
    let host = alloc::format!("{label}.{label}.{label}.example");
    assert_eq!(host.len(), 196);
    let url = alloc::format!("http://{host}:65535/");
    let ok = go(NETSURF, &[(&url, true)], true)
        .unwrap()
        .expect("a scope");
    let pills = crate::netgrant::pills(&ok.1);
    assert_eq!(pills[0], alloc::format!("+net {host}:65535 public"));
    assert!(pills[0].len() > 200, "{}", pills[0].len());
}

/// A host longer than the `grants` column keeps a space before what
/// follows, so its pill is the host alone (`pills` takes the first word of
/// the line); before 9g-url a 24-byte or longer target ran into "reach".
#[test]
fn a_long_host_keeps_its_pill_and_its_grants_line_apart() {
    let ok = go_nets(NET, &[], &["a-very-long-host-name.example.org:8080"], true)
        .unwrap()
        .expect("a scope");
    assert_eq!(
        crate::netgrant::pills(&ok.1),
        vec![String::from("+net a-very-long-host-name.example.org:8080")]
    );
    assert_eq!(
        crate::netgrant::describe(&ok.1),
        vec![String::from(
            "net: a-very-long-host-name.example.org:8080 reach (grant +net)"
        )]
    );
}

/// The architect's decision of 2026-11-14 on ADR-0328's open question: curl
/// and wget (no `net web`) take a literal URL's name with its public answers
/// only, on its port, 80 and 443 included; a dotted address keeps its rule,
/// a `+net` word that covers the URL keeps ADR-0223's rule, a URL from a
/// value still needs a word.
#[test]
fn curl_and_wget_take_a_url_name_with_public_answers_only() {
    use zoraya_abi::net::{NameGrant, Target};
    for d in [CURL, WGET] {
        for (url, port) in [
            ("http://Test.Zoraya/", 80),
            ("https://test.zoraya/", 443),
            ("https://test.zoraya:8443/x", 8443),
        ] {
            let ok = go(d, &[(url, true)], true).unwrap().expect("a scope");
            assert_eq!(ok.0, rights::REACH);
            let s = Scope::decode(&ok.1).unwrap();
            let rules: Vec<_> = s.reach_rules().collect();
            assert_eq!(rules.len(), 1, "{url}");
            assert_eq!(rules[0].target, Target::PublicName("test.zoraya"), "{url}");
            assert_eq!(rules[0].ports.list(), &[(port, port)], "{url}");
            assert_eq!(rules[0].protocols, Protocols::TCP);
            assert_eq!(
                s.name_grant("test.zoraya", port, Protocols::TCP),
                NameGrant::PublicOnly,
                "{url}"
            );
            assert!(!s.allows_name("test.zoraya", port, Protocols::TCP));
            assert_eq!(s.resolve_grant("test.zoraya"), NameGrant::PublicOnly);
            assert!(!s.reaches_address([10, 0, 2, 2]));
            assert_eq!(
                crate::netgrant::pills(&ok.1),
                vec![alloc::format!("+net test.zoraya:{port} public")]
            );
            // A +net word that names the host: ADR-0223's rule, no public-only
            // rule beside it.
            let named = go_nets(d, &[(url, true)], &["test.zoraya"], true)
                .unwrap()
                .expect("a scope");
            let s = Scope::decode(&named.1).unwrap();
            assert_eq!(
                s.name_grant("test.zoraya", port, Protocols::TCP),
                NameGrant::Named
            );
            assert!(
                s.reach_rules()
                    .all(|r| !matches!(r.target, Target::PublicName(_)))
            );
        }
        // A dotted address keeps its own rule, a private one included.
        let address = go(d, &[("http://10.0.2.2:8080/", true)], true)
            .unwrap()
            .expect("a scope");
        let s = Scope::decode(&address.1).unwrap();
        assert!(s.names_address([10, 0, 2, 2], 8080, Protocols::TCP));
        // From a value: refused without a word.
        assert!(go(d, &[("http://test.zoraya/", false)], true).is_err());
    }
}

const SSH: &str = include_str!("../../../user/describe/posix/ssh.cmd");

/// ADR-0340 item 4, INV-0340-1: ssh's network scope is the host its operand
/// names, on port 22 or the port `-p` gives, and nothing else.
#[test]
fn ssh_reaches_the_host_it_names_on_port_22_or_the_ports_option() {
    use zoraya_abi::net::Target;
    // An address keeps its own rule; the word after HOST is the far side's.
    let ok = go(
        SSH,
        &[
            ("-l", true),
            ("ada", true),
            ("10.0.2.50", true),
            ("ls", true),
        ],
        true,
    )
    .unwrap()
    .expect("a scope");
    assert_eq!(ok.0, rights::REACH);
    let s = Scope::decode(&ok.1).unwrap();
    assert!(s.names_address([10, 0, 2, 50], 22, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 50], 23, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 50], 22, Protocols::UDP));
    assert!(!s.names_address([10, 0, 2, 51], 22, Protocols::TCP));
    assert!(!s.allows_echo_address([10, 0, 2, 50]));
    // The first -p is the port, as OpenSSH reads it (review of 10g, finding 6).
    let first = go(
        SSH,
        &[
            ("-p", true),
            ("2222", true),
            ("-p", true),
            ("22", true),
            ("10.0.2.50", true),
        ],
        true,
    )
    .unwrap()
    .expect("a scope");
    let s = Scope::decode(&first.1).unwrap();
    assert!(s.names_address([10, 0, 2, 50], 2222, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 50], 22, Protocols::TCP));
    // -p replaces the port.
    let ok = go(
        SSH,
        &[("-p", true), ("2222", true), ("ada@10.0.2.50", true)],
        true,
    )
    .unwrap()
    .expect("a scope");
    let s = Scope::decode(&ok.1).unwrap();
    assert!(s.names_address([10, 0, 2, 50], 2222, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 50], 22, Protocols::TCP));
    // A name: public answers only, as for curl; USER@ is not part of it.
    let ok = go(
        SSH,
        &[(concat!("ada", "@", "Build.Example.ORG"), true)],
        true,
    )
    .unwrap()
    .unwrap();
    let s = Scope::decode(&ok.1).unwrap();
    let rules: Vec<_> = s.reach_rules().collect();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].target, Target::PublicName("build.example.org"));
    assert_eq!(rules[0].ports.list(), &[(22, 22)]);
    assert_eq!(rules[0].protocols, Protocols::TCP);
    // Words after the host never add a host: `-J` and `-o` are the far
    // side's words, and the host stays the one typed.
    let ok = go(
        SSH,
        &[
            ("10.0.2.50", true),
            ("-J", true),
            ("evil.example.org", true),
            ("-o", true),
            ("x", true),
        ],
        true,
    )
    .unwrap()
    .unwrap();
    let s = Scope::decode(&ok.1).unwrap();
    assert_eq!(s.reach_rules().count(), 1);
    assert!(!s.allows_name("evil.example.org", 22, Protocols::TCP));
}

#[test]
fn ssh_from_a_value_needs_a_net_word_and_a_bad_port_gives_nothing() {
    let r = go(
        SSH,
        &[("-l", true), ("ada", true), ("other.example.org", false)],
        true,
    )
    .unwrap_err();
    assert!(r.problem.contains("from a value"), "{}", r.problem);
    assert_eq!(
        r.hint.as_deref(),
        Some("if that is meant: +net other.example.org:22 COMMAND")
    );
    // A `+net HOST:22` word covers it; one for another port does not.
    let covered = go_nets(
        SSH,
        &[("other.example.org", false)],
        &["other.example.org:22"],
        true,
    )
    .unwrap()
    .expect("a scope");
    assert!(Scope::decode(&covered.1).unwrap().allows_name(
        "other.example.org",
        22,
        Protocols::TCP
    ));
    assert!(
        go_nets(
            SSH,
            &[("other.example.org", false)],
            &["other.example.org:80"],
            true
        )
        .is_err()
    );
    // A port that is no number: no grant, ssh says so itself.
    for bad in ["0", "65536", "x", "-1"] {
        let r = go(SSH, &[("-p", true), (bad, true), ("10.0.2.50", true)], true);
        assert!(matches!(r, Ok(None)), "-p {bad}: {r:?}");
    }
    // `ssh://` names user and port by itself: a usage mistake (finding 14).
    let r = go(SSH, &[("ssh://ada@10.0.2.50:2222", true)], true).unwrap_err();
    assert!(r.problem.contains("not a host"), "{}", r.problem);
    // Only the system image's description reaches (INV-0224-1).
    assert!(go(SSH, &[("10.0.2.50", true)], false).unwrap().is_none());
    // A name that is no host.
    assert!(go(SSH, &[("a b", true)], true).unwrap().is_none());
}

/// ADR-0462: a room's words make the scope its terminals' connection has.
#[test]
fn a_rooms_net_words_make_the_scope_of_its_terminals() {
    use crate::netgrant::words_scope;
    assert!(words_scope(&[]).unwrap().is_none());
    let words = [
        String::from("10.0.2.50:22"),
        String::from("build.example.org:2222"),
    ];
    let (r, bytes) = words_scope(&words).unwrap().expect("a scope");
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.names_address([10, 0, 2, 50], 22, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 50], 23, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 51], 22, Protocols::TCP));
    assert!(s.allows_name("build.example.org", 2222, Protocols::TCP));
    assert!(!s.allows_name("build.example.org", 22, Protocols::TCP));
    assert!(words_scope(&[String::from("a b")]).is_err());
}

fn go_room(
    text: &str,
    args: &[(&str, bool)],
    nets: &[&str],
    room: Option<(&str, &[&str])>,
) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    let d: Description = zoraya_describe::parse(text.as_bytes()).unwrap();
    let values: Vec<Value> = args
        .iter()
        .map(|(a, literal)| Value {
            text: String::from(*a),
            literal: *literal,
        })
        .collect();
    let nets: Vec<Value> = nets.iter().map(|a| Value::literal(a)).collect();
    let words: Vec<String> = room
        .map(|(_, w)| w.iter().map(|w| String::from(*w)).collect())
        .unwrap_or_default();
    let texts: Vec<&str> = values.iter().map(|v| v.text.as_str()).collect();
    let parsed: Parsed<'_> = match zoraya_command::parse(&d, &texts) {
        Ok(Outcome::Run(p)) => p,
        _ => Parsed::default(),
    };
    net_for(&Line {
        description: &d,
        parsed: &parsed,
        args: &values,
        cwd: "/",
        grants: &[],
        nets: &nets,
        listens: &[],
        socks: &[],
        base_system: true,
        room: room.map(|(name, _)| crate::grants::RoomBound {
            name,
            words: &words,
        }),
    })
}

/// ADR-0462, the decision of the fix pass: a room's words BOUND a line and
/// give it nothing. A line with no designation and no `+net` gets no network
/// (so no carved account), whatever the room holds.
#[test]
fn a_rooms_words_bound_a_line_and_give_it_nothing() {
    const CAT: &str = include_str!("../../../user/describe/commands/cat.cmd");
    let room = Some(("ops", &["10.0.2.50:22", "build.example.org:22"][..]));
    // `cat x` and `ssh --help` ask for nothing: no connection.
    assert!(go_room(CAT, &[("x", true)], &[], room).unwrap().is_none());
    assert!(
        go_room(SSH, &[("--help", true)], &[], room)
            .unwrap()
            .is_none()
    );
    // ssh's own designation, inside the words: the operand's rule only.
    let (r, bytes) = go_room(
        SSH,
        &[("-l", true), ("u", true), ("10.0.2.50", true)],
        &[],
        room,
    )
    .unwrap()
    .expect("a scope");
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.names_address([10, 0, 2, 50], 22, Protocols::TCP));
    assert_eq!(
        s.reach_rules().count(),
        1,
        "the room's other word is not given"
    );
    assert!(!s.allows_name("build.example.org", 22, Protocols::TCP));
    // A name inside a word: its public answers only, as everywhere.
    let ok = go_room(
        SSH,
        &[("-l", true), ("u", true), ("build.example.org", true)],
        &[],
        room,
    )
    .unwrap()
    .expect("a scope");
    assert_eq!(Scope::decode(&ok.1).unwrap().reach_rules().count(), 1);
    // A value inside the words designates (it is in the room's words, not the line's).
    assert!(
        go_room(
            SSH,
            &[("-l", true), ("u", true), ("10.0.2.50", false)],
            &[],
            room
        )
        .unwrap()
        .is_some()
    );
    // Outside the words: refused, naming them.
    for args in [
        &[("-l", true), ("u", true), ("10.0.2.51", true)][..],
        &[("-p", true), ("2222", true), ("10.0.2.50", true)][..],
        &[("-l", true), ("u", true), ("other.example.org", true)][..],
        &[("-l", true), ("u", true), ("10.0.2.51", false)][..],
    ] {
        let r = go_room(SSH, args, &[], room).unwrap_err();
        assert!(
            r.problem.contains("outside the network words of room ops")
                || r.problem.contains("from a value"),
            "{}",
            r.problem
        );
        if r.problem.contains("outside the network words") {
            let hint = r.hint.unwrap();
            assert!(
                hint.contains("+net 10.0.2.50:22") && hint.contains("+net build.example.org:22"),
                "{hint}"
            );
        }
    }
    // A `+net` word of the line must lie inside the room's too.
    assert!(
        go_room(CAT, &[("x", true)], &["10.0.2.50:22"], room)
            .unwrap()
            .is_some()
    );
    let r = go_room(CAT, &[("x", true)], &["any"], room).unwrap_err();
    assert!(
        r.problem.contains("outside the network words"),
        "{}",
        r.problem
    );
    // A room with no words: nothing is given, and a designation is refused with that said.
    let none = Some(("plain", &[][..]));
    assert!(go_room(CAT, &[("x", true)], &[], none).unwrap().is_none());
    let r = go_room(
        SSH,
        &[("-l", true), ("u", true), ("10.0.2.50", true)],
        &[],
        none,
    )
    .unwrap_err();
    assert_eq!(r.hint.as_deref(), Some("room plain has no +net words"));
    // A word of the room that is no word (a hand edit) bounds nothing and spoils nothing: the
    // other words stand and a line that asks for no network is unaffected (finding 9).
    let bad = Some(("ops", &["h:0", "10.0.2.50:22"][..]));
    assert!(go_room(CAT, &[("x", true)], &[], bad).unwrap().is_none());
    assert!(
        go_room(
            SSH,
            &[("-l", true), ("u", true), ("10.0.2.50", true)],
            &[],
            bad
        )
        .unwrap()
        .is_some()
    );
    // At home (no room) nothing changes.
    assert!(
        go_room(
            SSH,
            &[("-l", true), ("u", true), ("10.0.2.51", true)],
            &[],
            None
        )
        .unwrap()
        .is_some()
    );
}

/// A room made inside a room holds no more than the room (INV-0172-4), by the rules.
#[test]
fn a_word_is_inside_a_room_by_the_rules_not_the_text() {
    use crate::netgrant::word_within;
    let room = [
        String::from("*.example.org:22"),
        String::from("10.0.2.0/24"),
    ];
    assert!(word_within("api.example.org:22", &room));
    assert!(word_within("API.example.org:22", &room));
    assert!(word_within("10.0.2.50:2222", &room));
    assert!(!word_within("api.example.org:23", &room));
    assert!(!word_within("example.org:22", &room));
    assert!(!word_within("any", &room));
    assert!(!word_within("a b", &room));
    assert!(!word_within("10.0.2.50", &[]));
}

/// ADR-0462 item 5: no status in a room, with a hint; at home it is given.
#[test]
fn net_status_is_refused_in_a_room() {
    const NETCMD: &str = include_str!("../../../user/describe/commands/net.cmd");
    let room = Some(("ops", &["10.0.2.50:22"][..]));
    let r = go_room(NETCMD, &[("status", true)], &[], room).unwrap_err();
    assert_eq!(r.problem, "the network's status is not given in a room");
    assert!(r.hint.unwrap().contains("room --leave"));
    let none = Some(("plain", &[][..]));
    assert!(go_room(NETCMD, &[("status", true)], &[], none).is_err());
    let (rights_given, _) = go_room(NETCMD, &[("status", true)], &[], None)
        .unwrap()
        .expect("at home");
    assert_eq!(rights_given, rights::STATUS);
}

/// ADR-0462 item 4: what the line is told at `netd`'s account limit, and for another refusal.
#[test]
fn the_account_limit_has_its_own_refusal_and_hint() {
    use crate::netgrant::connection_refusal;
    let (problem, hint) = connection_refusal(true);
    assert_eq!(
        problem,
        "the network has no room for another program's connection"
    );
    assert!(hint.contains("wait for one to end"));
    let (problem, hint) = connection_refusal(false);
    assert!(problem.contains("refused"));
    assert!(hint.contains("+net words"));
}

/// The panel's scope for a room: the words that parse; a bad word spoils no other (finding 9).
#[test]
fn a_rooms_scope_skips_a_bad_word_and_keeps_the_good() {
    use crate::netgrant::room_scope;
    let words = |w: &[&str]| w.iter().map(|w| String::from(*w)).collect::<Vec<_>>();
    assert!(room_scope(&[]).is_none());
    assert!(room_scope(&words(&["h:0", "a b"])).is_none());
    let (r, bytes) = room_scope(&words(&["h:0", "10.0.2.50:22"])).expect("the good word");
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.names_address([10, 0, 2, 50], 22, Protocols::TCP));
    assert_eq!(s.reach_rules().count(), 1);
}

#[test]
fn a_listen_word_gives_the_listen_right_and_a_listen_rule_and_no_reach() {
    use crate::netgrant::{describe, parse_listen};
    let (r, bytes) = scope_of(go_all(CAT, &[], &[], &["8000"], false).unwrap());
    assert_eq!(r, rights::LISTEN);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_listen(8000, Protocols::TCP) && s.allows_listen(8000, Protocols::UDP));
    assert!(!s.allows_listen(8001, Protocols::TCP) && !s.allows_listen(7999, Protocols::TCP));
    assert_eq!(s.reach_rules().count(), 0, "a listener reaches nothing");
    assert_eq!(
        describe(&bytes),
        vec![String::from(
            "net: port 8000               listen (grant +listen)"
        )]
    );
    // With a `+net` word: both rights, the reach rules first in `grants`.
    let (r, bytes) =
        scope_of(go_all(CAT, &[], &["127.0.0.1:8000"], &["8000-8002", "9000"], false).unwrap());
    assert_eq!(r, rights::REACH | rights::LISTEN);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.allows_listen(8002, Protocols::TCP) && s.allows_listen(9000, Protocols::TCP));
    assert!(!s.allows_listen(8003, Protocols::TCP));
    let lines = describe(&bytes);
    assert_eq!(lines.len(), 3);
    assert!(
        lines[0].contains("127.0.0.1:8000") && lines[0].contains("reach"),
        "{lines:?}"
    );
    assert!(
        lines[1].contains("ports 8000-8002") && lines[2].contains("port 9000"),
        "{lines:?}"
    );
    // The pills of the reach rules still pair with their own lines.
    assert_eq!(
        crate::netgrant::pills(&bytes),
        vec![String::from("+net 127.0.0.1:8000")]
    );
    // The word is a port or a range, 1 to 65535, and nothing else.
    for bad in [
        "",
        "0",
        "65536",
        "-1",
        "80-",
        "-80",
        "90-80",
        "a",
        "80,81",
        "any",
        "80-90-100",
        " 80",
    ] {
        assert!(parse_listen(bad).is_err(), "{bad:?}");
        let e = go_all(CAT, &[], &[], &[bad], false).unwrap_err();
        assert_eq!(e.subject, format!("+listen {bad}"));
    }
    assert!(
        parse_listen("65535").is_ok()
            && parse_listen("1").is_ok()
            && parse_listen("1-65535").is_ok()
    );
    // Total on any text.
    for text in ["\u{0}", "9999999999999999999", "1-2-3", "\u{10ffff}"] {
        let _ = parse_listen(text);
    }
}

#[test]
fn no_words_and_no_listen_word_give_no_listen_right() {
    let (r, bytes) = scope_of(go_all(CAT, &[], &["example.org:443"], &[], false).unwrap());
    assert_eq!(r, rights::REACH);
    assert_eq!(Scope::decode(&bytes).unwrap().listen_count(), 0);
    assert_eq!(go_all(CAT, &[], &[], &[], false).unwrap(), None);
}

/// The loopback is the room's own (ADR-0442): a line may reach it in any room, words or none.
#[test]
fn the_loopback_asks_nothing_of_a_rooms_words() {
    let none = Some(("ra", &[][..]));
    let got = go_room(
        SSH,
        &[("-l", true), ("u", true), ("127.0.0.1", true)],
        &[],
        none,
    )
    .unwrap()
    .expect("a scope");
    assert!(
        Scope::decode(&got.1)
            .unwrap()
            .names_address([127, 0, 0, 1], 22, Protocols::TCP)
    );
    let got = go_room(
        SSH,
        &[("-l", true), ("u", true), ("10.0.2.50", true)],
        &["127.0.0.1"],
        none,
    );
    assert!(got.is_err(), "the other address still is");
}

const GIT_CLONE: &str = include_str!("../../../user/describe/posix/git-clone.cmd");
const GIT_LS_REMOTE: &str = include_str!("../../../user/describe/posix/git-ls-remote.cmd");
const GIT: &str = include_str!("../../../user/describe/posix/git.cmd");

/// ADR-0339 item 6, INV-0339-2: a typed URL gives its host and port; a line with no URL (`git push`,
/// `git fetch`) gives none, whatever a repository's configuration says.
#[test]
fn git_clone_reaches_the_host_of_its_url_and_a_push_reaches_nothing() {
    use zoraya_abi::net::Target;
    let ok = go(GIT_CLONE, &[("https://example.org/ada/p.git", true)], true)
        .unwrap()
        .expect("a scope");
    assert_eq!(ok.0, rights::REACH);
    let s = Scope::decode(&ok.1).unwrap();
    let rules: Vec<_> = s.reach_rules().collect();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].target, Target::PublicName("example.org"));
    assert_eq!(rules[0].ports.list(), &[(443, 443)]);
    // A port in the address is the port.
    let ok = go(
        GIT_LS_REMOTE,
        &[("https://10.0.2.2:8443/r.git", true)],
        true,
    )
    .unwrap()
    .unwrap();
    let s = Scope::decode(&ok.1).unwrap();
    assert!(s.names_address([10, 0, 2, 2], 8443, Protocols::TCP));
    assert!(!s.names_address([10, 0, 2, 2], 443, Protocols::TCP));
    // From a value: refused without a +net word.
    assert!(go(GIT_CLONE, &[("https://example.org/p.git", false)], true).is_err());
    // `git push`, `fetch`, `status`: no URL operand, no network.
    for words in [
        &["push"][..],
        &["fetch", "origin"],
        &["status"],
        &["pull", "https://example.org/x"],
    ] {
        let args: Vec<(&str, bool)> = words.iter().map(|w| (*w, true)).collect();
        assert!(go(GIT, &args, true).unwrap().is_none(), "{words:?}");
    }
    // With the word on the line, the host is given to the program as the word says.
    let ok = go_nets(GIT, &[("push", true)], &["example.org:443"], true)
        .unwrap()
        .expect("a scope");
    let s = Scope::decode(&ok.1).unwrap();
    assert_eq!(s.reach_rules().count(), 1);
}
