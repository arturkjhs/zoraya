use std::collections::VecDeque;
use std::vec;
use std::vec::Vec;

use super::*;

/// The storage tests use: one vector, cut into chunks as the kernel's frames
/// would be. It checks that no chunk crosses a page.
pub struct Flat(Vec<u8>);

impl Flat {
    pub fn new() -> Flat {
        Flat(vec![0; CAPACITY])
    }
}

impl Storage for Flat {
    fn chunk(&self, offset: usize, length: usize) -> &[u8] {
        assert_eq!(offset / PAGE, (offset + length - 1) / PAGE, "one page");
        &self.0[offset..offset + length]
    }

    fn chunk_mut(&mut self, offset: usize, length: usize) -> &mut [u8] {
        assert_eq!(offset / PAGE, (offset + length - 1) / PAGE, "one page");
        &mut self.0[offset..offset + length]
    }
}

/// A seeded generator, so that a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn write(ring: &mut Ring, storage: &mut Flat, bytes: &[u8]) -> Result<usize, Refused> {
    ring.write(storage, bytes.len(), |at, chunk: &mut [u8]| {
        chunk.copy_from_slice(&bytes[at..at + chunk.len()]);
        Ok::<(), ()>(())
    })
    .map_err(|e| match e {
        Error::Refused(r) => r,
        Error::Copy(()) => unreachable!(),
    })
}

fn read(ring: &mut Ring, storage: &Flat, room: usize) -> Result<Vec<u8>, Refused> {
    let mut out = vec![0; room];
    let n = ring
        .read(storage, room, |at, chunk| {
            out[at..at + chunk.len()].copy_from_slice(chunk);
            Ok::<(), ()>(())
        })
        .map_err(|e| match e {
            Error::Refused(r) => r,
            Error::Copy(()) => unreachable!(),
        })?;
    out.truncate(n);
    Ok(out)
}

/// A ring with `length` bytes in it, starting at `start`.
fn ring_at(start: usize, length: usize) -> Ring {
    let mut ring = Ring::new();
    ring.commit_write(start);
    ring.commit_read(start);
    ring.commit_write(length);
    ring
}

#[test]
fn a_new_ring_is_empty_open_and_writable() {
    let ring = Ring::new();
    assert!(ring.is_empty());
    assert_eq!(ring.free(), CAPACITY);
    assert!(ring.reader_open() && ring.writer_open());
    assert_eq!(ring.read_signals(), 0);
    assert_eq!(ring.write_signals(), WRITABLE);
    assert_eq!(Ring::default(), ring);
}

/// INV-0101-1's rule, at each boundary.
#[test]
fn small_writes_need_atomic_room_and_large_ones_take_what_fits() {
    let cases = [
        // (bytes already in, requested, result)
        (0, 1, Ok(1)),
        (0, ATOMIC, Ok(ATOMIC)),
        (0, CAPACITY, Ok(CAPACITY)),
        (0, CAPACITY + 1, Ok(CAPACITY)),
        (0, usize::MAX, Ok(CAPACITY)),
        (CAPACITY - ATOMIC, ATOMIC, Ok(ATOMIC)),
        (CAPACITY - ATOMIC, 1, Ok(1)),
        (CAPACITY - ATOMIC + 1, 1, Err(Refused::WouldBlock)),
        (CAPACITY - ATOMIC + 1, ATOMIC, Err(Refused::WouldBlock)),
        (CAPACITY - ATOMIC + 1, ATOMIC + 1, Ok(ATOMIC - 1)),
        (CAPACITY - 1, ATOMIC + 1, Ok(1)),
        (CAPACITY, ATOMIC + 1, Err(Refused::WouldBlock)),
        (CAPACITY, 1, Err(Refused::WouldBlock)),
        (CAPACITY, 0, Ok(0)),
        (0, 0, Ok(0)),
    ];
    for (held, requested, expected) in cases {
        let ring = ring_at(0, held);
        assert_eq!(ring.accept(requested), expected, "{held} {requested}");
    }
}

#[test]
fn a_write_with_no_reader_is_refused_whatever_its_size() {
    for held in [0, 10, CAPACITY] {
        let mut ring = ring_at(0, held);
        ring.close_reader();
        for requested in [0, 1, ATOMIC, ATOMIC + 1, CAPACITY] {
            assert_eq!(ring.accept(requested), Err(Refused::PeerClosed));
        }
        let mut storage = Flat::new();
        assert_eq!(
            write(&mut ring, &mut storage, b"x"),
            Err(Refused::PeerClosed)
        );
        assert_eq!(ring.len(), held, "nothing changed");
    }
}

