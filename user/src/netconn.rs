//! `connect` on a network connection (ADR-0222 item 3 and its note of
//! 2026-11-10; N3): the client makes the connection channel and the two
//! streams, so the 128 KiB of the connection are this process's (they appear
//! in its own `job_get_usage`, INV-0262-4), passes `netd` the channel's
//! server end, the write end of the stream it reads and the read end of the
//! stream it writes, and waits for `Connected` or `Failed` on the channel.

use alloc::vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, Handle, Mode, ObjectType, Packet, Status, StreamReadObject,
    StreamWriteObject, channel_signals,
};
use zoraya_proto_net::{Connect, Connected, Connecting, Failed, Reset, status};
use zoraya_protocol::{Below, Data, Header, decode, encode};

use crate::netclient::{WAIT_NS, exchange};
use crate::{
    channel_create, channel_read, clock_monotonic, handle_close, port_create, port_subscribe,
    port_wait, stream_create,
};
use zoraya_proto_net::{CONNECT_ATTEMPT_US, LOOKUP_US};

/// How long `connect` waits for `Connected`: `netd`'s lookup bound, one
/// address's attempt and a margin (proposed ADR-0316). A name with several
/// addresses that all stay silent takes `netd` one attempt each, longer than
/// this wait: the client then says `TimedOut` while `netd` still tries.
pub const CONNECT_WAIT_NS: u64 = (LOOKUP_US + CONNECT_ATTEMPT_US) * 1000 + WAIT_NS;

/// A connected TCP stream as the client holds it: read what the peer sends
/// from `inbound`, write to `outbound`, and look at `control` for `Reset`.
/// Closing `outbound` is `shutdown(write)`. The handles close on drop.
pub struct Connection {
    /// The read end of the stream `netd` writes the peer's bytes to.
    pub inbound: Handle,
    /// The write end of the stream `netd` reads our bytes from.
    pub outbound: Option<Handle>,
    /// The connection channel; `netd` sends `Reset` here.
    pub control: Handle,
    /// The address `netd` reached, and the port.
    pub peer: ([u8; 4], u16),
}

impl Connection {
    /// Ends our side of the stream: `netd` sends a FIN once what is written
    /// has gone out.
    pub fn shutdown_write(&mut self) {
        if let Some(h) = self.outbound.take() {
            let _ = handle_close(h);
        }
    }

    /// Whether `netd` has said the connection was reset. Consumes the
    /// message.
    pub fn was_reset(&self) -> bool {
        let mut buf = [0u8; 64];
        while let Ok(sizes) = channel_read(self.control, &mut buf, &mut []) {
            let n = (sizes.bytes as usize).min(buf.len());
            if Header::decode(&buf[..n]).is_ok_and(|(h, _)| h.ordinal == 18)
                && decode::<Reset>(&buf[..n], 0).is_ok()
            {
                return true;
            }
        }
        false
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        let _ = handle_close(self.inbound);
        self.shutdown_write();
        let _ = handle_close(self.control);
    }
}

/// Why a `connect` did not give a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectError {
    /// `netd` refused the request: one of `zoraya_proto_net::status`.
    Refused(u32),
    /// `netd` did not answer, or the network is not there.
    Unavailable,
    /// The connection did not open: one of `zoraya_proto_net::reason`.
    Failed(u32),
    /// `netd` said nothing within the wait: it is gone or stuck (it answers
    /// a lookup that timed out with `Failed(LOOKUP_TIMED_OUT)`).
    TimedOut,
}

/// Opens a TCP connection through `conn` to `name` (empty to use `address`)
/// and `port`.
pub fn connect(
    conn: Handle,
    name: &str,
    address: [u8; 4],
    port: u16,
    wait_ns: u64,
) -> Result<Connection, ConnectError> {
    let (control, in_read, out_write) = start(conn, name, address, port)?;
    wait(control, in_read, out_write, wait_ns)
}

