//! The network scope (ADR-0220): what a connection to the network service
//! may reach and listen on, as a value with a total decoder, a matcher and the
//! subset relation `narrow` needs.
//!
//! A scope is bytes on the wire (it travels in a message of `zoraya-proto-net`
//! and in a launcher's records). [`Scope::decode`] checks all of it once and
//! never panics, whatever the input; the rules it then hands out are borrowed
//! from the checked bytes and iterate without failing. It transforms nothing:
//! `encode` of a decoded scope gives the bytes back (INV-0220-3).
//!
//! ```text
//! version        u8, 1
//! reach count    u8
//! listen count   u8      reach + listen at most MAX_RULES
//! reserved       u8, 0
//! reach rules    kind u8, target, ports, protocols
//! listen rules   ports, protocols
//!
//! target   Name 0 / Wildcard 1: length u8, the ASCII bytes (a wildcard
//!          without its "*." prefix); Address 2: 4 bytes; Prefix 3: 4 bytes,
//!          length u8 (0..=32, the host bits zero); Any 4: nothing;
//!          Public 5: nothing (ADR-0324, the shell's `web`); PublicName 6:
//!          length u8, the ASCII bytes (ADR-0328, a URL's name under `web`)
//! ports    0xFF for any port, or a count 1..=8 and that many (low, high)
//!          pairs of u16, little endian, 1 <= low <= high
//! protocols u8, a set of bits: 1 (TCP), 2 (UDP), 4 (echo, ADR-0330); 1 to 7
//!          in a reach rule (1 alone in a Public or PublicName rule,
//!          INV-0324-1, INV-0328-1), 1 to 3 in a listen rule
//! ```
//!
//! Decisions where the ADR is silent (docs/adr/0281): addresses are IPv4 (the
//! stack has no IPv6, ADR-0229 item 5); a name's last label is not all digits
//! (so a name never reads as an address); a `Prefix` with host bits set is
//! refused (one spelling for one set); a child rule must lie inside one single
//! rule of its parent (a union of two rules does not cover it).

/// Version byte of the encoding.
pub const VERSION: u8 = 1;
/// Reach and listen rules together (ADR-0220 item 2).
pub const MAX_RULES: usize = 64;
/// Port ranges in one rule.
pub const MAX_RANGES: usize = 8;
/// DNS's limits (ADR-0220 item 2).
pub const MAX_NAME: usize = 253;
pub const MAX_LABEL: usize = 63;
/// The most bytes an encoded scope has: 64 rules of a name of 253 bytes and
/// eight ranges each, rounded up.
pub const MAX_SCOPE_BYTES: usize = 24 * 1024;

const PORTS_ANY: u8 = 0xff;

/// Rights on a network connection (ADR-0220 item 3).
pub mod rights {
    pub const REACH: u32 = 1;
    pub const LISTEN: u32 = 2;
    pub const STATUS: u32 = 4;
    pub const NARROW: u32 = 8;
    pub const ALL: u32 = REACH | LISTEN | STATUS | NARROW;
}

/// A set of TCP, UDP and ICMP echo (ADR-0330: a rule that grants echo
/// requests alone has [`Protocols::ECHO`]; a listen rule has TCP and UDP only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Protocols(u8);

impl Protocols {
    pub const TCP: Protocols = Protocols(1);
    pub const UDP: Protocols = Protocols(2);
    /// TCP and UDP: not echo.
    pub const BOTH: Protocols = Protocols(3);
    /// ICMP echo requests (`ping`), no connection and no datagram.
    pub const ECHO: Protocols = Protocols(4);
    /// TCP, UDP and echo.
    pub const ALL: Protocols = Protocols(7);

    pub const fn from_raw(raw: u8) -> Option<Protocols> {
        if raw >= 1 && raw <= 7 {
            Some(Protocols(raw))
        } else {
            None
        }
    }

    pub const fn raw(self) -> u8 {
        self.0
    }

    pub const fn within(self, outer: Protocols) -> bool {
        self.0 & !outer.0 == 0
    }

    pub const fn contains(self, one: Protocols) -> bool {
        one.within(self)
    }
}

/// Ports: any, or at most [`MAX_RANGES`] ranges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ports {
    any: bool,
    count: u8,
    ranges: [(u16, u16); MAX_RANGES],
}

