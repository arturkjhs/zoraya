//! INV-0264-2 and the arithmetic of INV-0264-1 and -4 on a simulated clock:
//! a model scheduler that does with the crate's queue what the kernel's
//! `schedule` does, over random-free but adversarial job trees.

use std::vec;
use std::vec::Vec;

use super::*;

const MS: u64 = 1_000_000;

struct Arena {
    parent: Vec<Option<usize>>,
    acct: Vec<CpuAcct>,
}

impl Arena {
    fn new() -> Arena {
        Arena {
            parent: vec![None],
            acct: vec![CpuAcct::new()],
        }
    }

    fn child(&mut self, parent: usize, limit: Option<u32>) -> usize {
        self.parent.push(Some(parent));
        self.acct.push(CpuAcct::new());
        let id = self.parent.len() - 1;
        set_limit(self, id, limit).expect("limit");
        id
    }
}

impl Tree for Arena {
    type Id = usize;
    fn parent(&self, id: usize) -> Option<usize> {
        self.parent[id]
    }
    fn get(&self, id: usize) -> CpuAcct {
        self.acct[id]
    }
    fn set(&mut self, id: usize, acct: CpuAcct) {
        self.acct[id] = acct;
    }
}

#[derive(Debug)]
struct Th {
    id: usize,
    job: Option<usize>,
    /// Its process is ending (ADR-0460).
    ending: bool,
}

/// One run of a thread: who, when, for how long.
#[derive(Debug, Clone, Copy)]
struct Run {
    thread: usize,
    start: u64,
    len: u64,
}

/// The kernel's loop in miniature: pick, run for the slice (cut at the next
/// period boundary while something is parked, where the kernel's deadline
/// preempts), charge with the boundary split, push back; release at each
/// boundary; idle to the next boundary when nothing is ready.
fn simulate(tree: &mut Arena, queue: &mut RunQueue<Th>, until: u64, period_ns: u64) -> Vec<Run> {
    let mut now = 0u64;
    let mut runs = Vec::new();
    let mut released_period = 0;
    while now < until {
        let p = period_of(now, period_ns);
        if p != released_period {
            released_period = p;
            while queue.release() > 0 {}
        }
        let picked = queue.pick(tree, now, period_ns, |t| t.job, |t| t.ending);
        let Some(picked) = picked else {
            // At the cap of parks per pick, idle calls again at once; with
            // nothing ready, idle waits for the boundary.
            if queue.is_empty() {
                now = next_boundary(now, period_ns);
            }
            continue;
        };
        let th = picked.thread;
        let mut len = picked.slice_ns.unwrap_or(SLICE_NS);
        if queue.parked_len() > 0 {
            len = len.min(next_boundary(now, period_ns) - now);
        }
        let end = now + len;
        let boundary = period_start(end, period_ns);
        let in_period = if boundary > now { end - boundary } else { len };
        if let Some(job) = th.job {
            charge(
                tree,
                job,
                len,
                in_period,
                period_of(end, period_ns),
                period_ns,
                false,
            );
        }
        runs.push(Run {
            thread: th.id,
            start: now,
            len,
        });
        now = end;
        queue.push_back(th);
    }
    runs
}

fn spinners(queue: &mut RunQueue<Th>, first: usize, n: usize, job: Option<usize>) {
    queue.try_reserve(n + queue.parked_len() + 256).unwrap();
    for i in 0..n {
        queue.push_back(Th {
            id: first + i,
            job,
            ending: false,
        });
    }
}

fn run_ns(runs: &[Run], pick: impl Fn(&Run) -> bool) -> u64 {
    runs.iter().filter(|r| pick(r)).map(|r| r.len).sum()
}

