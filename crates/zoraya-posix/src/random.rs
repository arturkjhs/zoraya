//! `getrandom` over the random service (ADR-0227 item 4; TASK-27 9a,
//! proposed ADR-0310): one `Bytes` request per 2 KiB on the connection the
//! launcher gave the program, at most 4096 bytes a request (the service's
//! bound). A program with no connection (a `fork` child, a program started
//! by a launcher that gives none) gets `ENOSYS`, as ADR-0227 item 1 says of a
//! machine with no source: stated, never silent, and never a fallback to
//! anything weaker (INV-0310-1).

#![forbid(unsafe_code)]

use zoraya_protocol::decode;
use zoraya_rng::{Bytes, BytesReply};

use crate::errno::{EINVAL, EIO, ENOSYS};
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
    /// nothing a caller may use. `flags` other than the two above are
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
            let size = self
                .net_call(conn, &Bytes { count: part as u32 }, &[])
                .map_err(|_| EIO)?;
            let bytes = match decode::<BytesReply>(&self.input[..size], 0) {
                Ok((_, r)) if r.bytes.0.len() == part => r.bytes.0.to_vec(),
                _ => return Err(EIO),
            };
            self.sys.copy_out(into.part(done, part), &bytes);
            done += part;
        }
        Ok(done)
    }
}
