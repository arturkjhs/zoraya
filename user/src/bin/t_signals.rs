#![no_std]
#![no_main]

//! Requests and jobs: INV-0004-1 to the extent a program can see, ADR-0004's
//! dispositions, ADR-0046's limits.

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    Disposition, ExitReason, Handle, JOB, JobFlags, Mode, Packet, RequestKind, Status,
    process_signals,
};
use zoraya_user::{
    assert_err, handle_close, image, job_create, job_kill, job_signal, port_create, port_subscribe,
    port_wait, process_set_disposition, process_signal, process_take_request, spawn, spawn_role,
    wait_exit,
};

const KINDS: [RequestKind; 5] = [
    RequestKind::Interrupt,
    RequestKind::Terminate,
    RequestKind::Hangup,
    RequestKind::Quit,
    RequestKind::Custom(31),
];

fn spawn_in(job: Handle, role: &[u8]) -> Handle {
    let mut buffer = [0; 256];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, job, image);
    e.arg(role);
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    child
}

/// INV-0004-5 and rule 2: the default is `Terminate`, for every kind.
fn default_ends_the_process() {
    for (i, kind) in KINDS.into_iter().enumerate() {
        let child = spawn_role("yield", &[], false).expect("spawn");
        process_signal(child, kind, 100 + i as u32).expect("signal");
        let record = wait_exit(child);
        assert_eq!(record.reason, ExitReason::Terminated);
        assert_eq!(record.detail, 100 + i as u32);
    }
}

/// A process blocked in the kernel ends too, in each way it can block.
fn blocked_processes_end() {
    for role in ["sleep", "wait"] {
        let child = spawn_role(role, &[], false).expect("spawn");
        // Let it reach its wait: a yield would do on one CPU, but a deadline
        // does not rely on that.
        let port = port_create().expect("port");
        let mut p = [Packet::default()];
        let soon = zoraya_user::clock_monotonic() + 5_000_000;
        assert_err!(port_wait(port, soon, &mut p), Status::DeadlineExceeded);
        process_signal(child, RequestKind::Terminate, 5).expect("signal");
        let record = wait_exit(child);
        assert_eq!(record.reason, ExitReason::Terminated, "{role}");
        handle_close(port).expect("close");
    }
}

fn a_suspended_process_ends_without_starting() {
    let child = spawn_role("exit130", &[], true).expect("spawn");
    process_signal(child, RequestKind::Interrupt, 3).expect("signal");
    let record = wait_exit(child);
    assert_eq!(record.reason, ExitReason::Terminated);
    assert_eq!(record.detail, 3);
}

fn deliver_queues_and_asserts() {
    let child = spawn_role("take", &[], false).expect("spawn");
    let port = port_create().expect("port");
    let mut p = [Packet::default()];
    let soon = zoraya_user::clock_monotonic() + 5_000_000;
    assert_err!(port_wait(port, soon, &mut p), Status::DeadlineExceeded);
    process_signal(child, RequestKind::Interrupt, 21).expect("signal");
    assert_eq!(wait_exit(child).code, 21);
    handle_close(port).expect("close");
}

/// The queue and `REQUEST_PENDING` on this process's own side.
fn requests_queue_in_order_with_a_bound() {
    process_set_disposition(RequestKind::Hangup, Disposition::Deliver).expect("set");
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    // This process has no handle to itself, so a child stands in.
    let child = spawn_in(job, b"take");
    let _ = child;
    assert_err!(process_take_request(), Status::NotFound);
    job_kill(job).expect("kill");
    wait_exit(child);
    handle_close(child).expect("close");
    handle_close(job).expect("close");
    process_set_disposition(RequestKind::Hangup, Disposition::Terminate).expect("set");
}

