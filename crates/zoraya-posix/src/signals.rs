//! Signals in the library (ADR-0106): each signal's POSIX action, the mask
//! and the pending set, which are library state, and what the kernel's
//! disposition must be for each action (item 1). The C boundary runs the
//! handlers; this module decides what happens to a request.

#![forbid(unsafe_code)]

use zoraya_abi::Disposition;
use zoraya_abi::posix::signal::{
    self, SIGCHLD, SIGCONT, SIGKILL, SIGSTOP, SIGTSTP, SIGTTIN, SIGTTOU, SIGURG, SIGWINCH,
};

use crate::errno::EINVAL;

/// `sa_flags` (musl's AArch64 values, Linux's generic ones).
pub mod flags {
    pub const SA_NOCLDSTOP: u32 = 1;
    pub const SA_NOCLDWAIT: u32 = 2;
    pub const SA_SIGINFO: u32 = 4;
    pub const SA_ONSTACK: u32 = 0x0800_0000;
    pub const SA_RESTART: u32 = 0x1000_0000;
    pub const SA_NODEFER: u32 = 0x4000_0000;
    pub const SA_RESETHAND: u32 = 0x8000_0000;
}

/// `SIG_DFL` and `SIG_IGN` as handler values.
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;

/// `sigprocmask`'s `how`.
pub const SIG_BLOCK: i32 = 0;
pub const SIG_UNBLOCK: i32 = 1;
pub const SIG_SETMASK: i32 = 2;

/// The bit of signal `n` in a set (musl's `sigset_t`: bit `n - 1`).
pub const fn bit(n: u32) -> u64 {
    if n >= 1 && n <= 64 { 1 << (n - 1) } else { 0 }
}

/// The signals no mask holds.
const UNBLOCKABLE: u64 = bit(SIGKILL) | bit(SIGSTOP);

/// A set in the bootstrap record's form (bit n for signal n, ADR-0107) from
/// musl's (bit n - 1); the standard signals only.
pub const fn to_record(set: u64) -> u32 {
    ((set & STANDARD) << 1) as u32
}

/// A set in musl's form from the record's.
pub const fn from_record(record: u32) -> u64 {
    (record as u64 >> 1) & STANDARD
}
/// The standard signals, 1 to 31.
const STANDARD: u64 = (1 << 31) - 1;

/// One signal's action (`struct sigaction` without the restorer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Action {
    /// `SIG_DFL`, `SIG_IGN`, or the handler's address.
    pub handler: usize,
    pub mask: u64,
    pub flags: u32,
}

impl Action {
    pub const DEFAULT: Action = Action {
        handler: SIG_DFL,
        mask: 0,
        flags: 0,
    };
    pub const IGNORE: Action = Action {
        handler: SIG_IGN,
        mask: 0,
        flags: 0,
    };

    pub fn is_handler(&self) -> bool {
        self.handler != SIG_DFL && self.handler != SIG_IGN
    }
}

/// What a signal's default action is (POSIX's table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefaultAction {
    Terminate,
    Ignore,
    Stop,
    Continue,
}

pub fn default_action(n: u32) -> DefaultAction {
    match n {
        SIGCHLD | SIGURG | SIGWINCH => DefaultAction::Ignore,
        SIGCONT => DefaultAction::Continue,
        SIGSTOP | SIGTSTP | SIGTTIN | SIGTTOU => DefaultAction::Stop,
        _ => DefaultAction::Terminate,
    }
}

/// What to do with a signal that arrived (a request taken, a `raise`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// Blocked: kept in the pending set until unblocked.
    Pending,
    /// Run this handler.
    Run(Action),
    /// The default action.
    Default(DefaultAction),
    /// Ignored.
    Drop,
}

/// The process's signal state (ADR-0106 items 1 and 2).
#[derive(Debug, Clone)]
pub struct Signals {
    actions: [Action; 32],
    pub blocked: u64,
    pub pending: u64,
    /// The process's own pending set: signals that every thread blocked
    /// when they were taken (ADR-0304 item 3 (c)); `pending` is the
    /// calling thread's.
    pub shared: u64,
}

impl Signals {
    /// The state at start-up: every action the default, but the signals the
    /// record marks as ignored on entry (ADR-0107 item 1), which stay
    /// ignored (bit n of `ignored` for signal n).
    pub fn new(ignored: u32) -> Signals {
        Signals::starting(ignored, 0)
    }