/// INV-0101-2's read half: 0 exactly when empty with no writer.
#[test]
fn a_read_returns_data_then_would_block_or_end_of_file() {
    let mut ring = ring_at(0, 10);
    assert_eq!(ring.available(4), Ok(4));
    assert_eq!(ring.available(100), Ok(10));
    assert_eq!(ring.available(0), Ok(0));
    ring.commit_read(10);
    assert_eq!(ring.available(1), Err(Refused::WouldBlock));
    ring.close_writer();
    assert_eq!(ring.available(1), Ok(0));
    let mut ring = ring_at(0, 3);
    ring.close_writer();
    assert_eq!(ring.available(8), Ok(3), "data first, then end of file");
}

#[test]
fn chunks_stay_in_a_page_wrap_around_and_add_up() {
    for (start, length, count) in [
        (0, 0, 0),
        (0, 0, CAPACITY),
        (100, 0, 5000),
        (CAPACITY - 10, 0, 30),
        (CAPACITY - 1, 0, CAPACITY),
        (4095, 0, 2),
        (0, CAPACITY, 1),
        (7, CAPACITY - 7, 1000),
    ] {
        let ring = ring_at(start, length);
        let free: Vec<_> = ring.free_chunks(count).collect();
        let total: usize = free.iter().map(|c| c.1).sum();
        assert_eq!(total, count.min(ring.free()));
        let mut expect = (start + length) % CAPACITY;
        for &(offset, len) in &free {
            assert_eq!(offset, expect, "contiguous around the ring");
            assert!(len > 0 && offset + len <= CAPACITY);
            assert_eq!(offset / PAGE, (offset + len - 1) / PAGE);
            expect = (offset + len) % CAPACITY;
        }
        let data: Vec<_> = ring.data_chunks(count).collect();
        let total: usize = data.iter().map(|c| c.1).sum();
        assert_eq!(total, count.min(length));
        if let Some(&(offset, _)) = data.first() {
            assert_eq!(offset, start % CAPACITY);
        }
    }
}

#[test]
fn commits_never_go_past_the_ring() {
    let mut ring = Ring::new();
    ring.commit_write(CAPACITY + 5);
    assert_eq!(ring.len(), CAPACITY);
    ring.commit_read(CAPACITY + 5);
    assert_eq!(ring.len(), 0);
    assert_eq!(
        ring.free_chunks(usize::MAX).map(|c| c.1).sum::<usize>(),
        CAPACITY
    );
    assert_eq!(ring.data_chunks(usize::MAX).count(), 0);
}

#[test]
fn signals_follow_the_bytes_and_the_ends() {
    let mut ring = ring_at(0, 1);
    assert_eq!(ring.read_signals(), READABLE);
    assert_eq!(ring.write_signals(), WRITABLE);
    let full = ring_at(0, CAPACITY - ATOMIC + 1);
    assert_eq!(full.write_signals(), 0, "not WRITABLE below ATOMIC free");
    assert_eq!(ring_at(0, CAPACITY - ATOMIC).write_signals(), WRITABLE);
    ring.close_reader();
    assert_eq!(ring.write_signals(), PEER_CLOSED);
    assert_eq!(ring.read_signals(), READABLE);
    let mut ring = Ring::new();
    ring.close_writer();
    assert_eq!(
        ring.read_signals(),
        READABLE | PEER_CLOSED,
        "end of file wakes"
    );
    assert_eq!(ring.write_signals(), WRITABLE);
}

/// A failed copy leaves the ring as it was, in both directions.
#[test]
fn a_failed_copy_changes_nothing() {
    let mut storage = Flat::new();
    let mut ring = ring_at(CAPACITY - 100, 50);
    let before = ring;
    let failed = ring.write(
        &mut storage,
        10_000,
        |at, _| if at > 0 { Err(7) } else { Ok(()) },
    );
    assert_eq!(failed, Err(Error::Copy(7)));
    assert_eq!(ring, before);
    let failed = ring.read(&storage, 40, |_, _| Err(8));
    assert_eq!(failed, Err(Error::Copy(8)));
    assert_eq!(ring, before);
    assert_eq!(
        ring.read(&storage, 40, |_, _| Ok::<(), ()>(())),
        Ok(40),
        "works afterwards"
    );
}

