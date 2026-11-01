#![no_std]
#![no_main]

//! Handles and rights, from a process's side: INV-0040-1 to INV-0040-3 and
//! INV-0006-8, and that every call refuses a handle without its right.

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    CONSOLE, DIRECTORY, Disposition, Handle, JOB, JobFlags, MAX_HANDLES, Mode, ObjectType, Packet,
    PortObject, ProcessObject, RequestKind, Rights, Status, job_signals, process_signals,
};
use zoraya_user::{
    assert_err, console_write, directory_lookup, handle_close, handle_duplicate, image, job_create,
    job_kill, job_signal, port_create, port_queue, port_subscribe, port_unsubscribe, port_wait,
    process_exit_record, process_resume, process_set_disposition, process_signal, spawn,
    spawn_role, wait_exit,
};

fn stale_handles_fail() {
    let copy = handle_duplicate(CONSOLE, Rights::WRITE).expect("duplicate");
    console_write(copy, b"").expect("copy works");
    handle_close(copy).expect("close");
    assert_err!(console_write(copy, b""), Status::BadHandle);
    assert_err!(handle_close(copy), Status::BadHandle);
    let again = handle_duplicate(CONSOLE, Rights::WRITE).expect("duplicate");
    assert_ne!(again, copy);
    assert_err!(handle_close(copy), Status::BadHandle);
    handle_close(again).expect("close");
    assert_err!(
        handle_close(Handle::new(0xffff_ffff).unwrap()),
        Status::BadHandle
    );
}

fn duplication_only_narrows() {
    let narrow = handle_duplicate(CONSOLE, Rights::DUPLICATE).expect("narrow");
    assert_err!(console_write(narrow, b""), Status::AccessDenied);
    assert_err!(
        handle_duplicate(narrow, Rights::WRITE),
        Status::AccessDenied
    );
    let none = handle_duplicate(narrow, Rights::NONE).expect("to nothing");
    assert_err!(handle_duplicate(none, Rights::NONE), Status::AccessDenied);
    handle_close(none).expect("close");
    handle_close(narrow).expect("close");
}

/// A right that the wrong type also has gets as far as the type check.
fn wrong_types_are_refused() {
    let port = port_create().expect("port");
    let process = spawn_role("yield", &[], false).expect("spawn");
    let wrong = Status::BadHandle;
    assert_err!(process_signal(JOB, RequestKind::Hangup, 0), wrong);
    assert_err!(job_signal(process, RequestKind::Hangup, 0), wrong);
    assert_err!(process_exit_record(JOB), wrong);
    assert_err!(port_queue(CONSOLE, 0, 0), wrong);
    assert_err!(console_write(port, b""), wrong);
    let mut packets = [Packet::default()];
    assert_err!(port_wait(JOB, 0, &mut packets), wrong);
    assert_err!(port_unsubscribe(CONSOLE, 0), wrong);
    // Otherwise the right itself is missing.
    assert_err!(process_resume(JOB), Status::AccessDenied);
    assert_err!(job_kill(CONSOLE), Status::AccessDenied);
    assert_err!(job_create(DIRECTORY, JobFlags::NONE), Status::AccessDenied);
    assert_err!(directory_lookup(JOB, "child"), Status::AccessDenied);
    assert_err!(
        port_subscribe(port, CONSOLE, 1, 1, Mode::Level),
        Status::AccessDenied
    );
    process_signal(process, RequestKind::Terminate, 0).expect("end it");
    wait_exit(process);
    handle_close(port).expect("close");
}

