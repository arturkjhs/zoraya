//! Exit records as `wait` statuses (ADR-0106 item 11, INV-0106-1).

#![forbid(unsafe_code)]

use zoraya_abi::posix::signal;
use zoraya_abi::{ExitReason, ExitRecord};

/// `WIFEXITED`'s status: the code in bits 8 to 15.
pub const fn exited(code: i32) -> i32 {
    (code & 0xff) << 8
}

/// `WIFSIGNALED`'s status: the signal in bits 0 to 6, never `WCOREDUMP`.
pub const fn signaled(sig: u32) -> i32 {
    (sig & 0x7f) as i32
}

/// `WIFSTOPPED`'s status.
pub const fn stopped(sig: u32) -> i32 {
    ((sig as i32 & 0xff) << 8) | 0x7f
}

/// `WIFCONTINUED`'s status.
pub const CONTINUED: i32 = 0xffff;

/// The signal a fault's exception class stands for (item 11).
pub const fn fault_signal(class: u32) -> u32 {
    match class {
        // Instruction and data aborts.
        0x20 | 0x21 | 0x24 | 0x25 => 11,
        // PC and SP alignment.
        0x22 | 0x26 => 7,
        // Unknown reason, trapped system instruction, illegal state.
        0x00 | 0x18 | 0x0e => 4,
        // Trapped floating point.
        0x2c => 8,
        // BRK.
        0x3c => 5,
        _ => 11,
    }
}

/// The `wait` status of an exit record: `WIFSIGNALED` from the reason only
/// (INV-0004-2), `WTERMSIG` from item 11's tables.
pub fn status(r: &ExitRecord) -> i32 {
    match r.reason {
        ExitReason::Exited => exited(r.code),
        ExitReason::Terminated => {
            let n = match r.request_kind {
                Some(kind) => signal::number(kind, r.detail),
                None if (1..=signal::MAX).contains(&r.detail) => r.detail,
                None => signal::SIGTERM,
            };
            signaled(n)
        }
        ExitReason::Killed => signaled(signal::SIGKILL),
        // ADR-0046's candidate: the nearest Linux analogue is the OOM
        // killer's SIGKILL.
        ExitReason::Limit => signaled(signal::SIGKILL),
        ExitReason::Fault => signaled(fault_signal(r.detail)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zoraya_abi::RequestKind;

    fn record(reason: ExitReason, code: i32, detail: u32, kind: Option<RequestKind>) -> ExitRecord {
        ExitRecord {
            code,
            reason,
            detail,
            request_kind: kind,
        }
    }

    // musl's macros, as C reads the status.
    fn wifexited(s: i32) -> bool {
        s & 0x7f == 0
    }
    fn wexitstatus(s: i32) -> i32 {
        (s & 0xff00) >> 8
    }
    fn wtermsig(s: i32) -> i32 {
        s & 0x7f
    }
    fn wifsignaled(s: i32) -> bool {
        ((s & 0xffff).wrapping_sub(1) as u32) < 0xff
    }
    fn wifstopped(s: i32) -> bool {
        ((((s & 0xffff) as u32).wrapping_mul(0x10001) >> 8) as i16) > 0x7f00
    }

    #[test]
    fn an_exit_is_wifexited_with_its_code() {
        let s = status(&record(ExitReason::Exited, 7, 0, None));
        assert!(wifexited(s) && !wifsignaled(s));
        assert_eq!(wexitstatus(s), 7);
        assert_eq!(
            wexitstatus(status(&record(ExitReason::Exited, 256 + 3, 0, None))),
            3
        );
    }

    #[test]
    fn signaled_comes_from_the_reason_and_the_tables() {
        // INV-0106-1's host half.
        let int = status(&record(
            ExitReason::Terminated,
            0,
            2,
            Some(RequestKind::Interrupt),
        ));
        assert!(wifsignaled(int) && !wifexited(int));
        assert_eq!(wtermsig(int), 2);
        // `process_exit_with_reason(Terminated, n)` with no request.
        let raised = status(&record(ExitReason::Terminated, 0, 6, None));
        assert_eq!(wtermsig(raised), 6);
        // A detail that is not a signal: the kind's own, else SIGTERM.
        let k = status(&record(
            ExitReason::Terminated,
            0,
            99,
            Some(RequestKind::Quit),
        ));
        assert_eq!(wtermsig(k), 3);
        let k = status(&record(
            ExitReason::Terminated,
            0,
            0,
            Some(RequestKind::Custom(13)),
        ));
        assert_eq!(wtermsig(k), 13);
        assert_eq!(
            wtermsig(status(&record(ExitReason::Terminated, 0, 77, None))),
            15
        );
        assert_eq!(wtermsig(status(&record(ExitReason::Killed, 0, 0, None))), 9);
        assert_eq!(wtermsig(status(&record(ExitReason::Limit, 0, 0, None))), 9);
        // A fault on an unmapped address (a data abort) is SIGSEGV; the
        // code of a signalled exit is never read.
        let f = status(&record(ExitReason::Fault, 123, 0x24, None));
        assert!(wifsignaled(f));
        assert_eq!(wtermsig(f), 11);
        for (class, sig) in [
            (0x22, 7),
            (0x26, 7),
            (0x00, 4),
            (0x18, 4),
            (0x0e, 4),
            (0x2c, 8),
            (0x3c, 5),
            (0x21, 11),
            (0x30, 11),
        ] {
            assert_eq!(fault_signal(class), sig, "class {class:#x}");
        }
    }

    #[test]
    fn stopped_and_continued_read_as_c_reads_them() {
        let s = stopped(20);
        assert!(wifstopped(s) && !wifexited(s) && !wifsignaled(s));
        assert_eq!(wexitstatus(s), 20, "WSTOPSIG is the same bits");
        assert!(!wifstopped(CONTINUED) && !wifexited(CONTINUED) && !wifsignaled(CONTINUED));
    }
}
