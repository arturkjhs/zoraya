#![no_std]
#![no_main]

//! Blocks on a port the test kernel closes under it (INV-0006-6). Its port is
//! the first handle after the three it started with.

use zoraya_abi::{DEADLINE_INFINITE, Packet, Status};
use zoraya_user::{port_create, port_wait};

#[unsafe(no_mangle)]
fn main() -> i32 {
    let port = port_create().expect("port");
    let mut packets = [Packet::default()];
    match port_wait(port, DEADLINE_INFINITE, &mut packets) {
        Err(Status::Cancelled) => 55,
        _ => 1,
    }
}
