//! A scripted clock (INV-0014-1): the test sets the wall clock and the
//! monotonic clock; clones share the same state.

use std::cell::Cell;
use std::rc::Rc;

use zoraya_fs::{Clock, Timestamp};

#[derive(Debug, Clone, Default)]
pub struct TestClock {
    wall: Rc<Cell<Timestamp>>,
    mono: Rc<Cell<u64>>,
    reads: Rc<Cell<u64>>,
}

impl TestClock {
    pub fn new(wall: Timestamp) -> TestClock {
        let c = TestClock::default();
        c.set_wall(wall);
        c
    }
    pub fn set_wall(&self, t: Timestamp) {
        self.wall.set(t);
    }
    pub fn wall_now(&self) -> Timestamp {
        self.wall.get()
    }
    pub fn advance_mono(&self, ns: u64) {
        self.mono.set(self.mono.get() + ns);
    }
    pub fn mono_now(&self) -> u64 {
        self.mono.get()
    }
    /// How often the engine read the wall clock.
    pub fn wall_reads(&self) -> u64 {
        self.reads.get()
    }
}

impl Clock for TestClock {
    fn wall(&mut self) -> Timestamp {
        self.reads.set(self.reads.get() + 1);
        self.wall.get()
    }
    fn monotonic_ns(&mut self) -> u64 {
        self.mono.get()
    }
}
