//! The record `netd` keeps for each network connection (ADR-0220 item 1):
//! a scope and rights that only narrow. `decide` is the one place a request
//! is checked against them, so a request kind added later is checked in the
//! same function.

use crate::accounts::SESSION;
use alloc::vec::Vec;
use zoraya_abi::net::{NameGrant, Protocols, Scope, rights};

/// What a request asks, reduced to what the check needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request<'a> {
    /// `status`.
    Status,
    /// An echo run to an address.
    Ping { address: [u8; 4] },
    /// An echo run to a name, which `netd` resolves (ADR-0330).
    PingName { name: &'a str },
    /// A TCP connection to a name or an address and a port (N3).
    Connect {
        target: ConnectTarget<'a>,
        port: u16,
    },
    /// The addresses of a name (N4).
    Resolve { name: &'a str },
    /// `narrow` with the rights and the encoded scope asked for.
    Narrow { rights: u32, scope: &'a [u8] },
}

/// What a `connect` names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectTarget<'a> {
    Name(&'a str),
    Address([u8; 4]),
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
    /// The account it belongs to (ADR-0262; N4): its parent's, unless a
    /// `narrow_account` carved a new one.
    account: usize,
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
            protocols: Protocols::ALL,
        };
        let listen = ListenRule {
            ports: Ports::ANY,
            protocols: Protocols::BOTH,
        };
        let n = zoraya_abi::net::encode(&[reach], &[listen], &mut bytes).expect("the root scope");
        Connection {
            scope: bytes[..n].to_vec(),
            rights: rights::ALL,
            account: SESSION,
        }
    }

    /// A connection with `rights` and an encoded `scope`; `None` if the scope
    /// does not decode or a right bit is unknown.
    pub fn new(rights: u32, scope: &[u8]) -> Option<Connection> {
        Scope::decode(scope)?;
        (rights & !rights::ALL == 0).then(|| Connection {
            scope: scope.to_vec(),
            rights,
            account: SESSION,
        })
    }

    /// The encoded scope, to judge answers against later (a name that
    /// resolves to a local address, ADR-0223 item 4).
    pub fn scope_bytes(&self) -> &[u8] {
        &self.scope
    }

    /// The account this connection belongs to.
    pub fn account(&self) -> usize {
        self.account
    }

    /// The same connection in another account (a carve).
    pub fn in_account(mut self, account: usize) -> Connection {
        self.account = account;
        self
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
            Request::Ping { address } => verdict(
                self.rights & rights::REACH != 0 && self.scope().allows_echo_address(address),
            ),
            Request::PingName { name } => {
                verdict(self.rights & rights::REACH != 0 && self.scope().allows_echo_name(name))
            }
            Request::Connect { target, port } => {
                if self.rights & rights::REACH == 0 || port == 0 {
                    return Verdict::NotGranted;
                }
                let scope = self.scope();
                verdict(match target {
                    // A web rule admits any name on its ports; the answers
                    // are checked when they come (ADR-0324).
                    ConnectTarget::Name(n) => {
                        scope.name_grant(n, port, Protocols::TCP) != NameGrant::No
                    }
                    ConnectTarget::Address(a) => scope.allows_address(a, port, Protocols::TCP),
                })
            }
            Request::Resolve { name } => verdict(
                self.rights & rights::REACH != 0
                    && self.scope().resolve_grant(name) != NameGrant::No,
            ),
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
            .map(|c| c.in_account(self.account))
    }
}

fn verdict(allowed: bool) -> Verdict {
    if allowed {
        Verdict::Allowed
    } else {
        Verdict::NotGranted
    }
}
