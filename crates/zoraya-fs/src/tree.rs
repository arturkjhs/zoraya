//! The copy-on-write B+tree (ADR-0011 rules 1 and 8).
//!
//! Nodes on disk are immutable. Changing one loads it, records its block as
//! freed by the open commit group, and replaces the parent's pointer with a
//! [`Child::Dirty`] copy held in memory. A commit writes every dirty node to
//! a new block, children before parents, so that each parent can record its
//! children's checksums (ADR-0012 rule 1).
//!
//! Dirty nodes are shared through `Rc`, so a clone of the [`Tree`] is a cheap
//! snapshot: the engine takes one before every request and restores it if
//! the request fails, which makes every request all-or-nothing in memory as
//! well as on disk.

use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;

use crate::error::FsError;
use crate::format::{
    BlockPtr, Body, Child, Key, MAX_DEPTH, NODE_CAPACITY, Node, Value, entry_len, item_len,
};

/// Reads and validates clean nodes.
pub(crate) trait Loader {
    /// The node at `ptr`, which must be at `level` and hold only keys in
    /// `[lower, upper)`.
    fn load_node(
        &mut self,
        ptr: &BlockPtr,
        level: u8,
        lower: Option<&Key>,
        upper: Option<&Key>,
    ) -> Result<Rc<Node>, FsError>;
}

/// What the open commit group will need and free.
#[derive(Debug, Clone, Default)]
pub(crate) struct Acct {
    /// Dirty tree nodes: each needs one block at commit.
    pub dirty_nodes: u64,
    /// Dirty data blocks: each needs one block at commit.
    pub dirty_data: u64,
    /// Blocks the group stopped referencing. Reusable only after the commit
    /// is durable (ADR-0011 rule 5).
    pub freed: Vec<u64>,
}

/// The tree: its root and height (levels, leaves included).
#[derive(Debug, Clone)]
pub(crate) struct Tree {
    pub root: Child,
    pub height: u8,
}

/// Index of the child of an interior node whose range holds `key`.
fn route(es: &[(Key, Child)], key: &Key) -> usize {
    match es.get(1..) {
        Some(rest) => rest.partition_point(|(k, _)| k <= key),
        None => 0,
    }
}

/// Bounds of child `i` of an interior node whose own bounds are
/// `[lower, upper)`.
fn child_bounds(
    es: &[(Key, Child)],
    i: usize,
    lower: Option<&Key>,
    upper: Option<&Key>,
) -> (Option<Key>, Option<Key>) {
    let lo = if i > 0 {
        es.get(i).map(|e| e.0.clone())
    } else {
        lower.cloned()
    };
    let up = match es.get(i + 1) {
        Some(e) => Some(e.0.clone()),
        None => upper.cloned(),
    };
    (lo, up)
}

/// The node behind `child`, loading it if it is clean.
fn view(
    ld: &mut impl Loader,
    child: &Child,
    level: u8,
    lo: Option<&Key>,
    up: Option<&Key>,
) -> Result<Rc<Node>, FsError> {
    match child {
        Child::Dirty(n) => Ok(n.clone()),
        Child::Clean(p) => ld.load_node(p, level, lo, up),
    }
}

/// Makes `child` dirty and returns the node for changing.
fn make_dirty<'c>(
    ld: &mut impl Loader,
    acct: &mut Acct,
    child: &'c mut Child,
    level: u8,
    lo: Option<&Key>,
    up: Option<&Key>,
) -> Result<&'c mut Node, FsError> {
    if let Child::Clean(p) = child {
        let p = *p;
        let n = ld.load_node(&p, level, lo, up)?;
        acct.freed.push(p.addr);
        acct.dirty_nodes += 1;
        *child = Child::Dirty(Rc::new((*n).clone()));
    }
    match child {
        Child::Dirty(rc) => Ok(Rc::make_mut(rc)),
        Child::Clean(_) => Err(FsError::Internal),
    }
}

/// Splits an overfull node's entries at about half of their encoded size.
fn split_point<T>(es: &[T], len: impl Fn(&T) -> usize) -> usize {
    let total: usize = es.iter().map(&len).sum();
    let mut acc = 0;
    for (i, e) in es.iter().enumerate() {
        acc += len(e);
        if acc * 2 >= total {
            // Keep both halves non-empty.
            return (i + 1).clamp(1, es.len().saturating_sub(1).max(1));
        }
    }
    es.len() / 2
}

/// Splits `node` if it no longer fits a block; returns the new right node
/// and its separator key.
fn split_if_needed(acct: &mut Acct, node: &mut Node) -> Option<(Key, Child)> {
    if node.used() <= NODE_CAPACITY || node.len() < 2 {
        return None;
    }
    let level = node.level;
    let (sep, body) = match &mut node.body {
        Body::Leaf(items) => {
            let at = split_point(items, |(k, v)| item_len(k, v));
            let right = items.split_off(at);
            (right.first()?.0.clone(), Body::Leaf(right))
        }
        Body::Interior(es) => {
            let at = split_point(es, |(k, _)| entry_len(k));
            let right = es.split_off(at);
            (right.first()?.0.clone(), Body::Interior(right))
        }
    };
    acct.dirty_nodes += 1;
    Some((sep, Child::Dirty(Rc::new(Node { level, body }))))
}