    /// As [`Signals::new`], with the mask on entry (`blocked`, in the
    /// record's form): POSIX keeps it across `execve` (ADR-0190 item 18).
    pub fn starting(ignored: u32, blocked: u32) -> Signals {
        let mut actions = [Action::DEFAULT; 32];
        for (n, a) in actions.iter_mut().enumerate().skip(1) {
            if ignored & (1 << n) != 0 && n as u32 != SIGKILL && n as u32 != SIGSTOP {
                *a = Action::IGNORE;
            }
        }
        Signals {
            actions,
            blocked: from_record(blocked) & !UNBLOCKABLE,
            pending: 0,
            shared: 0,
        }
    }

    pub fn action(&self, n: u32) -> Action {
        self.actions.get(n as usize).copied().unwrap_or_default()
    }

    /// `sigaction`: the old action; `EINVAL` for `SIGKILL`, `SIGSTOP` and
    /// numbers that are not signals (item 1).
    pub fn set(&mut self, n: u32, new: Option<Action>) -> Result<Action, i32> {
        if n == 0 || n > signal::MAX {
            return Err(EINVAL);
        }
        let old = self.actions[n as usize];
        if let Some(mut a) = new {
            if n == SIGKILL || n == SIGSTOP {
                return Err(EINVAL);
            }
            a.mask &= STANDARD & !UNBLOCKABLE;
            self.actions[n as usize] = a;
            if a.handler == SIG_IGN {
                // POSIX: setting SIG_IGN discards a pending one.
                self.pending &= !bit(n);
                self.shared &= !bit(n);
            }
        }
        Ok(old)
    }

    /// The kernel disposition that goes with signal `n`'s action (item 1).
    /// The stop and continue signals are the broker's and `Ignore` in the
    /// kernel whatever POSIX says, as are those whose default ignores.
    pub fn kernel_disposition(&self, n: u32) -> Disposition {
        let a = self.action(n);
        if a.is_handler() {
            Disposition::Deliver
        } else if a.handler == SIG_IGN || !signal::default_terminates(n) {
            Disposition::Ignore
        } else {
            Disposition::Terminate
        }
    }

    /// The signal numbers whose request kind is `Custom`, with what each
    /// must be (the four fixed kinds are the caller's to look up).
    pub fn handlers_installed(&self) -> bool {
        (1..=signal::MAX).any(|n| self.action(n).is_handler())
    }

    /// True if `n` is caught and not blocked: a wait point listens for it.
    pub fn catches(&self, n: u32) -> bool {
        self.action(n).is_handler() && self.blocked & bit(n) == 0
    }

    /// `sigprocmask`: the old mask. SIGKILL and SIGSTOP are never blocked.
    pub fn mask(&mut self, how: i32, set: Option<u64>) -> Result<u64, i32> {
        let old = self.blocked;
        if let Some(set) = set {
            let set = set & !UNBLOCKABLE;
            self.blocked = match how {
                SIG_BLOCK => old | set,
                SIG_UNBLOCK => old & !set,
                SIG_SETMASK => set,
                _ => return Err(EINVAL),
            };
        }
        Ok(old)
    }

    /// Signal `n` arrived (item 4): blocked goes pending, a handler runs,
    /// otherwise the default action or nothing.
    pub fn take(&mut self, n: u32) -> Taken {
        if n == 0 || n > signal::MAX {
            return Taken::Drop;
        }
        let a = self.action(n);
        if a.handler == SIG_IGN {
            return Taken::Drop;
        }
        if self.blocked & bit(n) != 0 {
            self.pending |= bit(n);
            return Taken::Pending;
        }
        if a.is_handler() {
            return Taken::Run(a);
        }
        match default_action(n) {
            DefaultAction::Ignore => Taken::Drop,
            d => Taken::Default(d),
        }
    }

    /// The lowest pending signal that is no longer blocked, taken off the
    /// pending set (standard signals do not queue).
    pub fn next_unblocked(&mut self) -> Option<u32> {
        let ready = (self.pending | self.shared) & !self.blocked;
        if ready == 0 {
            return None;
        }
        let n = ready.trailing_zeros() + 1;
        self.pending &= !bit(n);
        self.shared &= !bit(n);
        Some(n)
    }

    /// Before a handler runs (item 6): the mask it runs with, and the mask
    /// to put back after it. `SA_RESETHAND` puts the default back.
    pub fn enter(&mut self, n: u32, a: &Action) -> u64 {
        let old = self.blocked;
        let mut add = a.mask;
        if a.flags & flags::SA_NODEFER == 0 {
            add |= bit(n);
        }
        self.blocked |= add & !UNBLOCKABLE;
        if a.flags & flags::SA_RESETHAND != 0 && n != SIGCHLD {
            self.actions[n as usize] = Action::DEFAULT;
        }
        old
    }

