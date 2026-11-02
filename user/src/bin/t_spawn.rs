#![no_std]
#![no_main]

//! `spawn`: INV-0003-1, INV-0003-3, INV-0003-4, INV-0003-5, INV-0003-6,
//! INV-0042-3, INV-0043-1, and hostile buffers.

use zoraya_abi::spawn::{Encoder, HandleEntry, MAX_SPAWN_SIZE, Transfer};
use zoraya_abi::{
    CONSOLE, DIRECTORY, Disposition, ExitReason, ExitRecord, Handle, JOB, JobFlags, Mode,
    RequestKind, Rights, Status, job_signals, process_signals,
};
use zoraya_user::{
    assert_err, console_entry, console_write, dup, handle_close, handle_duplicate, image,
    job_create, job_kill, port_create, port_subscribe, port_wait, process_exit_record,
    process_resume, process_set_disposition, process_signal, spawn, spawn_role, wait_exit,
};

fn exited(code: i32) -> ExitRecord {
    ExitRecord {
        code,
        reason: ExitReason::Exited,
        detail: 0,
        request_kind: None,
    }
}

fn empty_list_gives_an_empty_table() {
    let child = spawn_role("empty", &[], false).expect("spawn");
    assert_eq!(wait_exit(child), exited(0));
}

fn slots_are_what_was_asked() {
    let list = [
        dup(3, JOB, Rights::DUPLICATE),
        dup(4, CONSOLE, Rights::DUPLICATE),
    ];
    let child = spawn_role("slots", &list, false).expect("spawn");
    assert_eq!(wait_exit(child), exited(0));
}

fn args_and_environment_arrive_as_vectors() {
    let mut buffer = [0; 512];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.arg(b"args")
        .arg(b"two words")
        .arg(b"\xff\0binary")
        .env(b"KEY=value");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    assert_eq!(wait_exit(child), exited(0));
    handle_close(image).expect("close");
}

fn a_move_leaves_the_parent_without_the_handle() {
    let moved = handle_duplicate(CONSOLE, Rights::WRITE.union(Rights::TRANSFER)).expect("dup");
    let list = [HandleEntry {
        slot: 1,
        handle: moved,
        rights: Rights::WRITE,
        transfer: Transfer::Move,
    }];
    let child = spawn_role("hello", &list, false).expect("spawn");
    assert_err!(console_write(moved, b""), Status::BadHandle);
    assert_eq!(wait_exit(child), exited(42));
}

fn rights_cannot_grow_on_the_way() {
    let weak = handle_duplicate(CONSOLE, Rights::DUPLICATE).expect("dup");
    assert_err!(
        spawn_role("empty", &[dup(1, weak, Rights::WRITE)], false),
        Status::AccessDenied
    );
    let no_transfer = handle_duplicate(CONSOLE, Rights::WRITE).expect("dup");
    let moved = HandleEntry {
        slot: 1,
        handle: no_transfer,
        rights: Rights::WRITE,
        transfer: Transfer::Move,
    };
    assert_err!(spawn_role("empty", &[moved], false), Status::AccessDenied);
    assert_err!(
        spawn_role("empty", &[dup(1, no_transfer, Rights::NONE)], false),
        Status::AccessDenied
    );
    handle_close(weak).expect("close");
    handle_close(no_transfer).expect("close");
}

/// INV-0042-3: the first entry would move, the last one fails.
fn a_refused_spawn_changes_nothing() {
    let moved = handle_duplicate(CONSOLE, Rights::WRITE.union(Rights::TRANSFER)).expect("dup");
    let list = [
        HandleEntry {
            slot: 1,
            handle: moved,
            rights: Rights::WRITE,
            transfer: Transfer::Move,
        },
        dup(2, Handle::new(900).unwrap(), Rights::NONE),
    ];
    assert_err!(spawn_role("empty", &list, false), Status::BadHandle);
    console_write(moved, b"").expect("still here");
    // The same for a job that was killed, which fails after all handle checks.
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    job_kill(job).expect("kill");
    let mut buffer = [0; 256];
    let child_image = image("child");
    let mut e = Encoder::new(&mut buffer, job, child_image);
    e.handle(HandleEntry {
        slot: 1,
        handle: moved,
        rights: Rights::WRITE,
        transfer: Transfer::Move,
    });
    let length = e.finish().expect("fits");
    assert_err!(spawn(&buffer[..length]), Status::BadState);
    console_write(moved, b"").expect("still here");
    handle_close(moved).expect("close");
    handle_close(job).expect("close");
    handle_close(child_image).expect("close");
}

