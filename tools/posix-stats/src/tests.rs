use super::*;

const LOG: &str = "\
[posix-w8-tcg] PSURF> sh /tests/w8.sh
[posix-w8-tcg] RUNNER-TIME boot 3.500
[posix-w8-tcg] W8-OK w8-j1
[posix-w8-tcg] RUNNER-TIME w8-j1 10.000
[posix-w8-tcg] RUNNER-TIME w8-pipe-j2 6.000
[posix-w8-tcg] POSIX-STATS images=10
[posix-w8-tcg] POSIX-STATS execs=4
[posix-w8-tcg] POSIX-STATS exec_ns=800000000
[posix-w8-tcg] POSIX-STATS spawns=10
[posix-w8-tcg] POSIX-STATS spawn_ns=2400000000
[posix-w8-tcg] POSIX-STATS forks=2
[posix-w8-tcg] POSIX-STATS snapshot_ns=10000000
[posix-w8-tcg] POSIX-STATS prepares=12
[posix-w8-tcg] POSIX-STATS commits=12
[posix-w8-tcg] POSIX-STATS reaps=12
[posix-w8-tcg] POSIX-STATS takes=50
[posix-w8-tcg] POSIX-STATS syscalls=1000
[posix-w8-tcg] POSIX-STATS waits=160
[posix-w8-tcg] POSIX-STATS peak_pids=7
[posix-w8-tcg] POSIX-STATS peak_tmp_bytes=65536
[posix-w8-tcg] POSIX-CALLS 0503
[posix-w8-tcg] BROKER-STATS prepares=12
";

#[test]
fn a_log_gives_its_counters_calls_and_times() {
    let s = parse("posix-w8-tcg", LOG).unwrap();
    assert_eq!(s.counters["execs"], 4);
    assert_eq!(s.counters.len(), 15);
    assert_eq!(s.calls.as_deref(), Some(&[0x05, 0x03][..]));
    assert_eq!(s.times.len(), 3);
    assert_eq!(s.wall(), 16.0, "every mark but the boot's");
    assert_eq!(s.accelerator(), Some("tcg"));
    assert_eq!(
        parse("plain", "POSIX-STATS x=1\n").unwrap().counters["x"],
        1
    );
}

#[test]
fn bad_or_doubled_lines_are_errors() {
    assert!(parse("l", "POSIX-STATS x\n").is_err());
    assert!(parse("l", "POSIX-STATS x=y\n").is_err());
    assert!(parse("l", "POSIX-CALLS 0g\n").is_err());
    assert!(parse("l", "POSIX-CALLS 012\n").is_err());
    assert!(parse("l", "POSIX-STATS x=1\nPOSIX-STATS x=2\n").is_err());
    assert!(parse("l", "POSIX-CALLS 00\nPOSIX-CALLS 00\n").is_err());
    assert!(parse("l", "RUNNER-TIME w1 fast\n").is_err());
    assert_eq!(unhex(""), Some(Vec::new()));
}

#[test]
fn the_figures_are_derived_from_the_counters() {
    let s = parse("posix-w8-tcg", LOG).unwrap();
    let f: BTreeMap<&str, Option<String>> = s.figures().into_iter().collect();
    let get = |k: &str| f[k].clone().unwrap_or_default();
    assert_eq!(get("wall time (s)"), "16.00");
    assert_eq!(get("exec time, mean (ms)"), "200.000");
    assert_eq!(get("posix_spawn time, mean (ms)"), "240.000");
    assert_eq!(get("share of wall time loading programs"), "20 %");
    assert_eq!(get("snapshot_process time, mean (ms)"), "5.000");
    assert_eq!(
        get("broker round trips per child (prepare, commit, reap)"),
        "3.00"
    );
    assert_eq!(get("request polls' share of system calls"), "5.00 %");
    assert_eq!(get("wake-ups per second"), "10");
    assert_eq!(get("peak /tmp (KiB)"), "64");
    assert_eq!(f["path resolutions"], None, "missing, not zero");
    let t = table(&[s]);
    assert!(t.contains("| exec count | 4 |"));
    assert!(t.contains("| path resolutions | – |"));
    assert!(t.contains("posix-w8-tcg: boot 3.50 s, w8-j1 10.00 s, w8-pipe-j2 6.00 s"));
}

