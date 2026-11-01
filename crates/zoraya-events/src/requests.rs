//! A process's queue of delivered requests (ADR-0004, ADR-0046): fixed memory,
//! so that delivery cannot fail for lack of it.

use zoraya_abi::RequestKind;

/// Requests queued per process (ADR-0046).
pub const MAX_REQUESTS: usize = 64;

/// The queue holds [`MAX_REQUESTS`] already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

/// First in, first out.
#[derive(Debug, Clone)]
pub struct RequestQueue {
    entries: [(RequestKind, u32); MAX_REQUESTS],
    first: usize,
    count: usize,
}

impl Default for RequestQueue {
    fn default() -> Self {
        RequestQueue::new()
    }
}

impl RequestQueue {
    pub const fn new() -> RequestQueue {
        RequestQueue {
            entries: [(RequestKind::Interrupt, 0); MAX_REQUESTS],
            first: 0,
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn push(&mut self, kind: RequestKind, detail: u32) -> Result<(), QueueFull> {
        if self.count == MAX_REQUESTS {
            return Err(QueueFull);
        }
        self.entries[(self.first + self.count) % MAX_REQUESTS] = (kind, detail);
        self.count += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<(RequestKind, u32)> {
        if self.count == 0 {
            return None;
        }
        let request = self.entries[self.first];
        self.first = (self.first + 1) % MAX_REQUESTS;
        self.count -= 1;
        Some(request)
    }

    /// The detail of the first queued request of `kind`.
    pub fn queued(&self, kind: RequestKind) -> Option<u32> {
        (0..self.count)
            .map(|i| self.entries[(self.first + i) % MAX_REQUESTS])
            .find(|(k, _)| *k == kind)
            .map(|(_, detail)| detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_come_out_in_order_across_the_wrap() {
        let mut q = RequestQueue::new();
        for round in 0..3u32 {
            for i in 0..MAX_REQUESTS as u32 {
                q.push(RequestKind::Custom(i % 32), round * 100 + i)
                    .unwrap();
            }
            assert_eq!(q.push(RequestKind::Quit, 0), Err(QueueFull));
            for i in 0..MAX_REQUESTS as u32 {
                assert_eq!(q.pop().map(|r| r.1), Some(round * 100 + i));
            }
            assert_eq!(q.pop(), None);
            q.push(RequestKind::Quit, 1).unwrap();
            q.pop();
        }
    }

    #[test]
    fn queued_finds_the_first_of_a_kind() {
        let mut q = RequestQueue::new();
        q.push(RequestKind::Quit, 1).unwrap();
        q.push(RequestKind::Hangup, 2).unwrap();
        q.push(RequestKind::Hangup, 3).unwrap();
        assert_eq!(q.queued(RequestKind::Hangup), Some(2));
        assert_eq!(q.queued(RequestKind::Interrupt), None);
    }
}
