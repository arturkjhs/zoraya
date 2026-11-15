//! TASK-35 probe of Route A: what Rust's prebuilt `std` for
//! `aarch64-unknown-linux-musl` does inside Zoraya when linked against
//! Zoraya's `libc.a`. `rprobe MODE [ARG...]`; each check prints one line,
//! `rprobe: CHECK ok ...` or `rprobe: CHECK FAIL ...`, and a mode ends with
//! `rprobe: MODE done`. A check that panics is caught and reported, so one
//! failure does not hide the next.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::panic;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex, RwLock, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs, thread};

fn check(name: &str, f: impl FnOnce() -> Result<String, String> + panic::UnwindSafe) {
    match panic::catch_unwind(f) {
        Ok(Ok(note)) => println!("rprobe: {name} ok {note}"),
        Ok(Err(e)) => println!("rprobe: {name} FAIL {e}"),
        Err(p) => println!("rprobe: {name} FAIL panicked: {}", payload(&p)),
    }
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

fn main() {
    // Quiet panics inside checks; the check line says what happened.
    let args: Vec<String> = env::args().collect();
    // Installed as /bin/zfetch, the probe is run under zfetch's description,
    // which grants the connection to the URL operand: the net mode.
    let as_zfetch = args.first().is_some_and(|a| a.ends_with("zfetch"));
    let mode = if as_zfetch { "net" } else { args.get(1).map(String::as_str).unwrap_or("hello") };
    if mode != "uncaught" && mode != "child" {
        panic::set_hook(Box::new(|_| {}));
    }
    match mode {
        "hello" => hello(&args),
        "syscall" => raw_syscall(),
        "threads" => threads(),
        "hash" => hash(),
        "time" => time(),
        "fs" => fs_checks(args.get(2).map(String::as_str).unwrap_or("/tmp")),
        "net" => net(&args[1..]),
        "process" => process(&args[0]),
        "child" => child(&args[2..]),
        "panic" => panics(),
        "uncaught" => uncaught(),
        "abort" => std::process::abort(),
        "overflow" => overflow(),
        other => {
            println!("rprobe: unknown mode {other}");
            std::process::exit(2);
        }
    }
    println!("rprobe: {mode} done");
}

fn hello(args: &[String]) {
    println!("hello from Rust std");
    check("args", || Ok(format!("{args:?}")));
    check("env", || {
        let n = env::vars_os().count();
        // SAFETY: single-threaded at this point.
        unsafe { env::set_var("RPROBE", "1") };
        match env::var("RPROBE") {
            Ok(v) if v == "1" => Ok(format!("{n} variables, set_var works")),
            other => Err(format!("{other:?}")),
        }
    });
    check("stderr", || {
        eprintln!("rprobe: to stderr");
        Ok(String::new())
    });
}

fn raw_syscall() {
    // SAFETY: gettid takes no arguments; the result is only printed.
    let r = unsafe { libc_syscall(178) }; // __NR_gettid on aarch64
    let errno = std::io::Error::last_os_error();
    println!("rprobe: syscall(SYS_gettid) returned {r} ({r:#x}), errno {errno}");
}

unsafe extern "C" {
    #[link_name = "syscall"]
    fn libc_syscall_raw(n: i64, ...) -> i64;
}

unsafe fn libc_syscall(n: i64) -> i64 {
    // SAFETY: the caller passes a call that takes no arguments.
    unsafe { libc_syscall_raw(n) }
}

fn threads() {
    check("available_parallelism", || {
        thread::available_parallelism().map(|n| n.to_string()).map_err(e)
    });
    check("spawn-join", || {
        let hs: Vec<_> = (0..8).map(|i| thread::spawn(move || i * 2)).collect();
        let s: i32 = hs.into_iter().map(|h| h.join().unwrap()).sum();
        if s == 56 { Ok("8 threads".into()) } else { Err(format!("sum {s}")) }
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
        if v == 80_000 { Ok(format!("80000 in {:?}", t.elapsed())) } else { Err(format!("{v}")) }
    });
    check("condvar-pingpong", || {
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
        Ok(format!("1000 rounds in {:?}", t.elapsed()))
    });
    check("condvar-wait_timeout", || {
        // Nobody notifies: a futex that waits returns after 200 ms with
        // timed_out; one that fails returns at once (a spurious wake-up).
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
        Ok(format!("timed out after {:?}, {spurious} spurious wake-ups", t.elapsed()))
    });
    check("park-timeout", || {
        let t = Instant::now();
        thread::park_timeout(Duration::from_millis(100));
        Ok(format!("{:?} (100 ms asked)", t.elapsed()))
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
        if n == 400 { Ok("400 messages".into()) } else { Err(format!("{n}")) }
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
        if s == 15 { Ok(String::new()) } else { Err(format!("{s}")) }
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
        if name.as_deref() == Some("worker") && d == 1 {
            Ok("name worker, thread_local dropped once".into())
        } else {
            Err(format!("name {name:?}, drops {d}"))
        }
    });
    check("scoped", || {
        let mut v = vec![1, 2, 3];
        thread::scope(|s| {
            s.spawn(|| v.push(4));
        });
        if v.len() == 4 { Ok(String::new()) } else { Err(format!("{v:?}")) }
    });
}

fn hash() {
    check("hashmap", || {
        let mut m = HashMap::new();
        for i in 0..1000 {
            m.insert(i, i * i);
        }
        if m[&30] == 900 { Ok("1000 entries, random keys".into()) } else { Err("value".into()) }
    });
}

fn time() {
    check("systemtime", || {
        let s = SystemTime::now().duration_since(UNIX_EPOCH).map_err(e)?.as_secs();
        if s > 1_700_000_000 { Ok(format!("{s} s since 1970")) } else { Err(format!("{s}")) }
    });
    check("instant-sleep", || {
        let t = Instant::now();
        thread::sleep(Duration::from_millis(50));
        let d = t.elapsed();
        if d >= Duration::from_millis(50) { Ok(format!("{d:?} for 50 ms")) } else { Err(format!("{d:?}")) }
    });
}

fn fs_checks(dir: &str) {
    let base = format!("{dir}/rprobe");
    let _ = fs::remove_dir_all(&base);
    check("create_dir_all", || fs::create_dir_all(format!("{base}/a/b")).map(|_| String::new()).map_err(e));
    check("write-read", || {
        fs::write(format!("{base}/f.txt"), b"hello file\n").map_err(e)?;
        let s = fs::read_to_string(format!("{base}/f.txt")).map_err(e)?;
        if s == "hello file\n" { Ok(String::new()) } else { Err(format!("{s:?}")) }
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
        if s == "file\nmore\n" && len == 5 { Ok(String::new()) } else { Err(format!("{s:?} {len}")) }
    });
    check("metadata", || {
        let m = fs::metadata(format!("{base}/f.txt")).map_err(e)?;
        let modified = m.modified().map_err(e)?;
        let created = m.created().map(|_| "created known").unwrap_or("created unknown");
        Ok(format!(
            "len {} file {} dir {} mtime {:?} {created}",
            m.len(),
            m.is_file(),
            m.is_dir(),
            modified.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
        ))
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
        Ok(names.join(" "))
    });
    check("rename", || fs::rename(format!("{base}/f.txt"), format!("{base}/g.txt")).map(|_| String::new()).map_err(e));
    check("copy", || {
        fs::write(format!("{base}/big"), vec![7u8; 300_000]).map_err(e)?;
        let n = fs::copy(format!("{base}/big"), format!("{base}/big2")).map_err(e)?;
        let same = fs::read(format!("{base}/big2")).map_err(e)? == vec![7u8; 300_000];
        if n == 300_000 && same { Ok("300000 bytes".into()) } else { Err(format!("copied {n}, same {same}")) }
    });
    check("symlink-read_link", || {
        std::os::unix::fs::symlink("g.txt", format!("{base}/link")).map_err(e)?;
        let t = fs::read_link(format!("{base}/link")).map_err(e)?;
        let through = fs::read_to_string(format!("{base}/link")).map_err(e)?;
        Ok(format!("-> {} reads {through:?}", t.display()))
    });
    check("hard_link", || match fs::hard_link(format!("{base}/g.txt"), format!("{base}/h.txt")) {
        Ok(()) => Ok("made".into()),
        Err(x) => Ok(format!("refused: {x}")),
    });
    check("set_permissions", || {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(format!("{base}/g.txt"), fs::Permissions::from_mode(0o600)).map_err(e)?;
        let m = fs::metadata(format!("{base}/g.txt")).map_err(e)?.permissions().mode();
        Ok(format!("mode {:o}", m & 0o7777))
    });
    check("canonicalize", || fs::canonicalize(format!("{base}/a/b/..")).map(|p| p.display().to_string()).map_err(e));
    check("current_dir", || env::current_dir().map(|p| p.display().to_string()).map_err(e));
    check("current_exe", || env::current_exe().map(|p| p.display().to_string()).map_err(e));
    check("temp_dir", || Ok(env::temp_dir().display().to_string()));
    check("not-found", || match fs::read(format!("{dir}/no-such-file")) {
        Err(x) if x.kind() == std::io::ErrorKind::NotFound => Ok(format!("{x}")),
        other => Err(format!("{other:?}")),
    });
    check("remove_dir_all", || fs::remove_dir_all(&base).map(|_| String::new()).map_err(e));
}

fn net(args: &[String]) {
    use std::net::{TcpStream, ToSocketAddrs};
    // ARG: a URL http://HOST:PORT/PATH (so that the zfetch description grants
    // the connection).
    let url = args.iter().find(|a| a.starts_with("http://")).cloned().unwrap_or_default();
    let rest = url.trim_start_matches("http://");
    let (hostport, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let hostport = hostport.to_string();
    let hp = hostport.clone();
    check("to_socket_addrs", move || {
        hp.to_socket_addrs().map(|a| format!("{:?}", a.collect::<Vec<_>>())).map_err(e)
    });
    check("tcp-get", move || {
        let t = Instant::now();
        let mut s = TcpStream::connect(&hostport).map_err(e)?;
        s.set_nodelay(true).ok();
        let host = hostport.split(':').next().unwrap_or("");
        write!(s, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").map_err(e)?;
        let mut body = Vec::new();
        s.read_to_end(&mut body).map_err(e)?;
        let first = String::from_utf8_lossy(&body).lines().next().unwrap_or("").to_string();
        Ok(format!("{first:?}, {} bytes in {:?}, peer {:?}", body.len(), t.elapsed(), s.peer_addr().ok()))
    });
    check("tcp-refused", || match TcpStream::connect_timeout(&"10.0.2.2:1".parse().unwrap(), Duration::from_secs(2)) {
        Err(x) => Ok(format!("{:?}: {x}", x.kind())),
        Ok(_) => Err("connected".into()),
    });
}

fn process(me: &str) {
    check("status", || {
        let s = Command::new(me).args(["child", "exit", "3"]).status().map_err(e)?;
        if s.code() == Some(3) { Ok(format!("{s}")) } else { Err(format!("{s}")) }
    });
    check("output", || {
        let o = Command::new(me).args(["child", "print"]).env("RPROBE_CHILD", "yes").output().map_err(e)?;
        Ok(format!(
            "status {} stdout {:?} stderr {:?}",
            o.status,
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        ))
    });
    check("stdin-pipe", || {
        let mut c = Command::new(me)
            .args(["child", "cat"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(e)?;
        c.stdin.take().unwrap().write_all(b"through a pipe").map_err(e)?;
        let o = c.wait_with_output().map_err(e)?;
        Ok(format!("{:?}", String::from_utf8_lossy(&o.stdout)))
    });
    check("kill", || {
        let mut c = Command::new(me).args(["child", "sleep"]).spawn().map_err(e)?;
        thread::sleep(Duration::from_millis(100));
        c.kill().map_err(e)?;
        let s = c.wait().map_err(e)?;
        Ok(format!("{s}"))
    });
    check("missing-program", || match Command::new("/bin/no-such-program").status() {
        Err(x) => Ok(format!("{:?}: {x}", x.kind())),
        Ok(s) => Err(format!("ran: {s}")),
    });
    check("current_dir", || {
        let o = Command::new(me).args(["child", "cwd"]).current_dir("/tmp").output().map_err(e)?;
        Ok(format!("{} {:?}", o.status, String::from_utf8_lossy(&o.stdout)))
    });
}

fn child(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("exit") => std::process::exit(args.get(1).and_then(|a| a.parse().ok()).unwrap_or(1)),
        Some("print") => {
            println!("out {}", env::var("RPROBE_CHILD").unwrap_or_default());
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
        Some("cwd") => {
            print!("{}", env::current_dir().map(|p| p.display().to_string()).unwrap_or_else(|x| x.to_string()));
            std::process::exit(0);
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
        Err(p) => Ok(payload(&p)),
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
        if m.is_poisoned() { Ok(String::new()) } else { Err("not poisoned".into()) }
    });
    check("resume_unwind", || {
        let r = panic::catch_unwind(|| panic::resume_unwind(Box::new(7u32)));
        match r {
            Err(p) if p.downcast_ref::<u32>() == Some(&7) => Ok(String::new()),
            _ => Err("payload".into()),
        }
    });
    check("backtrace", || {
        let b = std::backtrace::Backtrace::force_capture();
        let s = b.to_string();
        Ok(format!(
            "{:?}, {} lines, main named: {}",
            b.status(),
            s.lines().count(),
            s.contains("rprobe::main")
        ))
    });
    check("arithmetic-overflow-checked", || {
        let r = panic::catch_unwind(|| {
            let v: Vec<u8> = Vec::new();
            v[std::hint::black_box(3)]
        });
        if r.is_err() { Ok("index panic caught".into()) } else { Err("no panic".into()) }
    });
}

fn uncaught() {
    println!("rprobe: about to panic");
    let v: Option<u8> = None;
    v.expect("nobody catches this");
}

fn overflow() {
    // Recursion without end on purpose: what a stack overflow does.
    #[allow(unconditional_recursion)]
    fn rec(n: u64) -> u64 {
        let a = [n; 64];
        std::hint::black_box(&a);
        rec(n + 1) + a[3]
    }
    println!("rprobe: recursing");
    println!("{}", rec(0));
}
