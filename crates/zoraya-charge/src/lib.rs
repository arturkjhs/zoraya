//! The charge walk of ADR-0261: nested ceilings over a tree of jobs.
//!
//! A tree node keeps, per [`Field`], a [`Counter`]: an optional limit and what
//! the node and everything below it use (`used`), the highest `used` seen
//! (`peak`) and the charges its own limit refused (`refused`). A charge of `n`
//! made by a node walks from that node to the root. It is refused, with no
//! effect, if any limited node on the way would pass its limit; otherwise `n`
//! is added to every node on the way. A refund subtracts the same.
//!
//! The crate knows nothing of jobs, locks or memory: the caller gives it a
//! [`Tree`] and holds whatever lock keeps the tree still for the call. Every
//! node counts, limited or not (ADR-0261 point 4 keeps the counter only where
//! a limit is at or below the node; counting everywhere keeps `used` right when
//! a limit is set on a job that already holds something, and costs an add per
//! ancestor: at most 32, ADR-0046).
//!
//! Nothing here panics on any input and nothing allocates.

#![no_std]
#![forbid(unsafe_code)]

#[cfg(test)]
extern crate std;

/// A quantity a job's limits and use are counted in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Bytes: user frames and charged kernel objects (ADR-0260).
    Memory,
    /// Live processes.
    Processes,
    /// The part of `Memory` that is user frames. Counted, never limited.
    User,
}

impl Field {
    /// Every field, in the order of [`Counters`].
    pub const ALL: [Field; 3] = [Field::Memory, Field::Processes, Field::User];

    const fn index(self) -> usize {
        match self {
            Field::Memory => 0,
            Field::Processes => 1,
            Field::User => 2,
        }
    }
}

/// What memory is spent on, for the split `job_get_usage` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    User,
    Kernel,
}

/// One field of one node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counter {
    /// The node's own limit, if it has one.
    pub limit: Option<u64>,
    /// The node's own use and its subtree's.
    pub used: u64,
    /// The highest `used` since creation.
    pub peak: u64,
    /// Charges this node's limit refused.
    pub refused: u64,
}

/// The counters of a node.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counters([Counter; 3]);

impl Counters {
    pub const fn new() -> Counters {
        Counters(
            [Counter {
                limit: None,
                used: 0,
                peak: 0,
                refused: 0,
            }; 3],
        )
    }

    pub fn get(&self, field: Field) -> Counter {
        self.0[field.index()]
    }

    pub fn set(&mut self, field: Field, counter: Counter) {
        self.0[field.index()] = counter;
    }
}

/// A tree of nodes with counters. The caller keeps it still during a call.
pub trait Tree {
    type Id: Copy;

    fn parent(&self, id: Self::Id) -> Option<Self::Id>;
    fn counters(&self, id: Self::Id) -> Counters;
    fn set_counters(&mut self, id: Self::Id, counters: Counters);
}

/// A charge was refused: the node whose limit would have been passed, the
/// field, and the numbers that show it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refusal<Id> {
    pub at: Id,
    pub field: Field,
    pub used: u64,
    pub limit: u64,
}

/// A limit that may not be set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LimitError {
    /// Above the parent's effective limit (INV-0261-3).
    AboveParent,
    /// `User` is a part of `Memory`, not a field with a limit of its own.
    NotLimited,
}

/// Charges `n` of `field` to `id` and every ancestor, or refuses it.
///
/// The check covers every limited node first; the nearest one that would be
/// passed is the refusal, and only its `refused` count rises. A refusal changes
/// nothing else (INV-0261-2). `n` that overflows a counter is a refusal by the
/// node it overflows at, with `limit` `u64::MAX`.
pub fn charge<T: Tree>(
    tree: &mut T,
    id: T::Id,
    field: Field,
    n: u64,
) -> Result<(), Refusal<T::Id>> {
    let mut at = Some(id);
    while let Some(node) = at {
        let c = tree.counters(node).get(field);
        let would = c.used.checked_add(n);
        let over = match (would, c.limit) {
            (None, limit) => Some(limit.unwrap_or(u64::MAX)),
            (Some(w), Some(limit)) if w > limit => Some(limit),
            _ => None,
        };
        if let Some(limit) = over {
            let mut counters = tree.counters(node);
            let mut c = counters.get(field);
            c.refused = c.refused.saturating_add(1);
            counters.set(field, c);
            tree.set_counters(node, counters);
            return Err(Refusal {
                at: node,
                field,
                used: c.used,
                limit,
            });
        }
        at = tree.parent(node);
    }
    add(tree, id, field, n);
    Ok(())
}

