//! `netd`'s accounts (ADR-0262 item 2; N4): what a service holds for a
//! client is bounded per *account*, a tree of scope connections that one
//! launcher grant started. A connection made by `narrow` joins its parent's
//! account; `narrow_account` with a budget carves a new one from the
//! caller's. At a bound only that account's request is refused; the sum of
//! every live account's limit of a kind is the service's global bound,
//! always (INV-0262-3), so a carve can never make the service bigger.
//!
//! The kinds and their bounds (ADR-0262's table, scaled to what `netd` is:
//! 16 connections, not 64; `[assumption, every value]`, proposed ADR-0287):
//!
//! | kind | global | one carved account | all carved accounts |
//! |---|---|---|---|
//! | connections (TCP, any phase) | 16 | `budget`, 1 to 4 | 8 |
//! | listeners | 4 | 0 | 0 |
//! | pending resolutions | 8 | `budget / 2`, at least 1 | 4 |
//! | datagram sockets | 4 | 0 | 0 |
//! | resolver cache entries | 64 | `4 * budget` | 32 |
//!
//! At most four carved accounts live at once. Listeners and datagram
//! sockets have no request in `netd` yet; their bounds exist so that the
//! carve is whole when they do.

use alloc::vec::Vec;

/// What an account bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Connections = 0,
    Listeners = 1,
    Resolutions = 2,
    Datagrams = 3,
    CacheEntries = 4,
}

pub const KINDS: usize = 5;
pub const ALL: [Kind; KINDS] = [
    Kind::Connections,
    Kind::Listeners,
    Kind::Resolutions,
    Kind::Datagrams,
    Kind::CacheEntries,
];

/// The service's bound of each kind, in [`Kind`] order.
pub const GLOBAL: [u32; KINDS] = [16, 4, 8, 4, 64];
/// What all carved accounts together may hold of each kind.
pub const COLUMN: [u32; KINDS] = [8, 0, 4, 0, 32];
/// Carved accounts alive at once.
pub const MAX_CARVED: usize = 4;
/// The most connections one carve may ask for.
pub const MAX_BUDGET: u32 = 4;

/// The session's account: the root connection `init` hands out is in it.
pub const SESSION: usize = 0;

/// The limits a carve with `budget` connections asks for.
pub const fn carve_of(budget: u32) -> [u32; KINDS] {
    let resolutions = if budget / 2 == 0 { 1 } else { budget / 2 };
    [budget, 0, resolutions, 0, 4 * budget]
}

/// A charge past the account's limit (or on an account that is gone).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused;

/// Why a carve was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarveError {
    /// A budget of 0 or over [`MAX_BUDGET`], or an account that is gone.
    Invalid,
    /// The caller's account cannot give that much, or four carved accounts
    /// live, or the carved accounts together would pass their column.
    OverQuota,
}

#[derive(Debug, Clone)]
struct Account {
    parent: Option<usize>,
    limit: [u32; KINDS],
    used: [u32; KINDS],
    /// Scope connections in the account.
    handles: u32,
    /// Carved accounts below this one.
    kids: u32,
    /// Requests this account was refused, by kind.
    refused: [u64; KINDS],
}

impl Account {
    fn dead(&self) -> bool {
        self.handles == 0 && self.kids == 0 && self.used.iter().all(|u| *u == 0)
    }
}

/// The table. Account [`SESSION`] is there from the start and never ends.
#[derive(Debug, Clone)]
pub struct Accounts {
    slots: Vec<Option<Account>>,
}

impl Default for Accounts {
    fn default() -> Accounts {
        Accounts::new()
    }
}

impl Accounts {
    pub fn new() -> Accounts {
        Accounts {
            slots: alloc::vec![Some(Account {
                parent: None,
                limit: GLOBAL,
                used: [0; KINDS],
                handles: 0,
                kids: 0,
                refused: [0; KINDS],
            })],
        }
    }

    fn get(&self, a: usize) -> Option<&Account> {
        self.slots.get(a)?.as_ref()
    }

    /// Whether account `a` is live.
    pub fn exists(&self, a: usize) -> bool {
        self.get(a).is_some()
    }

