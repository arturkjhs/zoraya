//! The fd table (ADR-0103 items 4 and 5): `fds[0..OPEN_MAX)` of a
//! description entry and `FD_CLOEXEC`. `dup` and its kin make a second fd on
//! the same entry, in the process; the last fd of an entry gives it back to
//! be closed. Generic over what an entry holds, so it is tested on the host.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use crate::errno::{EBADF, EINVAL, EMFILE};
use zoraya_abi::posix::OPEN_MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Slot {
    entry: usize,
    cloexec: bool,
}

struct Entry<D> {
    item: D,
    fds: u32,
}

/// The table. `D` is a description entry.
pub struct Table<D> {
    fds: Vec<Option<Slot>>,
    entries: Vec<Option<Entry<D>>>,
}

impl<D> Default for Table<D> {
    fn default() -> Self {
        Table::new()
    }
}

impl<D> Table<D> {
    pub fn new() -> Table<D> {
        Table {
            fds: Vec::new(),
            entries: Vec::new(),
        }
    }

    fn slot(&self, fd: i32) -> Option<Slot> {
        usize::try_from(fd)
            .ok()
            .and_then(|i| self.fds.get(i).copied().flatten())
    }

    fn lowest_free(&self, min: u32) -> Option<u32> {
        (min..OPEN_MAX).find(|&fd| self.fds.get(fd as usize).is_none_or(Option::is_none))
    }

    fn put(&mut self, fd: u32, slot: Slot) {
        let i = fd as usize;
        if self.fds.len() <= i {
            self.fds.resize(i + 1, None);
        }
        self.fds[i] = Some(slot);
        if let Some(Some(e)) = self.entries.get_mut(slot.entry) {
            e.fds += 1;
        }
    }

    fn new_entry(&mut self, item: D) -> usize {
        let entry = Some(Entry { item, fds: 0 });
        match self.entries.iter().position(Option::is_none) {
            Some(i) => {
                self.entries[i] = entry;
                i
            }
            None => {
                self.entries.push(entry);
                self.entries.len() - 1
            }
        }
    }

    /// A new entry at the lowest free fd at or above `min`; the item back if
    /// there is none (`EMFILE`).
    pub fn install(&mut self, min: u32, item: D, cloexec: bool) -> Result<u32, D> {
        let Some(fd) = self.lowest_free(min) else {
            return Err(item);
        };
        let entry = self.new_entry(item);
        self.put(fd, Slot { entry, cloexec });
        Ok(fd)
    }

    /// A new entry at `fd`, which must be free: for start-up, from the slots
    /// the spawner filled (ADR-0103 item 7).
    pub fn install_at(&mut self, fd: u32, item: D) -> Result<(), D> {
        if fd >= OPEN_MAX || self.slot(fd as i32).is_some() {
            return Err(item);
        }
        let entry = self.new_entry(item);
        self.put(
            fd,
            Slot {
                entry,
                cloexec: false,
            },
        );
        Ok(())
    }

    pub fn get(&self, fd: i32) -> Option<&D> {
        let slot = self.slot(fd)?;
        self.entries.get(slot.entry)?.as_ref().map(|e| &e.item)
    }

    pub fn get_mut(&mut self, fd: i32) -> Option<&mut D> {
        let slot = self.slot(fd)?;
        self.entries
            .get_mut(slot.entry)?
            .as_mut()
            .map(|e| &mut e.item)
    }

    /// True if `a` and `b` are fds of one entry.
    pub fn same(&self, a: i32, b: i32) -> bool {
        matches!((self.slot(a), self.slot(b)), (Some(x), Some(y)) if x.entry == y.entry)
    }

    pub fn cloexec(&self, fd: i32) -> Result<bool, i32> {
        self.slot(fd).map(|s| s.cloexec).ok_or(EBADF)
    }

    pub fn set_cloexec(&mut self, fd: i32, cloexec: bool) -> Result<(), i32> {
        let slot = self.slot(fd).ok_or(EBADF)?;
        self.fds[fd as usize] = Some(Slot { cloexec, ..slot });
        Ok(())
    }

    /// `dup`, `F_DUPFD` and `F_DUPFD_CLOEXEC`: the lowest free fd at or above
    /// `min` on `fd`'s entry. `FD_CLOEXEC` is the new fd's own.
    pub fn dup(&mut self, fd: i32, min: i32, cloexec: bool) -> Result<u32, i32> {
        let slot = self.slot(fd).ok_or(EBADF)?;
        let min = u32::try_from(min).map_err(|_| EINVAL)?;
        if min >= OPEN_MAX {
            return Err(EINVAL);
        }
        let new = self.lowest_free(min).ok_or(EMFILE)?;
        self.put(
            new,
            Slot {
                entry: slot.entry,
                cloexec,
            },
        );
        Ok(new)
    }