fn add<T: Tree>(tree: &mut T, id: T::Id, field: Field, n: u64) {
    let mut at = Some(id);
    while let Some(node) = at {
        let mut counters = tree.counters(node);
        let mut c = counters.get(field);
        c.used = c.used.saturating_add(n);
        c.peak = c.peak.max(c.used);
        counters.set(field, c);
        tree.set_counters(node, counters);
        at = tree.parent(node);
    }
}

/// Charges `n` bytes of memory, of `kind`.
pub fn charge_memory<T: Tree>(
    tree: &mut T,
    id: T::Id,
    n: u64,
    kind: Kind,
) -> Result<(), Refusal<T::Id>> {
    charge(tree, id, Field::Memory, n)?;
    if kind == Kind::User {
        add(tree, id, Field::User, n);
    }
    Ok(())
}

/// Gives back `n` of `field` to `id` and every ancestor. A refund larger than
/// what is held stops at zero, which a correct caller never reaches.
pub fn refund<T: Tree>(tree: &mut T, id: T::Id, field: Field, n: u64) {
    let mut at = Some(id);
    while let Some(node) = at {
        let mut counters = tree.counters(node);
        let mut c = counters.get(field);
        c.used = c.used.saturating_sub(n);
        counters.set(field, c);
        tree.set_counters(node, counters);
        at = tree.parent(node);
    }
}

/// Gives back memory charged by [`charge_memory`] with the same `kind`.
pub fn refund_memory<T: Tree>(tree: &mut T, id: T::Id, n: u64, kind: Kind) {
    refund(tree, id, Field::Memory, n);
    if kind == Kind::User {
        refund(tree, id, Field::User, n);
    }
}

/// The smallest limit of `field` on `id` and its ancestors.
pub fn effective_limit<T: Tree>(tree: &T, id: T::Id, field: Field) -> Option<u64> {
    let mut best: Option<u64> = None;
    let mut at = Some(id);
    while let Some(node) = at {
        if let Some(limit) = tree.counters(node).get(field).limit {
            best = Some(best.map_or(limit, |b| b.min(limit)));
        }
        at = tree.parent(node);
    }
    best
}

/// Sets the limit of `field` on `id` (ADR-0261 points 2 and 3). A limit above
/// the parent's effective one is refused. A limit below the current use is
/// allowed: every later charge is refused until use falls. `None` removes the
/// node's own limit.
pub fn set_limit<T: Tree>(
    tree: &mut T,
    id: T::Id,
    field: Field,
    limit: Option<u64>,
) -> Result<(), LimitError> {
    if field == Field::User {
        return Err(LimitError::NotLimited);
    }
    if let (Some(new), Some(parent)) = (limit, tree.parent(id))
        && let Some(above) = effective_limit(tree, parent, field)
        && new > above
    {
        return Err(LimitError::AboveParent);
    }
    let mut counters = tree.counters(id);
    let mut c = counters.get(field);
    c.limit = limit;
    counters.set(field, c);
    tree.set_counters(id, counters);
    Ok(())
}

/// Clears the `refused` counts of `id` (the `CLEAR` flag does not need it; this
/// is for a node that is reused in tests).
pub fn clear_refused<T: Tree>(tree: &mut T, id: T::Id) {
    let mut counters = tree.counters(id);
    for field in Field::ALL {
        let mut c = counters.get(field);
        c.refused = 0;
        counters.set(field, c);
    }
    tree.set_counters(id, counters);
}

#[cfg(test)]
mod tests;

#[cfg(test)]
zoraya_test_alloc::cap_memory!();
