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
use zoraya_proto_net::{
    GetStatus, Narrow, NarrowAccount, Narrowed, Ping, PingReply, StatusReply, status,
};
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
    narrow_with(
        conn,
        &Narrow {
            rights,
            scope: Data(scope),
        },
    )
}

/// `narrow_account`: a new connection inside `conn`'s that also carves an
/// account of `budget` connections (ADR-0262 item 2; N4), or the status that
/// refused (`OVER_QUOTA` when the column is full).
pub fn narrow_account(conn: Handle, rights: u32, budget: u32, scope: &[u8]) -> Result<Handle, u32> {
    narrow_with(
        conn,
        &NarrowAccount {
            rights,
            budget: Below(budget),
            scope: Data(scope),
        },
    )
}

fn narrow_with<'a, M: zoraya_protocol::Message<'a>>(conn: Handle, m: &M) -> Result<Handle, u32> {
    let (client, server) = channel_create().map_err(|_| status::UNAVAILABLE)?;
    let message = request(m);
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
    start_network_with(device, config, false)
}

/// As [`start_network`]; with `crash_on_request`, `netd` ends when a
/// connection sends `Crash` (a test of `init`'s restart, proposed ADR-0296).
pub fn start_network_with(
    device: (Handle, u32),
    config: Option<Handle>,
    crash_on_request: bool,
) -> Option<Network> {
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
    // An event at slot 8 says so: a described program reads no argv
    // (INV-0155-2).
    if crash_on_request && let Ok(flag) = crate::event_create() {
        entries.push(zoraya_abi::spawn::HandleEntry {
            slot: 8,
            handle: flag,
            rights: Rights::TRANSFER,
            transfer: Transfer::Move,
        });
    }
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

/// What a launcher learns on its services channel (ADR-0221 item 6).
pub enum Services {
    /// Nothing new.
    Quiet,
    /// The network was restarted: the launcher's new root connection.
    Restarted(Handle),
    /// The restarts are used up.
    Down,
}

/// `init`'s side: tells a launcher that the network was restarted, with its
/// new root connection (moved). Never blocks; false if the launcher is gone.
pub fn send_restarted(services: Handle, root: Handle) -> bool {
    let mut out = [0u8; 64];
    let sent = encode(&zoraya_proto_net::NetworkRestarted {}, 0, &mut out).is_ok_and(|n| {
        let entry = ChannelEntry {
            handle: root,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        };
        channel_write(services, &out[..n], &[entry]).is_ok()
    });
    // A failed write moves nothing: the root is still the caller's, and
    // closed here so that no connection is left in its table (N6 review,
    // finding 9).
    if !sent {
        let _ = handle_close(root);
    }
    sent
}

/// `init`'s side: tells a launcher that no more restarts will come.
pub fn send_down(services: Handle) -> bool {
    let mut out = [0u8; 64];
    let Ok(n) = encode(&zoraya_proto_net::NetworkDown {}, 0, &mut out) else {
        return false;
    };
    channel_write(services, &out[..n], &[]).is_ok()
}

/// A launcher's side: what `init` sent since the last call, the last
/// restart's root connection winning (an earlier one is closed). Reads
/// without waiting; a message of another kind, or a handle with a message
/// that should have none, is closed and ignored.
pub fn poll_services(services: Handle) -> Services {
    let mut result = Services::Quiet;
    let mut bytes = [0u8; 64];
    let mut handles = [services; 2];
    while let Ok(sizes) = channel_read(services, &mut bytes, &mut handles) {
        let count = (sizes.handles as usize).min(handles.len());
        let message = &bytes[..(sizes.bytes as usize).min(bytes.len())];
        let is_restart = count == 1
            && decode::<zoraya_proto_net::NetworkRestarted>(message, 1).is_ok()
            && crate::handle_type(handles[0]) == Ok("channel");
        if is_restart {
            if let Services::Restarted(old) = result {
                let _ = handle_close(old);
            }
            result = Services::Restarted(handles[0]);
        } else {
            for h in &handles[..count] {
                let _ = handle_close(*h);
            }
            if count == 0 && decode::<zoraya_proto_net::NetworkDown>(message, 0).is_ok() {
                if let Services::Restarted(old) = result {
                    let _ = handle_close(old);
                }
                result = Services::Down;
            }
        }
    }
    result
}

/// Whether a root connection's service has ended: its far end is closed.
pub fn is_closed(conn: Handle) -> bool {
    crate::object_get_signals(conn)
        .map(|s| s & channel_signals::PEER_CLOSED != 0)
        .unwrap_or(true)
}

/// Asks the service to end (a test, see `Crash`).
pub fn crash(conn: Handle) -> bool {
    let mut out = [0u8; 64];
    let Ok(n) = encode(&zoraya_proto_net::Crash {}, 1, &mut out) else {
        return false;
    };
    channel_write(conn, &out[..n], &[]).is_ok()
}
