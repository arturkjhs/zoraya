use std::vec::Vec;

use super::*;

/// INV-0300-6: any interleaving of "end requested", "leave" and "create"
/// over four threads ends with the list empty and exactly one owner.
#[test]
fn every_interleaving_of_end_leave_and_create_ends_with_one_owner() {
    #[derive(Clone, Copy)]
    enum Op {
        Create(u64),
        Leave(u64),
        End,
    }
    #[derive(Clone)]
    struct State {
        m: Members<u64>,
        /// Threads the model believes live.
        live: Vec<u64>,
        /// Ids ever created (an id is created once).
        made: Vec<u64>,
        owners: u32,
    }
    let ops = [
        Op::Create(1),
        Op::Create(2),
        Op::Create(3),
        Op::Leave(0),
        Op::Leave(1),
        Op::Leave(2),
        Op::Leave(3),
        Op::End,
    ];
    fn go(state: &State, depth: u32, ops: &[Op], finished: &mut u32) {
        for op in ops {
            let mut s = state.clone();
            // A clone's list has no spare room: the kernel's has (`reserve_all`).
            s.m.reserve_all().unwrap();
            match *op {
                Op::Create(id) => {
                    if s.made.contains(&id) {
                        continue;
                    }
                    let r = s.m.join(id, id);
                    if s.m.is_ending() {
                        assert_eq!(r, Err(JoinError::Ending));
                    } else {
                        assert_eq!(r, Ok(()));
                        s.live.push(id);
                        s.made.push(id);
                    }
                }
                Op::Leave(id) => {
                    let left = s.m.leave(id);
                    assert_eq!(left.was_member, s.live.contains(&id));
                    s.live.retain(|i| *i != id);
                    assert_eq!(left.remaining, s.live.len());
                    if left.owner {
                        s.owners += 1;
                        assert!(s.m.is_ending() && s.live.is_empty());
                    }
                    assert!(!left.began_end || left.owner);
                }
                Op::End => {
                    s.m.begin_end();
                }
            }
            assert_eq!(s.m.len(), s.live.len());
            assert!(s.owners <= 1);
            if s.m.is_ending() && s.live.is_empty() {
                // Ending and empty: an owner was named at the last leave.
                assert_eq!(s.owners, 1, "an ending process that emptied has its owner");
                *finished += 1;
            }
            if depth > 0 {
                go(&s, depth - 1, ops, finished);
            }
        }
    }
    let mut m = Members::new().unwrap();
    m.join(0, 0).unwrap();
    let state = State {
        m,
        live: std::vec![0],
        made: std::vec![0],
        owners: 0,
    };
    let mut finished = 0;
    go(&state, 7, &ops, &mut finished);
    assert!(
        finished > 100,
        "the search reaches the end of processes: {finished}"
    );
}

/// A full run: every thread leaves after the end began; the last is owner.
#[test]
fn the_last_leave_of_an_ending_process_is_the_owner() {
    let mut m = Members::<u32>::new().unwrap();
    for id in 0..4 {
        m.join(id, id as u32).unwrap();
    }
    assert!(m.begin_end());
    assert!(!m.begin_end(), "the record is fixed once");
    assert_eq!(m.join(9, 9), Err(JoinError::Ending));
    for id in [2, 0, 3] {
        let l = m.leave(id);
        assert!(l.was_member && !l.owner);
    }
    let l = m.leave(1);
    assert_eq!((l.remaining, l.owner), (0, true));
    assert!(!m.leave(1).owner, "once");
}

#[test]
fn the_last_thread_of_a_process_that_was_not_ending_ends_it() {
    let mut m = Members::<u32>::new().unwrap();
    m.join(1, 1).unwrap();
    m.join(2, 2).unwrap();
    let l = m.leave(1);
    assert!(l.was_member && l.remaining == 1 && !l.owner && !l.began_end);
    let l = m.leave(2);
    assert!(l.owner && l.began_end && m.is_ending());
}