#[test]
fn arithmetic_of_periods_and_budgets() {
    assert_eq!(budget_ns(250, PERIOD_NS), 5 * MS);
    assert_eq!(budget_ns(1000, PERIOD_NS), PERIOD_NS);
    assert_eq!(budget_ns(1, 20_000_000), 20_000);
    assert_eq!(period_of(0, PERIOD_NS), 0);
    assert_eq!(period_of(19_999_999, PERIOD_NS), 0);
    assert_eq!(period_of(20 * MS, PERIOD_NS), 1);
    assert_eq!(period_start(45 * MS, PERIOD_NS), 40 * MS);
    assert_eq!(next_boundary(45 * MS, PERIOD_NS), 60 * MS);
    assert_eq!(next_boundary(40 * MS, PERIOD_NS), 60 * MS);
    assert_eq!(next_boundary(u64::MAX, PERIOD_NS), u64::MAX);
    assert_eq!(period_of(5, 0), 0);
    assert_eq!(next_boundary(5, 0), u64::MAX);
    // Huge inputs do not overflow.
    assert_eq!(budget_ns(1000, u64::MAX), u64::MAX);
}

#[test]
fn limits_follow_the_parent_and_the_range() {
    let mut a = Arena::new();
    let room = a.child(0, Some(500));
    let inner = a.child(room, Some(250));
    assert_eq!(
        set_limit(&mut a, inner, Some(600)),
        Err(LimitError::AboveParent)
    );
    assert_eq!(
        set_limit(&mut a, inner, Some(0)),
        Err(LimitError::OutOfRange)
    );
    assert_eq!(
        set_limit(&mut a, inner, Some(1001)),
        Err(LimitError::OutOfRange)
    );
    // A job without a ceiling of its own below a limited one is bound by it.
    let free = a.child(room, None);
    assert_eq!(effective_limit(&a, free), Some(500));
    assert_eq!(
        set_limit(&mut a, free, Some(501)),
        Err(LimitError::AboveParent)
    );
    set_limit(&mut a, free, Some(500)).unwrap();
    // Removing and lowering a parent's does not touch a child's.
    set_limit(&mut a, room, Some(100)).unwrap();
    assert_eq!(effective_limit(&a, inner), Some(100));
    set_limit(&mut a, inner, None).unwrap();
    assert_eq!(a.get(inner).limit, None);
}

#[test]
fn charge_walks_up_and_splits_at_a_boundary() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let inner = a.child(room, None);
    // 3 ms in period 0.
    charge(&mut a, inner, 3 * MS, 3 * MS, 0, PERIOD_NS, false);
    for n in [inner, room, 0] {
        assert_eq!(a.get(n).used_ns, 3 * MS);
        assert_eq!(a.get(n).used_in(0, PERIOD_NS), 3 * MS);
    }
    // A run of 4 ms that began 1 ms before the boundary: 3 ms count in period 1.
    charge(&mut a, inner, 4 * MS, 3 * MS, 1, PERIOD_NS, false);
    assert_eq!(a.get(room).used_ns, 7 * MS);
    assert_eq!(a.get(room).used_in(1, PERIOD_NS), 3 * MS);
    assert_eq!(
        a.get(room).used_in(0, PERIOD_NS),
        0,
        "a period that is gone reads as zero"
    );
    assert_eq!(a.get(room).used_in(2, PERIOD_NS), 0);
    // `in_period` above the run is clamped.
    charge(&mut a, inner, MS, 9 * MS, 1, PERIOD_NS, false);
    assert_eq!(a.get(room).used_in(1, PERIOD_NS), 4 * MS);
}

#[test]
fn the_slice_is_the_smaller_of_ten_ms_and_what_is_left() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let t = |a: &Arena, now| decide(a, room, now, PERIOD_NS);
    assert_eq!(t(&a, 0), Decision::Run { slice_ns: 5 * MS });
    assert_eq!(decide(&a, 0, 0, PERIOD_NS), Decision::Unlimited);
    charge(&mut a, room, 2 * MS, 2 * MS, 0, PERIOD_NS, false);
    assert_eq!(t(&a, 2 * MS), Decision::Run { slice_ns: 3 * MS });
    // Less than the floor left: the floor.
    charge(
        &mut a,
        room,
        3 * MS - 10_000,
        3 * MS - 10_000,
        0,
        PERIOD_NS,
        false,
    );
    assert_eq!(
        t(&a, 5 * MS),
        Decision::Run {
            slice_ns: MIN_SLICE_NS
        }
    );
    charge(&mut a, room, 10_000, 10_000, 0, PERIOD_NS, false);
    assert_eq!(t(&a, 5 * MS), Decision::Park { at: room });
    // The next period starts afresh.
    assert_eq!(t(&a, 20 * MS), Decision::Run { slice_ns: 5 * MS });
    // A ceiling of 100%: the slice is the ADR-0030 one.
    let full = a.child(0, Some(1000));
    assert_eq!(
        decide(&a, full, 0, PERIOD_NS),
        Decision::Run { slice_ns: SLICE_NS }
    );
}

