//! The network connection a base-system command gets (ADR-0224 items 2 and
//! 5; N3): none, the status of the network (`needs net-status`), or `reach`
//! of the host and port of a `url` operand, as `(rights, scope)` for
//! `narrow`. Pure, so every rule is tested on the host.
//!
//! Only a *literal* `url` operand designates, as only a literal path does
//! (ADR-0153 rule 4): one that came from a variable, a substitution or a
//! glob would let data choose what the command reaches, and is refused
//! before the line runs. ADR-0224 item 2 lets such an operand designate
//! inside a `+net` rule on the line or in the room; those words are not
//! built (N3b), so a value is always outside them.

use alloc::vec::Vec;

use zoraya_abi::net::{Ports, Protocols, ReachRule, Scope, Target, encode, rights};
use zoraya_describe::Kind;

use crate::grants::{Line, Refused, value_of};

/// The rights and scope to `narrow` the shell's connection to, if the
/// command gets a connection at all. A URL that does not parse gives none:
/// the command says what is wrong with it.
pub fn net_for(line: &Line<'_>) -> Result<Option<(u32, Vec<u8>)>, Refused> {
    let d = line.description;
    if !line.base_system {
        return Ok(None);
    }
    if d.needs_net_status {
        return Ok(Some((rights::STATUS, Scope::EMPTY.as_bytes().to_vec())));
    }
    let Some((_, text)) = line
        .parsed
        .operands
        .iter()
        .find(|(i, _)| d.operands[*i].kind == Kind::Url)
    else {
        return Ok(None);
    };
    // A text that is no argument's is taken as from a value: fails closed.
    if !value_of(line.args, text).is_some_and(|v| v.literal) {
        return Err(Refused {
            subject: (*text).into(),
            problem: "from a value, outside the line's network grants".into(),
            hint: Some(
                "write the address on the line itself; the +net words that would allow a value are not built yet"
                    .into(),
            ),
        });
    }
    let Ok(url) = zoraya_http::parse_url(text) else {
        return Ok(None);
    };
    let host = url.host.to_ascii_lowercase();
    let target = match zoraya_http::parse_ipv4(&host) {
        Some(a) => Target::Address(a),
        None => Target::Name(&host),
    };
    let rule = ReachRule {
        target,
        ports: Ports::one(url.port),
        protocols: Protocols::TCP,
    };
    let mut out = alloc::vec![0u8; 512];
    Ok(encode(&[rule], &[], &mut out).map(|n| {
        out.truncate(n);
        (rights::REACH, out)
    }))
}
