//! A room's limits and use as a person reads them (ADR-0267, ADR-0266
//! point 7): one [`Reading`] of the room keeper's usage record, and every
//! text made from it. `top`'s row, `room`'s words, `grants`' limits line, the
//! compositor's hover card and its pills all come from the functions here, so
//! that they cannot disagree (INV-0266-3, INV-0267-1). The panel sends the
//! compositor the readings in [`Reading::to_wire`]'s text; the compositor
//! checks them with [`Reading::from_wire`] and draws them from its own
//! records.
//!
//! Everything is total: a number that does not apply is `None`, a text that
//! does not parse is `None`, and arithmetic saturates.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::rooms::{Limits, format_size};

/// A word at this share of its limit or more shows a pill (ADR-0263
/// point 7, ADR-0267 point 3).
pub const NEAR_PERCENT: u64 = 90;
/// Usage is read at most this often (ADR-0267 driver 4, INV-0267-2), ns.
pub const MIN_INTERVAL_NS: u64 = 1_000_000_000;
/// Readings on the wire: the rooms of the file and `home`.
pub const MAX_READINGS: usize = 33;

/// One room's limits and use at one moment. `None` is a word the room does
/// not have, or a use nobody reports (disk and connections, until the
/// launchers forward the services' counts).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reading {
    pub name: String,
    pub running: bool,
    pub untrusted: bool,
    pub mem_used: u64,
    pub mem_limit: Option<u64>,
    pub mem_refused: u64,
    pub procs_used: u64,
    pub procs_limit: Option<u64>,
    pub procs_refused: u64,
    /// The CPU limit in thousandths of the machine.
    pub cpu_permille: Option<u64>,
    /// The share of the machine over the last reading's interval, percent.
    pub cpu_pct: Option<u64>,
    pub disk_limit: Option<u64>,
    pub conns_limit: Option<u64>,
    pub tmp_limit: Option<u64>,
}

/// The share of one CPU between two readings of a room's CPU time, percent.
/// `None` when the interval is empty or the counter went back (a restarted
/// room).
pub fn cpu_share(before_ns: u64, after_ns: u64, interval_ns: u64) -> Option<u64> {
    if interval_ns == 0 || after_ns < before_ns {
        return None;
    }
    let share = u128::from(after_ns - before_ns) * 100 / u128::from(interval_ns);
    Some(u64::try_from(share).unwrap_or(u64::MAX).min(100))
}

/// `used * 100 / limit`, rounded down; 0 for a limit of 0.
fn percent(used: u64, limit: u64) -> u64 {
    if limit == 0 {
        return 0;
    }
    u64::try_from(u128::from(used) * 100 / u128::from(limit)).unwrap_or(u64::MAX)
}

fn pair(used: String, limit: Option<String>) -> String {
    match limit {
        Some(l) => format!("{used} of {l}"),
        None => used,
    }
}

impl Reading {
    /// The room's limits as the line writes them: the same type
    /// `grants` and `room new` use.
    pub fn limits(&self) -> Limits {
        Limits {
            mem: self.mem_limit,
            cpu: self
                .cpu_permille
                .map(|p| u32::try_from(p.div_ceil(10).clamp(1, 100)).unwrap_or(100)),
            procs: self
                .procs_limit
                .map(|p| u32::try_from(p).unwrap_or(u32::MAX)),
            disk: self.disk_limit,
            conns: self
                .conns_limit
                .map(|p| u32::try_from(p).unwrap_or(u32::MAX)),
            tmp: self.tmp_limit,
        }
    }

    /// `limits --mem 512M --cpu 25% ...`, as `grants` prints it without its
    /// trailing `(room NAME)`; empty when the room has no limit.
    pub fn limits_line(&self) -> String {
        let words = self.limits().words();
        if words.is_empty() {
            String::new()
        } else {
            format!("limits {words}")
        }
    }

    fn mem_cell(&self) -> String {
        pair(format_size(self.mem_used), self.mem_limit.map(format_size))
    }

    fn cpu_cell(&self) -> String {
        let used = self
            .cpu_pct
            .map_or_else(|| String::from("-"), |p| format!("{p}%"));
        let limit = self.limits().cpu.map(|p| format!("{p}%"));
        pair(used, limit)
    }

    fn procs_cell(&self) -> String {
        pair(
            format!("{}", self.procs_used),
            self.procs_limit.map(|l| format!("{l}")),
        )
    }

    fn disk_cell(&self) -> String {
        match self.disk_limit {
            Some(l) => format!("- of {}", format_size(l)),
            None => String::from("-"),
        }
    }

    fn conns_cell(&self) -> String {
        match self.conns_limit {
            Some(l) => format!("- of {l}"),
            None => String::from("-"),
        }
    }

    /// The five cells of `top`'s row: memory, CPU, processes, disk,
    /// connections.
    pub fn cells(&self) -> [String; 5] {
        [
            self.mem_cell(),
            self.cpu_cell(),
            self.procs_cell(),
            self.disk_cell(),
            self.conns_cell(),
        ]
    }