#[test]
fn the_most_constrained_ancestor_decides() {
    let mut a = Arena::new();
    let parent = a.child(0, Some(500));
    let kid = a.child(parent, Some(500));
    // The parent's budget is 10 ms; the child's is 10 ms too.
    charge(&mut a, kid, 8 * MS, 8 * MS, 0, PERIOD_NS, false);
    assert_eq!(
        decide(&a, kid, 8 * MS, PERIOD_NS),
        Decision::Run { slice_ns: 2 * MS }
    );
    // A sibling spends the parent's budget: the child is parked by the parent.
    let sibling = a.child(parent, Some(500));
    charge(&mut a, sibling, 2 * MS, 2 * MS, 0, PERIOD_NS, false);
    assert_eq!(
        decide(&a, kid, 10 * MS, PERIOD_NS),
        Decision::Park { at: parent }
    );
}

/// INV-0264-1: eight spinners at 25% for 2 s use the budget plus at most one
/// floor of overrun per period, and nearly all of the budget.
#[test]
fn eight_spinners_at_a_quarter_use_a_quarter() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let mut q = RunQueue::new();
    spinners(&mut q, 0, 8, Some(room));
    let runs = simulate(&mut a, &mut q, 2000 * MS, PERIOD_NS);
    let used = a.get(room).used_ns;
    let periods = 100;
    let budget = 5 * MS * periods;
    assert_eq!(used, run_ns(&runs, |_| true));
    assert!(used <= budget + periods * MIN_SLICE_NS, "{used}");
    assert!(used * 100 >= budget * 99, "{used}");
    // And in every single period.
    for p in 0..periods {
        let in_p = run_ns(&runs, |r| period_of(r.start, PERIOD_NS) == p);
        assert!(in_p <= 5 * MS + MIN_SLICE_NS, "period {p}: {in_p}");
        assert!(in_p >= 5 * MS - MIN_SLICE_NS, "period {p}: {in_p}");
    }
}

/// INV-0264-4: three rooms at 25% under a parent at 50% use at most 50%.
#[test]
fn three_rooms_under_a_half_use_a_half() {
    let mut a = Arena::new();
    let untrusted = a.child(0, Some(500));
    let mut q = RunQueue::new();
    let mut rooms = Vec::new();
    for i in 0..3 {
        let room = a.child(untrusted, Some(250));
        rooms.push(room);
        spinners(&mut q, i * 8, 8, Some(room));
    }
    let runs = simulate(&mut a, &mut q, 2000 * MS, PERIOD_NS);
    let total = a.get(untrusted).used_ns;
    let periods = 100;
    assert!(
        total <= 10 * MS * periods + periods * MIN_SLICE_NS,
        "{total}"
    );
    assert!(total * 100 >= 10 * MS * periods * 99, "{total}");
    for p in 0..periods {
        let in_p = run_ns(&runs, |r| period_of(r.start, PERIOD_NS) == p);
        assert!(in_p <= 10 * MS + MIN_SLICE_NS, "period {p}: {in_p}");
    }
    // The rooms are each under their own, too.
    for room in rooms {
        assert!(a.get(room).used_ns <= 5 * MS * periods + periods * MIN_SLICE_NS);
        assert!(a.get(room).parked > 0);
    }
}

