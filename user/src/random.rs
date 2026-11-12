//! Calls on a connection to the random service (`zoraya-rng`, ADR-0227): what
//! `init`, the shell and a native program need of `rngd`.

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{ChannelEntry, ChannelObject, Handle, ObjectType};
use zoraya_protocol::{decode, encode};
use zoraya_rng::{Bytes, BytesReply, Share, ShareReply, status};

use crate::netclient::{WAIT_NS, exchange};
use crate::{channel_create, handle_close};

/// The most one call asks for, so that its reply fits `exchange`'s buffer.
const CHUNK: usize = 2048;

/// A new connection to the service made through `conn`, or `None`.
pub fn share(conn: Handle) -> Option<Handle> {
    let (mine, theirs) = channel_create().ok()?;
    let mut out = [0u8; 64];
    let n = encode(&Share {}, 1, &mut out).ok()?;
    let entry = ChannelEntry {
        handle: theirs,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    let reply = exchange(conn, &out[..n], &[entry], WAIT_NS);
    let ok = reply
        .as_deref()
        .and_then(|r| decode::<ShareReply>(r, 0).ok())
        .is_some_and(|(_, r)| r.status.0 == status::OK);
    if ok {
        Some(mine)
    } else {
        // The end given away is closed by the service or with the channel.
        let _ = handle_close(mine);
        None
    }
}

/// Fills `buffer` from the service over `conn`. `false` if the service did
/// not answer; then nothing in `buffer` may be used.
pub fn fill(conn: Handle, buffer: &mut [u8]) -> bool {
    for part in buffer.chunks_mut(CHUNK) {
        let mut out = [0u8; 64];
        let Ok(n) = encode(
            &Bytes {
                count: part.len() as u32,
            },
            1,
            &mut out,
        ) else {
            return false;
        };
        let Some(reply) = exchange(conn, &out[..n], &[], WAIT_NS) else {
            return false;
        };
        match decode::<BytesReply>(&reply, 0) {
            Ok((_, r)) if r.bytes.0.len() == part.len() => part.copy_from_slice(r.bytes.0),
            _ => return false,
        }
    }
    true
}

/// Fills `buffer` from the service this native program was given a connection
/// to (ADR-0227 item 6); `false` if it has none or the service did not
/// answer.
pub fn random(buffer: &mut [u8]) -> bool {
    crate::namespace::get()
        .and_then(|n| n.random())
        .is_some_and(|conn| fill(conn, buffer))
}