/// INV-0003-4: the job is not optional, and needs its right.
fn a_process_needs_a_job() {
    let mut buffer = [0; 256];
    let image = image("child");
    let weak = handle_duplicate(JOB, Rights::WAIT).expect("dup");
    let length = Encoder::new(&mut buffer, weak, image)
        .finish()
        .expect("fits");
    assert_err!(spawn(&buffer[..length]), Status::AccessDenied);
    let length = Encoder::new(&mut buffer, CONSOLE, image)
        .finish()
        .expect("fits");
    assert_err!(spawn(&buffer[..length]), Status::AccessDenied);
    // The job field zeroed.
    let length = Encoder::new(&mut buffer, JOB, image)
        .finish()
        .expect("fits");
    buffer[16..24].fill(0);
    assert_err!(spawn(&buffer[..length]), Status::BadHandle);
    handle_close(weak).expect("close");
    handle_close(image).expect("close");
}

fn an_image_needs_its_right() {
    let mut buffer = [0; 256];
    let image = image("child");
    let weak = handle_duplicate(image, Rights::NONE).expect("dup");
    let length = Encoder::new(&mut buffer, JOB, weak).finish().expect("fits");
    assert_err!(spawn(&buffer[..length]), Status::AccessDenied);
    let length = Encoder::new(&mut buffer, JOB, DIRECTORY)
        .finish()
        .expect("fits");
    assert_err!(spawn(&buffer[..length]), Status::AccessDenied);
    handle_close(weak).expect("close");
    handle_close(image).expect("close");
}

/// INV-0003-5, from the parent's side; the kernel test reads the PC.
fn suspended_means_not_started() {
    let child = spawn_role("exit130", &[], true).expect("spawn");
    let port = port_create().expect("port");
    port_subscribe(port, child, process_signals::SUSPENDED, 1, Mode::Level).expect("subscribe");
    let mut packets = [Default::default()];
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1));
    assert_err!(process_exit_record(child), Status::BadState);
    process_resume(child).expect("resume");
    assert_err!(process_resume(child), Status::BadState);
    assert_eq!(wait_exit(child).code, 130);
    assert_err!(process_resume(child), Status::BadState);
    handle_close(port).expect("close");

    // `process_resume` on one killed while suspended.
    let child = spawn_role("exit130", &[], true).expect("spawn");
    process_signal(child, RequestKind::Terminate, 0).expect("signal");
    wait_exit(child);
    assert_err!(process_resume(child), Status::BadState);

    // `SUSPENDED` goes with the resume.
    let child = spawn_role("yield", &[], true).expect("spawn");
    let port = port_create().expect("port");
    port_subscribe(port, child, process_signals::SUSPENDED, 1, Mode::Level).expect("subscribe");
    process_resume(child).expect("resume");
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    process_signal(child, RequestKind::Terminate, 0).expect("signal");
    wait_exit(child);
    handle_close(port).expect("close");
}

/// ADR-0046: the group is a job in the tree of the job field.
fn the_group_is_a_sub_job() {
    let sub = job_create(JOB, JobFlags::NONE).expect("job");
    let other = job_create(JOB, JobFlags::NONE).expect("job");
    let mut buffer = [0; 256];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, other, image);
    e.group(sub);
    let length = e.finish().expect("fits");
    assert_err!(spawn(&buffer[..length]), Status::InvalidArgument);

    // The job itself is a group of the job, and a group needs its right.
    let mut e = Encoder::new(&mut buffer, sub, image);
    e.group(sub).arg(b"exit130");
    let length_self = e.finish().expect("fits");
    let child_self = spawn(&buffer[..length_self]).expect("spawn");
    assert_eq!(wait_exit(child_self).code, 130);
    let weak = handle_duplicate(sub, Rights::WAIT).expect("dup");
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.group(weak).arg(b"exit130");
    let length_weak = e.finish().expect("fits");
    assert_err!(spawn(&buffer[..length_weak]), Status::AccessDenied);
    handle_close(weak).expect("close");

    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.group(sub).arg(b"yield");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    // Killing the group ends a process spawned with it.
    job_kill(sub).expect("kill");
    assert_eq!(wait_exit(child).reason, ExitReason::Killed);
    handle_close(sub).expect("close");
    handle_close(other).expect("close");
    handle_close(image).expect("close");
}