/// INV-0264-2: a thread that was parked does not run until its period ends.
#[test]
fn a_parked_thread_waits_for_the_boundary() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let mut q = RunQueue::new();
    spinners(&mut q, 0, 4, Some(room));
    // One unlimited thread keeps the queue busy.
    spinners(&mut q, 100, 1, None);
    let runs = simulate(&mut a, &mut q, 400 * MS, PERIOD_NS);
    // No limited thread starts a run once its job has spent the period's
    // budget: replay the charges and check each start.
    let mut b = Arena::new();
    let room_b = b.child(0, Some(250));
    for r in &runs {
        if r.thread < 100 {
            let p = period_of(r.start, PERIOD_NS);
            assert!(
                b.get(room_b).used_in(p, PERIOD_NS) < budget_ns(250, PERIOD_NS),
                "thread {} ran at {} with the budget spent",
                r.thread,
                r.start
            );
            let end = r.start + r.len;
            let boundary = period_start(end, PERIOD_NS);
            let in_period = if boundary > r.start {
                end - boundary
            } else {
                r.len
            };
            charge(
                &mut b,
                room_b,
                r.len,
                in_period,
                period_of(end, PERIOD_NS),
                PERIOD_NS,
                false,
            );
        }
    }
    // The unlimited thread took the rest: 75% of the CPU.
    let free = run_ns(&runs, |r| r.thread == 100);
    assert!(free >= 400 * MS * 3 / 4 - 4 * MS, "{free}");
    // Parked threads exist, and none is ready at a boundary's start before
    // `release`: nothing is lost either (every thread ran in the end).
    for t in (0..4).chain([100]) {
        assert!(runs.iter().any(|r| r.thread == t), "thread {t} never ran");
    }
}

#[test]
fn nothing_runs_between_the_budget_and_the_boundary_when_alone() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let mut q = RunQueue::new();
    spinners(&mut q, 0, 1, Some(room));
    let runs = simulate(&mut a, &mut q, 200 * MS, PERIOD_NS);
    // A lone thread: 5 ms from each boundary, then idle until the next.
    for r in &runs {
        let offset = r.start - period_start(r.start, PERIOD_NS);
        assert!(offset < 5 * MS, "ran at offset {offset}");
        assert!(r.len <= 5 * MS);
    }
    assert_eq!(runs.len(), 10);
}

/// A job over its budget with more threads than one pick may park: the pick
/// parks 16, puts the next back at the front and returns nothing (idle runs
/// and calls again); no thread of the spent job is picked.
#[test]
fn a_pick_parks_at_most_sixteen_and_runs_none() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    let mut q = RunQueue::new();
    spinners(&mut q, 0, 40, Some(room));
    let mut calls = 0;
    while !q.is_empty() {
        let before = q.parked_len();
        assert!(
            q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job, |t| t.ending)
                .is_none()
        );
        assert!(q.parked_len() - before <= MAX_PARKS_PER_PICK);
        calls += 1;
    }
    assert_eq!(calls, 3, "40 threads: 16, 16, 8");
    assert_eq!(q.parked_len(), 40);
    assert_eq!(a.get(room).parked, 40);
    // After the boundary the whole lot comes back, in batches, in order.
    let mut total = 0;
    loop {
        let n = q.release();
        if n == 0 {
            break;
        }
        assert!(n <= RELEASE_BATCH);
        total += n;
    }
    assert_eq!(total, 40);
    assert_eq!(q.parked_len(), 0);
}

/// ADR-0285 point 1, review W1: 200 threads in a room at 25% (the session's
/// process limit) use no more than the budget plus one floor per period, and
/// the other jobs' threads are not held up behind them.
#[test]
fn two_hundred_threads_in_a_spent_job_cost_one_floor_per_period() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let mut q = RunQueue::new();
    spinners(&mut q, 0, 200, Some(room));
    spinners(&mut q, 1000, 1, None);
    let runs = simulate(&mut a, &mut q, 400 * MS, PERIOD_NS);
    for p in 0..20 {
        let in_p = run_ns(&runs, |r| {
            r.thread < 1000 && period_of(r.start, PERIOD_NS) == p
        });
        assert!(in_p <= 5 * MS + MIN_SLICE_NS, "period {p}: {in_p}");
    }
    let free = run_ns(&runs, |r| r.thread == 1000);
    assert!(free >= 400 * MS * 3 / 4 - 4 * MS, "{free}");
}

