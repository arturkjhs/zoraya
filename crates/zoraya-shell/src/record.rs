//! How an exit record reads (ADR-0151 rules 3, 5 and 6): as the one word of
//! `$status`, and as the line the shell prints when an unchecked failure was
//! not a plain exit. Both are functions of the record alone (INV-0151-2).

use alloc::format;
use alloc::string::String;

use zoraya_abi::{ExitReason, ExitRecord, RequestKind};

/// Success is `Exited` with code 0, and nothing else (INV-0151-3).
pub fn is_success(r: &ExitRecord) -> bool {
    r.reason == ExitReason::Exited && r.code == 0
}

/// A record for a builtin's or function's exit code.
pub fn exited(code: i32) -> ExitRecord {
    ExitRecord {
        code,
        reason: ExitReason::Exited,
        detail: 0,
        request_kind: None,
    }
}

/// `Terminated` by `Interrupt`, as Ctrl-C leaves a command.
pub fn interrupted() -> ExitRecord {
    ExitRecord {
        code: 0,
        reason: ExitReason::Terminated,
        detail: 0,
        request_kind: Some(RequestKind::Interrupt),
    }
}

/// The highest POSIX signal number a `detail` can name (ADR-0046).
const MAX_SIGNAL: u32 = 64;

/// `$status` (rule 5).
pub fn status_word(r: &ExitRecord) -> String {
    match (r.reason, r.request_kind) {
        (ExitReason::Exited, _) => format!("{}", r.code),
        (ExitReason::Terminated, Some(RequestKind::Interrupt)) => "interrupted".into(),
        (ExitReason::Terminated, Some(RequestKind::Terminate)) => "terminated".into(),
        (ExitReason::Terminated, Some(RequestKind::Hangup)) => "hung-up".into(),
        (ExitReason::Terminated, Some(RequestKind::Quit)) => "quit".into(),
        (ExitReason::Terminated, Some(RequestKind::Custom(n))) => format!("request-{n}"),
        (ExitReason::Terminated, None) if (1..=MAX_SIGNAL).contains(&r.detail) => {
            format!("signal-{}", r.detail)
        }
        (ExitReason::Terminated, None) => "terminated".into(),
        (ExitReason::Killed, _) => "killed".into(),
        (ExitReason::Fault, _) => "fault".into(),
        (ExitReason::Limit, _) => "limit".into(),
    }
}

fn fault_name(class: u32) -> &'static str {
    match class {
        0x00 => "unknown instruction",
        0x15 => "system call",
        0x20 | 0x21 => "instruction abort",
        0x22 => "pc alignment",
        0x24 | 0x25 => "data abort",
        0x26 => "stack alignment",
        0x3c => "breakpoint",
        _ => "exception",
    }
}

/// What happened, in words: `interrupted`, `crashed (data abort, class
/// 0x24)`, `exit status 1` (rule 6's table).
pub fn what(r: &ExitRecord) -> String {
    match (r.reason, r.request_kind) {
        (ExitReason::Exited, _) => format!("exit status {}", r.code),
        (ExitReason::Terminated, Some(RequestKind::Interrupt)) => "interrupted".into(),
        (ExitReason::Terminated, Some(RequestKind::Terminate)) => "terminated".into(),
        (ExitReason::Terminated, Some(RequestKind::Hangup)) => "hung up".into(),
        (ExitReason::Terminated, Some(RequestKind::Quit)) => "quit".into(),
        (ExitReason::Terminated, Some(RequestKind::Custom(n))) => format!("ended by request {n}"),
        (ExitReason::Terminated, None) if (1..=MAX_SIGNAL).contains(&r.detail) => {
            format!("ended by signal {}", r.detail)
        }
        (ExitReason::Terminated, None) => "terminated".into(),
        (ExitReason::Killed, _) => "killed".into(),
        (ExitReason::Fault, _) => {
            format!("crashed ({}, class {:#x})", fault_name(r.detail), r.detail)
        }
        (ExitReason::Limit, _) => "stopped at its limit".into(),
    }
}

/// The line printed at the prompt for an unchecked failure of `name`, or
/// `None` for a plain exit, whose program said why itself. `ctrl_c`: the
/// terminal's Ctrl-C sent the interrupt.
pub fn screen_line(name: &str, r: &ExitRecord, ctrl_c: bool) -> Option<String> {
    if r.reason == ExitReason::Exited {
        return None;
    }
    let mut line = format!("{name}: {}", what(r));
    if ctrl_c && r.request_kind == Some(RequestKind::Interrupt) {
        line.push_str(" (Ctrl-C)");
    }
    Some(line)
}

/// `status`: the record in words.
pub fn describe(r: &ExitRecord) -> String {
    match r.reason {
        ExitReason::Exited if r.code == 0 => "succeeded (exit status 0)".into(),
        _ => what(r),
    }
}

/// `status --reason`.
pub fn reason_word(r: &ExitRecord) -> &'static str {
    match r.reason {
        ExitReason::Exited => "exited",
        ExitReason::Terminated => "terminated",
        ExitReason::Killed => "killed",
        ExitReason::Fault => "fault",
        ExitReason::Limit => "limit",
    }
}
