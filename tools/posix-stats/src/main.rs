//! `posix-stats` (ADR-0110 items 7, 8, 10 and 11).
//!
//! - `table [--w4] LOG...` prints the stage 6 table of the logs' sessions;
//!   with `--w4` it fails unless each carries INV-0110-3's counters and names
//!   its accelerator.
//! - `--calls NAMES LIST TSV EXEMPT LOG... [--require WORKLOAD...]` joins the logs'
//!   POSIX-CALLS (NAMES is `usr/lib/zoraya-calls.txt`), compares them with
//!   the tier list LIST and TASK-02's measured calls TSV
//!   (`tools/posix-surface/results/dynamic-programs.tsv`), prints what was
//!   called and not measured and what was measured and not called, and fails
//!   when a measured tier-list function was never called (INV-0005-3,
//!   INV-0110-2), unless EXEMPT (an ADR's `posix-calls-exempt` block, proposed
//!   ADR-0211 item 16) names it, or a required workload's log is missing.

use std::process::ExitCode;

use posix_stats::{
    W4_REQUIRED, call_names, called, check_calls, exempt_names, list_hash, measured, missing,
    parse, table, thread_calls, tier_list,
};

fn read(path: &str) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))
}

/// A log's session, named by the file's stem.
fn session(path: &str) -> Result<posix_stats::Session, String> {
    let name = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    parse(&name, &read(path)?)
}

fn run_table(args: &[String]) -> Result<bool, String> {
    let (w4, logs) = match args.split_first() {
        Some((f, rest)) if f == "--w4" => (true, rest),
        _ => (false, args),
    };
    if logs.is_empty() {
        return Err("usage: table [--w4] LOG...".into());
    }
    let sessions: Vec<_> = logs.iter().map(|l| session(l)).collect::<Result<_, _>>()?;
    print!("{}", table(&sessions));
    let mut ok = true;
    if w4 {
        for s in &sessions {
            let m = missing(s, W4_REQUIRED);
            if !m.is_empty() {
                eprintln!(
                    "posix-stats: {} is not a W4 report: no {}",
                    s.name,
                    m.join(", ")
                );
                ok = false;
            }
        }
    }
    Ok(ok)
}

fn run_calls(args: &[String]) -> Result<bool, String> {
    let [names, list, tsv, exempt, rest @ ..] = args else {
        return Err("usage: --calls NAMES LIST TSV EXEMPT LOG... [--require WORKLOAD...]".into());
    };
    let exempt = exempt_names(&read(exempt)?).map_err(|e| format!("{exempt}: {e}"))?;
    // `--no-thread-calls T9` (INV-0308-3) may precede `--require`.
    let (rest, t9) = match rest.iter().position(|a| a == "--no-thread-calls") {
        Some(i) if i + 1 < rest.len() => {
            let t9 = read(&rest[i + 1])?;
            let mut v = rest[..i].to_vec();
            v.extend_from_slice(&rest[i + 2..]);
            (v, Some(tier_list(&t9)))
        }
        _ => (rest.to_vec(), None),
    };
    let rest = &rest[..];
    let (logs, required) = match rest.iter().position(|a| a == "--require") {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &[][..]),
    };
    let mut ok = true;
    for w in required {
        let named = |l: &&String| {
            ["auto", "hvf", "tcg"]
                .iter()
                .any(|a| l.ends_with(&format!("posix-{w}-{a}.log")))
        };
        if !logs.iter().any(|l| named(&l)) {
            eprintln!(
                "posix-stats: no log of {w}: run make test-posix and make test-posix-long of the same build first"
            );
            ok = false;
        }
    }
    let names = call_names(&read(names)?);
    let sessions: Vec<_> = logs.iter().map(|l| session(l)).collect::<Result<_, _>>()?;
    // The bitmaps' order is the list's of the build that wrote them: each
    // session's max_calls_list is that list's hash (review C3).
    let want = list_hash(&names);
    for s in &sessions {
        match s.counters.get("max_calls_list") {
            Some(&h) if h == want => {}
            got => {
                eprintln!(
                    "posix-stats: {} was written by another libc.a's call log (list hash {got:x?}, this list's {want:x}): run its make target again",
                    s.name
                );
                ok = false;
            }
        }
    }
    let bitmaps: Vec<&[u8]> = sessions.iter().filter_map(|s| s.calls.as_deref()).collect();
    if bitmaps.len() != sessions.len() {
        for s in sessions.iter().filter(|s| s.calls.is_none()) {
            eprintln!("posix-stats: {} has no POSIX-CALLS line", s.name);
        }
        ok = false;
    }
    let union = called(&names, &bitmaps);
    let r = check_calls(
        &tier_list(&read(list)?),
        &names,
        &measured(&read(tsv)?),
        &exempt,
        &union,
    );
    println!(
        "calls over {} runs: {} functions called",
        sessions.len(),
        union.len()
    );
    let show = |what: &str, v: &[String]| println!("{what} ({}): {}", v.len(), v.join(" "));
    show("called, never measured by TASK-02", &r.called_not_measured);
    show(
        "measured by TASK-02, never called (fails)",
        &r.uncalled_measured,
    );
    show(
        "measured, never called, exempted (ADR-0211 item 16)",
        &r.uncalled_exempt,
    );
    show("static-only, never called", &r.uncalled_static_only);
    show("called, outside the tier list", &r.called_outside_list);
    show("tier-list data names, not logged", &r.not_logged);
    show("measured, not logged (fails)", &r.measured_not_logged);
    if !r.stale_exemptions.is_empty() {
        eprintln!(
            "posix-stats: warning: exempted but called, so the exemption is stale: {}",
            r.stale_exemptions.join(" ")
        );
    }
    if let Some(t9) = &t9 {
        let threads = thread_calls(t9, &union);
        show("thread functions called (fails)", &threads);
        if !threads.is_empty() {
            eprintln!("posix-stats: INV-0308-3: a workload called thread functions");
            ok = false;
        }
    }
    if !r.passed() {
        eprintln!("posix-stats: INV-0005-3: measured tier-list functions never called");
        ok = false;
    }
    Ok(ok)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match args.split_first() {
        Some((c, rest)) if c == "table" => run_table(rest),
        Some((c, rest)) if c == "--calls" => run_calls(rest),
        _ => Err("usage: posix-stats {table|--calls} ...".into()),
    };
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("posix-stats: {e}");
            ExitCode::FAILURE
        }
    }
}
