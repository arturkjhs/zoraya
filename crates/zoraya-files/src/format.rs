//! Sizes and dates as the list and the status line write them: `412 B`,
//! `1.9 kB`, `412 MB` (decimal, one decimal under 10); `Today 14:20`,
//! `Yesterday 16:30`, `3 Nov 2026` (UTC: the system has no time zone yet).

use alloc::format;
use alloc::string::String;

/// A size in bytes.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["kB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    let mut v = bytes;
    let mut unit = 0;
    // Tenths of the unit, rounded, without floats.
    let mut tenths = bytes * 10 / 1000;
    v /= 1000;
    while v >= 1000 && unit + 1 < UNITS.len() {
        tenths = v * 10 / 1000;
        v /= 1000;
        unit += 1;
    }
    if v < 10 {
        format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
    } else {
        format!("{v} {}", UNITS[unit])
    }
}

const NS_PER_DAY: u64 = 86_400 * 1_000_000_000;
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// The civil date of a day count since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// A wall-clock time as `YYYY-MM-DD HH:MM:SS UTC` (ADR-0226's examples).
pub fn utc(nanos: u64) -> String {
    let (y, m, d) = civil((nanos / NS_PER_DAY) as i64);
    let secs = (nanos % NS_PER_DAY) / 1_000_000_000;
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02} UTC",
        secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// A modification time against `now` (both nanoseconds since the epoch;
/// 0 is unknown).
pub fn date(at: u64, now: u64) -> String {
    if at == 0 {
        return String::from("\u{2014}");
    }
    let (day, now_day) = (at / NS_PER_DAY, now / NS_PER_DAY);
    let secs = (at % NS_PER_DAY) / 1_000_000_000;
    let time = format!("{:02}:{:02}", secs / 3600, secs / 60 % 60);
    if now != 0 && day == now_day {
        format!("Today {time}")
    } else if now != 0 && day + 1 == now_day {
        format!("Yesterday {time}")
    } else {
        let (y, m, d) = civil(day as i64);
        format!("{d} {} {y}", MONTHS[(m - 1) as usize])
    }
}