/// Sends the `Connect` and gives our three ends once `netd` has said
/// `Connecting` (and not yet `Connected` or `Failed`): the connection channel,
/// the stream to read and the stream to write. For a program that opens
/// many at once (the gate's `net-account-flood`, `t_netd`).
pub fn start(
    conn: Handle,
    name: &str,
    address: [u8; 4],
    port: u16,
) -> Result<(Handle, Handle, Handle), ConnectError> {
    let (control, server) = channel_create().map_err(|_| ConnectError::Unavailable)?;
    let streams = stream_create().and_then(|a| stream_create().map(|b| (a, b)));
    let Ok(((in_read, in_write), (out_read, out_write))) = streams else {
        let _ = handle_close(control);
        let _ = handle_close(server);
        return Err(ConnectError::Unavailable);
    };
    let mut message = vec![0u8; 512];
    let n = encode(
        &Connect {
            address: u32::from_be_bytes(address),
            port: Below(u32::from(port)),
            name: Data(name.as_bytes()),
        },
        1,
        &mut message,
    );
    let close_ours = || {
        for h in [control, in_read, out_write] {
            let _ = handle_close(h);
        }
    };
    let Ok(n) = n else {
        close_ours();
        for h in [server, in_write, out_read] {
            let _ = handle_close(h);
        }
        return Err(ConnectError::Refused(status::INVALID));
    };
    message.truncate(n);
    let entries = [
        ChannelEntry {
            handle: server,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        },
        ChannelEntry {
            handle: in_write,
            rights: StreamWriteObject::RIGHTS,
            transfer: Transfer::Move,
        },
        ChannelEntry {
            handle: out_read,
            rights: StreamReadObject::RIGHTS,
            transfer: Transfer::Move,
        },
    ];
    let reply = exchange(conn, &message, &entries, WAIT_NS);
    let code = reply
        .as_deref()
        .and_then(|r| decode::<Connecting>(r, 0).ok())
        .map(|(_, c)| c.status.0);
    match code {
        Some(status::OK) => {}
        Some(other) => {
            close_ours();
            return Err(ConnectError::Refused(other));
        }
        None => {
            close_ours();
            return Err(ConnectError::Unavailable);
        }
    }
    Ok((control, in_read, out_write))
}

/// Waits up to `wait_ns` for `Connected` or `Failed` on `control`, as
/// [`connect`] does; on an error the three ends are closed.
pub fn wait(
    control: Handle,
    in_read: Handle,
    out_write: Handle,
    wait_ns: u64,
) -> Result<Connection, ConnectError> {
    let close_ours = || {
        for h in [control, in_read, out_write] {
            let _ = handle_close(h);
        }
    };
    // `Connected` or `Failed` on the connection channel.
    let Ok(waiter) = port_create() else {
        close_ours();
        return Err(ConnectError::Unavailable);
    };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let _ = port_subscribe(waiter, control, wanted, 1, Mode::Level);
    let until = clock_monotonic() + wait_ns;
    let mut buf = [0u8; 128];
    let result = loop {
        match channel_read(control, &mut buf, &mut []) {
            Ok(sizes) => {
                let m = &buf[..(sizes.bytes as usize).min(buf.len())];
                break match Header::decode(m).map(|(h, _)| h.ordinal) {
                    Ok(16) => decode::<Connected>(m, 0)
                        .map(|(_, c)| (c.address.to_be_bytes(), c.port.0 as u16))
                        .map_err(|_| ConnectError::Unavailable),
                    Ok(17) => Err(decode::<Failed>(m, 0)
                        .map(|(_, f)| ConnectError::Failed(f.reason.0))
                        .unwrap_or(ConnectError::Unavailable)),
                    _ => Err(ConnectError::Unavailable),
                };
            }
            Err((Status::WouldBlock, _)) => {
                let mut packets = [Packet::default()];
                if port_wait(waiter, until, &mut packets).is_err() {
                    break Err(ConnectError::TimedOut);
                }
            }
            Err(_) => break Err(ConnectError::Unavailable),
        }
    };
    let _ = handle_close(waiter);
    match result {
        Ok(peer) => Ok(Connection {
            inbound: in_read,
            outbound: Some(out_write),
            control,
            peer,
        }),
        Err(e) => {
            close_ours();
            Err(e)
        }
    }
}
