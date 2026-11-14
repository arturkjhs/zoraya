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
        base_system: base,
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
/// and curl and wget (no `net web`) keep the URL's name rule.
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
    for d in [CURL, WGET] {
        for url in ["http://example.org/", "http://example.org:8080/"] {
            let ok = go(d, &[(url, true)], true).unwrap().expect("a scope");
            let t = targets(&ok.1);
            assert_eq!(t.len(), 1, "{t:?}");
            assert!(t[0].starts_with("Name(\"example.org\")"), "{t:?}");
        }
    }
    // A description outside the image has no web rule, so the URL keeps its
    // name rule there.
    let outside = go(NETSURF, &[("http://example.org:8080/", true)], false).unwrap();
    assert_eq!(outside, None);
}
