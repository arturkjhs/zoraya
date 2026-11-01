#![no_std]
#![no_main]

//! Many processes starting and ending, with requests and port waits
//! interleaved, under preemption by the children that spin: the kernel test
//! asserts that preemptions happened.

use zoraya_abi::{
    DEADLINE_INFINITE, ExitReason, Handle, Mode, Packet, RequestKind, process_signals,
};
use zoraya_user::{
    clock_monotonic, handle_close, port_create, port_subscribe, port_wait, process_exit_record,
    process_signal, spawn_role,
};

const ROUNDS: u64 = 300;
const WIDTH: usize = 12;

const ROLES: [&str; 6] = ["yield", "exit130", "sleep", "wait", "empty", "spin"];

#[unsafe(no_mangle)]
fn main() -> i32 {
    let port = port_create().expect("port");
    let mut children: [Option<Handle>; WIDTH] = [None; WIDTH];
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut ended = 0;
    let mut key = 1;
    for round in 0..ROUNDS {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let at = (x % WIDTH as u64) as usize;
        match children[at] {
            None => {
                let role = ROLES[(x >> 16) as usize % ROLES.len()];
                let child = spawn_role(role, &[], false).expect("spawn");
                port_subscribe(
                    port,
                    child,
                    process_signals::TERMINATED,
                    at as u64 + key * 100,
                    Mode::Level,
                )
                .expect("subscribe");
                key += 1;
                children[at] = Some(child);
            }
            Some(child) => {
                process_signal(child, RequestKind::Terminate, round as u32).expect("signal")
            }
        }
        let mut packets = [Packet::default(); 8];
        let soon = clock_monotonic() + (x >> 40) % 200_000;
        let count = port_wait(port, soon, &mut packets).unwrap_or(0);
        for packet in &packets[..count] {
            let slot = (packet.key % 100) as usize;
            if let Some(child) = children[slot].take() {
                let record = process_exit_record(child).expect("record");
                assert!(matches!(
                    record.reason,
                    ExitReason::Exited | ExitReason::Terminated
                ));
                handle_close(child).expect("close");
                ended += 1;
            }
        }
    }
    for child in children.into_iter().flatten() {
        process_signal(child, RequestKind::Terminate, 0).expect("signal");
    }
    let mut packets = [Packet::default(); 8];
    for child in children.iter().flatten() {
        let _ = child;
    }
    let mut left = children.iter().flatten().count();
    let mut live = children;
    while left > 0 {
        let count = port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
        for packet in &packets[..count] {
            let slot = (packet.key % 100) as usize;
            if let Some(child) = live[slot].take() {
                handle_close(child).expect("close");
                left -= 1;
                ended += 1;
            }
        }
    }
    assert!(ended > 0);
    0
}