    /// The trailer of `top`'s row: refusals since the room started.
    pub fn trailer(&self) -> String {
        let mut parts = Vec::new();
        if self.mem_refused > 0 {
            parts.push(format!("{} mem", self.mem_refused));
        }
        if self.procs_refused > 0 {
            parts.push(format!("{} procs", self.procs_refused));
        }
        if parts.is_empty() {
            String::new()
        } else {
            format!("(refused {})", parts.join(", "))
        }
    }

    /// The lines of a window's hover card and a rooms card, after the grants:
    /// `mem 498M of 512M`, one per word the room has, then `limits ...` in
    /// the words of `grants`.
    pub fn card_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("mem {}", self.mem_cell()));
        if self.cpu_permille.is_some() {
            out.push(format!("cpu {}", self.cpu_cell()));
        }
        if self.procs_limit.is_some() || self.procs_used > 0 {
            out.push(format!("procs {}", self.procs_cell()));
        }
        if self.disk_limit.is_some() {
            out.push(format!("disk {}", self.disk_cell()));
        }
        if self.conns_limit.is_some() {
            out.push(format!("conns {}", self.conns_cell()));
        }
        if let Some(t) = self.tmp_limit {
            out.push(format!("tmp {}", format_size(t)));
        }
        let limits = self.limits_line();
        if !limits.is_empty() {
            out.push(limits);
        }
        out
    }

    /// The pills of a room at its limits (ADR-0263 point 7): `mem 94%`,
    /// `procs 91%`, `cpu 97%`, for each word at [`NEAR_PERCENT`] or more.
    /// Nothing below it, so a calm desktop shows none.
    pub fn pills(&self) -> Vec<String> {
        let mut out = Vec::new();
        if !self.running {
            return out;
        }
        if let Some(l) = self.mem_limit {
            let p = percent(self.mem_used, l);
            if p >= NEAR_PERCENT {
                out.push(format!("mem {p}%"));
            }
        }
        if let Some(l) = self.procs_limit {
            let p = percent(self.procs_used, l);
            if p >= NEAR_PERCENT {
                out.push(format!("procs {p}%"));
            }
        }
        if let (Some(used), Some(l)) = (self.cpu_pct, self.cpu_permille) {
            let p = percent(used.saturating_mul(10), l);
            if p >= NEAR_PERCENT {
                out.push(format!("cpu {p}%"));
            }
        }
        out
    }

    /// The reading as one line of the panel's `SetUsage`.
    pub fn to_wire(&self) -> String {
        let n = |v: Option<u64>| v.map_or_else(|| String::from("-"), |v| format!("{v}"));
        format!(
            "{} {} {} {} {} {} {} {} {} {} {} {} {}",
            self.name,
            u8::from(self.running) | (u8::from(self.untrusted) << 1),
            self.mem_used,
            n(self.mem_limit),
            self.mem_refused,
            self.procs_used,
            n(self.procs_limit),
            self.procs_refused,
            n(self.cpu_permille),
            n(self.cpu_pct),
            n(self.disk_limit),
            n(self.conns_limit),
            n(self.tmp_limit),
        )
    }

    /// Total: a line that is not exactly what [`Reading::to_wire`] writes is
    /// `None`.
    pub fn from_wire(line: &str) -> Option<Reading> {
        let mut it = line.split(' ');
        let name = it.next()?;
        if !crate::rooms::is_room_name(name) && name != crate::rooms::HOME {
            return None;
        }
        let num = |s: Option<&str>| -> Option<u64> { s?.parse().ok() };
        let opt = |s: Option<&str>| -> Option<Option<u64>> {
            match s? {
                "-" => Some(None),
                t => t.parse().ok().map(Some),
            }
        };
        let flags = num(it.next())?;
        if flags > 3 {
            return None;
        }
        let r = Reading {
            name: String::from(name),
            running: flags & 1 != 0,
            untrusted: flags & 2 != 0,
            mem_used: num(it.next())?,
            mem_limit: opt(it.next())?,
            mem_refused: num(it.next())?,
            procs_used: num(it.next())?,
            procs_limit: opt(it.next())?,
            procs_refused: num(it.next())?,
            cpu_permille: opt(it.next())?,
            cpu_pct: opt(it.next())?,
            disk_limit: opt(it.next())?,
            conns_limit: opt(it.next())?,
            tmp_limit: opt(it.next())?,
        };
        it.next().is_none().then_some(r)
    }
}