#[test]
fn release_comes_in_batches_and_keeps_order() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    let mut q = RunQueue::new();
    q.try_reserve(300).unwrap();
    // Park 150 in picks of 16 (each pick also runs the 17th, which we drop).
    for i in 0..150 {
        q.push_back(Th {
            id: i,
            job: Some(room),
            ending: false,
        });
    }
    while !q.is_empty() {
        let _ = q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job, |t| t.ending);
    }
    let parked = q.parked_len();
    assert!(parked >= 100);
    let mut got = Vec::new();
    let mut moved = q.release();
    assert_eq!(moved, RELEASE_BATCH);
    while moved > 0 {
        while let Some(p) = q.pick(
            &mut Arena::new(),
            0,
            PERIOD_NS,
            |t| t.job.map(|_| 0),
            |t| t.ending,
        ) {
            got.push(p.thread.id);
        }
        moved = q.release();
    }
    let mut sorted = got.clone();
    sorted.sort_unstable();
    assert_eq!(got, sorted, "parked order is kept");
    assert_eq!(got.len(), parked);
}

#[test]
fn retain_covers_both_lists() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    let mut q = RunQueue::new();
    q.try_reserve(8).unwrap();
    for i in 0..4 {
        q.push_back(Th {
            id: i,
            job: Some(room),
            ending: false,
        });
    }
    let first = q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job, |t| t.ending);
    assert!(first.is_none());
    assert_eq!(q.parked_len(), 4);
    q.retain(|t| t.id != 2);
    assert_eq!(q.parked_len(), 3);
}

/// ADR-0460: a thread whose process is ending is taken from a spent job,
/// with the floor slice, and is not counted as parked; the job's other
/// threads are still parked, so no thread that is not ending runs.
#[test]
fn an_ending_thread_of_a_spent_job_runs_and_the_others_park() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    let mut q = RunQueue::new();
    q.try_reserve(8).unwrap();
    for i in 0..4 {
        q.push_back(Th {
            id: i,
            job: Some(room),
            ending: i == 2,
        });
    }
    let picked = q
        .pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job, |t| t.ending)
        .expect("the ending thread");
    assert_eq!(picked.thread.id, 2);
    assert_eq!(picked.slice_ns, Some(MIN_SLICE_NS));
    assert_eq!(q.parked_len(), 2, "the two ahead of it are parked");
    assert_eq!(a.get(room).parked, 2, "the ending thread is not counted");
    assert!(
        q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job, |t| t.ending)
            .is_none(),
        "the one behind it is not ending and does not run"
    );
    assert_eq!(q.parked_len(), 3);
    // With budget left, an ending thread is decided like any other.
    let mut b = Arena::new();
    let free = b.child(0, Some(250));
    q.push_back(Th {
        id: 9,
        job: Some(free),
        ending: true,
    });
    let p = q
        .pick(&mut b, 0, PERIOD_NS, |t| t.job, |t| t.ending)
        .expect("budget left");
    assert_eq!(p.slice_ns, Some(5 * MS));
}