fn job_waits_see_its_tree_empty() {
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    let inner = job_create(job, JobFlags::NONE).expect("job");
    let port = port_create().expect("port");
    port_subscribe(port, job, job_signals::TERMINATED, 1, Mode::Level).expect("subscribe");
    let mut packets = [Default::default()];
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "empty so far");

    let mut buffer = [0; 256];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, inner, image);
    e.suspended().arg(b"hello");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    process_resume(child).expect("resume");
    wait_exit(child);
    assert_eq!(port_wait(port, 0, &mut packets), Ok(1), "empty again");
    handle_close(port).expect("close");
    handle_close(inner).expect("close");
    handle_close(job).expect("close");
    handle_close(image).expect("close");
}

fn hostile_buffers() {
    let bad = Status::InvalidArgument;
    assert_err!(spawn(&[]), bad);
    assert_err!(spawn(&[0; 16]), bad);
    for (address, length, status) in [
        (0x1000u64, MAX_SPAWN_SIZE + 1, bad),
        (0x1000, 64, Status::BadAddress),
        (0xffff_0000_0000_0000, 64, Status::BadAddress),
    ] {
        let slice = core::ptr::slice_from_raw_parts(address as *const u8, length);
        // SAFETY: never read here: the kernel refuses it or its checked copy
        // faults.
        let slice = unsafe { &*slice };
        assert_err!(spawn(slice), status);
    }
    let mut buffer = [0; 256];
    let image = image("child");
    let length = Encoder::new(&mut buffer, JOB, image)
        .finish()
        .expect("fits");
    let mut x = 0x9e37_79b9_7f4a_7c15u64;
    for _ in 0..2000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mut m = buffer;
        m[(x % length as u64) as usize] = (x >> 32) as u8;
        // Whatever it says, it must come back; a child that starts is ended.
        if let Ok(child) = spawn(&m[..length]) {
            process_signal(child, RequestKind::Terminate, 0).expect("end it");
            wait_exit(child);
            handle_close(child).expect("close");
        }
    }
    handle_close(image).expect("close");
}

/// INV-0004-5, from the parent's side: its own ignore does not reach a child
/// spawned with the default block, and the block does.
fn dispositions_come_from_the_block() {
    process_set_disposition(RequestKind::Hangup, Disposition::Ignore).expect("set");
    let child = spawn_role("yield", &[], false).expect("spawn");
    process_signal(child, RequestKind::Hangup, 8).expect("signal");
    let record = wait_exit(child);
    assert_eq!(record.reason, ExitReason::Terminated);
    assert_eq!(
        (record.request_kind, record.detail),
        (Some(RequestKind::Hangup), 8)
    );

    let mut buffer = [0; 256];
    let image = image("child");
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.disposition(RequestKind::Hangup, Disposition::Ignore)
        .arg(b"yield");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    process_signal(child, RequestKind::Hangup, 8).expect("signal");
    process_signal(child, RequestKind::Quit, 9).expect("signal");
    assert_eq!(
        wait_exit(child).request_kind,
        Some(RequestKind::Quit),
        "the hangup was ignored"
    );
    handle_close(image).expect("close");
    process_set_disposition(RequestKind::Hangup, Disposition::Terminate).expect("set");
}

/// The console reaches a child only if listed: `hello` with no console must
/// not print and still ends normally.
fn no_console_no_output() {
    let child = spawn_role("hello", &[], false).expect("spawn");
    assert_eq!(wait_exit(child), exited(42));
    let child = spawn_role("hello", &[console_entry()], false).expect("spawn");
    assert_eq!(wait_exit(child), exited(42));
}

/// INV-0004-2's native half: reason and code are separate.
fn exit_reasons_are_distinct() {
    let child = spawn_role("terminated", &[], false).expect("spawn");
    let record = wait_exit(child);
    assert_eq!(record.reason, ExitReason::Terminated);
    assert_eq!(record.detail, 2);
    assert_eq!(record.request_kind, None);
    let child = spawn_role("exit130", &[], false).expect("spawn");
    assert_eq!(wait_exit(child), exited(130));
    let child = spawn_role("fault", &[], false).expect("spawn");
    assert_eq!(wait_exit(child).reason, ExitReason::Fault);
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    empty_list_gives_an_empty_table();
    slots_are_what_was_asked();
    args_and_environment_arrive_as_vectors();
    a_move_leaves_the_parent_without_the_handle();
    rights_cannot_grow_on_the_way();
    a_refused_spawn_changes_nothing();
    a_process_needs_a_job();
    an_image_needs_its_right();
    suspended_means_not_started();
    the_group_is_a_sub_job();
    job_waits_see_its_tree_empty();
    dispositions_come_from_the_block();
    no_console_no_output();
    exit_reasons_are_distinct();
    hostile_buffers();
    0
}