impl Tree {
    /// The value under `key`.
    pub fn get(&self, ld: &mut impl Loader, key: &Key) -> Result<Option<Value>, FsError> {
        let mut level = self.height.checked_sub(1).ok_or(FsError::Internal)?;
        let mut node = view(ld, &self.root, level, None, None)?;
        let mut lower: Option<Key> = None;
        let mut upper: Option<Key> = None;
        loop {
            let next = match &node.body {
                Body::Leaf(items) => {
                    return Ok(items
                        .binary_search_by(|(k, _)| k.cmp(key))
                        .ok()
                        .and_then(|i| items.get(i))
                        .map(|(_, v)| v.clone()));
                }
                Body::Interior(es) => {
                    let i = route(es, key);
                    let (lo, up) = child_bounds(es, i, lower.as_ref(), upper.as_ref());
                    level = level.checked_sub(1).ok_or(FsError::Internal)?;
                    let c = &es.get(i).ok_or(FsError::Internal)?.1;
                    let n = view(ld, c, level, lo.as_ref(), up.as_ref())?;
                    lower = lo;
                    upper = up;
                    n
                }
            };
            node = next;
        }
    }

    /// The first item with a key `>= from` (or `> from` if `!inclusive`).
    pub fn next(
        &self,
        ld: &mut impl Loader,
        from: &Key,
        inclusive: bool,
    ) -> Result<Option<(Key, Value)>, FsError> {
        let level = self.height.checked_sub(1).ok_or(FsError::Internal)?;
        let node = view(ld, &self.root, level, None, None)?;
        first_from(ld, &node, None, None, from, inclusive)
    }

    /// Inserts or replaces; returns the old value.
    pub fn insert(
        &mut self,
        ld: &mut impl Loader,
        acct: &mut Acct,
        key: Key,
        val: Value,
    ) -> Result<Option<Value>, FsError> {
        let level = self.height.checked_sub(1).ok_or(FsError::Internal)?;
        let mut old = None;
        let split = insert_rec(
            ld,
            acct,
            &mut self.root,
            level,
            None,
            None,
            key,
            val,
            &mut old,
        )?;
        if let Some((k, c)) = split {
            if self.height >= MAX_DEPTH {
                // The request is rolled back by the engine.
                return Err(FsError::NoSpace);
            }
            let left = core::mem::replace(
                &mut self.root,
                Child::Clean(BlockPtr {
                    addr: 0,
                    csum: 0,
                    generation: 0,
                }),
            );
            let root = Node {
                level: self.height,
                body: Body::Interior(vec![(Key::MIN, left), (k, c)]),
            };
            self.root = Child::Dirty(Rc::new(root));
            acct.dirty_nodes += 1;
            self.height += 1;
        }
        Ok(old)
    }

    /// Removes `key`; returns its value. A missing key changes nothing.
    pub fn remove(
        &mut self,
        ld: &mut impl Loader,
        acct: &mut Acct,
        key: &Key,
    ) -> Result<Option<Value>, FsError> {
        if self.get(ld, key)?.is_none() {
            return Ok(None);
        }
        let level = self.height.checked_sub(1).ok_or(FsError::Internal)?;
        let v = remove_rec(ld, acct, &mut self.root, level, None, None, key)?;
        // Collapse an interior root with one child, or with none.
        while let Child::Dirty(rc) = &self.root {
            let Body::Interior(es) = &rc.body else { break };
            match es.len() {
                0 => {
                    self.root = Child::Dirty(Rc::new(Node {
                        level: 0,
                        body: Body::Leaf(Vec::new()),
                    }));
                    self.height = 1;
                    break;
                }
                1 => {
                    let only = es.first().ok_or(FsError::Internal)?.1.clone();
                    self.root = only;
                    acct.dirty_nodes -= 1;
                    self.height -= 1;
                }
                _ => break,
            }
        }
        Ok(v)
    }
}

fn first_from(
    ld: &mut impl Loader,
    node: &Node,
    lower: Option<&Key>,
    upper: Option<&Key>,
    from: &Key,
    inclusive: bool,
) -> Result<Option<(Key, Value)>, FsError> {
    match &node.body {
        Body::Leaf(items) => {
            let p = items.partition_point(|(k, _)| if inclusive { k < from } else { k <= from });
            Ok(items.get(p).cloned())
        }
        Body::Interior(es) => {
            let level = node.level.checked_sub(1).ok_or(FsError::Internal)?;
            for i in route(es, from)..es.len() {
                let (lo, up) = child_bounds(es, i, lower, upper);
                let c = &es.get(i).ok_or(FsError::Internal)?.1;
                let child = view(ld, c, level, lo.as_ref(), up.as_ref())?;
                if let Some(x) = first_from(ld, &child, lo.as_ref(), up.as_ref(), from, inclusive)?
                {
                    return Ok(Some(x));
                }
            }
            Ok(None)
        }
    }
}

