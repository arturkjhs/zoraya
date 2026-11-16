//! The handover of a loopback `connect` (proposed ADR-0441), as `netd`'s
//! loop does it, over a [`Kernel`] so that the host tests can count the
//! handles: the client's two stream ends go to the listener or are closed,
//! the connection channel's end is always closed, and `netd` keeps nothing
//! (INV-0441-1).

use zoraya_proto_net::{Accepted, Connected, Failed, LOOPBACK, reason};
use zoraya_protocol::{Below, NO_REPLY, encode};

/// What the handover needs of the kernel.
pub trait Kernel {
    type H: Copy;
    /// Sends `Accepted { 127.0.0.1, peer_port }` with `read` and `write`
    /// moved to the listener's channel `end`. `false`: a full queue or a
    /// listener that went; the handles are still the caller's.
    fn accept(&mut self, end: Self::H, message: &[u8], read: Self::H, write: Self::H) -> bool;
    /// A message on the client's connection channel (no handles).
    fn notify(&mut self, end: Self::H, message: &[u8]);
    fn close(&mut self, handle: Self::H);
}

fn message<'a, M: zoraya_protocol::Message<'a>>(m: &M) -> alloc::vec::Vec<u8> {
    let mut out = alloc::vec![0u8; 64];
    let n = encode(m, NO_REPLY, &mut out).unwrap_or(0);
    out.truncate(n);
    out
}

/// Hands the client's `control` (channel end), `in_write` and `out_read`
/// (the stream ends it gave) to `listener`'s channel end, if there is one,
/// and tells the client; closes whatever is not handed over.
pub fn deliver<K: Kernel>(
    k: &mut K,
    listener: Option<K::H>,
    port: u16,
    peer_port: u16,
    [control, in_write, out_read]: [K::H; 3],
) {
    let accepted = message(&Accepted {
        address: LOOPBACK,
        port: Below(u32::from(peer_port)),
    });
    // The listener reads what the client writes and writes what the client
    // reads: the ends go to it as they came, the client keeps the others.
    let delivered = !accepted.is_empty()
        && listener.is_some_and(|end| k.accept(end, &accepted, out_read, in_write));
    if delivered {
        let connected = message(&Connected {
            address: LOOPBACK,
            port: Below(u32::from(port)),
        });
        k.notify(control, &connected);
    } else {
        k.notify(
            control,
            &message(&Failed {
                reason: Below(reason::REFUSED),
            }),
        );
        k.close(in_write);
        k.close(out_read);
    }
    // The message stays readable after this end closes.
    k.close(control);
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use zoraya_protocol::Header;

    /// A kernel that counts: handles open, messages sent.
    struct Fake {
        open: Vec<u32>,
        accepts: bool,
        sent: Vec<(u32, u32)>,
        accepted: Vec<(u32, u32, u32)>,
    }

    impl Kernel for Fake {
        type H = u32;
        fn accept(&mut self, end: u32, message: &[u8], read: u32, write: u32) -> bool {
            assert_eq!(Header::decode(message).unwrap().0.ordinal, 19);
            if !self.accepts {
                return false;
            }
            self.open.retain(|h| *h != read && *h != write);
            self.accepted.push((end, read, write));
            true
        }
        fn notify(&mut self, end: u32, message: &[u8]) {
            self.sent
                .push((end, Header::decode(message).unwrap().0.ordinal));
        }
        fn close(&mut self, handle: u32) {
            let at = self.open.iter().position(|h| *h == handle);
            assert!(at.is_some(), "handle {handle} closed twice or never open");
            self.open.swap_remove(at.unwrap());
        }
    }

    fn run(accepts: bool, listener: Option<u32>) -> Fake {
        let mut k = Fake {
            open: alloc::vec![1, 2, 3],
            accepts,
            sent: Vec::new(),
            accepted: Vec::new(),
        };
        deliver(&mut k, listener, 8000, 49153, [1, 2, 3]);
        k
    }

    #[test]
    fn a_delivered_connection_moves_the_two_ends_and_closes_the_control() {
        let k = run(true, Some(9));
        assert_eq!(k.accepted, vec![(9, 3, 2)], "read end first, then write");
        assert_eq!(k.sent, vec![(1, 16)], "Connected");
        assert!(
            k.open.is_empty(),
            "nothing of the request stays: {:?}",
            k.open
        );
    }

    #[test]
    fn a_refused_connection_closes_every_handle_and_says_refused() {
        // No listener, and a listener that does not take it (full or gone).
        for k in [run(true, None), run(false, Some(9))] {
            assert!(k.accepted.is_empty());
            assert_eq!(k.sent, vec![(1, 17)], "Failed");
            assert!(k.open.is_empty(), "a handle is held: {:?}", k.open);
        }
    }
}
