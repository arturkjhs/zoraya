//! `errno`: musl's AArch64 numbers, and the one table that maps a native
//! status and a file protocol status to them (ADR-0100 item 9, INV-0100-4).

#![forbid(unsafe_code)]

use zoraya_abi::Status;
use zoraya_proto_file::status as file;

pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const ESRCH: i32 = 3;
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const ENXIO: i32 = 6;
pub const E2BIG: i32 = 7;
pub const ENOEXEC: i32 = 8;
pub const EBADF: i32 = 9;
pub const ECHILD: i32 = 10;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EBUSY: i32 = 16;
pub const EEXIST: i32 = 17;
pub const EXDEV: i32 = 18;
pub const ENODEV: i32 = 19;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ENFILE: i32 = 23;
pub const EMFILE: i32 = 24;
pub const ENOTTY: i32 = 25;
pub const ENOSPC: i32 = 28;
pub const ESPIPE: i32 = 29;
pub const EPIPE: i32 = 32;
pub const ERANGE: i32 = 34;
pub const ENAMETOOLONG: i32 = 36;
pub const ENOSYS: i32 = 38;
pub const ENOTEMPTY: i32 = 39;
pub const ELOOP: i32 = 40;
pub const ETIMEDOUT: i32 = 110;
pub const EMSGSIZE: i32 = 90;
pub const EILSEQ: i32 = 84;
pub const ENOTSUP: i32 = 95;
pub const ECANCELED: i32 = 125;

/// What kind of call met a native status: `LimitReached` is a full handle
/// table for a call that makes a descriptor, a full mapping table for one
/// that maps memory, and a full queue for the rest (ADR-0100 item 9).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `open`, `dup`, `pipe` and the others that make a descriptor.
    Descriptor,
    /// `mmap`, `malloc`.
    Memory,
    Other,
}

/// The table: each native status, with its `errno` for each kind of call.
pub const NATIVE: &[(Status, [i32; 3])] = &[
    (Status::Ok, [0, 0, 0]),
    (Status::BadSyscall, [ENOSYS, ENOSYS, ENOSYS]),
    (Status::BadHandle, [EBADF, EBADF, EBADF]),
    (Status::BadAddress, [EFAULT, EFAULT, EFAULT]),
    (Status::InvalidArgument, [EINVAL, EINVAL, EINVAL]),
    (Status::NoMemory, [ENOMEM, ENOMEM, ENOMEM]),
    (Status::AccessDenied, [EACCES, EACCES, EACCES]),
    (Status::LimitReached, [EMFILE, ENOMEM, EAGAIN]),
    (Status::NotFound, [ENOENT, ENOENT, ENOENT]),
    (Status::BadState, [EBUSY, EBUSY, EBUSY]),
    (Status::DeadlineExceeded, [ETIMEDOUT, ETIMEDOUT, ETIMEDOUT]),
    (Status::Cancelled, [ECANCELED, ECANCELED, ECANCELED]),
    (Status::WouldBlock, [EAGAIN, EAGAIN, EAGAIN]),
    // A stream's reader is gone; a channel's peer is a service that is gone,
    // which ADR-0055 says a client treats as `StorageFailed`: the library
    // turns it into `EIO` where the peer is a service (`GONE`).
    (Status::PeerClosed, [EPIPE, EPIPE, EPIPE]),
    // Only the library's own reads meet it, with buffers of the largest
    // message: a message past that is not one this library asked for.
    (Status::BufferTooSmall, [EMSGSIZE, EMSGSIZE, EMSGSIZE]),
    (Status::NotSupported, [ENOTSUP, ENOTSUP, ENOTSUP]),
    // ADR-0263 item 4 maps by call and field (Q2): until then memory refusals
    // read as a short machine and the rest as a limit to retry under.
    (Status::OverQuota, [ENOMEM, ENOMEM, EAGAIN]),
];

/// `errno` for a native status in a call of kind `call`.
pub fn from_status(status: Status, call: Call) -> i32 {
    let column = match call {
        Call::Descriptor => 0,
        Call::Memory => 1,
        Call::Other => 2,
    };
    NATIVE
        .iter()
        .find(|(s, _)| *s == status)
        .map_or(EIO, |(_, e)| e[column])
}

/// The connection to a service is gone (ADR-0055: `StorageFailed`).
pub const GONE: u32 = 1000;

