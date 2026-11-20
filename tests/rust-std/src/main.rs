//! `rstest MODE [ARG...]`: Rust's prebuilt `std` for `aarch64-unknown-linux-musl`
//! as a POSIX program on Zoraya (ADR-0450 to ADR-0454), with every check of the
//! probe of TASK-35 (`docs/research/rust-std/probes`) asserted. A mode prints a
//! line per check and ends with `rstest: MODE passed`, or exits 1 after the
//! first failing check's line (`rstest: CHECK FAIL ...`) and all that came
//! before it. A check that panics is caught and reported, so the failure line
//! says what it was.
//!
//! The binary is installed as `/bin/rstest` (the modes that need no network)
//! and as `/bin/rsnet`, whose description grants the connection to the URL on
//! the line (the `net` mode). The modes that start the program itself
//! (`process`) run `rstest child ...`.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::panic;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs, thread};

/// What a check returns: a note for the line, or the reason it failed.
type Outcome = Result<String, String>;

static FAILED: AtomicUsize = AtomicUsize::new(0);

fn check(name: &str, f: impl FnOnce() -> Outcome + panic::UnwindSafe) {
    match panic::catch_unwind(f) {
        Ok(Ok(note)) => println!("rstest: {name} ok {note}"),
        Ok(Err(e)) => fail(name, &e),
        Err(p) => fail(name, &format!("panicked: {}", payload(&p))),
    }
}

fn fail(name: &str, why: &str) {
    FAILED.fetch_add(1, Ordering::SeqCst);
    println!("rstest: {name} FAIL {why}");
}

fn payload(p: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "(not a string)".into()
    }
}

fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}

/// `Ok(note)` if `cond`, else the reason.
fn expect(cond: bool, note: impl FnOnce() -> String) -> Outcome {
    if cond { Ok(String::new()) } else { Err(note()) }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let name = args.first().map(String::as_str).unwrap_or("rstest");
    // Installed as /bin/rsnet it is run under rsnet's description, which
    // grants the connection to the URL operand: the net mode.
    let as_rsnet = name.ends_with("rsnet");
    let mode = if as_rsnet { "net" } else { args.get(1).map(String::as_str).unwrap_or("hello") };
    // Quiet panics inside checks (the check line says what happened); an
    // uncaught panic and a child's keep std's own message.
    if !matches!(mode, "uncaught" | "child") {
        panic::set_hook(Box::new(|_| {}));
    }
    let rest: &[String] = if as_rsnet { &args[1..] } else { args.get(2..).unwrap_or(&[]) };
    match mode {
        "hello" => hello(&args),
        "syscall" => raw_syscall(),
        "threads" => threads(),
        "hash" => hash(),
        "time" => time(),
        "fs" => fs_checks(rest.first().map(String::as_str).unwrap_or("/tmp")),
        "denied" => denied(rest.first().map(String::as_str).unwrap_or("/work/secret")),
        "net" => net(rest),
        "process" => process(),
        "child" => child(rest),
        "panic" => panics(),
        "uncaught" => uncaught(),
        "abort" => std::process::abort(),
        "overflow" => overflow(),
        other => {
            println!("rstest: unknown mode {other}");
            std::process::exit(2);
        }
    }
    let failed = FAILED.load(Ordering::SeqCst);
    if failed > 0 {
        println!("rstest: {mode} FAILED ({failed} checks)");
        std::process::exit(1);
    }
    println!("rstest: {mode} passed");
}

fn hello(args: &[String]) {
    println!("hello from Rust std");
    check("args", || expect(args.len() >= 2, || format!("{args:?}")));
    check("env", || {
        // SAFETY: single-threaded at this point.
        unsafe { env::set_var("RSTEST", "1") };
        match env::var("RSTEST") {
            Ok(v) if v == "1" => Ok("set_var works".into()),
            other => Err(format!("{other:?}")),
        }
    });
    check("stderr", || {
        eprintln!("rstest: to stderr");
        Ok(String::new())
    });
    check("exe", || {
        let x = env::current_exe().map_err(e)?;
        expect(x.ends_with("rstest") || x.ends_with("rsnet"), || format!("{x:?}"))
    });
}

