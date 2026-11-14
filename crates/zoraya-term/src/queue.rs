//! What waits to be sent to the shell when its queue is full (ADR-0385): the
//! gestures' bytes in order, input in messages of at most [`MAX_INPUT`], a paste
//! in its parts. Input is bounded and what does not fit is dropped and counted
//! (the shell is told with the next input message); a paste is kept or dropped
//! whole, never in part, so the shell never joins half of one to the next.

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use zoraya_proto_term::MAX_INPUT;
use zoraya_proto_term::paste::MAX_PASTE_BYTES;

use crate::session::Outgoing;

/// Input bytes held for a slow shell: a person cannot type this far ahead.
pub const INPUT_MAX: usize = 4096;
/// Paste bytes held: two of the largest pastes.
pub const PASTE_MAX: usize = 2 * MAX_PASTE_BYTES;

/// The next message to send.
#[derive(Debug, PartialEq, Eq)]
pub enum Next<'a> {
    /// `Input`: how many bytes were dropped before these, and the bytes.
    Input { dropped: u32, bytes: &'a [u8] },
    /// `Paste`.
    Paste { last: bool, bytes: &'a [u8] },
}

/// The queue.
#[derive(Debug, Default)]
pub struct SendQueue {
    items: VecDeque<Outgoing>,
    input: usize,
    paste: usize,
    dropped: u32,
}

impl SendQueue {
    /// Adds what the session made. Returns the bytes of the pastes dropped
    /// because they did not fit.
    pub fn push(&mut self, made: Vec<Outgoing>) -> usize {
        let mut lost = 0;
        let mut group: Vec<Outgoing> = Vec::new();
        for item in made {
            match item {
                Outgoing::Input(bytes) => {
                    if self.input + bytes.len() <= INPUT_MAX {
                        self.input += bytes.len();
                        self.items.push_back(Outgoing::Input(bytes));
                    } else {
                        self.dropped = self.dropped.saturating_add(bytes.len() as u32);
                    }
                }
                Outgoing::Paste { bytes, last } => {
                    group.push(Outgoing::Paste { bytes, last });
                    if last {
                        let size: usize = group
                            .iter()
                            .map(|p| match p {
                                Outgoing::Paste { bytes, .. } => bytes.len(),
                                Outgoing::Input(_) => 0,
                            })
                            .sum();
                        if self.paste + size <= PASTE_MAX {
                            self.paste += size;
                            self.items.extend(group.drain(..));
                        } else {
                            lost += size;
                            group.clear();
                        }
                    }
                }
            }
        }
        lost
    }

    /// The message to send now, if anything waits.
    pub fn next(&self) -> Option<Next<'_>> {
        Some(match self.items.front()? {
            Outgoing::Input(bytes) => Next::Input {
                dropped: self.dropped,
                bytes: &bytes[..bytes.len().min(MAX_INPUT)],
            },
            Outgoing::Paste { bytes, last } => Next::Paste { last: *last, bytes },
        })
    }

    /// The message [`SendQueue::next`] gave was sent.
    pub fn sent(&mut self) {
        match self.items.front_mut() {
            Some(Outgoing::Input(bytes)) => {
                let n = bytes.len().min(MAX_INPUT);
                self.dropped = 0;
                self.input -= n;
                if n == bytes.len() {
                    self.items.pop_front();
                } else {
                    bytes.drain(..n);
                }
            }
            Some(Outgoing::Paste { bytes, .. }) => {
                self.paste -= bytes.len();
                self.items.pop_front();
            }
            None => {}
        }
    }

    /// The shell cannot be written to at all: what waits is dropped, and the
    /// input is counted.
    pub fn fail(&mut self) {
        self.dropped = self
            .dropped
            .saturating_add(u32::try_from(self.input).unwrap_or(u32::MAX));
        self.items.clear();
        self.input = 0;
        self.paste = 0;
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use alloc::vec;

    fn input(n: usize) -> Outgoing {
        Outgoing::Input(vec![b'k'; n])
    }

    fn paste(parts: &[usize]) -> Vec<Outgoing> {
        let last = parts.len() - 1;
        parts
            .iter()
            .enumerate()
            .map(|(i, n)| Outgoing::Paste {
                bytes: vec![b'p'; *n],
                last: i == last,
            })
            .collect()
    }

    #[test]
    fn input_goes_in_messages_of_at_most_max_input_in_order() {
        let mut q = SendQueue::default();
        assert_eq!(q.push(vec![input(MAX_INPUT + 10), input(3)]), 0);
        let Some(Next::Input { dropped: 0, bytes }) = q.next() else {
            panic!()
        };
        assert_eq!(bytes.len(), MAX_INPUT);
        q.sent();
        let Some(Next::Input { bytes, .. }) = q.next() else {
            panic!()
        };
        assert_eq!(bytes.len(), 10);
        q.sent();
        let Some(Next::Input { bytes, .. }) = q.next() else {
            panic!()
        };
        assert_eq!(bytes.len(), 3);
        q.sent();
        assert!(q.is_empty() && q.next().is_none());
    }

    #[test]
    fn input_past_the_bound_is_dropped_and_counted_once_it_is_told() {
        let mut q = SendQueue::default();
        q.push(vec![input(INPUT_MAX - 1)]);
        q.push(vec![input(2), input(1)]);
        // The two did not fit as a whole; the one after them did not either:
        // 1 + 2 > room is for the 2, then 1 fits exactly.
        let mut sent = 0;
        let mut told = None;
        while let Some(Next::Input { dropped, bytes }) = q.next() {
            sent += bytes.len();
            if dropped > 0 {
                told = Some(dropped);
            }
            q.sent();
        }
        assert_eq!(sent, INPUT_MAX);
        assert_eq!(told, Some(2), "the dropped are told with the next message");
        q.push(vec![input(1)]);
        let Some(Next::Input { dropped, .. }) = q.next() else {
            panic!()
        };
        assert_eq!(dropped, 0, "and only once");
    }

    #[test]
    fn a_paste_is_kept_or_dropped_whole_and_in_order_with_input() {
        let mut q = SendQueue::default();
        assert_eq!(
            q.push([vec![input(2)], paste(&[4096, 100]), vec![input(1)]].concat()),
            0
        );
        let mut got = Vec::new();
        while let Some(n) = q.next() {
            got.push(match n {
                Next::Input { bytes, .. } => ('i', bytes.len(), false),
                Next::Paste { last, bytes } => ('p', bytes.len(), last),
            });
            q.sent();
        }
        assert_eq!(
            got,
            [
                ('i', 2, false),
                ('p', 4096, false),
                ('p', 100, true),
                ('i', 1, false)
            ]
        );
        // Two pastes of 64 KiB fit; the third is dropped whole.
        let big = [16384usize; 4];
        assert_eq!(q.push(paste(&big)), 0);
        assert_eq!(q.push(paste(&big)), 0);
        assert_eq!(q.push(paste(&big)), 65536);
        let mut parts = 0;
        while q.next().is_some() {
            parts += 1;
            q.sent();
        }
        assert_eq!(parts, 8, "two pastes of four parts: never half of a third");
    }

    #[test]
    fn a_failed_write_drops_everything_and_counts_the_input() {
        let mut q = SendQueue::default();
        q.push([vec![input(5)], paste(&[10]), vec![input(7)]].concat());
        q.fail();
        assert!(q.is_empty());
        q.push(vec![input(1)]);
        let Some(Next::Input { dropped, .. }) = q.next() else {
            panic!()
        };
        assert_eq!(dropped, 12);
        // The bounds are back to zero.
        q.sent();
        assert_eq!(q.push(paste(&[1000])), 0);
    }
}
