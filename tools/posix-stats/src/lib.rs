//! What `tools/posix-stats` reads and computes (ADR-0110 items 7, 8 and 11):
//! the lines a POSIX session's broker and the runner print, the derived
//! figures of the stage 6 tables, and the call log's union against the
//! tier list (INV-0005-3, INV-0110-2).

use std::collections::{BTreeMap, BTreeSet};

/// One runner log: its name, the broker's counters, the call bitmap and the
/// runner's time marks, in order.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Session {
    pub name: String,
    pub counters: BTreeMap<String, u64>,
    pub calls: Option<Vec<u8>>,
    pub times: Vec<(String, f64)>,
}

/// The text after a runner's `[label] ` prefix, if any.
fn body(line: &str) -> &str {
    match line.strip_prefix('[') {
        Some(rest) => rest.split_once("] ").map_or(line, |(_, b)| b),
        None => line,
    }
}

/// Hex pairs to bytes; `None` for an odd length or a bad digit.
pub fn unhex(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Reads a log. Several sessions in one log are an error: one boot, one set
/// of totals.
pub fn parse(name: &str, text: &str) -> Result<Session, String> {
    let mut s = Session {
        name: name.to_string(),
        ..Session::default()
    };
    for line in text.lines() {
        let b = body(line.trim_end_matches('\r'));
        if let Some(rest) = b.strip_prefix("POSIX-STATS ") {
            let (k, v) = rest
                .split_once('=')
                .ok_or(format!("{name}: a POSIX-STATS line without '=': {b}"))?;
            let v: u64 = v
                .trim()
                .parse()
                .map_err(|_| format!("{name}: not a number: {b}"))?;
            if s.counters.insert(k.to_string(), v).is_some() {
                return Err(format!("{name}: {k} twice: more than one session"));
            }
        } else if let Some(rest) = b.strip_prefix("POSIX-CALLS") {
            let bits = unhex(rest.trim()).ok_or(format!("{name}: bad POSIX-CALLS: {b}"))?;
            if s.calls.replace(bits).is_some() {
                return Err(format!("{name}: POSIX-CALLS twice: more than one session"));
            }
        } else if let Some(rest) = b.strip_prefix("RUNNER-TIME ") {
            let (k, v) = rest
                .rsplit_once(' ')
                .ok_or(format!("{name}: bad RUNNER-TIME: {b}"))?;
            let v: f64 = v.parse().map_err(|_| format!("{name}: bad time: {b}"))?;
            s.times.push((k.to_string(), v));
        }
    }
    Ok(s)
}

impl Session {
    fn get(&self, k: &str) -> Option<u64> {
        self.counters.get(k).copied()
    }

    /// The accelerator, from the log's name (`posix-w1-tcg`, `w4-hvf`).
    pub fn accelerator(&self) -> Option<&'static str> {
        ["auto", "hvf", "tcg"]
            .into_iter()
            .find(|a| self.name.ends_with(&format!("-{a}")))
    }

    /// The workload's wall time: every mark but the boot's.
    pub fn wall(&self) -> f64 {
        self.times
            .iter()
            .filter(|(k, _)| k != "boot")
            .map(|(_, v)| v)
            .sum()
    }

    /// The figures of ADR-0110 items 10 and 11, by name; `None` where a
    /// counter is missing or a mean has nothing to divide.
    pub fn figures(&self) -> Vec<(&'static str, Option<String>)> {
        let ms = |ns: Option<u64>| ns.map(|n| format!("{:.2}", n as f64 / 1e6));
        let mean_ms = |total: &str, n: &str| match (self.get(total), self.get(n)) {
            (Some(t), Some(n)) if n > 0 => Some(format!("{:.3}", t as f64 / n as f64 / 1e6)),
            _ => None,
        };
        let wall = self.wall();
        let share = |a: &str, b: &str| match (self.get(a), self.get(b)) {
            (Some(x), Some(y)) if wall > 0.0 => {
                Some(format!("{:.0} %", (x + y) as f64 / 1e9 / wall * 100.0))
            }
            _ => None,
        };
        let count = |k: &str| self.get(k).map(|v| v.to_string());
        let ratio = |a: &str, b: &str, scale: f64, unit: &str| match (self.get(a), self.get(b)) {
            (Some(x), Some(y)) if y > 0 => {
                Some(format!("{:.2}{unit}", x as f64 / y as f64 * scale))
            }
            _ => None,
        };
        let round_trips = match (self.get("prepares"), self.get("commits"), self.get("reaps")) {
            (Some(p), Some(c), Some(r)) if p > 0 => {
                Some(format!("{:.2}", (p + c + r) as f64 / p as f64))
            }
            _ => None,
        };
        let per_second = |k: &str| match self.get(k) {
            Some(v) if wall > 0.0 => Some(format!("{:.0}", v as f64 / wall)),
            _ => None,
        };
        vec![
            ("accelerator", self.accelerator().map(str::to_string)),
            ("wall time (s)", (wall > 0.0).then(|| format!("{wall:.2}"))),
            ("images reported", count("images")),
            ("exec count", count("execs")),
            ("exec time, total (ms)", ms(self.get("exec_ns"))),
            ("exec time, mean (ms)", mean_ms("exec_ns", "execs")),
            ("posix_spawn count", count("spawns")),
            ("posix_spawn time, mean (ms)", mean_ms("spawn_ns", "spawns")),
            (
                "share of wall time loading programs",
                share("exec_ns", "spawn_ns"),
            ),
            ("fork count", count("forks")),
            (
                "snapshot_process time, total (ms)",
                ms(self.get("snapshot_ns")),
            ),
            (
                "snapshot_process time, mean (ms)",
                mean_ms("snapshot_ns", "forks"),
            ),
            (
                "fork time in the parent, mean (ms)",
                mean_ms("fork_ns", "forks"),
            ),
            ("broker round trips", count("broker")),
            (
                "broker round trips per child (prepare, commit, reap)",
                round_trips,
            ),
            (
                "share and seek requests",
                match (self.get("shares"), self.get("seeks")) {
                    (Some(a), Some(b)) => Some(format!("{a} and {b}")),
                    _ => None,
                },
            ),
            ("path resolutions", count("resolutions")),
            (
                "path resolution, mean (us)",
                ratio("resolve_ns", "resolutions", 1e-3, ""),
            ),
            ("system calls", count("syscalls")),
            (
                "request polls' share of system calls",
                ratio("takes", "syscalls", 100.0, " %"),
            ),
            ("wake-ups per second", per_second("waits")),
            ("largest spawn buffer (bytes)", count("max_spawn_buffer")),
            ("peak open fds in a process", count("max_fds")),
            ("peak live pids", count("peak_pids")),
            (
                "peak /tmp (KiB)",
                self.get("peak_tmp_bytes").map(|b| (b / 1024).to_string()),
            ),
        ]
    }
}

