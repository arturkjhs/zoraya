//! A bounded measurement of pointer input against frames (TASK-31,
//! `gui-smooth`): off unless `compd` finds `/config/gui-measure`, so a
//! desktop pays nothing for it. While on, it counts what one second of
//! pointer activity cost and prints one console line per second of
//! activity, at most [`MAX_REPORTS`] lines in a boot.
//!
//! The delay it reports ends where the compositor can see: the display
//! driver's answer to the flush that carried the change. The host's own
//! scan-out after that is not seen from the guest.

use alloc::format;
use alloc::string::String;

/// The longest a report line is held back.
pub const REPORT_NS: u64 = 1_000_000_000;
/// The most lines one boot prints.
pub const MAX_REPORTS: u32 = 64;

/// What a window of activity cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Messages read from an input driver.
    pub messages: u64,
    /// Pointer position events (absolute axes) in them.
    pub axis_events: u64,
    /// Rounds in which the pointer's position changed.
    pub moves: u64,
    /// Rounds of the loop, and those that woke with nothing ready (a
    /// deadline: a frame held to the refresh, a retry).
    pub rounds: u64,
    pub idle_rounds: u64,
    /// Wakeups by what was ready: managers, display, inputs, clients.
    pub wakes: [u64; 4],
    /// Frames composed.
    pub frames: u64,
    pub compose_ns: u64,
    /// Frames composed while a window was dragged, and their time.
    pub drag_frames: u64,
    pub drag_ns: u64,
    pub drag_max_ns: u64,
    /// The time between the starts of two frames in a row: sum, longest.
    pub gap_ns: u64,
    pub gap_max_ns: u64,
    /// Pixels the frames' damage covered (before flushing's union).
    pub damage_pixels: u64,
    /// Flush messages sent to the driver, and the bytes they cover
    /// (4 a pixel), and the pointer moves told to the driver.
    pub flushes: u64,
    pub flush_bytes: u64,
    pub cursor_moves: u64,
    /// Flushes answered, the time they took, and the delay from the first
    /// pointer event a flush carried to its answer.
    pub answered: u64,
    pub flush_ns: u64,
    pub delay_ns: u64,
    pub delay_max_ns: u64,
}

/// The state of a measurement.
#[derive(Debug, Default)]
pub struct Measure {
    pub on: bool,
    counts: Counts,
    since: u64,
    reports: u32,
    /// The first pointer event not yet in a flush sent.
    event_at: Option<u64>,
    /// The first pointer event of the flush in flight.
    flight_event: Option<u64>,
}

impl Measure {
    pub fn new() -> Measure {
        Measure::default()
    }

    pub fn message(&mut self, now: u64) {
        if self.on {
            self.begin(now);
            self.counts.messages += 1;
        }
    }

    pub fn axis(&mut self, now: u64) {
        if self.on {
            self.begin(now);
            self.counts.axis_events += 1;
        }
    }

    /// The pointer's position changed.
    pub fn moved(&mut self, now: u64) {
        if self.on {
            self.begin(now);
            self.counts.moves += 1;
            self.event_at.get_or_insert(now);
        }
    }

    /// A ready key's kind: 0 managers, 1 display, 2 input, 3 client.
    pub fn wake(&mut self, kind: usize) {
        if self.on {
            self.counts.wakes[kind.min(3)] += 1;
        }
    }

    pub fn round(&mut self, idle: bool) {
        if self.on {
            self.counts.rounds += 1;
            self.counts.idle_rounds += u64::from(idle);
        }
    }

    /// A round ended with no frame to compose (the host drew the move): the
    /// first event it brought is not one a frame will show.
    pub fn no_frame(&mut self) {
        self.event_at = None;
    }

    /// A frame started `gap` ns after the one before.
    pub fn gap(&mut self, gap: u64) {
        if self.on {
            self.counts.gap_ns += gap;
            self.counts.gap_max_ns = self.counts.gap_max_ns.max(gap);
        }
    }

    /// A frame of `pixels` damage took `took` ns; `drag`: a window was
    /// being moved or resized.
    pub fn frame(&mut self, took: u64, pixels: u64, drag: bool) {
        if self.on {
            let c = &mut self.counts;
            c.frames += 1;
            c.compose_ns += took;
            c.damage_pixels += pixels;
            if drag {
                c.drag_frames += 1;
                c.drag_ns += took;
                c.drag_max_ns = c.drag_max_ns.max(took);
            }
        }
    }

    pub fn cursor_move(&mut self) {
        if self.on {
            self.counts.cursor_moves += 1;
        }
    }

    /// A flush of `pixels` was sent.
    pub fn flush_sent(&mut self, pixels: u64) {
        if self.on {
            self.counts.flushes += 1;
            self.counts.flush_bytes += pixels * 4;
            self.flight_event = self.event_at.take();
        }
    }