impl Ports {
    pub const ANY: Ports = Ports {
        any: true,
        count: 0,
        ranges: [(0, 0); MAX_RANGES],
    };

    /// One port.
    pub const fn one(port: u16) -> Ports {
        let mut ranges = [(0, 0); MAX_RANGES];
        ranges[0] = (port, port);
        Ports {
            any: false,
            count: 1,
            ranges,
        }
    }

    /// Ranges, each `1 <= low <= high`; `None` if there are none, more than
    /// [`MAX_RANGES`], or one is malformed.
    pub fn ranges(list: &[(u16, u16)]) -> Option<Ports> {
        if list.is_empty() || list.len() > MAX_RANGES {
            return None;
        }
        let mut ranges = [(0, 0); MAX_RANGES];
        for (i, &(low, high)) in list.iter().enumerate() {
            if low == 0 || low > high {
                return None;
            }
            ranges[i] = (low, high);
        }
        Some(Ports {
            any: false,
            count: list.len() as u8,
            ranges,
        })
    }

    pub fn is_any(&self) -> bool {
        self.any
    }

    pub fn list(&self) -> &[(u16, u16)] {
        &self.ranges[..self.count as usize]
    }

    pub fn contains(&self, port: u16) -> bool {
        self.any || self.list().iter().any(|&(l, h)| l <= port && port <= h)
    }

    /// Whether every port of `self` is a port of `outer`.
    pub fn within(&self, outer: &Ports) -> bool {
        if outer.any {
            return true;
        }
        if self.any {
            return false;
        }
        // For each range, extend a covered prefix by any outer range that
        // contains the next port; ranges may overlap or be unordered.
        self.list().iter().all(|&(low, high)| {
            let mut next = u32::from(low);
            for _ in 0..=MAX_RANGES {
                if next > u32::from(high) {
                    return true;
                }
                let Some(&(_, h)) = outer
                    .list()
                    .iter()
                    .find(|&&(l, h)| u32::from(l) <= next && next <= u32::from(h))
                else {
                    return false;
                };
                next = u32::from(h) + 1;
            }
            next > u32::from(high)
        })
    }
}