    /// Accounts alive, the session's included.
    pub fn live(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Carved accounts alive (the column).
    pub fn carved(&self) -> usize {
        self.live() - 1
    }

    pub fn limit(&self, a: usize, kind: Kind) -> u32 {
        self.get(a).map_or(0, |x| x.limit[kind as usize])
    }

    pub fn used(&self, a: usize, kind: Kind) -> u32 {
        self.get(a).map_or(0, |x| x.used[kind as usize])
    }

    /// Requests refused to `a` at the bound of `kind` since it was made.
    pub fn refusals(&self, a: usize, kind: Kind) -> u64 {
        self.get(a).map_or(0, |x| x.refused[kind as usize])
    }

    /// The sum of every live account's limit of `kind`: [`GLOBAL`] always.
    pub fn total_limit(&self, kind: Kind) -> u32 {
        self.slots
            .iter()
            .flatten()
            .map(|x| x.limit[kind as usize])
            .sum()
    }

    /// The sum of every live account's use of `kind`.
    pub fn total_used(&self, kind: Kind) -> u32 {
        self.slots
            .iter()
            .flatten()
            .map(|x| x.used[kind as usize])
            .sum()
    }

    /// A scope connection joins `a`.
    pub fn join(&mut self, a: usize) {
        if let Some(Some(x)) = self.slots.get_mut(a) {
            x.handles += 1;
        }
    }

    /// A scope connection of `a` is gone; the account ends with its last.
    pub fn leave(&mut self, a: usize) {
        if let Some(Some(x)) = self.slots.get_mut(a) {
            x.handles = x.handles.saturating_sub(1);
        }
        self.free(a);
    }

    /// Carves a new account from `parent` for `budget` connections.
    pub fn carve(&mut self, parent: usize, budget: u32) -> Result<usize, CarveError> {
        if budget == 0 || budget > MAX_BUDGET || !self.exists(parent) {
            return Err(CarveError::Invalid);
        }
        let want = carve_of(budget);
        // From the session, the column bounds what all carved accounts hold
        // together; from a carved account the limits only move inside it.
        let column_ok = parent != SESSION
            || ALL
                .iter()
                .all(|k| self.column(*k) + want[*k as usize] <= COLUMN[*k as usize]);
        let parent_ok = self
            .get(parent)
            .is_some_and(|p| (0..KINDS).all(|i| p.limit[i] >= p.used[i] + want[i]));
        if self.carved() >= MAX_CARVED || !column_ok || !parent_ok {
            if let Some(Some(p)) = self.slots.get_mut(parent) {
                p.refused[Kind::Connections as usize] += 1;
            }
            return Err(CarveError::OverQuota);
        }
        let child = Account {
            parent: Some(parent),
            limit: want,
            used: [0; KINDS],
            handles: 0,
            kids: 0,
            refused: [0; KINDS],
        };
        if let Some(Some(p)) = self.slots.get_mut(parent) {
            for (l, w) in p.limit.iter_mut().zip(want) {
                *l -= w;
            }
            p.kids += 1;
        }
        let id = match self.slots.iter().position(Option::is_none) {
            Some(i) => {
                self.slots[i] = Some(child);
                i
            }
            None => {
                self.slots.push(Some(child));
                self.slots.len() - 1
            }
        };
        Ok(id)
    }

    /// What the carved accounts together hold of `kind` as limits: what the
    /// session gave away (limits only move inside the tree).
    fn column(&self, kind: Kind) -> u32 {
        self.slots
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != SESSION)
            .filter_map(|(_, x)| x.as_ref())
            .map(|x| x.limit[kind as usize])
            .sum()
    }

    /// Takes one of `kind` for `a`; at its limit the request is refused and
    /// nothing changes.
    pub fn charge(&mut self, a: usize, kind: Kind) -> Result<(), Refused> {
        let Some(Some(x)) = self.slots.get_mut(a) else {
            return Err(Refused);
        };
        let i = kind as usize;
        if x.used[i] >= x.limit[i] {
            x.refused[i] += 1;
            return Err(Refused);
        }
        x.used[i] += 1;
        Ok(())
    }

    /// Gives one of `kind` back; the account ends if that was its last hold.
    pub fn release(&mut self, a: usize, kind: Kind) {
        if let Some(Some(x)) = self.slots.get_mut(a) {
            x.used[kind as usize] = x.used[kind as usize].saturating_sub(1);
        }
        self.free(a);
    }

    /// Ends `a` if nothing holds it, and so on up: its limits go back to its
    /// parent.
    fn free(&mut self, a: usize) {
        let mut at = a;
        while at != SESSION {
            let Some(Some(x)) = self.slots.get(at) else {
                return;
            };
            if !x.dead() {
                return;
            }
            let (parent, limit) = (x.parent, x.limit);
            self.slots[at] = None;
            let Some(p) = parent else { return };
            if let Some(Some(px)) = self.slots.get_mut(p) {
                for (l, g) in px.limit.iter_mut().zip(limit) {
                    *l += g;
                }
                px.kids = px.kids.saturating_sub(1);
            }
            at = p;
        }
    }
}