/// ADR-0285 item 7 with ADR-0460: a room whose 20 threads are parked is
/// killed; each thread's way out (its teardown) takes 300 us against a
/// budget of 20 us a period, 6 ms in all. The threads come back at the next
/// boundary, every one runs its way out in that period, and the room is
/// charged for all of it. A way out is many turns, as in the kernel (it
/// yields at its step points once its slice ends): each turn is the slice
/// `pick` gave, and the thread goes to the back of the queue between turns,
/// so it is taken again and again from a spent job. Without the exception
/// each period's budget would end a turn or two (20 us and the floor), and
/// the room would not be gone within the 100 periods the test runs.
#[test]
fn a_killed_room_dies_in_the_period_after_the_kill_and_pays_for_it() {
    use core::cell::Cell;
    const TEARDOWN: u64 = 300_000;
    let mut a = Arena::new();
    // 1 thousandth of 20 ms: 20 us. Spend it.
    let room = a.child(0, Some(1));
    let mut q = RunQueue::new();
    q.try_reserve(64).unwrap();
    for i in 0..20 {
        q.push_back(Th {
            id: i,
            job: Some(room),
            ending: false,
        });
    }
    q.push_back(Th {
        id: 100,
        job: None,
        ending: false,
    });
    let killed = Cell::new(false);
    let ending = |t: &Th| killed.get() && t.job == Some(room);
    let mut now = 0u64;
    let mut alive = 20;
    let mut released = 0;
    let mut kill_period = None;
    let mut died = None;
    let used_before_kill = Cell::new(0);
    let mut left = [TEARDOWN; 20];
    while now < 100 * PERIOD_NS && died.is_none() {
        let p = period_of(now, PERIOD_NS);
        if p != released {
            released = p;
            while q.release() > 0 {}
        }
        // The kill lands 10 ms into period 2, with every room thread parked.
        if !killed.get() && now >= 2 * PERIOD_NS + 10 * MS {
            assert_eq!(q.parked_len(), 20, "parked when killed");
            killed.set(true);
            kill_period = Some(p);
            used_before_kill.set(a.get(room).used_ns);
        }
        let Some(picked) = q.pick(&mut a, now, PERIOD_NS, |t| t.job, ending) else {
            if q.is_empty() {
                now = next_boundary(now, PERIOD_NS);
            }
            continue;
        };
        let th = picked.thread;
        if th.job.is_none() {
            // The free thread: runs 1 ms at a time, never past a boundary.
            let len = MS.min(next_boundary(now, PERIOD_NS) - now);
            now += len;
            q.push_back(th);
            continue;
        }
        if ending(&th) {
            // One turn of the way out: the slice, or what is left of it.
            let turn = picked.slice_ns.unwrap_or(SLICE_NS).min(left[th.id]);
            charge(
                &mut a,
                room,
                turn,
                turn,
                period_of(now, PERIOD_NS),
                PERIOD_NS,
                true,
            );
            now += turn;
            left[th.id] -= turn;
            if left[th.id] > 0 {
                q.push_back(th);
                continue;
            }
            alive -= 1;
            if alive == 0 {
                died = Some(period_of(now, PERIOD_NS));
            }
            continue;
        }
        // Not yet killed: spin for the slice.
        let len = picked.slice_ns.unwrap_or(SLICE_NS);
        charge(
            &mut a,
            room,
            len,
            len,
            period_of(now + len, PERIOD_NS),
            PERIOD_NS,
            false,
        );
        now += len;
        q.push_back(th);
    }
    let kill_period = kill_period.expect("killed");
    assert_eq!(
        died,
        Some(kill_period + 1),
        "the room dies in the next period"
    );
    assert_eq!(
        a.get(room).used_ns - used_before_kill.get(),
        20 * TEARDOWN,
        "every teardown is charged"
    );
}

