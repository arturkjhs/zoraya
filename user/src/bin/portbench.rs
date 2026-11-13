#![no_std]
#![no_main]

//! INV-0006-3: `port_wait`'s cost does not depend on idle subscriptions.
//! Prints each median, and exits with the ratio in hundredths, or with
//! [`FAILED`] if a call failed (a panic's 101 would pass for a ratio).

use zoraya_abi::{DEADLINE_INFINITE, JOB, Mode, Packet, Status, job_signals};
use zoraya_user::{
    clock_monotonic, handle_close, job_create, port_create, port_queue, port_subscribe, port_wait,
    println, process_exit,
};

const CALLS: usize = 10_000;
/// Subscriptions on the port in the second measurement: the most a port, and
/// a source, holds (`zoraya_events::MAX_SUBSCRIPTIONS`, ADR-0299). INV-0006-3
/// asked for 10,000, which that bound refuses (`LimitReached`).
const IDLE: u64 = 1024;
/// The exit code of a failed call: above any bound the test accepts.
const FAILED: i32 = i32::MAX;

fn must<T>(result: Result<T, Status>, what: &str) -> T {
    result.unwrap_or_else(|e| {
        println!("portbench: {what}: {e:?}");
        process_exit(FAILED)
    })
}

/// Latencies of `CALLS` waits, each returning one packet.
fn median(port: zoraya_abi::Handle, samples: &mut [u32; CALLS]) -> u32 {
    let mut packets = [Packet::default()];
    for sample in samples.iter_mut() {
        must(port_queue(port, 0, 0), "queue");
        let start = clock_monotonic();
        must(port_wait(port, DEADLINE_INFINITE, &mut packets), "wait");
        *sample = (clock_monotonic() - start) as u32;
    }
    samples.sort_unstable();
    samples[CALLS / 2]
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let samples = &mut [0; CALLS];
    let port = must(port_create(), "port");
    // An empty job's `TERMINATED` is asserted, so a Level subscription would
    // be ready: Edge keeps them idle.
    let job = must(job_create(JOB, zoraya_abi::JobFlags::NONE), "job");
    must(
        port_subscribe(port, job, job_signals::TERMINATED, u64::MAX, Mode::Edge),
        "one",
    );
    let one = median(port, samples);
    for key in 0..IDLE - 1 {
        must(
            port_subscribe(port, job, job_signals::TERMINATED, key, Mode::Edge),
            "subscribe",
        );
    }
    let many = median(port, samples);
    must(handle_close(port), "close");
    let ratio = u64::from(many) * 100 / u64::from(one.max(1));
    println!("portbench: 1 idle: {one} ns, {IDLE} idle: {many} ns, ratio {ratio}%");
    ratio.min(i32::MAX as u64 - 1) as i32
}