/// What a reach rule names (ADR-0220 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// An exact host name.
    Name(&'a str),
    /// Every name under a suffix, not the suffix itself; the string is the
    /// suffix without `*.`.
    Wildcard(&'a str),
    Address([u8; 4]),
    Prefix([u8; 4], u8),
    /// Every name and every address.
    Any,
    /// Every public address, and every name through its answer only: an
    /// address [`is_public`] accepts (ADR-0324, the shell's word `web`). A
    /// name this rule alone admits may use only the public answers of its
    /// lookup, which `netd` checks: [`Scope::name_grant`] says so.
    Public,
    /// One exact name, through its public answers only, as a web rule admits
    /// a name (ADR-0328: the name of a URL operand under `net web`, on a port
    /// the web rule does not have). It covers no address and names none: a
    /// connection or lookup it admits is [`NameGrant::PublicOnly`].
    PublicName(&'a str),
}

/// The blocks a [`Target::Public`] rule never reaches (ADR-0324 item 2): this
/// network, the private ranges (QEMU's 10.0.2.0/24 is inside 10.0.0.0/8),
/// shared address space, loopback, link-local, the IETF's protocol block,
/// benchmarking, multicast, and the reserved block with the broadcast address.
pub const NOT_PUBLIC: [([u8; 4], u8); 11] = [
    ([0, 0, 0, 0], 8),
    ([10, 0, 0, 0], 8),
    ([100, 64, 0, 0], 10),
    ([127, 0, 0, 0], 8),
    ([169, 254, 0, 0], 16),
    ([172, 16, 0, 0], 12),
    ([192, 0, 0, 0], 24),
    ([192, 168, 0, 0], 16),
    ([198, 18, 0, 0], 15),
    ([224, 0, 0, 0], 4),
    ([240, 0, 0, 0], 4),
];

/// Whether `address` is outside every block of [`NOT_PUBLIC`]: what a web
/// rule may reach (ADR-0324). The documentation ranges (192.0.2.0/24,
/// 198.51.100.0/24, 203.0.113.0/24) are public here: no network is one of
/// them, and the tests use 203.0.113.0/24 as the Internet.
pub fn is_public(address: [u8; 4]) -> bool {
    NOT_PUBLIC
        .iter()
        .all(|&(block, len)| !prefix_has(block, len, address))
}

/// How a scope admits a name for a connection or a lookup (ADR-0324).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameGrant {
    /// No rule covers it.
    No,
    /// Only a web rule ([`Target::Public`]) or a public-only name rule
    /// ([`Target::PublicName`]): only answers that are public, and not local
    /// to this machine's network, may be used.
    PublicOnly,
    /// A rule names it (a name, a wildcard or `any`): every answer, local
    /// ones as ADR-0223 item 4 says.
    Named,
}

/// A host name as the scope holds it: ASCII, lower case, DNS's limits, and
/// not an address (its last label is not all digits).
pub fn valid_name(name: &str) -> bool {
    let b = name.as_bytes();
    if b.is_empty() || b.len() > MAX_NAME {
        return false;
    }
    let mut last_all_digits = false;
    for label in b.split(|&c| c == b'.') {
        if label.is_empty() || label.len() > MAX_LABEL {
            return false;
        }
        if !label
            .iter()
            .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        {
            return false;
        }
        last_all_digits = label.iter().all(u8::is_ascii_digit);
    }
    !last_all_digits
}

fn mask(len: u8) -> u32 {
    if len == 0 {
        0
    } else {
        u32::MAX << (32 - u32::from(len))
    }
}

fn prefix_has(prefix: [u8; 4], len: u8, address: [u8; 4]) -> bool {
    let m = mask(len);
    u32::from_be_bytes(prefix) & m == u32::from_be_bytes(address) & m
}

/// Whether `name` is strictly under the suffix `suffix` (a wildcard's).
fn under(name: &str, suffix: &str) -> bool {
    let (n, s) = (name.as_bytes(), suffix.as_bytes());
    n.len() > s.len() + 1 && n[n.len() - s.len() - 1] == b'.' && n.ends_with(s)
}

impl Target<'_> {
    /// Whether every thing `self` names is named by `outer` (ADR-0220 item 5).
    pub fn within(&self, outer: &Target<'_>) -> bool {
        match (self, outer) {
            (_, Target::Any) => true,
            (Target::Any, _) => false,
            // Only a web rule is inside a web rule (INV-0324-2): an address or
            // a name rule would skip the public check of its answers.
            (Target::Public, Target::Public) => true,
            // A public-only name (ADR-0328) is inside a web rule (which
            // admits every name through the same check of its answers), the
            // same public-only name, and a rule that names the name (which
            // admits every answer); nothing else is inside it, since every
            // other rule skips the check or names more.
            (Target::PublicName(_), Target::Public) => true,
            (Target::PublicName(n), Target::PublicName(p)) => n == p,
            (Target::PublicName(n), Target::Name(p)) => n == p,
            (Target::PublicName(n), Target::Wildcard(s)) => under(n, s),
            (_, Target::PublicName(_)) | (Target::PublicName(_), _) => false,
            (_, Target::Public) | (Target::Public, _) => false,
            (Target::Name(n), Target::Name(p)) => n == p,
            (Target::Name(n), Target::Wildcard(s)) => under(n, s),
            (Target::Wildcard(a), Target::Wildcard(b)) => a == b || under(a, b),
            (Target::Address(a), Target::Address(b)) => a == b,
            (Target::Address(a), Target::Prefix(p, l)) => prefix_has(*p, *l, *a),
            (Target::Prefix(c, l), Target::Prefix(p, pl)) => pl <= l && prefix_has(*p, *pl, *c),
            // A name and an address are never under each other (INV-0220-4);
            // a wildcard is under no exact name; a prefix under no address.
            _ => false,
        }
    }

    /// Whether a request for `name` is covered (not a wildcard's suffix
    /// itself). Names and addresses never match each other. A web rule
    /// covers no name here: it admits names through their answers only
    /// ([`Scope::name_grant`]).
    pub fn covers_name(&self, name: &str) -> bool {
        match self {
            Target::Any => true,
            Target::Name(n) => *n == name,
            Target::Wildcard(s) => under(name, s),
            _ => false,
        }
    }

    pub fn covers_address(&self, address: [u8; 4]) -> bool {
        match self {
            Target::Any => true,
            Target::Address(a) => *a == address,
            Target::Prefix(p, l) => prefix_has(*p, *l, address),
            Target::Public => is_public(address),
            _ => false,
        }
    }

    /// As [`Target::covers_address`], but not through a web rule: an
    /// address, a prefix or `any` that names the address (what lets a local
    /// address through, ADR-0223 item 4).
    pub fn names_address(&self, address: [u8; 4]) -> bool {
        !matches!(self, Target::Public) && self.covers_address(address)
    }

    /// Whether this rule admits `name` through its public answers only: a
    /// web rule admits every name so, a public-only name rule its own name
    /// (ADR-0324 item 2, ADR-0328).
    pub fn admits_public_answers(&self, name: &str) -> bool {
        match self {
            Target::Public => true,
            Target::PublicName(n) => *n == name,
            _ => false,
        }
    }

    fn valid(&self) -> bool {
        match self {
            Target::Name(n) | Target::Wildcard(n) | Target::PublicName(n) => valid_name(n),
            Target::Prefix(p, l) => *l <= 32 && u32::from_be_bytes(*p) & !mask(*l) == 0,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReachRule<'a> {
    pub target: Target<'a>,
    pub ports: Ports,
    pub protocols: Protocols,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenRule {
    pub ports: Ports,
    pub protocols: Protocols,
}

impl ReachRule<'_> {
    pub fn within(&self, outer: &ReachRule<'_>) -> bool {
        self.target.within(&outer.target)
            && self.ports.within(&outer.ports)
            && self.protocols.within(outer.protocols)
    }
}

impl ListenRule {
    pub fn within(&self, outer: &ListenRule) -> bool {
        self.ports.within(&outer.ports) && self.protocols.within(outer.protocols)
    }
}

/// Reads fields from unchecked bytes; every read can fail.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> Option<u8> {
        let v = *self.bytes.get(self.at)?;
        self.at += 1;
        Some(v)
    }

    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.bytes.get(self.at..end)?;
        self.at = end;
        Some(s)
    }

    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.take(2)?.try_into().ok()?))
    }

    fn ports(&mut self) -> Option<Ports> {
        let count = self.u8()?;
        if count == PORTS_ANY {
            return Some(Ports::ANY);
        }
        if count == 0 || usize::from(count) > MAX_RANGES {
            return None;
        }
        let mut ranges = [(0, 0); MAX_RANGES];
        for r in ranges.iter_mut().take(usize::from(count)) {
            let (low, high) = (self.u16()?, self.u16()?);
            if low == 0 || low > high {
                return None;
            }
            *r = (low, high);
        }
        Some(Ports {
            any: false,
            count,
            ranges,
        })
    }

    fn reach(&mut self) -> Option<ReachRule<'a>> {
        let kind = self.u8()?;
        let target = match kind {
            0 | 1 | 6 => {
                let length = usize::from(self.u8()?);
                let name = core::str::from_utf8(self.take(length)?).ok()?;
                match kind {
                    0 => Target::Name(name),
                    1 => Target::Wildcard(name),
                    _ => Target::PublicName(name),
                }
            }
            2 => Target::Address(self.take(4)?.try_into().ok()?),
            3 => {
                let address = self.take(4)?.try_into().ok()?;
                Target::Prefix(address, self.u8()?)
            }
            4 => Target::Any,
            5 => Target::Public,
            _ => return None,
        };
        if !target.valid() {
            return None;
        }
        let ports = self.ports()?;
        let protocols = Protocols::from_raw(self.u8()?)?;
        // A web rule admits no protocol but TCP (INV-0324-1), nor does a
        // public-only name (INV-0328-1): the decoder, and so `netd`, holds
        // them to that whoever made the scope.
        if matches!(target, Target::Public | Target::PublicName(_)) && protocols != Protocols::TCP {
            return None;
        }
        Some(ReachRule {
            target,
            ports,
            protocols,
        })
    }

    fn listen(&mut self) -> Option<ListenRule> {
        let ports = self.ports()?;
        // Nothing listens for an echo.
        let protocols = Protocols::from_raw(self.u8()?)?;
        if !protocols.within(Protocols::BOTH) {
            return None;
        }
        Some(ListenRule { ports, protocols })
    }
}

