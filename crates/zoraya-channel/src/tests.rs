use std::vec;
use std::vec::Vec;

use super::*;

fn message(bytes: usize, handles: Vec<u32>) -> Box<Message<u32>> {
    Box::new(Message::new(vec![0; bytes], handles, ()))
}

fn send(c: &mut Channel<u32>, end: usize, bytes: usize) -> Result<(), Refused> {
    c.prepare(end, bytes)?;
    c.write(end, message(bytes, Vec::new()));
    Ok(())
}

#[test]
fn messages_arrive_whole_and_in_order() {
    let mut c = Channel::new();
    for i in 0..5 {
        c.prepare(0, 1).unwrap();
        c.write(0, Box::new(Message::new(vec![i], vec![u32::from(i)], ())));
    }
    for i in 0..5 {
        let m = c.read(1).unwrap();
        assert_eq!(m.bytes, [i]);
        assert_eq!(m.handles, [u32::from(i)]);
    }
    assert!(c.read(1).is_none());
    assert!(c.read(0).is_none(), "each direction apart");
}

/// INV-0050-3.
#[test]
fn a_queue_stops_at_64_messages() {
    let mut c = Channel::new();
    for _ in 0..MAX_QUEUED {
        send(&mut c, 0, 1).unwrap();
    }
    assert_eq!(send(&mut c, 0, 1), Err(Refused::Full));
    c.read(1);
    send(&mut c, 0, 1).unwrap();
}

/// INV-0050-3.
#[test]
fn a_queue_stops_at_256_kib() {
    let mut c = Channel::new();
    for _ in 0..4 {
        send(&mut c, 0, MAX_MESSAGE).unwrap();
    }
    assert_eq!(send(&mut c, 0, 1), Err(Refused::Full));
    assert_eq!(c.signals(0) & WRITABLE, 0);
    c.read(1);
    assert_ne!(c.signals(0) & WRITABLE, 0);
}

#[test]
fn writable_needs_room_for_any_message() {
    let mut c = Channel::new();
    assert_eq!(c.signals(0), WRITABLE);
    for _ in 0..3 {
        send(&mut c, 0, MAX_MESSAGE).unwrap();
    }
    send(&mut c, 0, 1).unwrap();
    assert_eq!(
        c.signals(0) & WRITABLE,
        0,
        "1 byte too many for a full message"
    );
    send(&mut c, 0, 1).unwrap();
    assert_eq!(c.signals(1), READABLE | WRITABLE);
}

#[test]
fn closing_shows_at_the_peer_and_keeps_what_it_sent() {
    let mut c = Channel::new();
    send(&mut c, 0, 3).unwrap();
    send(&mut c, 1, 4).unwrap();
    let pile = c.close(0);
    assert_eq!(c.signals(1), READABLE | PEER_CLOSED);
    assert_eq!(send(&mut c, 1, 1), Err(Refused::PeerClosed));
    assert_eq!(c.peek(1), Some((3, 0)));
    c.read(1);
    assert_eq!(c.signals(1), PEER_CLOSED);
    drop(pile);
}

/// INV-0050-4, the data half: closing gives every unread message.
#[test]
fn a_pile_gives_every_message_and_its_handles() {
    let mut c = Channel::new();
    for i in 0..3 {
        c.prepare(0, 0).unwrap();
        c.write(0, message(0, vec![i]));
    }
    assert!(c.holds_handles(1));
    let mut pile = c.close(1);
    assert!(!c.holds_handles(1));
    let mut seen = Vec::new();
    while let Some(mut m) = pile.pop() {
        seen.append(&mut m.handles);
    }
    seen.sort_unstable();
    assert_eq!(seen, [0, 1, 2]);
}

/// INV-0050-6, the data half.
#[test]
fn a_long_pile_drops_with_no_recursion() {
    let mut pile = Pile::new();
    for _ in 0..1_000_000 {
        pile.push(message(0, Vec::new()));
    }
    drop(pile);
}

/// From the end whose queue holds a handle, to the end that is queued.
type Edge = ((usize, usize), (usize, usize));

/// INV-0050-2: random sends under the rule keep the graph of ends in queues
/// acyclic. An edge F -> E means E is queued for reading on F.
#[test]
fn random_sends_under_the_rule_make_no_cycle() {
    const CHANNELS: usize = 12;
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    let mut queued = [[0u32; 2]; CHANNELS];
    let mut edges: Vec<Edge> = Vec::new();
    let mut held = [[true; 2]; CHANNELS];
    for _ in 0..200_000 {
        let (a, side) = ((next() as usize) % CHANNELS, (next() as usize) % 2);
        let (b, other) = ((next() as usize) % CHANNELS, (next() as usize) % 2);
        if !held[a][side] || !held[b][other] {
            continue;
        }
        if may_travel(a == b, queued[b][other] != 0) {
            held[b][other] = false;
            queued[a][1 - side] += 1;
            edges.push(((a, 1 - side), (b, other)));
            assert_acyclic(&edges);
        }
        // Reads bring ends back, and thin the graph.
        if next().is_multiple_of(2)
            && let Some(i) = edges.iter().position(|e| held[e.0.0][e.0.1])
        {
            let (from, to) = edges.swap_remove(i);
            queued[from.0][from.1] -= 1;
            held[to.0][to.1] = true;
        }
    }
}

fn assert_acyclic(edges: &[Edge]) {
    for start in edges {
        let mut at = start.1;
        let mut steps = 0;
        while let Some(edge) = edges.iter().find(|e| e.0 == at) {
            assert_ne!(edge.0, start.0, "a cycle through {start:?}");
            at = edge.1;
            steps += 1;
            assert!(steps <= edges.len());
        }
    }
}

#[test]
fn the_rule_refuses_the_same_channel_and_a_full_end() {
    assert!(may_travel(false, false));
    assert!(!may_travel(true, false));
    assert!(!may_travel(false, true));
}
