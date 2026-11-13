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
fn a_literal_host_operand_reaches_that_host_and_nothing_else() {
    let (r, bytes) =
        scope_of(go(PING, &[("-c", true), ("2", true), ("10.0.2.2", true)], true).unwrap());
    assert_eq!(r, rights::REACH);
    let s = Scope::decode(&bytes).unwrap();
    assert!(s.reaches_address([10, 0, 2, 2]));
    assert!(!s.reaches_address([10, 0, 2, 3]));
    // The rule is TCP, any port, of that address and nothing else (the extra
    // right ADR-0330 states): not UDP, not another host.
    assert!(s.allows_address([10, 0, 2, 2], 1, Protocols::TCP));
    assert!(s.allows_address([10, 0, 2, 2], 65535, Protocols::TCP));
    assert!(!s.allows_address([10, 0, 2, 2], 53, Protocols::UDP));
    assert!(!s.allows_address([10, 0, 2, 3], 80, Protocols::TCP));
    assert_eq!(
        crate::netgrant::describe(&bytes),
        [format!(
            "net: {:<24}reach, any port (operand HOST)",
            "10.0.2.2"
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
    let covered = go_nets(PING, &[("10.0.2.2", false)], &["10.0.2.0/24"], true).unwrap();
    assert!(
        Scope::decode(&scope_of(covered).1)
            .unwrap()
            .reaches_address([10, 0, 2, 2])
    );
    // Only the base system's commands are given a network by an operand.
    assert!(go(PING, &[("10.0.2.2", true)], false).unwrap().is_none());
}

#[test]
fn a_host_operand_that_is_not_a_dotted_address_makes_no_grant() {
    for name in ["example.org", "10.0.2", "10.0.2.2.2", "any", "*.org"] {
        let r = go(PING, &[(name, true)], true).unwrap();
        assert!(r.is_none(), "{name} made a grant");
    }
}