/// INV-0110-3: a W4 report missing a counter or the accelerator is not one.
#[test]
fn a_w4_report_needs_every_counter_and_the_accelerator() {
    let s = parse("posix-w8-tcg", LOG).unwrap();
    assert!(missing(&s, W4_REQUIRED).is_empty());
    let mut t = s.clone();
    t.counters.remove("peak_tmp_bytes");
    t.name = "w4".into();
    assert_eq!(missing(&t, W4_REQUIRED), ["peak_tmp_bytes", "accelerator"]);
}

#[test]
fn calls_are_decoded_bit_by_bit_and_joined() {
    let names: Vec<String> = call_names("a\nb\nc\n\nd\ne\nf\ng\nh\ni\nj\n");
    assert_eq!(names.len(), 10);
    let got = called(&names, &[&[0x05], &[0x00, 0x02]]);
    let want: BTreeSet<String> = ["a", "c", "j"].iter().map(|s| s.to_string()).collect();
    assert_eq!(got, want);
    assert!(called(&names, &[]).is_empty());
}

#[test]
fn the_measured_set_is_task_02s_lower_bound_and_task_02bs_six() {
    let tsv = "run\tkind\ttier\tprogram\tsymbol\n\
        W1-make\tcore\tT1\tgmake\topen\n\
        W1-check\tcore\tT1\tbuilt:tcc\tfputc\n\
        W2-make-p\tcomparison\tT1\tpdpmake\tglob\n\
        W4-make\tcore\tT2\tgmake\tmkdir\n\
        W5-dash\tcore\tT0\tdash\tread\n";
    let m = measured(tsv);
    for n in ["open", "read", "kill", "writev"] {
        assert!(m.contains(n), "{n}");
    }
    for n in ["fputc", "glob", "mkdir"] {
        assert!(!m.contains(n), "{n}");
    }
}

/// The real table: TASK-02's lower bound is 134 names (`posix-surface.md`).
#[test]
fn the_real_lower_bound_has_134_names() {
    let tsv = include_str!("../../posix-surface/results/dynamic-programs.tsv");
    let m = measured(tsv);
    let added: BTreeSet<&str> = TASK_02B_ADDED.iter().copied().collect();
    let without: Vec<&String> = m.iter().filter(|n| !added.contains(n.as_str())).collect();
    assert_eq!(without.len(), 134);
}

#[test]
fn the_union_fails_only_on_a_measured_function_never_called() {
    let set = |v: &[&str]| -> BTreeSet<String> { v.iter().map(|s| s.to_string()).collect() };
    let list = set(&["open", "read", "fork", "environ", "kill"]);
    let logged: Vec<String> = ["fork", "kill", "open", "read", "glob"]
        .map(String::from)
        .to_vec();
    let measured = set(&["open", "read", "kill"]);
    let none = BTreeSet::new();
    let r = check_calls(
        &list,
        &logged,
        &measured,
        &none,
        &set(&["open", "kill", "fork", "glob"]),
    );
    assert_eq!(r.uncalled_measured, ["read"]);
    assert!(!r.passed());
    assert_eq!(r.called_not_measured, ["fork"]);
    assert_eq!(r.called_outside_list, ["glob"]);
    assert_eq!(r.not_logged, ["environ"]);
    let r = check_calls(
        &list,
        &logged,
        &measured,
        &none,
        &set(&["open", "read", "kill"]),
    );
    assert!(r.passed());
    assert_eq!(r.uncalled_static_only, ["fork"]);
    // An exemption passes a measured name, and only that one.
    let r = check_calls(&list, &logged, &measured, &set(&["read"]), &set(&["open"]));
    assert_eq!(r.uncalled_exempt, ["read"]);
    assert_eq!(r.uncalled_measured, ["kill"]);
}

#[test]
fn exemptions_come_from_an_adrs_block_each_with_a_reason() {
    let adr = "x\n```posix-calls-exempt\nvfork the patch\n\ngetopt_long a build\n```\n";
    let want: BTreeSet<String> = ["getopt_long", "vfork"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(exempt_names(adr).unwrap(), want);
    assert!(exempt_names("no block").is_err());
    assert!(
        exempt_names("```posix-calls-exempt\nvfork\n```\n").is_err(),
        "no reason"
    );
    assert!(exempt_names("```posix-calls-exempt\nvfork why\n").is_err());
    let real = include_str!("../../../docs/adr/0211-acceptance-choices-of-stage-6e.md");
    assert_eq!(exempt_names(real).unwrap().len(), 3);
}
