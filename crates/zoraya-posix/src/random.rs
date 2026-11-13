//! `getrandom` over the random service (ADR-0227 item 4; TASK-27 9a,
//! proposed ADR-0310): one `Bytes` request per 2 KiB on the connection the
//! launcher gave the program, at most 4096 bytes a request (the service's
//! bound). A program with no connection (a `fork` child, a program started
//! by a launcher that gives none) gets `ENOSYS`, as ADR-0227 item 1 says of a
//! machine with no source: stated, never silent, and never a fallback to
//! anything weaker (INV-0310-1).
//!
//! The connection is the program's own, made by the service for the shell
//! (`Share`, depth 2): a service connection may make at most
//! `zoraya_rng::MAX_CHILDREN` (64) others at a time, and a program's own may
//! make none. The 65th live connection a shell holds is refused by the
//! service: `zosh` then starts that program without the connection, and its
//! `getrandom` is `ENOSYS`, so OpenSSL reports that it cannot seed (it never
//! falls back, INV-0310-1). A connection is released when its program ends.

#![forbid(unsafe_code)]

use zoraya_protocol::decode;
use zoraya_rng::{Bytes, BytesReply};

use crate::errno::{EINTR, EINVAL, EIO, ENOSYS};
use crate::process::Posix;
use crate::sys::{Sys, User};

/// `getrandom`'s flags: wait for the pool (never needed: the generator never
/// blocks) and take the `/dev/random` pool (the same generator).
pub const GRND_NONBLOCK: u32 = 1;
pub const GRND_RANDOM: u32 = 2;

/// Bytes asked in one request, so that the reply fits a wire message.
const CHUNK: usize = 2048;

impl<S: Sys> Posix<S> {
    /// Fills `into` from the random service: all of it, or an error and
    /// nothing written; once a chunk has been written, a failure returns the
    /// number of bytes written so far instead (a short count, never an
    /// error beside written bytes). `flags` other than the two above are
    /// `EINVAL`.
    pub fn getrandom(&mut self, into: User, flags: u32) -> Result<usize, i32> {
        if flags & !(GRND_NONBLOCK | GRND_RANDOM) != 0 {
            return Err(EINVAL);
        }
        let Some(conn) = self.random else {
            return Err(ENOSYS);
        };
        let mut done = 0;
        while done < into.length {
            let part = CHUNK.min(into.length - done);
            // A signal to deliver ends the wait with `EINTR`, which the C
            // boundary restarts or reports; the service being gone or
            // sending nonsense is `EIO`, a request that cannot be built
            // `EINVAL`.
            // After a first chunk has been copied out, a failure is not an
            // error: the call returns what it wrote, as Linux's does
            // (`getrandom(2)`: interrupted after some bytes, it returns
            // their number), so that no failure leaves bytes in a buffer
            // that the caller must treat as untouched (INV-0310-1).
            let fail = |e: i32, done: usize| if done > 0 { Ok(done) } else { Err(e) };
            let size = match self.net_call_with(conn, &Bytes { count: part as u32 }, &[], true) {
                Ok(size) => size,
                Err(e) => return fail(if e == EINTR || e == EINVAL { e } else { EIO }, done),
            };
            let bytes = match decode::<BytesReply>(&self.input[..size], 0) {
                Ok((_, r)) if r.bytes.0.len() == part => r.bytes.0.to_vec(),
                _ => return fail(EIO, done),
            };
            self.sys.copy_out(into.part(done, part), &bytes);
            done += part;
        }
        Ok(done)
    }
}
