//! Prints `PID KIB` for each PID given: the memory the process holds.
//!
//! On macOS this is the physical footprint (`proc_pid_rusage`,
//! `ri_phys_footprint`), which counts the process's compressed pages: a
//! process whose memory compresses well (a vector of one repeated value) can
//! hold gigabytes while `ps` shows a small resident size, which is how the
//! TASK-20 mutation run froze the host (docs/backlog.md). Elsewhere it is the
//! resident size from `/proc/PID/statm`. A process that is gone prints
//! nothing.
//!
//! Usage: `zoraya-footprint PID...`

fn main() {
    for arg in std::env::args().skip(1) {
        let Ok(pid) = arg.parse::<i32>() else {
            eprintln!("zoraya-footprint: not a pid: {arg}");
            std::process::exit(2);
        };
        if let Some(bytes) = footprint(pid) {
            println!("{pid} {}", bytes / 1024);
        }
    }
}

#[cfg(target_os = "macos")]
fn footprint(pid: i32) -> Option<u64> {
    /// `struct rusage_info_v0` of `<sys/resource.h>`.
    #[repr(C)]
    #[derive(Default)]
    struct RusageInfoV0 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
    }
    const RUSAGE_INFO_V0: i32 = 0;
    unsafe extern "C" {
        // libproc, in libSystem.
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageInfoV0) -> i32;
    }
    let mut info = RusageInfoV0::default();
    // SAFETY: `info` is a `rusage_info_v0`, the flavour asked for, which the
    // call fills and does not keep.
    let r = unsafe { proc_pid_rusage(pid, RUSAGE_INFO_V0, &mut info) };
    (r == 0).then_some(info.phys_footprint)
}

#[cfg(not(target_os = "macos"))]
fn footprint(pid: i32) -> Option<u64> {
    // The resident pages, the second field of statm, times the page size.
    let statm = std::fs::read_to_string(format!("/proc/{pid}/statm")).ok()?;
    let pages: u64 = statm.split_whitespace().nth(1)?.parse().ok()?;
    Some(pages * 4096)
}

#[cfg(test)]
mod tests {
    #[test]
    fn this_process_has_a_footprint() {
        let me = super::footprint(std::process::id() as i32).unwrap();
        assert!(me > 0);
        let hold = vec![1u8; 64 << 20];
        let more = super::footprint(std::process::id() as i32).unwrap();
        assert!(more >= me + (48 << 20), "{me} then {more}");
        drop(hold);
        assert_eq!(super::footprint(-1), None);
    }
}