#[test]
fn a_small_list_refuses_a_second_thread_until_it_is_grown() {
    let mut m = Members::<u32>::new_small().unwrap();
    m.join(1, 1).unwrap();
    assert_eq!(m.join(2, 2), Err(JoinError::Full), "no room was made");
    m.reserve_all().unwrap();
    m.join(2, 2).unwrap();
    m.reserve_all().unwrap();
    assert_eq!(m.len(), 2);
}

#[test]
fn the_list_is_bounded_and_a_refusal_changes_nothing() {
    let mut m = Members::<usize>::new().unwrap();
    for i in 0..MAX_THREADS {
        m.join(i as u64, i).unwrap();
    }
    assert_eq!(m.join(999, 0), Err(JoinError::Full));
    assert_eq!(m.has_room(), Err(JoinError::Full));
    assert_eq!(m.len(), MAX_THREADS);
    assert_eq!(m.first(), Some(&0));
    m.leave(0);
    assert_eq!(m.has_room(), Ok(()));
    assert_eq!(m.first(), Some(&1));
}

#[test]
fn a_walk_takes_steps_of_the_size_it_is_given() {
    let mut m = Members::<usize>::new().unwrap();
    for i in 0..MAX_THREADS {
        m.join(i as u64, i).unwrap();
    }
    let mut seen = Vec::new();
    let mut from = Some(0);
    let mut steps = 0;
    while let Some(at) = from {
        from = m.step(at, WAKE_STEP, |t| seen.push(*t));
        steps += 1;
    }
    assert_eq!(steps, 2, "128 threads are two steps of 64");
    assert_eq!(seen, (0..MAX_THREADS).collect::<Vec<_>>());
}

#[test]
fn the_wait_list_is_fifo_per_key_and_never_grows() {
    let mut w = WaitList::<u32>::new(8).unwrap();
    for (key, who) in [(1, 10), (2, 20), (1, 11), (1, 12), (2, 21)] {
        w.push(key, who).unwrap();
    }
    let mut woke = Vec::new();
    assert_eq!(w.take(1, 2, |t| woke.push(t)), 2);
    assert_eq!(woke, [10, 11], "oldest first");
    assert_eq!(w.waiting_on(1), 1);
    assert_eq!(w.take(3, 5, |_| panic!("nobody waits on 3")), 0);
    for i in 0..5 {
        w.push(7, i).unwrap();
    }
    assert_eq!(w.push(7, 99), Err(Full));
    assert_eq!(w.len(), 8);
    assert_eq!(w.remove_where(|t| *t == 20), Some(20));
    assert_eq!(w.pop_front(), Some(12));
}

/// INV-0301-2's model half: a wake takes exactly the waiters of its address
/// up to its count, and a waiter taken is not taken twice.
#[test]
fn a_wake_takes_what_it_names_and_no_waiter_twice() {
    let mut w = WaitList::<u32>::new(MAX_THREADS).unwrap();
    let mut id = 0;
    for round in 0..200u32 {
        let key = u64::from(round % 3);
        while w.len() < MAX_THREADS && id < 1000 {
            w.push(u64::from(id % 3), id).unwrap();
            id += 1;
        }
        let before = w.waiting_on(key);
        let mut got = Vec::new();
        let n = w.take(key, WAKE_STEP, |t| got.push(t));
        assert_eq!(n, before.min(WAKE_STEP));
        assert_eq!(got.len(), n);
        assert!(got.windows(2).all(|p| p[0] < p[1]), "arrival order");
        assert_eq!(w.waiting_on(key), before - n);
    }
}

/// A failed first `thread_create` gives the grown list back, and a zombie
/// keeps no list memory.
#[test]
fn shrink_and_release_give_the_room_back() {
    let mut m = Members::<u64>::new_small().unwrap();
    m.join(0, 0).unwrap();
    m.reserve_all().unwrap();
    assert!(m.capacity() >= MAX_THREADS);
    m.shrink();
    assert!(m.capacity() < MAX_THREADS);
    assert_eq!(m.len(), 1, "the live entry stays");
    m.reserve_all().unwrap();
    assert!(m.leave(0).owner);
    m.release();
    assert_eq!(m.capacity(), 0);
    assert!(m.is_empty());
}
