#![no_std]
#![no_main]

//! INV-0006-3: `port_wait`'s cost does not depend on idle subscriptions.
//! Prints each median, and exits with the ratio in hundredths.

use zoraya_abi::{DEADLINE_INFINITE, JOB, Mode, Packet, job_signals};
use zoraya_user::{
    clock_monotonic, handle_close, job_create, port_create, port_queue, port_subscribe, port_wait,
    println,
};

const CALLS: usize = 10_000;
const IDLE: u64 = 10_000;

/// Latencies of `CALLS` waits, each returning one packet.
fn median(port: zoraya_abi::Handle, samples: &mut [u32; CALLS]) -> u32 {
    let mut packets = [Packet::default()];
    for sample in samples.iter_mut() {
        port_queue(port, 0, 0).expect("queue");
        let start = clock_monotonic();
        port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
        *sample = (clock_monotonic() - start) as u32;
    }
    samples.sort_unstable();
    samples[CALLS / 2]
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let samples = &mut [0; CALLS];
    let port = port_create().expect("port");
    // An empty job's `TERMINATED` is asserted, so a Level subscription would
    // be ready: Edge keeps them idle.
    let job = job_create(JOB, zoraya_abi::JobFlags::NONE).expect("job");
    port_subscribe(port, job, job_signals::TERMINATED, u64::MAX, Mode::Edge).expect("one");
    let one = median(port, samples);
    for key in 0..IDLE - 1 {
        port_subscribe(port, job, job_signals::TERMINATED, key, Mode::Edge).expect("subscribe");
    }
    let many = median(port, samples);
    handle_close(port).expect("close");
    let ratio = u64::from(many) * 100 / u64::from(one.max(1));
    println!("portbench: 1 idle: {one} ns, {IDLE} idle: {many} ns, ratio {ratio}%");
    ratio as i32
}