    /// `dup2` and `dup3`: `new` on `old`'s entry, closing what `new` held
    /// first, whose entry comes back if it was its last fd. With `old ==
    /// new`, nothing changes ([P4]).
    pub fn dup2(&mut self, old: i32, new: i32, cloexec: bool) -> Result<Option<D>, i32> {
        let slot = self.slot(old).ok_or(EBADF)?;
        let target = u32::try_from(new).map_err(|_| EBADF)?;
        if target >= OPEN_MAX {
            return Err(EBADF);
        }
        if old == new {
            return Ok(None);
        }
        let closed = match self.slot(new) {
            Some(_) => self.close(new)?,
            None => None,
        };
        self.put(
            target,
            Slot {
                entry: slot.entry,
                cloexec,
            },
        );
        Ok(closed)
    }

    /// Removes `fd`; its entry comes back if this was its last fd.
    pub fn close(&mut self, fd: i32) -> Result<Option<D>, i32> {
        let slot = self.slot(fd).ok_or(EBADF)?;
        self.fds[fd as usize] = None;
        let Some(Some(e)) = self.entries.get_mut(slot.entry) else {
            return Ok(None);
        };
        e.fds -= 1;
        if e.fds > 0 {
            return Ok(None);
        }
        Ok(self.entries[slot.entry].take().map(|e| e.item))
    }

    /// Open fds.
    pub fn count(&self) -> usize {
        self.fds.iter().flatten().count()
    }

    /// Live entries.
    pub fn entries(&self) -> usize {
        self.entries.iter().flatten().count()
    }