/// INV-0110-3: what a W4 report must carry.
pub const W4_REQUIRED: &[&str] = &[
    "forks",
    "snapshot_ns",
    "execs",
    "exec_ns",
    "peak_pids",
    "peak_tmp_bytes",
];

/// The counters of `required` a session lacks, and "accelerator" when its
/// name gives none.
pub fn missing(s: &Session, required: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = required
        .iter()
        .filter(|k| s.get(k).is_none())
        .map(|k| k.to_string())
        .collect();
    if s.accelerator().is_none() {
        out.push("accelerator".into());
    }
    out
}

/// A markdown table: one row a figure, one column a session, then each
/// session's time marks.
pub fn table(sessions: &[Session]) -> String {
    let mut out = String::from("| |");
    for s in sessions {
        out.push_str(&format!(" {} |", s.name));
    }
    out.push_str("\n|---|");
    out.push_str(&"---|".repeat(sessions.len()));
    out.push('\n');
    let rows: Vec<Vec<(&str, Option<String>)>> = sessions.iter().map(Session::figures).collect();
    if let Some(first) = rows.first() {
        for (i, (name, _)) in first.iter().enumerate() {
            out.push_str(&format!("| {name} |"));
            for r in &rows {
                out.push_str(&format!(" {} |", r[i].1.as_deref().unwrap_or("–")));
            }
            out.push('\n');
        }
    }
    for s in sessions {
        if !s.times.is_empty() {
            let marks: Vec<String> = s
                .times
                .iter()
                .map(|(k, v)| format!("{k} {v:.2} s"))
                .collect();
            out.push_str(&format!("\n{}: {}\n", s.name, marks.join(", ")));
        }
    }
    out
}

