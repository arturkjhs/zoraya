#![no_std]
#![no_main]

//! One round of everything that makes and ends kernel objects, for
//! INV-0041-1: jobs, a kill, requests to blocked processes, a suspended child
//! that never starts, and a refused `spawn`.

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{Handle, JOB, JobFlags, Mode, RequestKind, Status, process_signals};
use zoraya_user::{
    assert_err, clock_monotonic, handle_close, image, job_create, job_kill, port_create,
    port_subscribe, port_wait, process_signal, spawn, spawn_role, wait_exit,
};

fn spawn_in(job: Handle, role: &[u8], suspended: bool) -> Handle {
    let mut buffer = [0; 256];
    let child = image("child");
    let mut e = Encoder::new(&mut buffer, job, child);
    if suspended {
        e.suspended();
    }
    e.arg(role);
    let length = e.finish().expect("fits");
    let process = spawn(&buffer[..length]).expect("spawn");
    handle_close(child).expect("close");
    process
}

fn pause() {
    let port = port_create().expect("port");
    let mut p = [Default::default()];
    let soon = clock_monotonic() + 1_000_000;
    assert_err!(port_wait(port, soon, &mut p), Status::DeadlineExceeded);
    handle_close(port).expect("close");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    // A job tree, killed with a process of each kind in it.
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let inner = job_create(job, JobFlags::NONE).expect("job");
    let a = spawn_in(job, b"nap", false);
    let b = spawn_in(inner, b"wait", false);
    let c = spawn_in(inner, b"exit130", true);
    pause();
    job_kill(job).expect("kill");
    for p in [a, b, c] {
        wait_exit(p);
        handle_close(p).expect("close");
    }
    handle_close(inner).expect("close");
    handle_close(job).expect("close");

    // A request to a process blocked with a deadline, which wakes it early.
    let p = spawn_role("nap", &[], false).expect("spawn");
    pause();
    process_signal(p, RequestKind::Terminate, 1).expect("signal");
    wait_exit(p);
    handle_close(p).expect("close");

    // A child with a subscription, ended while it is still suspended.
    let p = spawn_role("exit130", &[], true).expect("spawn");
    let port = port_create().expect("port");
    port_subscribe(port, p, process_signals::TERMINATED, 1, Mode::Level).expect("subscribe");
    process_signal(p, RequestKind::Terminate, 1).expect("signal");
    wait_exit(p);
    handle_close(port).expect("close");
    handle_close(p).expect("close");

    // A refused `spawn`, after everything it can get wrong was built.
    assert_err!(spawn(&[0; 16]), Status::InvalidArgument);
    0
}