/// INV-0101-1: writers of `ATOMIC`-byte records, interleaved at random with a
/// reader of random sizes; every record comes out whole.
#[test]
fn atomic_records_never_interleave() {
    let mut rng = Rng(0x2545_f491_4f6c_dd1d);
    let mut storage = Flat::new();
    let mut ring = Ring::new();
    let mut sent = [0u32; 3];
    let mut received: Vec<u8> = Vec::new();
    let mut would_block = 0;
    for _ in 0..20_000 {
        if rng.below(3) < 2 {
            let writer = rng.below(3);
            let mut record = [0u8; ATOMIC];
            record[0] = writer as u8;
            record[1..5].copy_from_slice(&sent[writer].to_le_bytes());
            for (i, b) in record[5..].iter_mut().enumerate() {
                *b = (writer as u8).wrapping_mul(31).wrapping_add(i as u8);
            }
            match write(&mut ring, &mut storage, &record) {
                Ok(n) => {
                    assert_eq!(n, ATOMIC, "whole");
                    sent[writer] += 1;
                }
                Err(Refused::WouldBlock) => would_block += 1,
                Err(e) => panic!("{e:?}"),
            }
        } else {
            let room = 1 + rng.below(3 * ATOMIC);
            if let Ok(bytes) = read(&mut ring, &storage, room) {
                received.extend_from_slice(&bytes);
            }
        }
    }
    while let Ok(bytes) = read(&mut ring, &storage, CAPACITY) {
        received.extend_from_slice(&bytes);
    }
    assert!(would_block > 0, "the ring filled up at some point");
    assert_eq!(received.len() % ATOMIC, 0);
    let mut next = [0u32; 3];
    for record in received.chunks(ATOMIC) {
        let writer = record[0] as usize;
        let number = u32::from_le_bytes(record[1..5].try_into().unwrap());
        assert_eq!(number, next[writer], "in order, none lost");
        next[writer] += 1;
        for (i, b) in record[5..].iter().enumerate() {
            assert_eq!(*b, (writer as u8).wrapping_mul(31).wrapping_add(i as u8));
        }
    }
    assert_eq!(next, sent);
}

/// The ring against a queue of bytes, over random transfers and closes.
#[test]
fn the_ring_behaves_as_a_queue() {
    for seed in 1..24u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut storage = Flat::new();
        let mut ring = Ring::new();
        let mut model: VecDeque<u8> = VecDeque::new();
        let (mut reader, mut writer) = (true, true);
        let mut counter = 0u8;
        for _ in 0..2000 {
            match rng.below(20) {
                0 if seed % 3 == 0 => {
                    ring.close_reader();
                    reader = false;
                }
                1 if seed % 4 == 0 => {
                    ring.close_writer();
                    writer = false;
                }
                2..=10 => {
                    let size = match rng.below(4) {
                        0 => rng.below(ATOMIC + 1),
                        1 => ATOMIC + rng.below(2 * ATOMIC),
                        2 => rng.below(8),
                        _ => rng.below(CAPACITY + 10),
                    };
                    let bytes: Vec<u8> = (0..size)
                        .map(|_| {
                            counter = counter.wrapping_add(1);
                            counter
                        })
                        .collect();
                    let free = CAPACITY - model.len();
                    let expected = if !reader {
                        Err(Refused::PeerClosed)
                    } else if size == 0 {
                        Ok(0)
                    } else if size <= ATOMIC {
                        if free >= ATOMIC {
                            Ok(size)
                        } else {
                            Err(Refused::WouldBlock)
                        }
                    } else if free == 0 {
                        Err(Refused::WouldBlock)
                    } else {
                        Ok(size.min(free))
                    };
                    let got = write(&mut ring, &mut storage, &bytes);
                    assert_eq!(got, expected);
                    if let Ok(n) = got {
                        model.extend(&bytes[..n]);
                    }
                }
                _ => {
                    let room = rng.below(CAPACITY + 10);
                    let got = read(&mut ring, &storage, room);
                    let expected: Result<Vec<u8>, Refused> = if room == 0 {
                        Ok(Vec::new())
                    } else if model.is_empty() {
                        if writer {
                            Err(Refused::WouldBlock)
                        } else {
                            Ok(Vec::new())
                        }
                    } else {
                        let n = room.min(model.len());
                        Ok(model.drain(..n).collect())
                    };
                    assert_eq!(got, expected);
                }
            }
            assert_eq!(ring.len(), model.len());
            let readable = !model.is_empty() || !writer;
            assert_eq!(ring.read_signals() & READABLE != 0, readable);
            assert_eq!(ring.read_signals() & PEER_CLOSED != 0, !writer);
            let writable = reader && CAPACITY - model.len() >= ATOMIC;
            assert_eq!(ring.write_signals() & WRITABLE != 0, writable);
            assert_eq!(ring.write_signals() & PEER_CLOSED != 0, !reader);
        }
    }
}
