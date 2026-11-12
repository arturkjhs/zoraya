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
const NET: &str = include_str!("../../../user/describe/commands/net.cmd");
const CAT: &str = include_str!("../../../user/describe/commands/cat.cmd");

fn go(text: &str, args: &[(&str, bool)], base: bool) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    let d: Description = zoraya_describe::parse(text.as_bytes()).unwrap();
    let values: Vec<Value> = args
        .iter()
        .map(|(a, literal)| Value {
            text: String::from(*a),
            literal: *literal,
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