/// The names the call log's bitmap stands for, in order: one a line of
/// `zoraya-calls.txt`.
pub fn call_names(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// FNV-1a, 64 bits, over the names each followed by a newline, as
/// `posix-surface-check calllog` puts it into `libc.a`.
pub fn list_hash(names: &[String]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for n in names {
        for &b in n.as_bytes().iter().chain(b"\n") {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// The names whose bit is set in any of `bitmaps`.
pub fn called(names: &[String], bitmaps: &[&[u8]]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (i, n) in names.iter().enumerate() {
        if bitmaps
            .iter()
            .any(|b| b.get(i / 8).is_some_and(|byte| byte >> (i % 8) & 1 != 0))
        {
            out.insert(n.clone());
        }
    }
    out
}

/// The tier list's names (`posix-surface-T1.txt`: one a line, `#` comments).
pub fn tier_list(text: &str) -> BTreeSet<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| l.split_whitespace().next().unwrap_or(l).to_string())
        .collect()
}

/// INV-0308-3: the thread functions (the names of `docs/adr/posix-surface-T9.txt`)
/// that the union of the runs called. The workloads are built without
/// `-pthread`, so there must be none.
pub fn thread_calls(t9: &BTreeSet<String>, union: &BTreeSet<String>) -> Vec<String> {
    t9.iter().filter(|n| union.contains(*n)).cloned().collect()
}

/// The functions TASK-02 and TASK-02b saw called by the T0 and T1 tools:
/// TASK-02's lower bound, the `core` runs of tiers T0 and T1 in
/// `dynamic-programs.tsv` by measured programs (`built:` products are not
/// counted), which is 134 names; and the six TASK-02b added
/// (`task-02b/parallel-surface.md`, "New libc functions").
pub fn measured(dynamic_tsv: &str) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = dynamic_tsv
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            match f.as_slice() {
                [_, "core", "T0" | "T1", program, symbol] if !program.starts_with("built:") => {
                    Some(symbol.to_string())
                }
                _ => None,
            }
        })
        .collect();
    out.extend(TASK_02B_ADDED.iter().map(|s| s.to_string()));
    out
}

/// TASK-02b's functions new to TASK-02's lower bound.
pub const TASK_02B_ADDED: &[&str] = &["kill", "mkfifo", "pselect", "stpncpy", "ttyname", "writev"];

/// INV-0005-3 and INV-0110-2 over the union of the runs' calls.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CallReport {
    /// Tier-list functions TASK-02 saw called and no run here called: the
    /// check fails on any.
    pub uncalled_measured: Vec<String>,
    /// Tier-list functions TASK-02 did not see called (`static-only`) and
    /// no run here called.
    pub uncalled_static_only: Vec<String>,
    /// Measured functions no run called that an ADR exempts, with a reason
    /// (ADR-0211 item 16).
    pub uncalled_exempt: Vec<String>,
    /// Tier-list functions called here that TASK-02 never saw called.
    pub called_not_measured: Vec<String>,
    /// Functions called here outside the tier list (the added blocks).
    pub called_outside_list: Vec<String>,
    /// Tier-list names that are not functions of the call log (data).
    pub not_logged: Vec<String>,
    /// Of those, the ones TASK-02 saw called: a measured name the call log
    /// cannot see fails the check (review R7).
    pub measured_not_logged: Vec<String>,
    /// Exemptions for functions that were called after all: stale.
    pub stale_exemptions: Vec<String>,
}

impl CallReport {
    pub fn passed(&self) -> bool {
        self.uncalled_measured.is_empty() && self.measured_not_logged.is_empty()
    }
}

/// The names of an ADR's `posix-calls-exempt` block: the first word of each
/// line, which must give a reason after it. No block is an error, so that a
/// wrong file is not silently empty.
pub fn exempt_names(adr: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = adr.lines().map(str::trim);
    if !lines.any(|l| l == "```posix-calls-exempt") {
        return Err("no posix-calls-exempt block".into());
    }
    let mut out = BTreeSet::new();
    for l in lines {
        if l == "```" {
            return Ok(out);
        }
        if l.is_empty() {
            continue;
        }
        match l.split_once(' ') {
            Some((name, reason)) if !reason.trim().is_empty() => {
                out.insert(name.to_string());
            }
            _ => return Err(format!("an exemption without a reason: {l}")),
        }
    }
    Err("unterminated posix-calls-exempt block".into())
}

pub fn check_calls(
    list: &BTreeSet<String>,
    logged: &[String],
    measured: &BTreeSet<String>,
    exempt: &BTreeSet<String>,
    called: &BTreeSet<String>,
) -> CallReport {
    let logged_set: BTreeSet<&String> = logged.iter().collect();
    let mut r = CallReport::default();
    for n in list {
        if !logged_set.contains(n) {
            r.not_logged.push(n.clone());
            if measured.contains(n) {
                r.measured_not_logged.push(n.clone());
            }
        } else if !called.contains(n) {
            if measured.contains(n) && exempt.contains(n) {
                r.uncalled_exempt.push(n.clone());
            } else if measured.contains(n) {
                r.uncalled_measured.push(n.clone());
            } else {
                r.uncalled_static_only.push(n.clone());
            }
        } else if !measured.contains(n) {
            r.called_not_measured.push(n.clone());
        }
    }
    r.called_outside_list = called
        .iter()
        .filter(|n| !list.contains(*n))
        .cloned()
        .collect();
    r.stale_exemptions = exempt
        .iter()
        .filter(|n| called.contains(*n))
        .cloned()
        .collect();
    r
}

#[cfg(test)]
mod tests;