    /// The fds in use, in order.
    pub fn fds(&self) -> impl Iterator<Item = u32> + '_ {
        self.fds
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_some())
            .map(|(i, _)| i as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn install_takes_the_lowest_free_fd() {
        let mut t = Table::new();
        assert_eq!(t.install(0, 'a', false), Ok(0));
        assert_eq!(t.install(0, 'b', false), Ok(1));
        assert_eq!(t.install(5, 'c', true), Ok(5));
        assert_eq!(t.install(0, 'd', false), Ok(2));
        assert_eq!(t.close(1), Ok(Some('b')));
        assert_eq!(t.install(0, 'e', false), Ok(1));
        assert_eq!(t.get(5), Some(&'c'));
        assert_eq!((t.cloexec(5), t.cloexec(0)), (Ok(true), Ok(false)));
        assert_eq!(t.get(3), None);
        assert_eq!(t.get(-1), None);
        assert_eq!(t.get(OPEN_MAX as i32), None);
        assert_eq!(t.fds().collect::<vec::Vec<_>>(), vec![0, 1, 2, 5]);
    }

    #[test]
    fn the_table_is_full_at_open_max() {
        let mut t = Table::new();
        for fd in 0..OPEN_MAX {
            assert_eq!(t.install(0, fd, false), Ok(fd));
        }
        assert_eq!(t.install(0, 9999, false), Err(9999));
        assert_eq!(t.dup(0, 0, false), Err(EMFILE));
        assert_eq!(t.count(), OPEN_MAX as usize);
        assert_eq!(t.close(7), Ok(Some(7)));
        assert_eq!(t.dup(0, 0, false), Ok(7));
    }

    /// ADR-0103 item 5: a dup is a second fd on one entry, which ends with
    /// its last fd, in either order; `FD_CLOEXEC` is per fd.
    #[test]
    fn dup_shares_an_entry_until_the_last_fd() {
        for first in [0, 1] {
            let mut t = Table::new();
            t.install(0, 'f', true).unwrap();
            assert_eq!(t.dup(0, 0, false), Ok(1));
            assert!(t.same(0, 1));
            assert_eq!((t.cloexec(0), t.cloexec(1)), (Ok(true), Ok(false)));
            assert_eq!(t.entries(), 1);
            *t.get_mut(1).unwrap() = 'g';
            assert_eq!(t.get(0), Some(&'g'), "one entry");
            assert_eq!(t.close(first), Ok(None));
            assert_eq!(t.close(1 - first), Ok(Some('g')));
            assert_eq!(t.entries(), 0);
            assert_eq!(t.close(first), Err(EBADF));
        }
    }

    #[test]
    fn dup_with_a_floor_and_bad_arguments() {
        let mut t = Table::new();
        t.install(0, 'f', false).unwrap();
        assert_eq!(t.dup(0, 10, true), Ok(10));
        assert_eq!(t.cloexec(10), Ok(true));
        assert_eq!(t.dup(0, 10, false), Ok(11));
        assert_eq!(t.dup(3, 0, false), Err(EBADF));
        assert_eq!(t.dup(0, -1, false), Err(EINVAL));
        assert_eq!(t.dup(0, OPEN_MAX as i32, false), Err(EINVAL));
        assert_eq!(t.set_cloexec(11, true), Ok(()));
        assert_eq!(t.cloexec(11), Ok(true));
        assert_eq!(t.set_cloexec(12, true), Err(EBADF));
    }

    /// `dup2` closes its target first, gives back the target's entry if
    /// that was its last fd, and is a no-op on equal fds ([P4]).
    #[test]
    fn dup2_replaces_its_target() {
        let mut t = Table::new();
        t.install(0, 'a', false).unwrap();
        t.install(0, 'b', false).unwrap();
        assert_eq!(t.dup2(0, 1, false), Ok(Some('b')));
        assert!(t.same(0, 1));
        assert_eq!(
            t.dup2(0, 1, false),
            Ok(None),
            "already there: nothing closes"
        );
        assert_eq!(t.entries(), 1);
        assert_eq!(t.dup2(1, 1, true), Ok(None));
        assert_eq!(t.cloexec(1), Ok(false), "equal fds change nothing");
        assert_eq!(t.dup2(0, 7, true), Ok(None));
        assert_eq!(t.cloexec(7), Ok(true));
        assert_eq!(t.dup2(4, 5, false), Err(EBADF), "a closed source");
        assert_eq!(t.dup2(4, 4, false), Err(EBADF), "equal but closed");
        assert_eq!(t.dup2(0, -1, false), Err(EBADF));
        assert_eq!(t.dup2(0, OPEN_MAX as i32, false), Err(EBADF));
        // The target held a dup of another entry: that entry lives on.
        t.install(0, 'c', false).unwrap();
        let c = 2;
        assert_eq!(t.dup(c, 0, false), Ok(3));
        assert_eq!(t.dup2(0, c, false), Ok(None));
        assert_eq!(t.get(3), Some(&'c'));
        assert_eq!(t.close(3), Ok(Some('c')));
    }

    #[test]
    fn install_at_fills_only_a_free_slot() {
        let mut t = Table::new();
        assert_eq!(t.install_at(3, 'x'), Ok(()));
        assert_eq!(t.install_at(3, 'y'), Err('y'));
        assert_eq!(t.install_at(OPEN_MAX, 'z'), Err('z'));
        assert_eq!(t.install(0, 'a', false), Ok(0));
        assert_eq!(t.install(3, 'b', false), Ok(4));
        assert_eq!(t.cloexec(3), Ok(false));
    }

    /// A seeded model: random installs, dups, dup2s and closes against a
    /// plain array of (entry id, cloexec), checking which entries come back.
    #[test]
    fn the_table_matches_a_model() {
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut t: Table<u32> = Table::new();
        let mut model: vec::Vec<Option<(u32, bool)>> = vec![None; 40];
        let mut ids = 0u32;
        let lowest =
            |m: &[Option<(u32, bool)>], min: usize| (min..m.len()).find(|&i| m[i].is_none());
        let last = |m: &[Option<(u32, bool)>], id: u32| m.iter().flatten().all(|(e, _)| *e != id);
        for _ in 0..20_000 {
            let fd = next(40) as usize;
            let other = next(40) as usize;
            match next(5) {
                0 => {
                    if let Some(free) = lowest(&model, 0) {
                        ids += 1;
                        assert_eq!(t.install(0, ids, false), Ok(free as u32));
                        model[free] = Some((ids, false));
                    }
                }
                1 => match model[fd] {
                    Some((id, _)) => {
                        if let Some(free) = lowest(&model, other) {
                            let cloexec = next(2) == 0;
                            assert_eq!(t.dup(fd as i32, other as i32, cloexec), Ok(free as u32));
                            model[free] = Some((id, cloexec));
                        }
                    }
                    None => assert_eq!(t.dup(fd as i32, 0, false), Err(EBADF)),
                },
                2 => match model[fd] {
                    Some((id, _)) if fd != other => {
                        let old = model[other].take();
                        let back = t.dup2(fd as i32, other as i32, false).unwrap();
                        model[other] = Some((id, false));
                        let expect = old.map(|(e, _)| e).filter(|&e| last(&model, e));
                        assert_eq!(back, expect);
                    }
                    Some(_) => assert_eq!(t.dup2(fd as i32, other as i32, true), Ok(None)),
                    None => assert_eq!(t.dup2(fd as i32, other as i32, false), Err(EBADF)),
                },
                _ => match model[fd].take() {
                    Some((id, _)) => {
                        let expect = last(&model, id).then_some(id);
                        assert_eq!(t.close(fd as i32), Ok(expect));
                    }
                    None => assert_eq!(t.close(fd as i32), Err(EBADF)),
                },
            }
            for (i, m) in model.iter().enumerate() {
                assert_eq!(t.get(i as i32).copied(), m.map(|(e, _)| e));
                if let Some((_, c)) = m {
                    assert_eq!(t.cloexec(i as i32), Ok(*c));
                }
            }
        }
    }
}
