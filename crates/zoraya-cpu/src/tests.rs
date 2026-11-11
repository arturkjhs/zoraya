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
        let picked = queue.pick(tree, now, period_ns, |t| t.job);
        let Some(picked) = picked else {
            now = next_boundary(now, period_ns);
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
            charge(tree, job, len, in_period, period_of(end, period_ns));
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
        queue.push_back(Th { id: first + i, job });
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
    charge(&mut a, inner, 3 * MS, 3 * MS, 0);
    for n in [inner, room, 0] {
        assert_eq!(a.get(n).used_ns, 3 * MS);
        assert_eq!(a.get(n).used_in(0), 3 * MS);
    }
    // A run of 4 ms that began 1 ms before the boundary: 3 ms count in period 1.
    charge(&mut a, inner, 4 * MS, 3 * MS, 1);
    assert_eq!(a.get(room).used_ns, 7 * MS);
    assert_eq!(a.get(room).used_in(1), 3 * MS);
    assert_eq!(
        a.get(room).used_in(0),
        0,
        "a period that is gone reads as zero"
    );
    assert_eq!(a.get(room).used_in(2), 0);
    // `in_period` above the run is clamped.
    charge(&mut a, inner, MS, 9 * MS, 1);
    assert_eq!(a.get(room).used_in(1), 4 * MS);
}

#[test]
fn the_slice_is_the_smaller_of_ten_ms_and_what_is_left() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    let t = |a: &Arena, now| decide(a, room, now, PERIOD_NS);
    assert_eq!(t(&a, 0), Decision::Run { slice_ns: 5 * MS });
    assert_eq!(decide(&a, 0, 0, PERIOD_NS), Decision::Unlimited);
    charge(&mut a, room, 2 * MS, 2 * MS, 0);
    assert_eq!(t(&a, 2 * MS), Decision::Run { slice_ns: 3 * MS });
    // Less than the floor left: the floor.
    charge(&mut a, room, 3 * MS - 10_000, 3 * MS - 10_000, 0);
    assert_eq!(
        t(&a, 5 * MS),
        Decision::Run {
            slice_ns: MIN_SLICE_NS
        }
    );
    charge(&mut a, room, 10_000, 10_000, 0);
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
    charge(&mut a, kid, 8 * MS, 8 * MS, 0);
    assert_eq!(
        decide(&a, kid, 8 * MS, PERIOD_NS),
        Decision::Run { slice_ns: 2 * MS }
    );
    // A sibling spends the parent's budget: the child is parked by the parent.
    let sibling = a.child(parent, Some(500));
    charge(&mut a, sibling, 2 * MS, 2 * MS, 0);
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
                b.get(room_b).used_in(p) < budget_ns(250, PERIOD_NS),
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
            charge(&mut b, room_b, r.len, in_period, period_of(end, PERIOD_NS));
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

/// A job over its budget with more threads than one pick may park: the rest
/// run for the floor and are parked at their next pick.
#[test]
fn a_pick_parks_at_most_sixteen() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    charge(&mut a, room, 5 * MS, 5 * MS, 0);
    let mut q = RunQueue::new();
    spinners(&mut q, 0, 40, Some(room));
    let picked = q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job).unwrap();
    assert_eq!(picked.slice_ns, Some(MIN_SLICE_NS));
    assert_eq!(picked.thread.id, MAX_PARKS_PER_PICK);
    assert_eq!(q.parked_len(), MAX_PARKS_PER_PICK);
    assert_eq!(a.get(room).parked, MAX_PARKS_PER_PICK as u64);
    // After the boundary the whole lot comes back, in batches, in order.
    let mut batches = 0;
    let mut total = 0;
    loop {
        let n = q.release();
        if n == 0 {
            break;
        }
        assert!(n <= RELEASE_BATCH);
        batches += 1;
        total += n;
    }
    assert_eq!((total, batches), (MAX_PARKS_PER_PICK, 1));
    assert_eq!(q.parked_len(), 0);
}

#[test]
fn release_comes_in_batches_and_keeps_order() {
    let mut a = Arena::new();
    let room = a.child(0, Some(250));
    charge(&mut a, room, 5 * MS, 5 * MS, 0);
    let mut q = RunQueue::new();
    q.try_reserve(300).unwrap();
    // Park 150 in picks of 16 (each pick also runs the 17th, which we drop).
    for i in 0..150 {
        q.push_back(Th {
            id: i,
            job: Some(room),
        });
    }
    while !q.is_empty() {
        let _ = q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job);
    }
    let parked = q.parked_len();
    assert!(parked >= 100);
    let mut got = Vec::new();
    let mut moved = q.release();
    assert_eq!(moved, RELEASE_BATCH);
    while moved > 0 {
        while let Some(p) = q.pick(&mut Arena::new(), 0, PERIOD_NS, |t| t.job.map(|_| 0)) {
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
    charge(&mut a, room, 5 * MS, 5 * MS, 0);
    let mut q = RunQueue::new();
    q.try_reserve(8).unwrap();
    for i in 0..4 {
        q.push_back(Th {
            id: i,
            job: Some(room),
        });
    }
    let first = q.pick(&mut a, 5 * MS, PERIOD_NS, |t| t.job);
    assert!(first.is_none());
    assert_eq!(q.parked_len(), 4);
    q.retain(|t| t.id != 2);
    assert_eq!(q.parked_len(), 3);
}