/// The file protocol's statuses, and `GONE` (ADR-0010's personality mapping,
/// ADR-0013, ADR-0055).
pub const FILE: &[(u32, i32)] = &[
    (file::OK, 0),
    (file::NOT_FOUND, ENOENT),
    (file::EXISTS, EEXIST),
    (file::NO_SPACE, ENOSPC),
    (file::NOT_EMPTY, ENOTEMPTY),
    // ADR-0013 rule 3: a name the service refuses.
    (file::INVALID_NAME, EINVAL),
    (file::NOT_DIRECTORY, ENOTDIR),
    (file::IS_DIRECTORY, EISDIR),
    // The resolver follows links; one left over is a loop or a link where a
    // directory was needed.
    (file::IS_LINK, ELOOP),
    (file::INVALID_ARGUMENT, EINVAL),
    (file::CORRUPT, EIO),
    (file::STORAGE_FAILED, EIO),
    (file::ACCESS_DENIED, EACCES),
    (file::TOO_MANY_OPEN, ENFILE),
    // The service's mappings for buffers are all in use.
    (file::LIMIT_REACHED, ENOMEM),
    (file::UNSUPPORTED, ENOSYS),
    (GONE, EIO),
];

/// `errno` for a file protocol status.
pub fn from_file(status: u32) -> i32 {
    FILE.iter()
        .find(|(s, _)| *s == status)
        .map_or(EIO, |(_, e)| *e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// INV-0100-4: every native status, once in the table, one `errno` for
    /// each kind of call, and an error for every status but `Ok`.
    #[test]
    fn every_status_maps_to_exactly_one_errno_per_call() {
        for status in Status::ALL {
            let rows = NATIVE.iter().filter(|(s, _)| s == status).count();
            assert_eq!(rows, 1, "{status:?}");
            for call in [Call::Descriptor, Call::Memory, Call::Other] {
                let e = from_status(*status, call);
                assert_eq!(e == 0, *status == Status::Ok, "{status:?} {call:?}");
            }
        }
        assert_eq!(
            NATIVE.len(),
            Status::ALL.len(),
            "no row for a status that is not one"
        );
    }

    #[test]
    fn the_listed_mappings_are_adr_0100s() {
        let other = |s| from_status(s, Call::Other);
        assert_eq!(other(Status::NotFound), ENOENT);
        assert_eq!(other(Status::AccessDenied), EACCES);
        assert_eq!(other(Status::WouldBlock), EAGAIN);
        assert_eq!(other(Status::PeerClosed), EPIPE);
        assert_eq!(from_status(Status::LimitReached, Call::Descriptor), EMFILE);
        assert_eq!(from_status(Status::LimitReached, Call::Memory), ENOMEM);
        assert_eq!(other(Status::LimitReached), EAGAIN);
        assert_eq!(other(Status::NoMemory), ENOMEM);
        assert_eq!(other(Status::BadState), EBUSY);
        assert_eq!(other(Status::InvalidArgument), EINVAL);
        assert_eq!(other(Status::BadAddress), EFAULT);
        assert_eq!(other(Status::BadHandle), EBADF);
        assert_eq!(from_file(file::STORAGE_FAILED), EIO);
        assert_eq!(from_file(file::NO_SPACE), ENOSPC);
        assert_eq!(from_file(file::TOO_MANY_OPEN), ENFILE);
        assert_eq!(from_file(GONE), EIO);
    }

    #[test]
    fn every_file_status_maps_once_and_only_ok_to_zero() {
        for s in 0..16 {
            assert_eq!(FILE.iter().filter(|(f, _)| *f == s).count(), 1, "{s}");
            assert_eq!(from_file(s) == 0, s == file::OK, "{s}");
        }
        assert_eq!(from_file(16), EIO, "a status the protocol does not have");
        // The numbers are musl's for AArch64 (arch/generic/bits/errno.h).
        let musl = [
            (ENOENT, 2),
            (EBADF, 9),
            (EAGAIN, 11),
            (EACCES, 13),
            (EEXIST, 17),
            (ENOTDIR, 20),
            (EISDIR, 21),
            (EINVAL, 22),
            (EMFILE, 24),
            (ESPIPE, 29),
            (EPIPE, 32),
            (ENOSYS, 38),
            (ENOTEMPTY, 39),
            (ELOOP, 40),
            (EILSEQ, 84),
            (ENOTSUP, 95),
        ];
        for (ours, theirs) in musl {
            assert_eq!(ours, theirs);
        }
    }
}