/// A decoded scope: the checked bytes and the rule counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope<'a> {
    bytes: &'a [u8],
    reach: usize,
    listen: usize,
}

impl<'a> Scope<'a> {
    /// The scope of no rules: a connection that may only ask `status`.
    pub const EMPTY: Scope<'static> = Scope {
        bytes: &[VERSION, 0, 0, 0],
        reach: 0,
        listen: 0,
    };

    /// Checks `bytes` entirely. Any input gives a result, never a panic.
    pub fn decode(bytes: &'a [u8]) -> Option<Scope<'a>> {
        if bytes.len() > MAX_SCOPE_BYTES {
            return None;
        }
        let head = bytes.get(..4)?;
        if head[0] != VERSION || head[3] != 0 {
            return None;
        }
        let (reach, listen) = (usize::from(head[1]), usize::from(head[2]));
        if reach + listen > MAX_RULES {
            return None;
        }
        let mut r = Reader { bytes, at: 4 };
        for _ in 0..reach {
            r.reach()?;
        }
        for _ in 0..listen {
            r.listen()?;
        }
        if r.at != bytes.len() {
            return None;
        }
        Some(Scope {
            bytes,
            reach,
            listen,
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn reach_count(&self) -> usize {
        self.reach
    }

    pub fn listen_count(&self) -> usize {
        self.listen
    }

    /// The reach rules; the bytes were checked, so this does not fail.
    pub fn reach_rules(&self) -> impl Iterator<Item = ReachRule<'a>> + 'a {
        let mut r = Reader {
            bytes: self.bytes,
            at: 4,
        };
        (0..self.reach).map_while(move |_| r.reach())
    }

    pub fn listen_rules(&self) -> impl Iterator<Item = ListenRule> + 'a {
        let mut r = Reader {
            bytes: self.bytes,
            at: 4,
        };
        for _ in 0..self.reach {
            if r.reach().is_none() {
                break;
            }
        }
        (0..self.listen).map_while(move |_| r.listen())
    }

    pub fn is_empty(&self) -> bool {
        self.reach + self.listen == 0
    }

    /// Whether a connection to `name`:`port` over `protocol` (one of the two)
    /// is inside the scope. A name is matched as a name only.
    pub fn allows_name(&self, name: &str, port: u16, protocol: Protocols) -> bool {
        self.reach_rules().any(|r| {
            r.target.covers_name(name) && r.ports.contains(port) && r.protocols.contains(protocol)
        })
    }

    /// Whether some reach rule covers the name on some port: what `resolve`
    /// has (it names no port yet). A web rule does not count here; see
    /// [`Scope::resolve_grant`].
    pub fn reaches_name(&self, name: &str) -> bool {
        self.reach_rules().any(|r| r.target.covers_name(name))
    }

    /// How a connection to `name`:`port` over `protocol` is admitted
    /// (ADR-0324, ADR-0328): by a rule that names it, by a web rule or a
    /// public-only name rule alone (its answers must then be public), or not
    /// at all.
    pub fn name_grant(&self, name: &str, port: u16, protocol: Protocols) -> NameGrant {
        if self.allows_name(name, port, protocol) {
            NameGrant::Named
        } else if self.reach_rules().any(|r| {
            r.target.admits_public_answers(name)
                && r.ports.contains(port)
                && r.protocols.contains(protocol)
        }) {
            NameGrant::PublicOnly
        } else {
            NameGrant::No
        }
    }

    /// As [`Scope::name_grant`] for a lookup, which names no port: any rule
    /// that covers the name, else any web rule or the name's public-only rule.
    pub fn resolve_grant(&self, name: &str) -> NameGrant {
        if self.reaches_name(name) {
            NameGrant::Named
        } else if self
            .reach_rules()
            .any(|r| r.target.admits_public_answers(name))
        {
            NameGrant::PublicOnly
        } else {
            NameGrant::No
        }
    }

    /// As [`Scope::allows_name`] for an address: an address rule, or a web
    /// rule for a public address.
    pub fn allows_address(&self, address: [u8; 4], port: u16, protocol: Protocols) -> bool {
        self.reach_rules().any(|r| {
            r.target.covers_address(address)
                && r.ports.contains(port)
                && r.protocols.contains(protocol)
        })
    }

    /// As [`Scope::allows_address`], but not through a web rule: what lets
    /// a local address through (ADR-0223 item 4, ADR-0324 item 2).
    pub fn names_address(&self, address: [u8; 4], port: u16, protocol: Protocols) -> bool {
        self.reach_rules().any(|r| {
            r.target.names_address(address)
                && r.ports.contains(port)
                && r.protocols.contains(protocol)
        })
    }

    /// Whether a rule that grants echo requests covers `name`: what a `ping`
    /// by name needs (ADR-0330). A rule of TCP or UDP only does not.
    pub fn allows_echo_name(&self, name: &str) -> bool {
        self.reach_rules()
            .any(|r| r.target.covers_name(name) && r.protocols.contains(Protocols::ECHO))
    }

    /// As [`Scope::allows_echo_name`] for an address.
    pub fn allows_echo_address(&self, address: [u8; 4]) -> bool {
        self.reach_rules()
            .any(|r| r.target.covers_address(address) && r.protocols.contains(Protocols::ECHO))
    }

    /// Whether some reach rule of any protocol covers `address` on some port
    /// (what `resolve` asks of a local answer). Not through a web rule: a
    /// local answer needs a rule that names it.
    pub fn reaches_address(&self, address: [u8; 4]) -> bool {
        self.reach_rules().any(|r| r.target.names_address(address))
    }

    pub fn allows_listen(&self, port: u16, protocol: Protocols) -> bool {
        self.listen_rules()
            .any(|r| r.ports.contains(port) && r.protocols.contains(protocol))
    }

    /// Whether every rule of `self` is inside one rule of `outer`: `narrow`'s
    /// condition (ADR-0220 item 5).
    pub fn within(&self, outer: &Scope<'_>) -> bool {
        self.reach_rules()
            .all(|c| outer.reach_rules().any(|p| c.within(&p)))
            && self
                .listen_rules()
                .all(|c| outer.listen_rules().any(|p| c.within(&p)))
    }
}

fn put_ports(put: &mut dyn FnMut(&[u8]) -> Option<()>, p: &Ports) -> Option<()> {
    if p.is_any() {
        return put(&[PORTS_ANY]);
    }
    put(&[p.list().len() as u8])?;
    for &(low, high) in p.list() {
        put(&low.to_le_bytes())?;
        put(&high.to_le_bytes())?;
    }
    Some(())
}

/// Writes a scope into `out`; its size, or `None` if it does not fit, has too
/// many rules or a rule `decode` would refuse.
pub fn encode(reach: &[ReachRule<'_>], listen: &[ListenRule], out: &mut [u8]) -> Option<usize> {
    if reach.len() + listen.len() > MAX_RULES {
        return None;
    }
    let mut at = 0usize;
    let mut put = |bytes: &[u8]| -> Option<()> {
        let end = at.checked_add(bytes.len())?;
        out.get_mut(at..end)?.copy_from_slice(bytes);
        at = end;
        Some(())
    };
    put(&[VERSION, reach.len() as u8, listen.len() as u8, 0])?;
    for r in reach {
        match r.target {
            Target::Name(n) | Target::Wildcard(n) | Target::PublicName(n) => {
                let kind = match r.target {
                    Target::Name(_) => 0,
                    Target::Wildcard(_) => 1,
                    _ => 6,
                };
                put(&[kind, u8::try_from(n.len()).ok()?])?;
                put(n.as_bytes())?;
            }
            Target::Address(a) => {
                put(&[2])?;
                put(&a)?;
            }
            Target::Prefix(a, l) => {
                put(&[3])?;
                put(&a)?;
                put(&[l])?;
            }
            Target::Any => put(&[4])?,
            Target::Public => put(&[5])?,
        }
        put_ports(&mut put, &r.ports)?;
        put(&[r.protocols.raw()])?;
    }
    for l in listen {
        put_ports(&mut put, &l.ports)?;
        put(&[l.protocols.raw()])?;
    }
    let size = at;
    Scope::decode(&out[..size]).map(|_| size)
}