/// The readings in one `SetUsage` text, one a line, as many as fit in
/// `max` bytes (the rest are left out, last first).
pub fn to_wire(readings: &[Reading], max: usize) -> String {
    let mut out = String::new();
    for r in readings.iter().take(MAX_READINGS) {
        let line = r.to_wire();
        if out.len() + line.len() + 1 > max {
            break;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

/// Total: every line must be a reading, at most [`MAX_READINGS`], names
/// distinct; otherwise `None` and nothing is taken.
pub fn from_wire(text: &str) -> Option<Vec<Reading>> {
    let mut out: Vec<Reading> = Vec::new();
    for l in text.split('\n').filter(|l| !l.is_empty()) {
        let r = Reading::from_wire(l)?;
        if out.len() == MAX_READINGS || out.iter().any(|o| o.name == r.name) {
            return None;
        }
        out.push(r);
    }
    Some(out)
}

/// The table of `top` (ADR-0267 point 1): a header, a row per reading, and
/// a trailer in parentheses after a row that has refusals.
pub fn top_table(rows: &[Reading]) -> String {
    const HEAD: [&str; 6] = ["ROOM", "MEM", "CPU", "PROCS", "DISK", "CONNS"];
    let cells: Vec<(String, [String; 5], String)> = rows
        .iter()
        .map(|r| (r.name.clone(), r.cells(), r.trailer()))
        .collect();
    let mut width = [0usize; 6];
    for (i, h) in HEAD.iter().enumerate() {
        width[i] = h.len();
    }
    for (name, c, _) in &cells {
        width[0] = width[0].max(name.chars().count());
        for (i, s) in c.iter().enumerate() {
            width[i + 1] = width[i + 1].max(s.chars().count());
        }
    }
    let line = |cols: [&str; 6], trailer: &str| -> String {
        let mut s = String::new();
        for (i, c) in cols.iter().enumerate() {
            s.push_str(c);
            if i + 1 < cols.len() {
                for _ in c.chars().count()..width[i] + 2 {
                    s.push(' ');
                }
            }
        }
        if !trailer.is_empty() {
            s.push_str("  ");
            s.push_str(trailer);
        }
        s.push('\n');
        s
    };
    let mut out = line(HEAD, "");
    for (name, c, trailer) in &cells {
        out.push_str(&line([name, &c[0], &c[1], &c[2], &c[3], &c[4]], trailer));
    }
    out
}

/// The word `room` prints after a limit when the room runs:
/// `--mem 512M (498M)`, the limits in the line's order with their use.
pub fn room_words(r: &Reading) -> String {
    let mut parts: Vec<String> = Vec::new();
    let l = r.limits();
    if let Some(m) = l.mem {
        parts.push(format!(
            "--mem {} ({})",
            format_size(m),
            format_size(r.mem_used)
        ));
    }
    if let Some(c) = l.cpu {
        let used = r
            .cpu_pct
            .map_or_else(|| String::from("-"), |p| format!("{p}%"));
        parts.push(format!("--cpu {c}% ({used})"));
    }
    if let Some(p) = l.procs {
        parts.push(format!("--procs {p} ({})", r.procs_used));
    }
    if let Some(d) = l.disk {
        parts.push(format!("--disk {}", format_size(d)));
    }
    if let Some(c) = l.conns {
        parts.push(format!("--conns {c}"));
    }
    if let Some(t) = l.tmp {
        parts.push(format!("--tmp {}", format_size(t)));
    }
    parts.join(" ")
}

/// The throttle of every reader of usage (INV-0267-2): `due` is true at most
/// once in [`MIN_INTERVAL_NS`] of the clock it is given, whatever calls it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Throttle {
    last: Option<u64>,
}

impl Throttle {
    pub fn new() -> Throttle {
        Throttle { last: None }
    }

    /// True, and the time is remembered, when a reading is allowed at
    /// `now_ns`. A clock that went back allows one and starts again.
    pub fn due(&mut self, now_ns: u64) -> bool {
        let ok = match self.last {
            None => true,
            Some(t) => now_ns < t || now_ns - t >= MIN_INTERVAL_NS,
        };
        if ok {
            self.last = Some(now_ns);
        }
        ok
    }

    /// How long until the next reading is allowed, ns.
    pub fn wait(&self, now_ns: u64) -> u64 {
        match self.last {
            Some(t) if now_ns >= t => MIN_INTERVAL_NS.saturating_sub(now_ns - t),
            _ => 0,
        }
    }
}

/// `top --watch`'s loop (ADR-0267 point 1, INV-0267-2). `read` is called at
/// most once in [`MIN_INTERVAL_NS`] of `now`'s clock however often the loop
/// turns; `sleep` is given the time to wait; the loop ends when `stop` is
/// true or `read` gives `None` (no keeper).
pub fn watch(
    mut now: impl FnMut() -> u64,
    mut sleep: impl FnMut(u64),
    mut read: impl FnMut() -> Option<Vec<Reading>>,
    mut show: impl FnMut(&str),
    mut stop: impl FnMut() -> bool,
) {
    let mut throttle = Throttle::new();
    while !stop() {
        let t = now();
        if throttle.due(t) {
            match read() {
                Some(rows) => show(&top_table(&rows)),
                None => return,
            }
        }
        sleep(throttle.wait(now()).max(1_000_000));
    }
}