    pub fn leave(&mut self, old: u64) {
        self.blocked = old;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_abi::posix::signal::{SIGINT, SIGPIPE, SIGQUIT, SIGTERM};

    const HANDLER: Action = Action {
        handler: 0x4000,
        mask: 0,
        flags: 0,
    };

    #[test]
    fn the_start_up_state_keeps_the_signals_ignored_on_entry() {
        let s = Signals::new((1 << SIGINT) | (1 << SIGKILL));
        assert_eq!(s.action(SIGINT), Action::IGNORE);
        assert_eq!(
            s.action(SIGKILL),
            Action::DEFAULT,
            "SIGKILL cannot be ignored"
        );
        assert_eq!(s.action(SIGTERM), Action::DEFAULT);
        assert_eq!(s.kernel_disposition(SIGINT), Disposition::Ignore);
        assert_eq!(s.kernel_disposition(SIGTERM), Disposition::Terminate);
    }

    #[test]
    fn the_mask_on_entry_is_kept() {
        // ADR-0190 item 18: the record's mask (bit n) is musl's (bit n - 1).
        assert_eq!(to_record(bit(SIGINT) | bit(31)), (1 << SIGINT) | (1 << 31));
        assert_eq!(
            from_record((1 << SIGINT) | 1),
            bit(SIGINT),
            "bit 0 is no signal"
        );
        for n in 1..=31 {
            assert_eq!(from_record(to_record(bit(n))), bit(n), "{n}");
        }
        assert_eq!(to_record(bit(40)), 0, "not a standard signal");
        let mut s = Signals::starting(
            1 << SIGQUIT,
            (1 << SIGINT) | (1 << SIGKILL) | (1 << SIGQUIT),
        );
        assert_eq!(
            s.blocked,
            bit(SIGINT) | bit(SIGQUIT),
            "SIGKILL is never blocked"
        );
        assert_eq!(s.action(SIGQUIT), Action::IGNORE);
        assert_eq!(Signals::new(0).blocked, 0);
        // A handler installed after `exec` is held by the inherited mask.
        s.set(SIGINT, Some(HANDLER)).unwrap();
        assert_eq!(s.take(SIGINT), Taken::Pending);
        s.mask(SIG_UNBLOCK, Some(bit(SIGINT))).unwrap();
        assert_eq!(s.next_unblocked(), Some(SIGINT));
    }

    #[test]
    fn kernel_dispositions_follow_item_1s_table() {
        let mut s = Signals::new(0);
        for n in 1..=31 {
            let want = if signal::default_terminates(n) {
                Disposition::Terminate
            } else {
                Disposition::Ignore
            };
            assert_eq!(s.kernel_disposition(n), want, "signal {n}");
        }
        s.set(SIGTSTP, Some(HANDLER)).unwrap();
        assert_eq!(s.kernel_disposition(SIGTSTP), Disposition::Deliver);
        s.set(SIGTERM, Some(Action::IGNORE)).unwrap();
        assert_eq!(s.kernel_disposition(SIGTERM), Disposition::Ignore);
        assert_eq!(s.set(SIGKILL, Some(HANDLER)), Err(EINVAL));
        assert_eq!(s.set(SIGSTOP, Some(Action::IGNORE)), Err(EINVAL));
        assert_eq!(s.set(0, None), Err(EINVAL));
        assert_eq!(s.set(32, None), Err(EINVAL));
        assert_eq!(
            s.set(SIGKILL, None),
            Ok(Action::DEFAULT),
            "reading is allowed"
        );
    }

    #[test]
    fn a_blocked_signal_is_held_and_delivered_on_unblock() {
        // INV-0106-4's library half.
        let mut s = Signals::new(0);
        s.set(SIGCHLD, Some(HANDLER)).unwrap();
        s.set(SIGINT, Some(Action::IGNORE)).unwrap();
        s.mask(SIG_BLOCK, Some(bit(SIGCHLD) | bit(SIGINT) | bit(SIGKILL)))
            .unwrap();
        assert_eq!(s.blocked & bit(SIGKILL), 0, "never blocked");
        assert_eq!(s.take(SIGCHLD), Taken::Pending);
        assert_eq!(
            s.take(SIGCHLD),
            Taken::Pending,
            "standard signals do not queue"
        );
        assert_eq!(s.take(SIGINT), Taken::Drop, "ignored: never pending");
        assert!(!s.catches(SIGCHLD));
        assert_eq!(s.next_unblocked(), None, "still blocked");
        s.mask(SIG_UNBLOCK, Some(bit(SIGCHLD))).unwrap();
        assert_eq!(s.next_unblocked(), Some(SIGCHLD));
        assert_eq!(s.next_unblocked(), None, "once");
        assert_eq!(s.take(SIGCHLD), Taken::Run(HANDLER));
        // Setting SIG_IGN drops a pending one.
        s.mask(SIG_BLOCK, Some(bit(SIGPIPE))).unwrap();
        assert_eq!(s.take(SIGPIPE), Taken::Pending);
        s.set(SIGPIPE, Some(Action::IGNORE)).unwrap();
        s.mask(SIG_SETMASK, Some(0)).unwrap();
        assert_eq!(s.next_unblocked(), None);
    }

    #[test]
    fn default_actions_are_posixs() {
        let mut s = Signals::new(0);
        assert_eq!(s.take(SIGTERM), Taken::Default(DefaultAction::Terminate));
        assert_eq!(s.take(SIGCHLD), Taken::Drop);
        assert_eq!(s.take(SIGWINCH), Taken::Drop);
        assert_eq!(s.take(SIGTSTP), Taken::Default(DefaultAction::Stop));
        assert_eq!(s.take(SIGCONT), Taken::Default(DefaultAction::Continue));
        assert_eq!(s.take(0), Taken::Drop);
        assert_eq!(s.take(40), Taken::Drop);
    }

    #[test]
    fn a_handler_runs_with_its_mask_and_the_signal_blocked() {
        let mut s = Signals::new(0);
        let a = Action {
            handler: 0x4000,
            mask: bit(SIGTERM),
            flags: 0,
        };
        s.set(SIGINT, Some(a)).unwrap();
        let old = s.enter(SIGINT, &a);
        assert_eq!(s.blocked, bit(SIGINT) | bit(SIGTERM));
        assert_eq!(s.take(SIGINT), Taken::Pending, "not nested");
        s.leave(old);
        assert_eq!(s.blocked, 0);
        assert_eq!(s.next_unblocked(), Some(SIGINT));
        let nodefer = Action {
            flags: flags::SA_NODEFER | flags::SA_RESETHAND,
            ..a
        };
        s.set(SIGINT, Some(nodefer)).unwrap();
        s.enter(SIGINT, &nodefer);
        assert_eq!(s.blocked & bit(SIGINT), 0);
        assert_eq!(s.action(SIGINT), Action::DEFAULT, "reset by SA_RESETHAND");
    }

    #[test]
    fn the_mutation_runs_cases() {
        // A number outside 1 to 64 has no bit.
        assert_eq!(bit(0), 0);
        assert_eq!(bit(65), 0);
        // A handler's mask keeps the standard signals but SIGKILL and SIGSTOP.
        let mut s = Signals::new(0);
        let a = Action {
            handler: 0x4000,
            mask: bit(SIGTERM) | bit(SIGKILL),
            flags: 0,
        };
        s.set(SIGINT, Some(a)).unwrap();
        let stored = Action {
            mask: bit(SIGTERM),
            ..a
        };
        assert_eq!(s.take(SIGINT), Taken::Run(stored));
        // The highest standard signal is a signal.
        assert_eq!(s.set(signal::MAX, None), Ok(Action::DEFAULT));
        assert_eq!(
            s.take(signal::MAX),
            Taken::Default(DefaultAction::Terminate)
        );
        // Blocking what is blocked keeps it blocked.
        s.mask(SIG_BLOCK, Some(bit(SIGTERM))).unwrap();
        s.mask(SIG_BLOCK, Some(bit(SIGTERM))).unwrap();
        assert_eq!(s.blocked, bit(SIGTERM));
        // A handler without SA_RESETHAND stays.
        s.enter(SIGINT, &stored);
        assert_eq!(s.action(SIGINT), stored);
    }

    #[test]
    fn handlers_installed_and_catches() {
        let mut s = Signals::new(0);
        assert!(!s.handlers_installed());
        s.set(SIGCHLD, Some(HANDLER)).unwrap();
        assert!(s.handlers_installed());
        assert!(s.catches(SIGCHLD));
        s.mask(SIG_BLOCK, Some(bit(SIGCHLD))).unwrap();
        assert!(!s.catches(SIGCHLD));
        assert_eq!(s.mask(7, Some(0)), Err(EINVAL));
    }
}
