//! The record `netd` keeps for each network connection (ADR-0220 item 1):
//! a scope and rights that only narrow. `decide` is the one place a request
//! is checked against them, so a request kind added later is checked in the
//! same function.

use alloc::vec::Vec;
use zoraya_abi::net::{Scope, rights};

/// What a request asks, reduced to what the check needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// `status`.
    Status,
    /// An echo run to an address.
    Ping { address: [u8; 4] },
    /// `narrow` with the rights and the encoded scope asked for.
    Narrow { rights: u32, scope: &'a [u8] },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allowed,
    /// The right is missing, or the target lies outside the scope.
    NotGranted,
    /// The scope does not decode, or a right bit is unknown.
    Invalid,
}

/// A connection's scope (encoded, checked at creation) and rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    scope: Vec<u8>,
    rights: u32,
}

impl Connection {
    /// The root connection `init` hands a launcher: every right, scope `any`
    /// on every port and protocol (ADR-0221 item 4).
    pub fn root() -> Connection {
        use zoraya_abi::net::{ListenRule, Ports, Protocols, ReachRule, Target};
        let mut bytes = [0u8; 64];
        let reach = ReachRule {
            target: Target::Any,
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        };
        let listen = ListenRule {
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        };
        let n = zoraya_abi::net::encode(&[reach], &[listen], &mut bytes).expect("the root scope");
        Connection {
            scope: bytes[..n].to_vec(),
            rights: rights::ALL,
        }
    }

    /// A connection with `rights` and an encoded `scope`; `None` if the scope
    /// does not decode or a right bit is unknown.
    pub fn new(rights: u32, scope: &[u8]) -> Option<Connection> {
        Scope::decode(scope)?;
        (rights & !rights::ALL == 0).then(|| Connection {
            scope: scope.to_vec(),
            rights,
        })
    }

    pub fn rights(&self) -> u32 {
        self.rights
    }

    pub fn scope(&self) -> Scope<'_> {
        // Checked in `new` and `root`, and never changed.
        Scope::decode(&self.scope).unwrap_or(Scope::EMPTY)
    }

    pub fn decide(&self, request: Request<'_>) -> Verdict {
        match request {
            Request::Status => verdict(self.rights & rights::STATUS != 0),
            Request::Ping { address } => {
                verdict(self.rights & rights::REACH != 0 && self.scope().reaches_address(address))
            }
            Request::Narrow { rights, scope } => {
                if self.rights & rights::NARROW == 0 {
                    return Verdict::NotGranted;
                }
                let Some(child) = Scope::decode(scope) else {
                    return Verdict::Invalid;
                };
                if rights & !rights::ALL != 0 {
                    return Verdict::Invalid;
                }
                // Never a right the caller lacks, never a scope outside its own.
                verdict(rights & !self.rights == 0 && child.within(&self.scope()))
            }
        }
    }

    /// The connection a successful `narrow` makes; `None` if `decide` would
    /// not allow it.
    pub fn narrowed(&self, rights: u32, scope: &[u8]) -> Option<Connection> {
        (self.decide(Request::Narrow { rights, scope }) == Verdict::Allowed)
            .then(|| Connection::new(rights, scope))?
    }
}

fn verdict(allowed: bool) -> Verdict {
    if allowed {
        Verdict::Allowed
    } else {
        Verdict::NotGranted
    }
}
