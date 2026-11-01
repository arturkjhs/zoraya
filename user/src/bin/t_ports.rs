#![no_std]
#![no_main]

//! Ports: INV-0006-4, INV-0006-5, INV-0006-6 and the rest of ADR-0006 from a
//! process's side.

use zoraya_abi::{
    DEADLINE_INFINITE, Mode, PACKET_SIGNAL, PACKET_USER, Packet, RequestKind, Rights, Status,
    process_signals,
};
use zoraya_user::{
    assert_err, clock_monotonic, handle_close, handle_duplicate, port_create, port_queue,
    port_subscribe, port_unsubscribe, port_wait, process_signal, sleep_until, spawn_role,
    wait_exit,
};

use process_signals::{REQUEST_PENDING, SUSPENDED, TERMINATED};

fn user_packets_round_trip() {
    let port = port_create().expect("port");
    port_queue(port, 5, 6).expect("queue");
    port_queue(port, 7, 8).expect("queue");
    let mut p = [Packet::default(); 4];
    assert_eq!(port_wait(port, DEADLINE_INFINITE, &mut p), Ok(2));
    assert_eq!((p[0].key, p[0].kind, p[0].data), (5, PACKET_USER, 6));
    assert_eq!((p[1].key, p[1].data), (7, 8));
    handle_close(port).expect("close");
}

fn a_deadline_ends_a_wait() {
    let port = port_create().expect("port");
    let mut p = [Packet::default()];
    let start = clock_monotonic();
    let at = start + 3_000_000;
    assert_err!(port_wait(port, at, &mut p), Status::DeadlineExceeded);
    assert!(clock_monotonic() >= at);
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    handle_close(port).expect("close");
}

/// INV-0006-5, a child ended before the subscription.
fn level_catches_what_came_first() {
    let child = spawn_role("exit130", &[], false).expect("spawn");
    wait_exit(child);
    let port = port_create().expect("port");
    port_subscribe(port, child, TERMINATED, 9, Mode::Level).expect("subscribe");
    let mut p = [Packet::default()];
    assert_eq!(port_wait(port, 0, &mut p), Ok(1));
    assert_eq!(
        (p[0].key, p[0].kind, p[0].signals),
        (9, PACKET_SIGNAL, TERMINATED)
    );
    // And again, while asserted.
    assert_eq!(port_wait(port, 0, &mut p), Ok(1));
    port_unsubscribe(port, 9).expect("unsubscribe");
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    handle_close(port).expect("close");
}

fn edge_counts_assertions() {
    let child = spawn_role("exit130", &[], true).expect("spawn");
    let port = port_create().expect("port");
    port_subscribe(port, child, SUSPENDED | TERMINATED, 1, Mode::Edge).expect("subscribe");
    let mut p = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    zoraya_user::process_resume(child).expect("resume");
    assert_eq!(port_wait(port, DEADLINE_INFINITE, &mut p), Ok(1));
    assert_eq!(p[0].signals, TERMINATED);
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    handle_close(port).expect("close");
}

/// INV-0006-4: subscribe, close, assert through another handle, and hear
/// nothing.
fn closing_a_handle_ends_its_subscriptions() {
    let mut buffer = [0; 256];
    let image = zoraya_user::image("child");
    let mut e = zoraya_abi::spawn::Encoder::new(&mut buffer, zoraya_abi::JOB, image);
    e.disposition(RequestKind::Quit, zoraya_abi::Disposition::Deliver)
        .arg(b"yield");
    let length = e.finish().expect("fits");
    let child = zoraya_user::spawn(&buffer[..length]).expect("spawn");
    zoraya_user::handle_close(image).expect("close");

    let other = handle_duplicate(
        child,
        <zoraya_abi::ProcessObject as zoraya_abi::ObjectType>::RIGHTS,
    )
    .expect("duplicate");
    let port = port_create().expect("port");
    port_subscribe(port, child, REQUEST_PENDING, 1, Mode::Level).expect("subscribe");
    port_subscribe(port, other, TERMINATED, 2, Mode::Level).expect("subscribe");
    handle_close(child).expect("close");
    process_signal(other, RequestKind::Quit, 1).expect("assert it");
    let mut p = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    // The other handle's subscription is still there.
    process_signal(other, RequestKind::Terminate, 2).expect("end it");
    assert_eq!(port_wait(port, DEADLINE_INFINITE, &mut p), Ok(1));
    assert_eq!(p[0].key, 2);
    handle_close(port).expect("close");
    handle_close(other).expect("close");
}