#[allow(clippy::too_many_arguments)] // a recursive helper; the arguments are its state
fn insert_rec(
    ld: &mut impl Loader,
    acct: &mut Acct,
    child: &mut Child,
    level: u8,
    lower: Option<&Key>,
    upper: Option<&Key>,
    key: Key,
    val: Value,
    old: &mut Option<Value>,
) -> Result<Option<(Key, Child)>, FsError> {
    let node = make_dirty(ld, acct, child, level, lower, upper)?;
    match &mut node.body {
        Body::Leaf(items) => match items.binary_search_by(|(k, _)| k.cmp(&key)) {
            Ok(i) => {
                let slot = items.get_mut(i).ok_or(FsError::Internal)?;
                *old = Some(core::mem::replace(&mut slot.1, val));
            }
            Err(i) => items.insert(i, (key, val)),
        },
        Body::Interior(es) => {
            let i = route(es, &key);
            let (lo, up) = child_bounds(es, i, lower, upper);
            let c = &mut es.get_mut(i).ok_or(FsError::Internal)?.1;
            let child_level = level.checked_sub(1).ok_or(FsError::Internal)?;
            if let Some(e) = insert_rec(
                ld,
                acct,
                c,
                child_level,
                lo.as_ref(),
                up.as_ref(),
                key,
                val,
                old,
            )? {
                es.insert(i + 1, e);
            }
        }
    }
    Ok(split_if_needed(acct, node))
}

fn remove_rec(
    ld: &mut impl Loader,
    acct: &mut Acct,
    child: &mut Child,
    level: u8,
    lower: Option<&Key>,
    upper: Option<&Key>,
    key: &Key,
) -> Result<Option<Value>, FsError> {
    let node = make_dirty(ld, acct, child, level, lower, upper)?;
    match &mut node.body {
        Body::Leaf(items) => match items.binary_search_by(|(k, _)| k.cmp(key)) {
            Ok(i) => Ok(Some(items.remove(i).1)),
            Err(_) => Ok(None),
        },
        Body::Interior(es) => {
            let i = route(es, key);
            let (lo, up) = child_bounds(es, i, lower, upper);
            let child_level = level.checked_sub(1).ok_or(FsError::Internal)?;
            let c = &mut es.get_mut(i).ok_or(FsError::Internal)?.1;
            let v = remove_rec(ld, acct, c, child_level, lo.as_ref(), up.as_ref(), key)?;
            rebalance(ld, acct, es, i, child_level, lower, upper)?;
            Ok(v)
        }
    }
}

/// After a removal below entry `i`: drop the child if it is empty, or merge
/// it with a sibling if it is less than a quarter full and the two fit in
/// one block.
fn rebalance(
    ld: &mut impl Loader,
    acct: &mut Acct,
    es: &mut Vec<(Key, Child)>,
    i: usize,
    child_level: u8,
    lower: Option<&Key>,
    upper: Option<&Key>,
) -> Result<(), FsError> {
    let (len, used) = match &es.get(i).ok_or(FsError::Internal)?.1 {
        Child::Dirty(n) => (n.len(), n.used()),
        Child::Clean(_) => return Err(FsError::Internal),
    };
    if len == 0 {
        es.remove(i);
        acct.dirty_nodes -= 1;
        return Ok(());
    }
    if used >= NODE_CAPACITY / 4 || es.len() < 2 {
        return Ok(());
    }
    let (l, r) = if i + 1 < es.len() {
        (i, i + 1)
    } else {
        (i - 1, i)
    };
    let sib = if l == i { r } else { l };
    let (slo, sup) = child_bounds(es, sib, lower, upper);
    let sib_used = view(
        ld,
        &es.get(sib).ok_or(FsError::Internal)?.1,
        child_level,
        slo.as_ref(),
        sup.as_ref(),
    )?
    .used();
    if used + sib_used + entry_len(&es.get(r).ok_or(FsError::Internal)?.0) > NODE_CAPACITY {
        return Ok(());
    }
    // Both become dirty; the right one's entries move into the left one.
    for j in [l, r] {
        let (lo, up) = child_bounds(es, j, lower, upper);
        make_dirty(
            ld,
            acct,
            &mut es.get_mut(j).ok_or(FsError::Internal)?.1,
            child_level,
            lo.as_ref(),
            up.as_ref(),
        )?;
    }
    let (sep, right) = es.remove(r);
    let Child::Dirty(right) = right else {
        return Err(FsError::Internal);
    };
    acct.dirty_nodes -= 1;
    let right = Rc::try_unwrap(right).unwrap_or_else(|rc| (*rc).clone());
    let Child::Dirty(left) = &mut es.get_mut(l).ok_or(FsError::Internal)?.1 else {
        return Err(FsError::Internal);
    };
    let left = Rc::make_mut(left);
    match (&mut left.body, right.body) {
        (Body::Leaf(a), Body::Leaf(b)) => a.extend(b),
        (Body::Interior(a), Body::Interior(mut b)) => {
            // The right node's first key was not a routing key; the parent's
            // separator is its lower bound.
            if let Some(first) = b.first_mut() {
                first.0 = sep;
            }
            a.extend(b);
        }
        _ => return Err(FsError::Internal),
    }
    Ok(())
}