    /// The driver answered the flush in flight, which took `took` ns.
    pub fn flush_answered(&mut self, now: u64, took: u64) {
        if self.on {
            let c = &mut self.counts;
            c.answered += 1;
            c.flush_ns += took;
            if let Some(at) = self.flight_event.take() {
                let delay = now.saturating_sub(at);
                c.delay_ns += delay;
                c.delay_max_ns = c.delay_max_ns.max(delay);
            }
        }
    }

    fn begin(&mut self, now: u64) {
        if self.since == 0 {
            self.since = now.max(1);
        }
    }

    /// A report line when a second of activity has passed, then the counts
    /// start again.
    pub fn report(&mut self, now: u64) -> Option<String> {
        if !self.on
            || self.since == 0
            || self.reports >= MAX_REPORTS
            || now.saturating_sub(self.since) < REPORT_NS
        {
            return None;
        }
        let c = self.counts;
        let secs_us = now.saturating_sub(self.since) / 1000;
        let per = |n: u64, d: u64| n.checked_div(d).unwrap_or(0);
        let line = format!(
            "compd: measure: {} us: input {} messages {} axis {} moves; {} rounds ({} idle; wakes mgr {} disp {} in {} cl {}) {} frames \
             (mean {} us, {} px) drag {} frames (mean {} us, max {} us); flush {} sent {} KiB \
             (mean {} us) cursor {} moves; delay mean {} us max {} us; frame gap mean {} us max {} us",
            secs_us,
            c.messages,
            c.axis_events,
            c.moves,
            c.rounds,
            c.idle_rounds,
            c.wakes[0],
            c.wakes[1],
            c.wakes[2],
            c.wakes[3],
            c.frames,
            per(c.compose_ns, c.frames) / 1000,
            per(c.damage_pixels, c.frames),
            c.drag_frames,
            per(c.drag_ns, c.drag_frames) / 1000,
            c.drag_max_ns / 1000,
            c.flushes,
            c.flush_bytes / 1024,
            per(c.flush_ns, c.answered) / 1000,
            c.cursor_moves,
            per(c.delay_ns, c.answered) / 1000,
            c.delay_max_ns / 1000,
            per(c.gap_ns, c.frames.saturating_sub(1)) / 1000,
            c.gap_max_ns / 1000,
        );
        self.counts = Counts::default();
        self.since = 0;
        self.reports += 1;
        Some(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_it_counts_and_prints_nothing() {
        let mut m = Measure::new();
        m.message(1);
        m.moved(2);
        m.frame(5, 5, true);
        assert_eq!(m.report(10_000_000_000), None);
        assert_eq!(m.counts, Counts::default());
    }

    #[test]
    fn a_second_of_activity_makes_one_line_and_starts_again() {
        let mut m = Measure::new();
        m.on = true;
        m.message(100);
        m.axis(100);
        m.moved(100);
        m.round(false);
        m.frame(2_000, 64, false);
        m.frame(4_000, 64, true);
        m.flush_sent(100);
        m.flush_answered(1_000_100, 500_000);
        assert_eq!(m.report(REPORT_NS), None, "not yet a second");
        let line = m.report(100 + REPORT_NS).unwrap();
        assert!(line.contains("input 1 messages 1 axis 1 moves"), "{line}");
        assert!(line.contains("2 frames (mean 3 us, 64 px)"), "{line}");
        assert!(line.contains("flush 1 sent 0 KiB"), "{line}");
        assert!(line.contains("delay mean 1000 us max 1000 us"), "{line}");
        assert_eq!(m.report(10 * REPORT_NS), None, "no activity, no line");
    }

    #[test]
    fn the_delay_is_from_the_first_event_the_flush_carried() {
        let mut m = Measure::new();
        m.on = true;
        m.moved(1_000);
        m.moved(5_000);
        m.flush_sent(1);
        // An event after the send belongs to the next flush.
        m.moved(9_000);
        m.flush_answered(21_000, 10);
        assert_eq!(m.counts.delay_ns, 20_000);
        m.flush_sent(1);
        m.flush_answered(30_000, 10);
        assert_eq!(m.counts.delay_max_ns, 21_000);
    }

    #[test]
    fn a_move_the_host_drew_has_no_delay_to_report() {
        let mut m = Measure::new();
        m.on = true;
        m.moved(1_000);
        m.no_frame();
        m.moved(9_000_000);
        m.flush_sent(1);
        m.flush_answered(9_100_000, 10);
        assert_eq!(m.counts.delay_max_ns, 100_000);
    }

    #[test]
    fn at_most_a_bounded_number_of_lines() {
        let mut m = Measure::new();
        m.on = true;
        let mut t = 1;
        let mut lines = 0;
        for _ in 0..MAX_REPORTS * 2 {
            m.message(t);
            t += REPORT_NS;
            lines += u32::from(m.report(t).is_some());
        }
        assert_eq!(lines, MAX_REPORTS);
    }
}
