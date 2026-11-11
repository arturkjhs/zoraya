//! INV-0261-1 and INV-0261-2: a model test over random trees, charges, refunds
//! and limit changes, comparing every counter with a recomputation.

use std::vec;
use std::vec::Vec;

use super::*;

struct Arena {
    parent: Vec<Option<usize>>,
    counters: Vec<Counters>,
}

impl Arena {
    fn new() -> Arena {
        Arena {
            parent: vec![None],
            counters: vec![Counters::new()],
        }
    }

    fn add_child(&mut self, parent: usize) -> usize {
        self.parent.push(Some(parent));
        self.counters.push(Counters::new());
        self.parent.len() - 1
    }

    fn is_within(&self, mut node: usize, ancestor: usize) -> bool {
        loop {
            if node == ancestor {
                return true;
            }
            match self.parent[node] {
                Some(p) => node = p,
                None => return false,
            }
        }
    }
}

impl Tree for Arena {
    type Id = usize;

    fn parent(&self, id: usize) -> Option<usize> {
        self.parent[id]
    }

    fn counters(&self, id: usize) -> Counters {
        self.counters[id]
    }

    fn set_counters(&mut self, id: usize, counters: Counters) {
        self.counters[id] = counters;
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A live charge the model remembers.
#[derive(Clone, Copy)]
struct Held {
    payer: usize,
    field: Field,
    n: u64,
    kind: Kind,
}

fn check(arena: &Arena, held: &[Held]) {
    for node in 0..arena.parent.len() {
        for field in [Field::Memory, Field::Processes, Field::User] {
            let want: u64 = held
                .iter()
                .filter(|h| arena.is_within(h.payer, node))
                .map(|h| match (field, h.field, h.kind) {
                    (Field::Memory, Field::Memory, _) => h.n,
                    (Field::User, Field::Memory, Kind::User) => h.n,
                    (Field::Processes, Field::Processes, _) => h.n,
                    _ => 0,
                })
                .sum();
            let c = arena.counters[node].get(field);
            assert_eq!(c.used, want, "node {node} {field:?}");
            assert!(c.peak >= c.used);
        }
    }
}

#[test]
fn random_trees_keep_every_counter_equal_to_its_recomputation() {
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let mut arena = Arena::new();
        let mut held: Vec<Held> = Vec::new();
        for _ in 0..400 {
            match rng.below(10) {
                0 => {
                    let p = rng.below(arena.parent.len() as u64) as usize;
                    if arena.parent.len() < 40 {
                        arena.add_child(p);
                    }
                }
                1 => {
                    let node = rng.below(arena.parent.len() as u64) as usize;
                    let field = if rng.below(2) == 0 {
                        Field::Memory
                    } else {
                        Field::Processes
                    };
                    let limit = (rng.below(3) != 0).then(|| rng.below(5000));
                    let before = arena.counters[node];
                    let result = set_limit(&mut arena, node, field, limit);
                    let parent_limit =
                        arena.parent[node].and_then(|p| effective_limit(&arena, p, field));
                    match (limit, parent_limit) {
                        (Some(l), Some(pl)) if l > pl => {
                            assert_eq!(result, Err(LimitError::AboveParent));
                            assert_eq!(arena.counters[node], before);
                        }
                        _ => assert_eq!(result, Ok(())),
                    }
                }
                2..=6 => {
                    let payer = rng.below(arena.parent.len() as u64) as usize;
                    let (field, kind) = match rng.below(3) {
                        0 => (Field::Processes, Kind::Kernel),
                        1 => (Field::Memory, Kind::User),
                        _ => (Field::Memory, Kind::Kernel),
                    };
                    let n = 1 + rng.below(if field == Field::Processes { 3 } else { 900 });
                    let before: Vec<Counters> = arena.counters.clone();
                    let result = if field == Field::Memory {
                        charge_memory(&mut arena, payer, n, kind)
                    } else {
                        charge(&mut arena, payer, field, n)
                    };
                    match result {
                        Ok(()) => held.push(Held {
                            payer,
                            field,
                            n,
                            kind,
                        }),
                        Err(r) => {
                            // INV-0261-2: only `refused` of the refusing node moved.
                            assert!(arena.is_within(payer, r.at));
                            assert!(r.used + n > r.limit);
                            for (i, b) in before.iter().enumerate() {
                                let mut now = arena.counters[i];
                                if i == r.at {
                                    let mut c = now.get(field);
                                    assert_eq!(c.refused, b.get(field).refused + 1);
                                    c.refused -= 1;
                                    now.set(field, c);
                                }
                                assert_eq!(&now, b, "node {i}");
                            }
                        }
                    }
                }
                _ => {
                    if !held.is_empty() {
                        let at = rng.below(held.len() as u64) as usize;
                        let h = held.swap_remove(at);
                        if h.field == Field::Memory {
                            refund_memory(&mut arena, h.payer, h.n, h.kind);
                        } else {
                            refund(&mut arena, h.payer, h.field, h.n);
                        }
                    }
                }
            }
            check(&arena, &held);
        }
        // Everything given back: every counter is zero (the shape of INV-0261-6).
        for h in held.drain(..) {
            if h.field == Field::Memory {
                refund_memory(&mut arena, h.payer, h.n, h.kind);
            } else {
                refund(&mut arena, h.payer, h.field, h.n);
            }
        }
        check(&arena, &[]);
    }
}

#[test]
fn a_charge_within_every_limit_succeeds_and_the_nearest_refusal_is_named() {
    let mut a = Arena::new();
    let room = a.add_child(0);
    let cmd = a.add_child(room);
    set_limit(&mut a, 0, Field::Memory, Some(1000)).unwrap();
    set_limit(&mut a, room, Field::Memory, Some(100)).unwrap();
    charge(&mut a, cmd, Field::Memory, 100).unwrap();
    let r = charge(&mut a, cmd, Field::Memory, 1).unwrap_err();
    assert_eq!((r.at, r.used, r.limit), (room, 100, 100));
    assert_eq!(a.counters[room].get(Field::Memory).refused, 1);
    assert_eq!(a.counters[0].get(Field::Memory).refused, 0);
    // A sibling outside the room still has the root's room (INV-0261-4).
    let other = a.add_child(0);
    charge(&mut a, other, Field::Memory, 900).unwrap();
    assert!(charge(&mut a, other, Field::Memory, 1).is_err());
}

#[test]
fn a_limit_cannot_pass_the_parents_effective_limit() {
    let mut a = Arena::new();
    let room = a.add_child(0);
    let cmd = a.add_child(room);
    let leaf = a.add_child(cmd);
    set_limit(&mut a, room, Field::Memory, Some(100)).unwrap();
    // `cmd` has no limit of its own; its effective limit is the room's.
    assert_eq!(
        set_limit(&mut a, leaf, Field::Memory, Some(101)),
        Err(LimitError::AboveParent)
    );
    set_limit(&mut a, leaf, Field::Memory, Some(100)).unwrap();
    assert_eq!(
        set_limit(&mut a, leaf, Field::User, Some(1)),
        Err(LimitError::NotLimited)
    );
}

#[test]
fn lowering_below_the_use_refuses_until_use_falls() {
    let mut a = Arena::new();
    let room = a.add_child(0);
    charge(&mut a, room, Field::Memory, 80).unwrap();
    set_limit(&mut a, room, Field::Memory, Some(50)).unwrap();
    assert!(charge(&mut a, room, Field::Memory, 1).is_err());
    refund(&mut a, room, Field::Memory, 40);
    charge(&mut a, room, Field::Memory, 10).unwrap();
    assert_eq!(a.counters[room].get(Field::Memory).used, 50);
    assert_eq!(a.counters[room].get(Field::Memory).peak, 80);
}

#[test]
fn a_charge_that_overflows_is_a_refusal() {
    let mut a = Arena::new();
    charge(&mut a, 0, Field::Memory, u64::MAX).unwrap();
    let r = charge(&mut a, 0, Field::Memory, 1).unwrap_err();
    assert_eq!(r.limit, u64::MAX);
    assert_eq!(a.counters[0].get(Field::Memory).used, u64::MAX);
}

#[test]
fn user_memory_is_a_part_of_memory() {
    let mut a = Arena::new();
    let room = a.add_child(0);
    set_limit(&mut a, room, Field::Memory, Some(100)).unwrap();
    charge_memory(&mut a, room, 60, Kind::User).unwrap();
    charge_memory(&mut a, room, 30, Kind::Kernel).unwrap();
    assert_eq!(a.counters[room].get(Field::Memory).used, 90);
    assert_eq!(a.counters[room].get(Field::User).used, 60);
    assert!(charge_memory(&mut a, room, 11, Kind::User).is_err());
    assert_eq!(a.counters[room].get(Field::User).used, 60);
}