/// ADR-0460's debt: what an ending thread ran past the budget is paid back
/// from the next periods' budgets, a whole budget a period at most; a period
/// with nothing counted pays too; without a ceiling there is no debt.
#[test]
fn a_debt_is_carried_and_paid_back_a_budget_a_period() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250)); // 5 ms of 20.
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    assert_eq!(a.get(room).debt, 0, "a plain run makes no debt");
    charge(&mut a, room, 7 * MS, 7 * MS, 0, PERIOD_NS, true);
    assert_eq!(a.get(room).debt, 7 * MS);
    assert_eq!(a.get(room).used_ns, 12 * MS, "the usage is the time run");
    // Period 1 begins with a whole budget paid: parked all through.
    assert_eq!(a.get(room).used_in(1, PERIOD_NS), 5 * MS);
    assert_eq!(
        decide(&a, room, PERIOD_NS + MS, PERIOD_NS),
        Decision::Park { at: room }
    );
    // Period 2 begins with the rest, 2 ms, and has 3 ms left.
    assert_eq!(a.get(room).used_in(2, PERIOD_NS), 2 * MS);
    assert_eq!(
        decide(&a, room, 2 * PERIOD_NS, PERIOD_NS),
        Decision::Run { slice_ns: 3 * MS }
    );
    // Periods with nothing counted pay a budget each: read in period 3, the
    // 7 ms owed in period 0 are paid by periods 1 and 2.
    assert_eq!(a.get(room).used_in(3, PERIOD_NS), 0);
    let settled = settle(a.get(room), 2, PERIOD_NS);
    assert_eq!((settled.period_used, settled.debt), (2 * MS, 0));
    // An ending run inside the budget makes no debt; only the part past it.
    let mut b = Arena::new();
    let r = b.child(0, Some(250));
    charge(&mut b, r, 4 * MS, 4 * MS, 0, PERIOD_NS, true);
    assert_eq!(b.get(r).debt, 0);
    charge(&mut b, r, 3 * MS, 3 * MS, 0, PERIOD_NS, true);
    assert_eq!(b.get(r).debt, 2 * MS);
    // No ceiling, no debt.
    let mut c = Arena::new();
    let free = c.child(0, None);
    charge(&mut c, free, 30 * MS, 30 * MS, 0, PERIOD_NS, true);
    assert_eq!(c.get(free).debt, 0);
    // Removing the ceiling clears the debt.
    set_limit(&mut b, r, None).unwrap();
    assert_eq!(b.get(r).debt, 0);
}

/// The cap: one long way out parks the job for at most
/// `DEBT_CAP_PERIODS` periods; the rest is forgiven.
#[test]
fn a_debt_is_capped_at_a_hundred_budgets() {
    let mut a = Arena::new();
    let room = a.child(0, Some(10)); // 200 us of 20 ms.
    charge(&mut a, room, 200_000, 200_000, 0, PERIOD_NS, false);
    charge(&mut a, room, 10_000 * MS, 10_000 * MS, 0, PERIOD_NS, true);
    assert_eq!(a.get(room).debt, debt_cap(10, PERIOD_NS));
    assert_eq!(debt_cap(10, PERIOD_NS), 100 * 200_000);
    assert_eq!(
        decide(&a, room, 100 * PERIOD_NS, PERIOD_NS),
        Decision::Park { at: room }
    );
    assert!(matches!(
        decide(&a, room, 101 * PERIOD_NS, PERIOD_NS),
        Decision::Run { .. }
    ));
}

/// The same rule up the chain: each node with a ceiling takes as debt the
/// part of the run past its own budget.
#[test]
fn a_debt_follows_the_parent_chain_by_each_budget() {
    let mut a = Arena::new();
    let parent = a.child(0, Some(500)); // 10 ms.
    let room = a.child(parent, Some(250)); // 5 ms.
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    charge(&mut a, room, 8 * MS, 8 * MS, 0, PERIOD_NS, true);
    assert_eq!(a.get(room).debt, 8 * MS);
    assert_eq!(a.get(parent).debt, 3 * MS, "13 ms against 10");
    assert_eq!(a.get(0).debt, 0, "the root has no ceiling");
}

/// A room that ends for good keeps no debt; what it owed, as of then, goes
/// to the nearest ancestor with a ceiling.
#[test]
fn a_dead_rooms_debt_goes_to_the_nearest_ancestor_with_a_ceiling() {
    let mut a = Arena::new();
    let top = a.child(0, Some(500)); // 10 ms.
    let middle = a.child(top, None);
    let room = a.child(middle, Some(250)); // 5 ms.
    charge(&mut a, room, 5 * MS, 5 * MS, 0, PERIOD_NS, false);
    charge(&mut a, room, 12 * MS, 12 * MS, 0, PERIOD_NS, true);
    assert_eq!(a.get(room).debt, 12 * MS);
    assert_eq!(a.get(top).debt, 7 * MS);
    // It dies in period 1, which paid 5 ms of its 12: 7 ms go up, past the
    // middle job (no ceiling) to the top, which paid 7 ms of its own 7.
    let room_acct = a.get(room);
    let passed = pass_debt(&mut a, room_acct, Some(middle), PERIOD_NS + MS, PERIOD_NS);
    assert_eq!(passed, 7 * MS);
    assert_eq!(a.get(middle).debt, 0);
    assert_eq!(settle(a.get(top), 1, PERIOD_NS).debt, 7 * MS);
    // With no ancestor that has a ceiling, nothing is passed.
    let mut b = Arena::new();
    let lone = b.child(0, Some(250));
    charge(&mut b, lone, 9 * MS, 9 * MS, 0, PERIOD_NS, true);
    let lone_acct = b.get(lone);
    assert_eq!(pass_debt(&mut b, lone_acct, Some(0), MS, PERIOD_NS), 0);
}

