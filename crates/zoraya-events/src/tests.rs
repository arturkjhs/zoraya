use std::vec;
use std::vec::Vec;

use super::*;

const A: u32 = 1;
const B: u32 = 2;

type E = Events<u32>;

fn no_wake() -> impl FnMut(u32) {
    |w| panic!("woke {w}")
}

fn drop_wake() -> impl FnMut(u32) {
    |_| {}
}

fn take(e: &mut E, port: PortId) -> Vec<Packet> {
    let mut out = [Packet::default(); 16];
    let n = e.take(port, &mut out).unwrap();
    out[..n].to_vec()
}

fn keys(e: &mut E, port: PortId) -> Vec<u64> {
    take(e, port).iter().map(|p| p.key).collect()
}

fn setup() -> (E, PortId, SourceId) {
    let mut e = E::new();
    let port = e.new_port().unwrap();
    let source = e.new_source(0).unwrap();
    (e, port, source)
}

/// INV-0006-5.
#[test]
fn level_sees_what_was_asserted_before_it_subscribed() {
    let (mut e, port, source) = setup();
    e.set(source, A, &mut no_wake());
    e.subscribe(port, source, A, 7, Mode::Level, 1).unwrap();
    let packets = take(&mut e, port);
    assert_eq!(packets.len(), 1);
    assert_eq!(packets[0].key, 7);
    assert_eq!(packets[0].kind, PACKET_SIGNAL);
    assert_eq!(packets[0].signals, A);
}

#[test]
fn level_yields_on_each_wait_while_asserted() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 7, Mode::Level, 1).unwrap();
    assert_eq!(keys(&mut e, port), [0u64; 0]);
    e.set(source, A, &mut no_wake());
    assert_eq!(keys(&mut e, port), [7]);
    assert_eq!(keys(&mut e, port), [7]);
    e.clear(source, A);
    assert_eq!(keys(&mut e, port), [0u64; 0]);
    assert_eq!(keys(&mut e, port), [0u64; 0]);
}

#[test]
fn level_appears_once_per_take_however_large_the_buffer() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 7, Mode::Level, 1).unwrap();
    e.set(source, A, &mut no_wake());
    assert_eq!(take(&mut e, port).len(), 1);
}

#[test]
fn a_cleared_then_reasserted_level_is_not_doubled() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 7, Mode::Level, 1).unwrap();
    e.set(source, A, &mut no_wake());
    e.clear(source, A);
    e.set(source, A, &mut no_wake());
    assert_eq!(keys(&mut e, port), [7]);
}

#[test]
fn edge_gives_one_packet_per_assertion() {
    let (mut e, port, source) = setup();
    e.set(source, A, &mut no_wake());
    e.subscribe(port, source, A, 7, Mode::Edge, 1).unwrap();
    assert_eq!(keys(&mut e, port), [0u64; 0], "asserted before subscribing");
    e.clear(source, A);
    e.set(source, A, &mut no_wake());
    assert_eq!(keys(&mut e, port), [7]);
    assert_eq!(
        keys(&mut e, port),
        [0u64; 0],
        "the bit stays up, no new edge"
    );
    e.clear(source, A);
    e.set(source, A, &mut no_wake());
    e.clear(source, A);
    e.set(source, A, &mut no_wake());
    assert_eq!(keys(&mut e, port), [7]);
    assert_eq!(keys(&mut e, port), [7]);
    assert_eq!(keys(&mut e, port), [0u64; 0]);
}

#[test]
fn the_mask_selects_bits() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 7, Mode::Level, 1).unwrap();
    e.set(source, B, &mut no_wake());
    assert_eq!(keys(&mut e, port), [0u64; 0]);
    e.set(source, A, &mut no_wake());
    assert_eq!(take(&mut e, port)[0].signals, A);
}

#[test]
fn a_key_is_unique_on_a_port_and_reusable_after() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 7, Mode::Level, 1).unwrap();
    assert_eq!(
        e.subscribe(port, source, B, 7, Mode::Level, 1),
        Err(Error::KeyInUse)
    );
    e.unsubscribe(port, 7).unwrap();
    assert_eq!(e.unsubscribe(port, 7), Err(Error::NoSuchKey));
    e.subscribe(port, source, B, 7, Mode::Level, 1).unwrap();
}

/// INV-0006-4.
#[test]
fn closing_a_handle_ends_its_subscriptions_only() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 1, Mode::Level, 100).unwrap();
    e.subscribe(port, source, A, 2, Mode::Level, 200).unwrap();
    e.subscribe(port, source, A, 3, Mode::Edge, 100).unwrap();
    e.remove_owner(source, 100);
    e.set(source, A, &mut no_wake());
    assert_eq!(keys(&mut e, port), [2]);
    assert_eq!(e.subscription_count(), 1);
}

#[test]
fn a_queued_item_goes_with_its_subscription() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 1, Mode::Level, 1).unwrap();
    e.set(source, A, &mut no_wake());
    e.unsubscribe(port, 1).unwrap();
    assert_eq!(keys(&mut e, port), [0u64; 0]);
    // The slot is reused by another subscription: no stale item can reach it.
    e.subscribe(port, source, B, 2, Mode::Level, 1).unwrap();
    assert_eq!(keys(&mut e, port), [0u64; 0]);
}

