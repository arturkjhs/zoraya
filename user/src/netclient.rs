//! Calls on a network connection (`zoraya-proto-net`): what `init`, the shell
//! and the test programs of N2 need of `netd`. Each call is one request and
//! one reply on the connection, waiting at most `WAIT_NS` for the reply (the
//! connection serves one request at a time, ADR-0055). `connect`, `datagram`
//! and `listen` come with N3.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, Handle, Mode, ObjectType, Packet, Status, channel_signals,
};
use zoraya_proto_net::{GetStatus, Narrow, Narrowed, Ping, PingReply, StatusReply, status};
use zoraya_protocol::{Below, Data, Message, decode, encode};

use zoraya_abi::{CONSOLE, Rights};

use crate::gui::{Running, start_driver};
use crate::stack::{moved, start};
use crate::{
    channel_create, channel_read, channel_write, clock_monotonic, dup, handle_close, port_create,
    port_subscribe, port_wait,
};

/// How long a call waits for its reply: `netd` answers at once, except a
/// `ping`, which takes its run.
pub const WAIT_NS: u64 = 5_000_000_000;

/// Writes `bytes` (with `entries`) to `conn` and reads the one reply, waiting
/// up to `wait_ns`. `None` if the connection is closed or the wait ends.
pub fn exchange(
    conn: Handle,
    bytes: &[u8],
    entries: &[ChannelEntry],
    wait_ns: u64,
) -> Option<Vec<u8>> {
    channel_write(conn, bytes, entries).ok()?;
    let port = port_create().ok()?;
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let _ = port_subscribe(port, conn, wanted, 1, Mode::Level);
    let until = clock_monotonic() + wait_ns;
    let mut buffer = vec![0u8; 4096];
    let reply = loop {
        match channel_read(conn, &mut buffer, &mut []) {
            Ok(sizes) => break Some(buffer[..sizes.bytes as usize].to_vec()),
            Err((Status::WouldBlock, _)) => {
                let mut packets = [Packet::default()];
                if port_wait(port, until, &mut packets).is_err() {
                    break None;
                }
            }
            Err(_) => break None,
        }
    };
    let _ = handle_close(port);
    reply
}

fn request<'a, M: Message<'a>>(m: &M) -> Vec<u8> {
    let mut out = vec![0u8; 1024];
    let n = encode(m, 1, &mut out).expect("a request fits");
    out.truncate(n);
    out
}

/// `status`: the reply bytes (decode with `StatusReply`).
pub fn status(conn: Handle) -> Option<Vec<u8>> {
    exchange(conn, &request(&GetStatus {}), &[], WAIT_NS)
}

/// Decodes the bytes `status` returned.
pub fn parse_status(bytes: &[u8]) -> Option<StatusReply<'_>> {
    decode::<StatusReply<'_>>(bytes, 0).ok().map(|(_, s)| s)
}

/// `narrow`: a new connection inside `conn`'s, or the status that refused.
pub fn narrow(conn: Handle, rights: u32, scope: &[u8]) -> Result<Handle, u32> {
    let (client, server) = channel_create().map_err(|_| status::UNAVAILABLE)?;
    let message = request(&Narrow {
        rights,
        scope: Data(scope),
    });
    let entry = ChannelEntry {
        handle: server,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    let reply = exchange(conn, &message, &[entry], WAIT_NS);
    let code = reply
        .as_deref()
        .and_then(|r| decode::<Narrowed>(r, 0).ok())
        .map(|(_, n)| n.status.0);
    match code {
        Some(status::OK) => Ok(client),
        other => {
            let _ = handle_close(client);
            Err(other.unwrap_or(status::UNAVAILABLE))
        }
    }
}

/// `ping`: `count` echo requests to `address`.
pub fn ping(conn: Handle, address: [u8; 4], count: u32, timeout_ms: u32) -> Option<PingReply> {
    let message = request(&Ping {
        address: u32::from_be_bytes(address),
        count: Below(count),
        timeout_ms: Below(timeout_ms),
    });
    let wait = WAIT_NS + u64::from(count) * 200_000_000 + u64::from(timeout_ms) * 1_000_000;
    let reply = exchange(conn, &message, &[], wait)?;
    decode::<PingReply>(&reply, 0).ok().map(|(_, p)| p)
}

/// The scope of a launcher's root connection: every name and address, every
/// port, both protocols, and every listening port (ADR-0221 item 4).
pub fn root_scope() -> Vec<u8> {
    use zoraya_abi::net::{ListenRule, Ports, Protocols, ReachRule, Target, encode};
    let reach = ReachRule {
        target: Target::Any,
        ports: Ports::ANY,
        protocols: Protocols::BOTH,
    };
    let listen = ListenRule {
        ports: Ports::ANY,
        protocols: Protocols::BOTH,
    };
    let mut out = vec![0u8; 64];
    let n = encode(&[reach], &[listen], &mut out).expect("the root scope encodes");
    out.truncate(n);
    out
}

/// The network (ADR-0221 item 4): the driver, the service, and `init`'s root
/// connection to the service, from which it makes each launcher's.
pub struct Network {
    pub ethd: Running,
    pub netd: Running,
    pub control: Handle,
}

impl Network {
    /// Ends both jobs, the service first (ADR-0054's restart order: the new
    /// driver resets the device and releases the dead one's frames).
    pub fn end(self) {
        self.netd.end();
        self.ethd.end();
        let _ = handle_close(self.control);
    }
}

/// Starts `ethd` on `device`, then `netd` with the driver's channel at slot
/// 5, `/config` at 6 if there is one, and the root connection's service end
/// at 7.
pub fn start_network(device: (Handle, u32), config: Option<Handle>) -> Option<Network> {
    let close_config = |config: Option<Handle>| {
        if let Some(c) = config {
            let _ = handle_close(c);
        }
    };
    let Some((ethd, to_netd)) = start_driver("ethd", device) else {
        close_config(config);
        return None;
    };
    let Ok((control, theirs)) = channel_create() else {
        ethd.end();
        let _ = handle_close(to_netd);
        close_config(config);
        return None;
    };
    let mut entries = alloc::vec![
        dup(CONSOLE.slot(), CONSOLE, Rights::WRITE),
        moved(5, to_netd)
    ];
    if let Some(c) = config {
        entries.push(moved(6, c));
    }
    entries.push(moved(7, theirs));
    match start("netd", &entries, &[]) {
        Some((job, process)) => Some(Network {
            ethd,
            netd: Running { job, process },
            control,
        }),
        None => {
            ethd.end();
            let _ = handle_close(control);
            None
        }
    }
}