/// Every right, by taking only it away. `ProcessObject::RIGHTS` and friends
/// come from `zoraya-abi`.
fn every_call_needs_its_right() {
    let without =
        |h, right: Rights, all: Rights| handle_duplicate(h, all.without(right)).expect("duplicate");
    let denied = Status::AccessDenied;

    let port = port_create().expect("port");
    let process = spawn_role("yield", &[], false).expect("spawn");
    let job_all = zoraya_abi::JobObject::RIGHTS;
    let proc_all = ProcessObject::RIGHTS;

    let j = without(JOB, Rights::CREATE_PROCESS, job_all);
    assert_err!(job_create(j, JobFlags::NONE), denied);
    let j = without(JOB, Rights::SIGNAL, job_all);
    assert_err!(job_signal(j, RequestKind::Hangup, 0), denied);
    let j = without(JOB, Rights::KILL, job_all);
    assert_err!(job_kill(j), denied);
    let j = without(JOB, Rights::WAIT, job_all);
    assert_err!(
        port_subscribe(port, j, job_signals::TERMINATED, 1, Mode::Level),
        denied
    );

    let p = without(process, Rights::SIGNAL, proc_all);
    assert_err!(process_signal(p, RequestKind::Hangup, 0), denied);
    let p = without(process, Rights::RESUME, proc_all);
    assert_err!(process_resume(p), denied);
    let p = without(process, Rights::WAIT, proc_all);
    assert_err!(process_exit_record(p), denied);
    // INV-0006-8.
    assert_err!(
        port_subscribe(port, p, process_signals::TERMINATED, 1, Mode::Level),
        denied
    );

    let c = handle_duplicate(CONSOLE, Rights::DUPLICATE).expect("duplicate");
    assert_err!(console_write(c, b""), denied);
    let d = handle_duplicate(DIRECTORY, Rights::DUPLICATE).expect("duplicate");
    assert_err!(directory_lookup(d, "child"), denied);

    // A port cannot be duplicated, so a narrower one is reached by a move:
    // t_ports does that.
    assert_err!(handle_duplicate(port, PortObject::RIGHTS), denied);
    assert!(!PortObject::RIGHTS.contains(Rights::DUPLICATE));

    process_signal(process, RequestKind::Terminate, 0).expect("end it");
    wait_exit(process);
}

fn ports_refuse_bad_subscriptions() {
    let port = port_create().expect("port");
    let process = spawn_role("yield", &[], false).expect("spawn");
    assert_err!(
        port_subscribe(port, process, 0, 1, Mode::Level),
        Status::InvalidArgument
    );
    assert_err!(
        port_subscribe(port, process, 1 << 9, 1, Mode::Level),
        Status::InvalidArgument
    );
    port_subscribe(port, process, process_signals::TERMINATED, 1, Mode::Level).expect("first");
    assert_err!(
        port_subscribe(port, process, process_signals::TERMINATED, 1, Mode::Level),
        Status::InvalidArgument
    );
    port_unsubscribe(port, 1).expect("known key");
    assert_err!(port_unsubscribe(port, 1), Status::NotFound);
    // A port is not waitable (ADR-0006 rule 1).
    assert_err!(
        port_subscribe(port, port, 1, 2, Mode::Level),
        Status::BadHandle
    );
    assert_err!(port_wait(port, 0, &mut []), Status::InvalidArgument);
    process_signal(process, RequestKind::Terminate, 0).expect("end it");
    wait_exit(process);
    handle_close(port).expect("close");
}

/// INV-0040-3 from user space.
fn a_table_can_be_filled() {
    // Made while there is room: `spawn` at the limit must start nothing, which
    // shows as its job staying empty.
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let port = port_create().expect("port");
    port_subscribe(port, job, job_signals::TERMINATED, 1, Mode::Edge).expect("subscribe");
    let child = image("child");
    let mut buffer = [0; 256];
    let mut e = Encoder::new(&mut buffer, job, child);
    e.suspended().arg(b"hello");
    let length = e.finish().expect("fits");
    let mut held = [None; MAX_HANDLES as usize];
    let mut count = 0;
    let status = loop {
        match handle_duplicate(CONSOLE, Rights::WRITE) {
            Ok(h) => {
                held[count] = Some(h);
                count += 1;
            }
            Err(status) => break status,
        }
    };
    assert_eq!(status, Status::LimitReached);
    assert!(count >= MAX_HANDLES as usize - 32, "{count}");
    assert_err!(port_create(), Status::LimitReached);
    assert_err!(job_create(JOB, JobFlags::NONE), Status::LimitReached);
    assert_err!(image_at_limit(), Status::LimitReached);
    assert_err!(spawn(&buffer[..length]), Status::LimitReached);
    let mut packets = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    let mut signals_port = None;
    core::mem::swap(&mut signals_port, &mut held[0]);
    handle_close(signals_port.expect("a handle")).expect("close");
    port_subscribe(port, job, job_signals::TERMINATED, 2, Mode::Level).expect("subscribe");
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "no child started");
    for h in held.iter().flatten() {
        handle_close(*h).expect("close");
    }
    handle_close(child).expect("close");
    handle_close(port).expect("close");
    handle_close(job).expect("close");
    handle_close(image("child")).expect("usable again");
}

fn image_at_limit() -> Result<Handle, Status> {
    directory_lookup(DIRECTORY, "child")
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    stale_handles_fail();
    duplication_only_narrows();
    wrong_types_are_refused();
    every_call_needs_its_right();
    ports_refuse_bad_subscriptions();
    process_set_disposition(RequestKind::Hangup, Disposition::Ignore).expect("disposition");
    a_table_can_be_filled();
    0
}