fn a_bad_buffer_loses_no_packet() {
    let port = port_create().expect("port");
    port_queue(port, 5, 6).expect("queue");
    for address in [0x1000u64, 0xffff_0000_0000_0000] {
        let bad = core::ptr::slice_from_raw_parts_mut(address as *mut Packet, 1);
        // SAFETY: never touched here: the kernel's checked copy faults.
        let bad = unsafe { &mut *bad };
        assert_err!(port_wait(port, 0, bad), Status::BadAddress);
    }
    let mut p = [Packet::default()];
    assert_eq!(port_wait(port, 0, &mut p), Ok(1));
    assert_eq!(p[0].key, 5);
    handle_close(port).expect("close");
}

fn a_bounded_port_says_so() {
    let port = port_create().expect("port");
    for i in 0..1024 {
        port_queue(port, i, i).expect("room");
    }
    assert_err!(port_queue(port, 0, 0), Status::LimitReached);
    handle_close(port).expect("close");
}

/// A port can go to a child, and no handle to it stays behind (ADR-0003 rule
/// 3: it cannot be duplicated).
fn a_port_moves_but_is_not_copied() {
    let port = port_create().expect("port");
    assert_err!(
        spawn_role("empty", &[zoraya_user::dup(3, port, Rights::WAIT)], false),
        Status::AccessDenied
    );
    let moved = zoraya_abi::spawn::HandleEntry {
        slot: 3,
        handle: port,
        rights: Rights::WAIT,
        transfer: zoraya_abi::spawn::Transfer::Move,
    };
    let child = spawn_role("exit130", &[moved], false).expect("spawn");
    let mut p = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut p), Status::BadHandle);
    assert_eq!(wait_exit(child).code, 130);
}

/// INV-0040-2 for ports: each right, in a child that holds only it.
fn port_rights_are_checked() {
    use zoraya_abi::spawn::{HandleEntry, Transfer};
    for (role, right) in [
        ("port-wait-only", Rights::WAIT),
        ("port-write-only", Rights::WRITE),
    ] {
        let port = port_create().expect("port");
        let list = [
            HandleEntry {
                slot: 3,
                handle: port,
                rights: right,
                transfer: Transfer::Move,
            },
            zoraya_user::dup(4, zoraya_abi::JOB, Rights::WAIT),
        ];
        let child = spawn_role(role, &list, false).expect("spawn");
        assert_eq!(wait_exit(child).code, 0, "{role}");
    }
}

/// INV-0006-4 for a move: the subscription belonged to the handle that left.
fn moving_a_handle_ends_its_subscriptions() {
    use zoraya_abi::spawn::{HandleEntry, Transfer};
    let target = spawn_role("exit130", &[], true).expect("spawn");
    let mine = handle_duplicate(
        target,
        <zoraya_abi::ProcessObject as zoraya_abi::ObjectType>::RIGHTS,
    )
    .expect("duplicate");
    let port = port_create().expect("port");
    port_subscribe(port, mine, TERMINATED, 1, Mode::Level).expect("subscribe");
    let list = [HandleEntry {
        slot: 3,
        handle: mine,
        rights: Rights::NONE,
        transfer: Transfer::Move,
    }];
    let holder = spawn_role("exit130", &list, false).expect("spawn");
    wait_exit(holder);
    zoraya_user::process_resume(target).expect("resume");
    wait_exit(target);
    let mut p = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut p), Status::DeadlineExceeded);
    handle_close(port).expect("close");
}

/// Nothing but a packet, a deadline or a close ends a wait: a child's requests
/// and exit do not, unless subscribed (INV-0006-6).
fn unrelated_events_do_not_wake() {
    let port = port_create().expect("port");
    let child = spawn_role("exit130", &[], false).expect("spawn");
    let at = clock_monotonic() + 2_000_000;
    let mut p = [Packet::default()];
    assert_err!(port_wait(port, at, &mut p), Status::DeadlineExceeded);
    wait_exit(child);
    handle_close(port).expect("close");
    sleep_until(clock_monotonic());
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    user_packets_round_trip();
    a_deadline_ends_a_wait();
    level_catches_what_came_first();
    edge_counts_assertions();
    closing_a_handle_ends_its_subscriptions();
    a_bad_buffer_loses_no_packet();
    a_bounded_port_says_so();
    a_port_moves_but_is_not_copied();
    unrelated_events_do_not_wake();
    port_rights_are_checked();
    moving_a_handle_ends_its_subscriptions();
    0
}