unsafe extern "C" {
    #[link_name = "syscall"]
    fn libc_syscall(n: i64, ...) -> i64;
}

/// ADR-0451 item 1: `syscall()` is a `long` and answers the three numbers
/// and nothing else.
fn raw_syscall() {
    check("gettid", || {
        // SAFETY: gettid takes no arguments.
        let t = unsafe { libc_syscall(178) };
        expect(t > 0 && t < 1 << 30, || format!("{t}"))
    });
    check("unanswered-is-minus-one", || {
        // SAFETY: a number no table holds takes no arguments here.
        let r = unsafe { libc_syscall(9999) };
        let err = std::io::Error::last_os_error().raw_os_error();
        expect(r == -1 && err == Some(38), || format!("{r} {err:?}"))
    });
    check("statx-falls-back", || {
        // `metadata` reaches statx through syscall() first and must work all the same.
        let m = fs::metadata("/bin").map_err(e)?;
        expect(m.is_dir(), || "not a directory".into())
    });
    check("std-thread-id", || {
        // The id in a panic message is the thread's, not 18446744073709551615.
        let id = format!("{:?}", thread::current().id());
        expect(!id.contains("18446744073709551615"), || id)
    });
}

fn threads() {
    check("available_parallelism", || {
        thread::available_parallelism().map(|n| n.to_string()).map_err(e)
    });
    check("spawn-join", || {
        let hs: Vec<_> = (0..8).map(|i| thread::spawn(move || i * 2)).collect();
        let s: i32 = hs.into_iter().map(|h| h.join().unwrap()).sum();
        expect(s == 56, || format!("sum {s}"))
    });
    check("mutex-contended", || {
        let t = Instant::now();
        let m = Arc::new(Mutex::new(0u64));
        let hs: Vec<_> = (0..4)
            .map(|_| {
                let m = m.clone();
                thread::spawn(move || {
                    for _ in 0..20_000 {
                        *m.lock().unwrap() += 1;
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        let v = *m.lock().unwrap();
        expect(v == 80_000, || format!("{v}"))?;
        Ok(format!("80000 in {:?}", t.elapsed()))
    });
    check("condvar-pingpong", || {
        // With a futex that waits: 10 ms in the probe. With waits that return
        // at once, every round waits for the scheduler's tick: 37 s.
        let t = Instant::now();
        let p = Arc::new((Mutex::new(0u32), Condvar::new()));
        let q = p.clone();
        let h = thread::spawn(move || {
            let (m, c) = &*q;
            for i in 0..1000 {
                let mut g = m.lock().unwrap();
                while *g != 2 * i + 1 {
                    g = c.wait(g).unwrap();
                }
                *g += 1;
                c.notify_one();
            }
        });
        let (m, c) = &*p;
        for i in 0..1000 {
            let mut g = m.lock().unwrap();
            *g = 2 * i + 1;
            c.notify_one();
            while *g != 2 * i + 2 {
                g = c.wait(g).unwrap();
            }
        }
        h.join().map_err(|_| "join".to_string())?;
        let took = t.elapsed();
        expect(took < Duration::from_secs(3), || format!("1000 rounds took {took:?}"))?;
        Ok(format!("1000 rounds in {took:?}"))
    });
    check("condvar-wait_timeout", || {
        // Nobody notifies: a futex that waits returns after 200 ms, timed out.
        let m = Mutex::new(());
        let c = Condvar::new();
        let t = Instant::now();
        let mut spurious = 0;
        let mut g = m.lock().unwrap();
        loop {
            let (g2, r) = c.wait_timeout(g, Duration::from_millis(200)).unwrap();
            g = g2;
            if r.timed_out() {
                break;
            }
            spurious += 1;
            if t.elapsed() > Duration::from_secs(2) {
                return Err(format!("{spurious} spurious wake-ups in 2 s, never timed out"));
            }
        }
        let took = t.elapsed();
        expect(took >= Duration::from_millis(200) && spurious < 3, || {
            format!("{took:?}, {spurious} spurious")
        })?;
        Ok(format!("timed out after {took:?}"))
    });
    check("park-timeout", || {
        let t = Instant::now();
        thread::park_timeout(Duration::from_millis(100));
        let took = t.elapsed();
        expect(took >= Duration::from_millis(100) && took < Duration::from_secs(2), || format!("{took:?}"))
    });
    check("mpsc", || {
        let (tx, rx) = mpsc::channel();
        for i in 0..4 {
            let tx = tx.clone();
            thread::spawn(move || {
                for j in 0..100 {
                    tx.send(i * 100 + j).unwrap();
                }
            });
        }
        drop(tx);
        let n = rx.iter().count();
        expect(n == 400, || format!("{n}"))
    });
    check("rwlock-barrier", || {
        let l = Arc::new(RwLock::new(5));
        let b = Arc::new(Barrier::new(3));
        let hs: Vec<_> = (0..3)
            .map(|_| {
                let (l, b) = (l.clone(), b.clone());
                thread::spawn(move || {
                    b.wait();
                    *l.read().unwrap()
                })
            })
            .collect();
        let s: i32 = hs.into_iter().map(|h| h.join().unwrap()).sum();
        expect(s == 15, || format!("{s}"))
    });
    check("thread-name-and-local", || {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct D;
        impl Drop for D {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }
        thread_local!(static T: D = const { D });
        let name = thread::Builder::new()
            .name("worker".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                T.with(|_| ());
                thread::current().name().map(str::to_string)
            })
            .map_err(e)?
            .join()
            .map_err(|_| "join")?;
        let d = DROPS.load(Ordering::SeqCst);
        expect(name.as_deref() == Some("worker") && d == 1, || format!("name {name:?}, drops {d}"))
    });
    check("scoped", || {
        let mut v = vec![1, 2, 3];
        thread::scope(|s| {
            s.spawn(|| v.push(4));
        });
        expect(v.len() == 4, || format!("{v:?}"))
    });
}

fn hash() {
    check("hashmap", || {
        let mut m = HashMap::new();
        for i in 0..1000 {
            m.insert(i, i * i);
        }
        expect(m[&30] == 900, || "value".into())
    });
}

fn time() {
    check("systemtime", || {
        let s = SystemTime::now().duration_since(UNIX_EPOCH).map_err(e)?.as_secs();
        expect(s > 1_700_000_000, || format!("{s}"))
    });
    check("instant-sleep", || {
        let t = Instant::now();
        thread::sleep(Duration::from_millis(50));
        let d = t.elapsed();
        expect(d >= Duration::from_millis(50) && d < Duration::from_secs(2), || format!("{d:?}"))
    });
}

fn fs_checks(dir: &str) {
    let base = format!("{dir}/rstest");
    let _ = fs::remove_dir_all(&base);
    check("create_dir_all", || fs::create_dir_all(format!("{base}/a/b")).map(|_| String::new()).map_err(e));
    check("write-read", || {
        fs::write(format!("{base}/f.txt"), b"hello file\n").map_err(e)?;
        let s = fs::read_to_string(format!("{base}/f.txt")).map_err(e)?;
        expect(s == "hello file\n", || format!("{s:?}"))
    });
    check("append-seek-set_len", || {
        use std::io::{Seek, SeekFrom};
        let mut f = fs::OpenOptions::new().append(true).open(format!("{base}/f.txt")).map_err(e)?;
        f.write_all(b"more\n").map_err(e)?;
        let mut f = fs::OpenOptions::new().read(true).write(true).open(format!("{base}/f.txt")).map_err(e)?;
        f.seek(SeekFrom::Start(6)).map_err(e)?;
        let mut s = String::new();
        f.read_to_string(&mut s).map_err(e)?;
        f.set_len(5).map_err(e)?;
        f.sync_all().map_err(e)?;
        let len = fs::metadata(format!("{base}/f.txt")).map_err(e)?.len();
        expect(s == "file\nmore\n" && len == 5, || format!("{s:?} {len}"))
    });
    check("metadata", || {
        let m = fs::metadata(format!("{base}/f.txt")).map_err(e)?;
        let modified = m.modified().map_err(e)?;
        // Zoraya keeps no birth time: `created` is an error (a known limit, ADR-0453 item 2).
        expect(
            m.len() == 5 && m.is_file() && !m.is_dir() && modified.duration_since(UNIX_EPOCH).is_ok() && m.created().is_err(),
            || format!("{m:?}"),
        )
    });
    check("read_dir", || {
        let mut names: Vec<String> = fs::read_dir(&base)
            .map_err(e)?
            .map(|d| {
                let d = d.unwrap();
                format!("{}{}", d.file_name().to_string_lossy(), if d.file_type().unwrap().is_dir() { "/" } else { "" })
            })
            .collect();
        names.sort();
        expect(names == ["a/", "f.txt"], || format!("{names:?}"))
    });
    check("rename", || fs::rename(format!("{base}/f.txt"), format!("{base}/g.txt")).map(|_| String::new()).map_err(e));
    check("copy", || {
        // The probe's wrong copy ("copied 18446744073709551615") came from syscall().
        fs::write(format!("{base}/big"), vec![7u8; 300_000]).map_err(e)?;
        let n = fs::copy(format!("{base}/big"), format!("{base}/big2")).map_err(e)?;
        let same = fs::read(format!("{base}/big2")).map_err(e)? == vec![7u8; 300_000];
        expect(n == 300_000 && same, || format!("copied {n}, same {same}"))
    });
    check("symlink-read_link", || {
        std::os::unix::fs::symlink("g.txt", format!("{base}/link")).map_err(e)?;
        let t = fs::read_link(format!("{base}/link")).map_err(e)?;
        let through = fs::read_to_string(format!("{base}/link")).map_err(e)?;
        expect(t.to_str() == Some("g.txt") && through == "file\n", || format!("{t:?} {through:?}"))
    });
    check("hard_link-refused", || match fs::hard_link(format!("{base}/g.txt"), format!("{base}/h.txt")) {
        // ADR-0013: no hard links (EPERM).
        Err(x) if x.kind() == std::io::ErrorKind::PermissionDenied => Ok(format!("{x}")),
        other => Err(format!("{other:?}")),
    });
    check("set_permissions", || {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(format!("{base}/g.txt"), fs::Permissions::from_mode(0o600)).map_err(e)?;
        let m = fs::metadata(format!("{base}/g.txt")).map_err(e)?.permissions().mode();
        expect(m & 0o7777 == 0o600, || format!("mode {:o}", m & 0o7777))
    });
    check("canonicalize", || {
        let p = fs::canonicalize(format!("{base}/a/b/..")).map_err(e)?;
        expect(p.to_str() == Some(format!("{base}/a").as_str()), || format!("{p:?}"))
    });
    check("current_dir", || env::current_dir().map(|p| p.display().to_string()).map_err(e));
    check("current_exe", || {
        let x = env::current_exe().map_err(e)?;
        expect(x.ends_with("rstest"), || format!("{x:?}"))
    });
    check("temp_dir", || {
        let t = env::temp_dir();
        expect(t.to_str() == Some("/tmp"), || format!("{t:?}"))
    });
    check("not-found", || match fs::read(format!("{dir}/no-such-file")) {
        Err(x) if x.kind() == std::io::ErrorKind::NotFound => Ok(format!("{x}")),
        other => Err(format!("{other:?}")),
    });
    check("remove_dir_all", || {
        // The openat walk over dirfd (ADR-0451 item 3).
        fs::remove_dir_all(&base).map_err(e)?;
        expect(fs::metadata(&base).is_err(), || "still there".into())
    });
}

/// A directory the line does not grant: every operation is `NotFound` (ADR-0453 item 2).
fn denied(dir: &str) {
    check("denied-ops", || {
        let nf = |what: &str, k: std::io::Result<()>| match k {
            Err(x) if x.kind() == std::io::ErrorKind::NotFound => Ok(()),
            other => Err(format!("{what}: {other:?}")),
        };
        nf("read", fs::read(format!("{dir}/key.txt")).map(|_| ()))?;
        nf("read_dir", fs::read_dir(dir).map(|_| ()))?;
        nf("create", fs::write(format!("{dir}/new"), b"x"))?;
        nf("create_dir", fs::create_dir(format!("{dir}/d")))?;
        nf("metadata", fs::metadata(dir).map(|_| ()))?;
        Ok(String::new())
    });
}

fn net(args: &[String]) {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    // ARG: a URL http://HOST:PORT/PATH (so that the rsnet description grants
    // the connection to HOST:PORT and nothing else).
    let url = args.iter().find(|a| a.starts_with("http://")).cloned().unwrap_or_default();
    let rest = url.trim_start_matches("http://");
    let (hostport, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let hostport = hostport.to_string();
    let hp = hostport.clone();
    check("to_socket_addrs", move || {
        let a: Vec<SocketAddr> = hp.to_socket_addrs().map_err(e)?.collect();
        expect(!a.is_empty(), || "no address".into())
    });
    let hp = hostport.clone();
    check("tcp-get", move || {
        let t = Instant::now();
        let mut s = TcpStream::connect(&hp).map_err(e)?;
        s.set_nodelay(true).ok();
        let host = hp.split(':').next().unwrap_or("");
        write!(s, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").map_err(e)?;
        let mut body = Vec::new();
        s.read_to_end(&mut body).map_err(e)?;
        let first = String::from_utf8_lossy(&body).lines().next().unwrap_or("").to_string();
        expect(first.contains(" 200") && body.len() > 100, || format!("{first:?} {} bytes", body.len()))?;
        Ok(format!("{first:?}, {} bytes in {:?}", body.len(), t.elapsed()))
    });
    let hp = hostport.clone();
    check("connect_timeout", move || {
        // The non-blocking connect: ioctl(FIONBIO), connect, poll, SO_ERROR (ADR-0451 item 2).
        let addr = hp.to_socket_addrs().map_err(e)?.next().ok_or("no address")?;
        let s = TcpStream::connect_timeout(&addr, Duration::from_secs(5)).map_err(e)?;
        expect(s.peer_addr().is_ok(), || "no peer".into())
    });
    let hp = hostport.clone();
    check("set_nonblocking", move || {
        let mut s = TcpStream::connect(&hp).map_err(e)?;
        s.set_nonblocking(true).map_err(e)?;
        let mut b = [0u8; 1];
        match s.read(&mut b) {
            Err(x) if x.kind() == std::io::ErrorKind::WouldBlock => {}
            other => return Err(format!("read: {other:?}")),
        }
        s.set_nonblocking(false).map_err(e)?;
        Ok(String::new())
    });
    check("another-port-refused", || {
        // The grant names one host and port: another port is EACCES (ADR-0453 item 4).
        match TcpStream::connect("10.0.2.2:1") {
            Err(x) if x.kind() == std::io::ErrorKind::PermissionDenied => Ok(format!("{x}")),
            other => Err(format!("{other:?}")),
        }
    });
}

fn me() -> String {
    env::current_exe().map(|p| p.display().to_string()).unwrap_or_else(|_| "/bin/rstest".into())
}

fn process() {
    let me = me();
    check("status", || {
        let s = Command::new(&me).args(["child", "exit", "3"]).status().map_err(e)?;
        expect(s.code() == Some(3), || format!("{s}"))
    });
    check("output", || {
        // Command::output puts both pipes in non-blocking mode: ioctl(FIONBIO).
        let o = Command::new(&me).args(["child", "print"]).env("RSTEST_CHILD", "yes").output().map_err(e)?;
        expect(
            o.status.success() && o.stdout == b"out yes\n" && o.stderr == b"err\n",
            || format!("{} {:?} {:?}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)),
        )
    });
    check("stdin-pipe", || {
        let mut c = Command::new(&me).args(["child", "cat"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().map_err(e)?;
        c.stdin.take().unwrap().write_all(b"through a pipe").map_err(e)?;
        let o = c.wait_with_output().map_err(e)?;
        expect(o.stdout == b"through a pipe", || format!("{:?}", String::from_utf8_lossy(&o.stdout)))
    });
    check("kill", || {
        let mut c = Command::new(&me).args(["child", "sleep"]).spawn().map_err(e)?;
        thread::sleep(Duration::from_millis(100));
        c.kill().map_err(e)?;
        let s = c.wait().map_err(e)?;
        expect(s.signal() == Some(9), || format!("{s}"))
    });
    check("missing-program", || match Command::new("/bin/no-such-program").status() {
        Err(x) if x.kind() == std::io::ErrorKind::NotFound => Ok(format!("{x}")),
        other => Err(format!("{other:?}")),
    });
    // INV-0453-1: a child, started by posix_spawnp and by the fork path (a bare
    // name with PATH set), has a random connection of its own and no network.
    check("child-hashmap-spawn", || {
        let o = Command::new(&me).args(["child", "hash"]).output().map_err(e)?;
        expect(o.status.success() && o.stdout == b"hashmap 2", || {
            format!("{} {:?} {:?}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
        })
    });
    check("child-hashmap-fork-path", || {
        let o = Command::new("rstest").env("PATH", "/bin").args(["child", "hash"]).output().map_err(e)?;
        expect(o.status.success() && o.stdout == b"hashmap 2", || {
            format!("{} {:?} {:?}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
        })
    });
    check("child-no-network", || {
        for fork_path in [false, true] {
            let mut c = if fork_path { Command::new("rstest") } else { Command::new(&me) };
            if fork_path {
                c.env("PATH", "/bin");
            }
            let o = c.args(["child", "connect"]).output().map_err(e)?;
            expect(o.status.success() && o.stdout == b"connect refused", || {
                format!("fork path {fork_path}: {} {:?}", o.status, String::from_utf8_lossy(&o.stdout))
            })?;
        }
        Ok(String::new())
    });
    check("current_dir", || {
        // posix_spawn_file_actions_addchdir (ADR-0451 item 3).
        let o = Command::new(&me).args(["child", "cwd"]).current_dir("/tmp").output().map_err(e)?;
        expect(o.status.success() && o.stdout == b"/tmp", || format!("{} {:?}", o.status, String::from_utf8_lossy(&o.stdout)))
    });
    // INV-0453-2: a Rust program cannot change identity: the call fails, so
    // the child never runs.
    let marker = "/tmp/rstest-identity";
    let _ = fs::remove_file(marker);
    check("identity-control", || {
        let s = Command::new(&me).args(["child", "touch", marker]).status().map_err(e)?;
        expect(s.success() && fs::metadata(marker).is_ok(), || format!("{s}"))?;
        fs::remove_file(marker).map(|_| String::new()).map_err(e)
    });
    check("identity-uid", || never_runs(Command::new(&me).args(["child", "touch", marker]).uid(1000).spawn(), marker));
    check("identity-gid", || never_runs(Command::new(&me).args(["child", "touch", marker]).gid(1000).spawn(), marker));
    // `CommandExt::groups` and `chroot` are unstable in Rust 1.99.1 (`setgroups`,
    // `process_chroot`): they reach `setgroups` and `chroot`, which the Rust-only archive
    // fails with ENOSYS as well, and a stable program cannot call them.
}

/// A spawn that must have failed with `ENOSYS`, the child never having run.
fn never_runs(r: std::io::Result<std::process::Child>, marker: &str) -> Outcome {
    match r {
        Ok(mut c) => {
            let _ = c.wait();
            Err("the spawn succeeded".into())
        }
        Err(x) => {
            thread::sleep(Duration::from_millis(50));
            expect(x.raw_os_error() == Some(38) && fs::metadata(marker).is_err(), || format!("{x}, marker {:?}", fs::metadata(marker).is_ok()))
        }
    }
}

fn child(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("exit") => std::process::exit(args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1)),
        Some("print") => {
            println!("out {}", env::var("RSTEST_CHILD").unwrap_or_default());
            eprintln!("err");
            std::process::exit(0);
        }
        Some("cat") => {
            let mut s = String::new();
            std::io::stdin().read_to_string(&mut s).unwrap();
            print!("{s}");
            std::process::exit(0);
        }
        Some("sleep") => {
            thread::sleep(Duration::from_secs(30));
            std::process::exit(0);
        }
        Some("hash") => {
            let mut m = HashMap::new();
            m.insert(1, 2);
            print!("hashmap {}", m[&1]);
            std::process::exit(0);
        }
        Some("connect") => {
            match std::net::TcpStream::connect("10.0.2.2:1") {
                Err(x) if x.kind() == std::io::ErrorKind::PermissionDenied => print!("connect refused"),
                other => print!("connect: {other:?}"),
            }
            std::process::exit(0);
        }
        Some("cwd") => {
            print!("{}", env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|x| x.to_string()));
            std::process::exit(0);
        }
        Some("touch") => {
            let ok = args.get(1).is_some_and(|p| fs::write(p, b"ran").is_ok());
            std::process::exit(if ok { 0 } else { 1 });
        }
        _ => std::process::exit(2),
    }
}

fn panics() {
    check("catch_unwind-drops", || {
        static DROPS: AtomicUsize = AtomicUsize::new(0);
        struct D;
        impl Drop for D {
            fn drop(&mut self) {
                DROPS.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn deep(n: u32) {
            let _d = D;
            if n == 0 {
                panic!("from 20 frames down");
            }
            deep(n - 1);
        }
        let r = panic::catch_unwind(|| deep(19));
        let d = DROPS.load(Ordering::SeqCst);
        match r {
            Err(p) if d == 20 => Ok(format!("{:?}, 20 destructors ran", payload(&p))),
            Err(_) => Err(format!("{d} destructors")),
            Ok(()) => Err("no panic".into()),
        }
    });
    check("thread-panic-join", || match thread::spawn(|| panic!("in a thread {}", 42)).join() {
        Err(p) => expect(payload(&p) == "in a thread 42", || payload(&p)),
        Ok(()) => Err("joined Ok".into()),
    });
    check("mutex-poison", || {
        let m = Arc::new(Mutex::new(0));
        let m2 = m.clone();
        let _ = thread::spawn(move || {
            let _g = m2.lock().unwrap();
            panic!("poison");
        })
        .join();
        expect(m.is_poisoned(), || "not poisoned".into())
    });
    check("resume_unwind", || {
        let r = panic::catch_unwind(|| panic::resume_unwind(Box::new(7u32)));
        match r {
            Err(p) if p.downcast_ref::<u32>() == Some(&7) => Ok(String::new()),
            _ => Err("payload".into()),
        }
    });
    check("backtrace", || {
        // Frames without names in a build without debug information (ADR-0454 item 5).
        let b = std::backtrace::Backtrace::force_capture();
        Ok(format!("{:?}", b.status()))
    });
    check("index-out-of-bounds", || {
        let r = panic::catch_unwind(|| {
            let v: Vec<u8> = Vec::new();
            v[std::hint::black_box(3)]
        });
        expect(r.is_err(), || "no panic".into())
    });
}

/// ADR-0454 item 2: ends with std's message and exit status 101.
fn uncaught() {
    println!("rstest: about to panic");
    let v: Option<u8> = None;
    v.expect("nobody catches this");
}

/// ADR-0454 item 3: a stack overflow is a fault, with no Rust message.
fn overflow() {
    #[allow(unconditional_recursion)]
    fn rec(n: u64) -> u64 {
        let a = [n; 64];
        std::hint::black_box(&a);
        rec(n + 1) + a[3]
    }
    println!("rstest: recursing");
    println!("{}", rec(0));
}
