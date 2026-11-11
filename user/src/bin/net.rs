#![no_std]
#![no_main]

//! `net` and `net status` (ADR-0229 item 4): the state the network service
//! holds, in words. The shell gives it a connection with the `status` right
//! and an empty scope (its description says `needs net-status`); it reads
//! no file and holds nothing else of the network.

use zoraya_abi::{DEADLINE_INFINITE, Packet, Status};
use zoraya_proto_net::{GetStatus, StatusReply, render, status};
use zoraya_protocol::{decode, encode};
use zoraya_user::{
    channel_read, channel_write, command, namespace, port_create, port_subscribe, port_wait,
};

zoraya_user::describe!("commands", "net");

/// How long the service may take to answer.
const WAIT_NS: u64 = 5_000_000_000;

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    if let Some(word) = p.operand(&c.d, "WORD")
        && word != "status"
    {
        c.error(Some(word), "net shows the state: net status");
        return 2;
    }
    let Some(end) = namespace::get().and_then(|n| n.net()) else {
        c.error(None, "the network is not available");
        return 1;
    };
    let mut out = [0u8; 64];
    let n = encode(&GetStatus {}, 1, &mut out).expect("fits");
    if channel_write(end, &out[..n], &[]).is_err() {
        c.error(None, "the network is not available");
        return 1;
    }
    let port = port_create().expect("port");
    let wanted = zoraya_abi::channel_signals::READABLE | zoraya_abi::channel_signals::PEER_CLOSED;
    let _ = port_subscribe(port, end, wanted, 1, zoraya_abi::Mode::Level);
    let mut bytes = [0u8; 1024];
    let deadline = zoraya_user::clock_monotonic() + WAIT_NS;
    loop {
        match channel_read(end, &mut bytes, &mut []) {
            Ok(sizes) => {
                let message = &bytes[..sizes.bytes as usize];
                let Ok((_, reply)) = decode::<StatusReply<'_>>(message, 0) else {
                    c.error(None, "the network service's answer is not understood");
                    return 1;
                };
                if reply.status.0 != status::OK {
                    c.error(None, "the network is not granted to this command");
                    return 1;
                }
                zoraya_user::print!("{}", render(&reply));
                return 0;
            }
            Err((Status::WouldBlock, _)) => {
                let mut packets = [Packet::default()];
                if port_wait(port, deadline.min(DEADLINE_INFINITE - 1), &mut packets).is_err() {
                    c.error(None, "the network service did not answer");
                    return 1;
                }
            }
            Err(_) => {
                c.error(None, "the network is not available");
                return 1;
            }
        }
    }
}