/// INV-0041-4, each side first.
#[test]
fn a_subscription_cannot_outlive_either_side() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 1, Mode::Level, 1).unwrap();
    e.free_source(source);
    assert_eq!(e.subscription_count(), 0);
    assert_eq!(keys(&mut e, port), [0u64; 0]);

    let source = e.new_source(A).unwrap();
    e.subscribe(port, source, A, 1, Mode::Level, 1).unwrap();
    e.free_port(port, &mut no_wake());
    assert_eq!(e.subscription_count(), 0);
    e.set(source, B, &mut no_wake());
    assert_eq!(e.signals(source), A | B);
    assert_eq!(e.live(), (1, 0));
}

#[test]
fn many_subscriptions_come_and_go_in_any_order() {
    let (mut e, port, source) = setup();
    let other = e.new_source(0).unwrap();
    for key in 0..100u64 {
        let target = if key % 3 == 0 { other } else { source };
        e.subscribe(port, target, A, key, Mode::Level, key % 5)
            .unwrap();
    }
    for key in (0..100).step_by(7) {
        e.unsubscribe(port, key).unwrap();
    }
    e.remove_owner(source, 2);
    e.set(source, A, &mut no_wake());
    let expected: Vec<u64> = (0..100)
        .filter(|k| k % 3 != 0 && k % 7 != 0 && k % 5 != 2)
        .collect();
    let mut out = [Packet::default(); 100];
    let n = e.take(port, &mut out).unwrap();
    let mut seen: Vec<u64> = out[..n].iter().map(|p| p.key).collect();
    seen.sort();
    assert_eq!(seen, expected);
}

#[test]
fn slots_are_reused() {
    let (mut e, port, source) = setup();
    for _ in 0..1000 {
        e.subscribe(port, source, A, 1, Mode::Level, 1).unwrap();
        e.unsubscribe(port, 1).unwrap();
    }
    assert!(e.subscriptions.len() < 3);
}

#[test]
fn user_packets_keep_their_order_and_have_a_bound() {
    let (mut e, port, _) = setup();
    for i in 0..MAX_USER_PACKETS as u64 {
        e.queue(port, i, i * 2, &mut no_wake()).unwrap();
    }
    assert_eq!(e.queue(port, 0, 0, &mut no_wake()), Err(Error::Full));
    let packets = take(&mut e, port);
    assert_eq!(packets[3].key, 3);
    assert_eq!(packets[3].data, 6);
    assert_eq!(packets[3].kind, PACKET_USER);
    e.queue(port, 0, 0, &mut no_wake()).unwrap();
}

#[test]
fn a_small_buffer_leaves_the_rest_ready() {
    let (mut e, port, _) = setup();
    for i in 0..3 {
        e.queue(port, i, 0, &mut no_wake()).unwrap();
    }
    let mut one = [Packet::default()];
    assert_eq!(e.take(port, &mut one), Ok(1));
    assert_eq!(one[0].key, 0);
    assert_eq!(e.take(port, &mut []), Ok(0));
    assert_eq!(keys(&mut e, port), [1, 2]);
}

/// INV-0006-6: packets and closing wake a waiter, and nothing else does.
#[test]
fn a_waiter_wakes_for_a_packet_and_for_a_close_only() {
    let (mut e, port, source) = setup();
    e.subscribe(port, source, A, 1, Mode::Level, 1).unwrap();
    e.add_waiter(port, 5).unwrap();
    e.set(source, B, &mut no_wake());
    e.clear(source, A);
    let mut woken = vec![];
    e.set(source, A, &mut |w| woken.push(w));
    assert_eq!(woken, [5]);

    e.add_waiter(port, 6).unwrap();
    let mut woken = vec![];
    e.queue(port, 0, 0, &mut |w| woken.push(w)).unwrap();
    assert_eq!(woken, [6]);

    e.add_waiter(port, 7).unwrap();
    let mut woken = vec![];
    e.close_port(port, &mut |w| woken.push(w));
    assert_eq!(woken, [7]);
    assert!(e.is_closed(port));
}

#[test]
fn a_closed_port_refuses_everything() {
    let (mut e, port, source) = setup();
    e.queue(port, 1, 1, &mut no_wake()).unwrap();
    e.close_port(port, &mut drop_wake());
    let mut out = [Packet::default()];
    assert_eq!(e.take(port, &mut out), Err(Error::Closed));
    assert_eq!(e.queue(port, 0, 0, &mut no_wake()), Err(Error::Closed));
    assert_eq!(e.unsubscribe(port, 0), Err(Error::Closed));
    assert_eq!(
        e.subscribe(port, source, A, 0, Mode::Level, 0),
        Err(Error::Closed)
    );
}

#[test]
fn a_waiter_can_be_taken_back() {
    let (mut e, port, _) = setup();
    e.add_waiter(port, 5).unwrap();
    assert_eq!(e.remove_waiter(port, |w| *w == 5), Some(5));
    assert_eq!(e.remove_waiter(port, |w| *w == 5), None);
    e.queue(port, 0, 0, &mut no_wake()).unwrap();
}