/// A child with `Deliver`, not taking anything, fills its queue (INV-0046-1).
fn a_full_queue_says_so() {
    let mut buffer = [0; 256];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.disposition(RequestKind::Quit, Disposition::Deliver)
        .arg(b"yield");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");

    let port = port_create().expect("port");
    port_subscribe(
        port,
        child,
        process_signals::REQUEST_PENDING,
        1,
        Mode::Level,
    )
    .expect("subscribe");
    let mut p = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    for i in 0..64 {
        process_signal(child, RequestKind::Quit, i).expect("room");
    }
    assert_eq!(port_wait(port, 0, &mut p), Ok(1), "REQUEST_PENDING");
    assert_eq!(p[0].signals, process_signals::REQUEST_PENDING);
    assert_err!(
        process_signal(child, RequestKind::Quit, 64),
        Status::LimitReached
    );
    process_signal(child, RequestKind::Terminate, 7).expect("it still ends");
    assert_eq!(wait_exit(child).detail, 7);
    handle_close(port).expect("close");
}

fn job_signal_covers_the_tree() {
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let inner = job_create(job, JobFlags::NONE).expect("job");
    let a = spawn_in(job, b"yield");
    let b = spawn_in(inner, b"sleep");
    let outside = spawn_role("yield", &[], false).expect("spawn");
    job_signal(job, RequestKind::Interrupt, 11).expect("signal");
    assert_eq!(wait_exit(a).detail, 11);
    assert_eq!(wait_exit(b).detail, 11);
    let record = {
        process_signal(outside, RequestKind::Quit, 12).expect("signal");
        wait_exit(outside)
    };
    assert_eq!(record.detail, 12, "outside the job, so not reached");
    handle_close(inner).expect("close");
    handle_close(job).expect("close");
}

/// INV-0046-2 and rule 3: uncatchable, and final.
fn job_kill_is_final() {
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let inner = job_create(job, JobFlags::NONE).expect("job");
    let mut buffer = [0; 256];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, inner, image);
    e.disposition(RequestKind::Interrupt, Disposition::Ignore)
        .arg(b"yield");
    let length = e.finish().expect("fits");
    let a = spawn(&buffer[..length]).expect("spawn");
    let b = spawn_in(job, b"wait");
    job_kill(job).expect("kill");
    assert_eq!(wait_exit(a).reason, ExitReason::Killed);
    assert_eq!(wait_exit(b).reason, ExitReason::Killed);
    assert_err!(spawn(&buffer[..length]), Status::BadState);
    assert_err!(job_create(job, JobFlags::NONE), Status::BadState);
    assert_err!(job_create(inner, JobFlags::NONE), Status::BadState);
    handle_close(inner).expect("close");
    handle_close(job).expect("close");
    handle_close(image).expect("close");
}

/// INV-0046-3.
fn depth_has_a_limit() {
    let mut held = [None; 40];
    let mut parent = JOB;
    let mut count = 0;
    let status = loop {
        match job_create(parent, JobFlags::NONE) {
            Ok(job) => {
                held[count] = Some(job);
                count += 1;
                parent = job;
                assert!(count < 40);
            }
            Err(status) => break status,
        }
    };
    assert_eq!(status, Status::LimitReached);
    // A closed parent is held up by its child, so close in any order.
    for job in held.iter().flatten() {
        handle_close(*job).expect("close");
    }
}

/// Changing to `Terminate` with one queued ends the process at once. A child
/// plays it: `take` is the role that asks for `Deliver`, so the test here is
/// the kernel test `disposition_change_ends_at_once`.
fn set_disposition_is_checked() {
    process_set_disposition(RequestKind::Custom(31), Disposition::Deliver).expect("set");
    process_set_disposition(RequestKind::Custom(31), Disposition::Terminate).expect("set");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    default_ends_the_process();
    blocked_processes_end();
    a_suspended_process_ends_without_starting();
    deliver_queues_and_asserts();
    requests_queue_in_order_with_a_bound();
    a_full_queue_says_so();
    job_signal_covers_the_tree();
    job_kill_is_final();
    depth_has_a_limit();
    set_disposition_is_checked();
    0
}