/// A room that builds a child process and lets it end, again and again: a
/// builder thread that runs 4 ms per child, and each child's way out, 6 ms,
/// taken unparked in turns of the floor slice, with nothing else ready (a
/// thread of another job with a 10 ms slice between the turns would stretch
/// the way out over many periods). Returns the room's use over periods 5 to
/// 55 and the bound.
fn start_and_end_loop(debt: bool) -> (u64, u64) {
    const BUILD: u64 = 4 * MS;
    const TEARDOWN: u64 = 6 * MS;
    let mut a = Arena::new();
    let room = a.child(0, Some(100)); // 2 ms of 20.
    let mut q = RunQueue::new();
    q.try_reserve(64).unwrap();
    q.push_back(Th {
        id: 0,
        job: Some(room),
        ending: false,
    });
    let mut left = std::collections::BTreeMap::new();
    let mut built = 0;
    let mut next_id = 1;
    let mut now = 0u64;
    let mut released = 0;
    let mut at = [None, None];
    while now < 60 * PERIOD_NS {
        let p = period_of(now, PERIOD_NS);
        if p != released {
            released = p;
            while q.release() > 0 {}
        }
        for (i, edge) in [5, 55].into_iter().enumerate() {
            if p >= edge && at[i].is_none() {
                at[i] = Some(a.get(room).used_ns);
            }
        }
        let Some(picked) = q.pick(&mut a, now, PERIOD_NS, |t| t.job, |t| t.ending) else {
            if q.is_empty() {
                now = next_boundary(now, PERIOD_NS);
            }
            continue;
        };
        let th = picked.thread;
        let mut len = picked.slice_ns.unwrap_or(SLICE_NS);
        len = len.min(next_boundary(now, PERIOD_NS) - now);
        if th.ending {
            let rest: &mut u64 = left.get_mut(&th.id).unwrap();
            len = len.min(*rest);
            *rest -= len;
            charge(&mut a, room, len, len, p, PERIOD_NS, debt);
            now += len;
            if *rest > 0 {
                q.push_back(th);
            }
            continue;
        }
        if th.job.is_some() {
            charge(&mut a, room, len, len, p, PERIOD_NS, false);
            built += len;
            if built >= BUILD {
                built -= BUILD;
                left.insert(next_id, TEARDOWN);
                q.push_back(Th {
                    id: next_id,
                    job: Some(room),
                    ending: true,
                });
                next_id += 1;
            }
        }
        now += len;
        q.push_back(th);
    }
    let used = at[1].unwrap() - at[0].unwrap();
    (
        used,
        50 * budget_ns(100, PERIOD_NS) + TEARDOWN + budget_ns(100, PERIOD_NS) + 50 * MIN_SLICE_NS,
    )
}

/// ADR-0460, the architect's decision on open question 2: with the debt,
/// the start-and-end loop gets no more than its ceiling over 50 periods,
/// plus one way out, one budget and a floor slice a period; without it (the
/// ending thread charged but nothing carried) it gets far more.
#[test]
fn a_start_and_end_loop_keeps_its_ceiling_with_the_debt() {
    let (used, bound) = start_and_end_loop(true);
    assert!(used <= bound, "with the debt: {used} ns, bound {bound} ns");
    let (old, _) = start_and_end_loop(false);
    assert!(
        old > bound * 3 / 2,
        "without the debt: {old} ns, bound {bound} ns"
    );
}
