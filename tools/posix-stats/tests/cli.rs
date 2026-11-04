//! The command line: exit statuses and what fails (ADR-0110 items 8 and 10).

use std::path::PathBuf;
use std::process::Command;

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("posix-stats-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_posix-stats"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

const W4: &str = "POSIX-STATS forks=2\nPOSIX-STATS snapshot_ns=10\nPOSIX-STATS execs=1\n\
POSIX-STATS exec_ns=5\nPOSIX-STATS peak_pids=3\nPOSIX-STATS peak_tmp_bytes=4096\n";

#[test]
fn table_prints_and_w4_fails_without_a_counter_or_the_accelerator() {
    let d = dir("table");
    let good = d.join("posix-w4-tcg.log");
    std::fs::write(&good, W4).unwrap();
    let no_accel = d.join("w4.log");
    std::fs::write(&no_accel, W4).unwrap();
    let short = d.join("posix-w4-hvf.log");
    std::fs::write(&short, "POSIX-STATS forks=2\n").unwrap();
    let (ok, out, _) = run(&["table", good.to_str().unwrap()]);
    assert!(ok);
    assert!(out.contains("| fork count | 2 |"));
    assert!(run(&["table", "--w4", good.to_str().unwrap()]).0);
    let (ok, _, err) = run(&["table", "--w4", no_accel.to_str().unwrap()]);
    assert!(!ok && err.contains("accelerator"), "{err}");
    let (ok, _, err) = run(&["table", "--w4", short.to_str().unwrap()]);
    assert!(!ok && err.contains("peak_pids"), "{err}");
    assert!(
        run(&["table", "--w4", short.to_str().unwrap()])
            .1
            .contains("| fork count | 2 |")
    );
    assert!(!run(&["table", "--w4"]).0);
    assert!(!run(&["table", d.join("missing.log").to_str().unwrap()]).0);
    assert!(!run(&["nothing"]).0);
    assert!(!run(&[]).0);
}

#[test]
fn calls_pass_fail_and_refuse_as_they_should() {
    let d = dir("calls");
    let tsv = d.join("dynamic.tsv");
    std::fs::write(
        &tsv,
        "run\tkind\ttier\tprogram\tsymbol\nW1\tcore\tT1\tgmake\topen\nW1\tcore\tT1\tgmake\tread\n",
    )
    .unwrap();
    let list = d.join("list.txt");
    std::fs::write(&list, "# list\nopen\nread\nenviron\n").unwrap();
    let exempt = d.join("adr.md");
    std::fs::write(&exempt, "```posix-calls-exempt\nwritev a reason\n```\n").unwrap();
    let names = d.join("calls.txt");
    // The order: kill (0), open (1), read (2), writev (3).
    std::fs::write(&names, "kill\nopen\nread\nwritev\n").unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    let both = d.join("posix-w8-auto.log");
    std::fs::write(&both, "POSIX-CALLS 06\n").unwrap();
    let only_open = d.join("posix-w5-tcg.log");
    std::fs::write(&only_open, "POSIX-CALLS 02\n").unwrap();
    let none = d.join("posix-w10-tcg.log");
    std::fs::write(&none, "POSIX-STATS images=1\n").unwrap();
    let a = |logs: &[&PathBuf], req: &[&str]| {
        let mut v: Vec<String> = vec![
            "--calls".into(),
            names.display().to_string(),
            list.display().to_string(),
            tsv.display().to_string(),
            exempt.display().to_string(),
        ];
        v.extend(logs.iter().map(|l| l.display().to_string()));
        if !req.is_empty() {
            v.push("--require".into());
            v.extend(req.iter().map(|s| s.to_string()));
        }
        v
    };
    let call = |v: Vec<String>| run(&v.iter().map(String::as_str).collect::<Vec<_>>());
    let (ok, out, err) = call(a(&[&both], &["w8"]));
    assert!(ok, "{out}{err}");
    assert!(
        out.contains("calls over 1 runs: 2 functions called"),
        "{out}"
    );
    let (ok, _, err) = call(a(&[&only_open], &[]));
    assert!(
        !ok && err.contains("INV-0005-3"),
        "read never called: {err}"
    );
    let (ok, _, err) = call(a(&[&both], &["w8", "w5"]));
    assert!(!ok && err.contains("no log of w5"), "{err}");
    let (ok, _, err) = call(a(&[&both, &none], &[]));
    assert!(!ok && err.contains("no POSIX-CALLS"), "{err}");
    // A log older than the call list is refused.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(&names, "kill\nopen\nread\nwritev\n").unwrap();
    let (ok, _, err) = call(a(&[&both], &[]));
    assert!(!ok && err.contains("older than"), "{err}");
    assert!(!run(&["--calls", "x"]).0);
}
