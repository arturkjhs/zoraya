#![no_std]
#![no_main]

//! Runs a POSIX program from the volume (ADR-0108) with a bootstrap record
//! (ADR-0107): the native launcher of stage 6c, until the broker of 6d.
//!
//! ```text
//! posixrun [-m PREFIX=PATH[:ro]]... [-t PREFIX=PATH] [-C CWD] [-o FILE]
//!          [-e NAME=VALUE]... [-s] PROGRAM [ARG]...
//! ```
//!
//! `-m` gives the program a namespace entry: a directory of the volume, with
//! every right or (`:ro`) to read; with none, `/` is the volume's root. `-t`
//! is a `Tmp` entry, a directory of the volume standing in for 6d's
//! temporary service. `/dev/null` is always there. `-C` is cwd, a path in the
//! table (default `/`). `-o FILE` makes fd 1 a file of the volume, shared
//! into the child (ADR-0103's `share`), whose position the launcher reads
//! back after the child ends. `-e` adds to the environment, which is
//! otherwise empty. `-s` measures `share` and `seek` round trips first.
//!
//! fd 0 is `/dev/null`; fds 1 and 2 are one stream, whose bytes go to the
//! console. The child's `BROKER` slot is a channel nobody serves, whose
//! messages are counted (INV-0103-3), and `REPORT` the refused-path channel
//! of ADR-0153 rule 11, whose reports are printed after the child ends. The
//! one message answered is the statistics report a `libposix` built with
//! `stats` sends before it exits (ADR-0110 item 7): it is observation, not a
//! request of the file paths, so it is answered and not counted.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_abi::posix::{self as abi, Entry, EntryKind, Fd, FdKind, NO_HANDLE, Report, slot};
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, ExitReason, Handle, JOB, JobFlags, JobObject,
    MemoryFlags, Mode, ObjectType, Packet, Rights, StreamWriteObject, channel_signals,
    process_signals, stream_signals,
};
use zoraya_proto_file::{open_flags, rights, whence};
use zoraya_user::files::{ALL, File, status_name};
use zoraya_user::{
    channel_create, channel_read, channel_write, clock_monotonic, console_write, handle_close,
    handle_duplicate, image_create, job_create, memory_create, port_create, port_subscribe,
    port_wait, print, println, process_exit, process_exit_record, spawn, startup, stream_create,
    stream_read,
};

/// The volume's root, which `zosh` gives programs that use files.
const VOLUME: u64 = 5;
const CONSOLE: Handle = zoraya_abi::CONSOLE;

fn fail(what: &str) -> ! {
    println!("posixrun: {what}");
    process_exit(125)
}

/// Answers a statistics report (ADR-0110 item 7) and gives 0; any other
/// message is left unanswered and gives 1, to be counted.
fn answer_report(end: Handle, message: &[u8]) -> u32 {
    use zoraya_proto_posix::{ReportStats, ReportStatsReply, status};
    use zoraya_protocol::{Below, Header, Message, encode};
    match Header::decode(message) {
        Ok((h, _)) if h.ordinal == ReportStats::ORDINAL => {
            let mut out = [0u8; 64];
            let reply = ReportStatsReply {
                status: Below(status::OK),
            };
            if let Ok(n) = encode(&reply, h.transaction, &mut out) {
                let _ = channel_write(end, &out[..n], &[]);
            }
            0
        }
        _ => 1,
    }
}

/// A directory of the volume, by its absolute path, with `wanted`.
fn directory(root: &File, path: &str, wanted: u32) -> File {
    let relative = path.trim_start_matches('/');
    let found = if relative.is_empty() {
        root.clone_as(wanted)
    } else {
        root.open(relative, open_flags::DIRECTORY, wanted)
    };
    found.unwrap_or_else(|s| fail(&alloc::format!("{path}: {}", status_name(s))))
}

/// The program's bytes in a memory object, and their length.
fn load(root: &File, path: &str) -> (Handle, u64) {
    let file = root
        .open(
            path.trim_start_matches('/'),
            0,
            rights::READ | rights::CLONE,
        )
        .unwrap_or_else(|s| fail(&alloc::format!("{path}: {}", status_name(s))));
    let length = file.stat().unwrap_or_else(|_| fail("stat")).size;
    let size = length.next_multiple_of(4096).max(4096);
    let object = memory_create(size, MemoryFlags::NONE).unwrap_or_else(|_| fail("memory"));
    let copy =
        handle_duplicate(object, zoraya_abi::MemoryObject::RIGHTS).unwrap_or_else(|_| fail("dup"));
    file.attach(copy, size).unwrap_or_else(|_| fail("attach"));
    let mut at = 0;
    while at < length {
        let n = (length - at).min(1 << 20) as u32;
        match file.read_buffer(at, at, n) {
            Ok(0) | Err(_) => fail("read"),
            Ok(got) => at += got,
        }
    }
    (object, length)
}

struct Options {
    mounts: Vec<(String, String, bool, EntryKind)>,
    cwd: String,
    output: Option<String>,
    env: Vec<String>,
    measure: bool,
    program: String,
    args: Vec<Vec<u8>>,
}

fn options() -> Options {
    let mut o = Options {
        mounts: Vec::new(),
        cwd: String::from("/"),
        output: None,
        env: Vec::new(),
        measure: false,
        program: String::new(),
        args: Vec::new(),
    };
    let block = startup();
    let mut words = block
        .argv
        .map(|w| core::str::from_utf8(w).unwrap_or_else(|_| fail("UTF-8 arguments")));
    while let Some(w) = words.next() {
        let mut value = || {
            words
                .next()
                .unwrap_or_else(|| fail("an option without its value"))
        };
        match w {
            "-m" | "-t" => {
                let v = value();
                let (prefix, rest) = v.split_once('=').unwrap_or_else(|| fail("-m PREFIX=PATH"));
                let (path, read_only) = match rest.strip_suffix(":ro") {
                    Some(p) => (p, true),
                    None => (rest, false),
                };
                let kind = if w == "-t" {
                    EntryKind::Tmp
                } else {
                    EntryKind::Directory
                };
                o.mounts.push((prefix.into(), path.into(), read_only, kind));
            }
            "-C" => o.cwd = value().into(),
            "-o" => o.output = Some(value().into()),
            "-e" => o.env.push(value().into()),
            "-s" => o.measure = true,
            program => {
                o.program = program.into();
                o.args.push(program.as_bytes().to_vec());
                for a in words.by_ref() {
                    o.args.push(a.as_bytes().to_vec());
                }
            }
        }
    }
    if o.program.is_empty() {
        fail(
            "usage: posixrun [-m PREFIX=PATH[:ro]]... [-t PREFIX=PATH] [-C CWD] [-o FILE] [-e NAME=VALUE]... [-s] PROGRAM [ARG]...",
        );
    }
    if o.mounts.is_empty() {
        o.mounts
            .push(("/".into(), "/".into(), false, EntryKind::Directory));
    }
    // Longest prefix first, as the record requires.
    o.mounts.sort_by_key(|m| core::cmp::Reverse(m.0.len()));
    o
}

/// The entry that covers `path` and the path in it (ADR-0107 item 6).
fn place(prefixes: &[&str], path: &str) -> Option<(u32, String)> {
    let covers =
        |p: &str| p == "/" || path == p || path.strip_prefix(p).is_some_and(|r| r.starts_with('/'));
    let (i, p) = prefixes
        .iter()
        .enumerate()
        .filter(|(_, p)| covers(p))
        .max_by_key(|(_, p)| p.len())?;
    Some((i as u32, path[p.len()..].trim_matches('/').into()))
}

/// The round trips of `share` and `seek` from a native client, mean of
/// `n`, in nanoseconds (the task's measurement).
fn measure(root: &File) {
    let file = root
        .open(
            "posixrun-measure",
            open_flags::CREATE | open_flags::TRUNCATE,
            ALL,
        )
        .unwrap_or_else(|_| fail("measure"));
    let n = 200u64;
    let started = clock_monotonic();
    for _ in 0..n {
        file.seek(whence::CURRENT, 0)
            .unwrap_or_else(|_| fail("seek"));
    }
    let seek = (clock_monotonic() - started) / n;
    let started = clock_monotonic();
    for _ in 0..n {
        drop(file.share(ALL).unwrap_or_else(|_| fail("share")));
    }
    let share = (clock_monotonic() - started) / n;
    drop(file);
    let _ = root.remove("posixrun-measure");
    println!("posixrun: round trips: seek {seek} ns, share {share} ns (mean of {n})");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let o = options();
    let root = File {
        end: Handle::new(VOLUME).unwrap_or_else(|| fail("no volume at slot 5")),
    };
    if o.measure {
        measure(&root);
    }
    let (object, length) = load(&root, &o.program);
    let image = image_create(object, length)
        .unwrap_or_else(|s| fail(&alloc::format!("{}: {s:?}", o.program)));
    let _ = handle_close(object);

    // The table, longest first, then /dev/null where its length puts it.
    let mut entries: Vec<(String, EntryKind, Option<File>)> = o
        .mounts
        .iter()
        .map(|(prefix, path, ro, kind)| {
            let wanted = if *ro {
                rights::READ | rights::CLONE
            } else {
                ALL
            };
            (prefix.clone(), *kind, Some(directory(&root, path, wanted)))
        })
        .collect();
    entries.push(("/dev/null".into(), EntryKind::Null, None));
    entries.sort_by_key(|e| core::cmp::Reverse(e.0.len()));
    let prefixes: Vec<&str> = entries.iter().map(|e| e.0.as_str()).collect();
    let (cwd_entry, cwd) =
        place(&prefixes, &o.cwd).unwrap_or_else(|| fail("-C: outside the table"));

    // fd 1 and 2: a stream to the console, or fd 1 a shared file.
    let (out_read, out_write) = stream_create().unwrap_or_else(|_| fail("stream"));
    let output = o.output.as_ref().map(|path| {
        let flags = open_flags::CREATE | open_flags::TRUNCATE;
        let file = root
            .open(path.trim_start_matches('/'), flags, ALL)
            .unwrap_or_else(|s| fail(&alloc::format!("{path}: {}", status_name(s))));
        let shared = file.share(ALL).unwrap_or_else(|_| fail("share"));
        (file, shared)
    });
    let mut fds = Vec::new();
    fds.push(Fd {
        fd: 0,
        kind: FdKind::Null,
        flags: 0,
        tty: NO_HANDLE,
    });
    let one = if output.is_some() {
        FdKind::File
    } else {
        FdKind::StreamWrite
    };
    fds.push(Fd {
        fd: 1,
        kind: one,
        flags: 0,
        tty: NO_HANDLE,
    });
    fds.push(Fd {
        fd: 2,
        kind: FdKind::StreamWrite,
        flags: 0,
        tty: NO_HANDLE,
    });

    // The record and its handles: one per directory entry, in order.
    let mut moved: Vec<ChannelEntry> = Vec::new();
    let mut record_entries: Vec<Entry<'_>> = Vec::new();
    for (prefix, kind, file) in &entries {
        let handle = match file {
            Some(f) => {
                moved.push(ChannelEntry {
                    handle: f.end,
                    rights: ChannelObject::RIGHTS,
                    transfer: Transfer::Move,
                });
                (moved.len() - 1) as u8
            }
            None => NO_HANDLE,
        };
        record_entries.push(Entry {
            prefix,
            kind: *kind,
            handle,
        });
    }
    let facts = abi::Facts {
        uid: 1000,
        gid: 1000,
        umask: 0o022,
        ..abi::Facts::default()
    };
    let mut record = alloc::vec![0u8; 16 * 1024];
    let size = abi::encode(
        &facts,
        0,
        &o.program,
        cwd_entry,
        &cwd,
        &fds,
        &record_entries,
        moved.len(),
        &mut record,
    )
    .unwrap_or_else(|| fail("the record"));
    let (boot_mine, boot_theirs) = channel_create().unwrap_or_else(|_| fail("channel"));
    channel_write(boot_mine, &record[..size], &moved)
        .unwrap_or_else(|_| fail("the record's message"));
    for (_, _, file) in entries {
        // Moved with the message.
        core::mem::forget(file);
    }
    let (broker_mine, broker_theirs) = channel_create().unwrap_or_else(|_| fail("channel"));
    let (report_theirs, report_mine) = channel_create().unwrap_or_else(|_| fail("channel"));

    // The child's job, inside the line's, with the posix flag (ADR-0002 rule 3).
    let job = job_create(JOB, JobFlags::POSIX).unwrap_or_else(|_| fail("job"));
    let mut buffer = alloc::vec![0u8; 64 * 1024];
    let mut e = Encoder::new(&mut buffer, job, image);
    let at = |s: Handle| s.slot();
    let channel = |slot: u32, handle: Handle| HandleEntry {
        slot,
        handle,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    e.handle(channel(at(slot::BOOTSTRAP), boot_theirs));
    e.handle(channel(at(slot::BROKER), broker_theirs));
    e.handle(channel(at(slot::REPORT), report_theirs));
    e.handle(HandleEntry {
        slot: at(slot::PROCESS_JOB),
        handle: job,
        rights: JobObject::RIGHTS,
        transfer: Transfer::Duplicate,
    });
    let stream = |slot: u32, transfer| HandleEntry {
        slot,
        handle: out_write,
        rights: StreamWriteObject::RIGHTS,
        transfer,
    };
    match &output {
        Some((_, shared)) => {
            e.handle(channel(1, shared.end));
        }
        None => {
            e.handle(stream(1, Transfer::Duplicate));
        }
    }
    e.handle(stream(2, Transfer::Duplicate));
    for a in &o.args {
        e.arg(a);
    }
    for v in &o.env {
        e.env(v.as_bytes());
    }
    let length = e.finish().unwrap_or_else(|| fail("the spawn buffer"));
    let child = spawn(&buffer[..length]).unwrap_or_else(|s| fail(&alloc::format!("spawn: {s:?}")));
    // Moved to the child: our connection stays, to read the position back.
    let output = output.map(|(file, shared)| {
        core::mem::forget(shared);
        file
    });
    let _ = handle_close(out_write);
    let _ = handle_close(image);
    // The second message: the child's own handle, which spawn could not list.
    // TRANSFER to move it; the child receives WAIT alone.
    let me = handle_duplicate(child, Rights::WAIT.union(Rights::TRANSFER))
        .unwrap_or_else(|_| fail("dup"));
    let self_entry = ChannelEntry {
        handle: me,
        rights: Rights::WAIT,
        transfer: Transfer::Move,
    };
    channel_write(boot_mine, abi::SELF_MESSAGE, &[self_entry]).unwrap_or_else(|_| fail("SELF"));
    let _ = handle_close(boot_mine);

    // The child's output to the console until it ends and its stream drains.
    let port = port_create().unwrap_or_else(|_| fail("port"));
    port_subscribe(port, child, process_signals::TERMINATED, 1, Mode::Level)
        .unwrap_or_else(|_| fail("subscribe"));
    let wanted = stream_signals::READABLE | stream_signals::PEER_CLOSED;
    port_subscribe(port, out_read, wanted, 2, Mode::Level).unwrap_or_else(|_| fail("subscribe"));
    port_subscribe(port, broker_mine, channel_signals::READABLE, 3, Mode::Level)
        .unwrap_or_else(|_| fail("subscribe"));
    let mut ended = false;
    let mut open = true;
    let mut chunk = [0u8; 4096];
    let mut broker = 0;
    let mut bytes = alloc::vec![0u8; 64 * 1024];
    let mut handles = [CONSOLE; 64];
    while !ended || open {
        let mut packets = [Packet::default(); 3];
        let n = port_wait(port, DEADLINE_INFINITE, &mut packets).unwrap_or_else(|_| fail("wait"));
        for p in &packets[..n] {
            match p.key {
                1 => ended = true,
                3 => {
                    if let Ok(sizes) = channel_read(broker_mine, &mut bytes, &mut handles) {
                        broker += answer_report(broker_mine, &bytes[..sizes.bytes as usize]);
                        for &h in &handles[..sizes.handles as usize] {
                            let _ = handle_close(h);
                        }
                    }
                }
                _ => loop {
                    match stream_read(out_read, &mut chunk) {
                        Ok(0) => {
                            open = false;
                            break;
                        }
                        Ok(got) => {
                            let _ = console_write(CONSOLE, &chunk[..got]);
                        }
                        Err(_) => break,
                    }
                },
            }
        }
        // A child that ended with a duplicate of the stream's end alive
        // somewhere would keep `open`; nothing else holds one.
    }
    let record = process_exit_record(child).unwrap_or_else(|_| fail("record"));

    // What the child said to the broker that is not there (INV-0103-3).
    while let Ok(sizes) = channel_read(broker_mine, &mut bytes, &mut handles) {
        broker += answer_report(broker_mine, &bytes[..sizes.bytes as usize]);
    }
    println!("posixrun: broker messages {broker}");
    while let Ok(sizes) = channel_read(report_mine, &mut bytes, &mut handles) {
        match Report::decode(&bytes[..sizes.bytes as usize]) {
            Some(r) => {
                let access = match r.access {
                    abi::Access::Read => "read",
                    abi::Access::Write => "write",
                };
                println!(
                    "posixrun: refused {access} {} (dropped before: {})",
                    r.path, r.dropped
                );
            }
            None => println!("posixrun: a malformed report"),
        }
    }
    if let Some(file) = output {
        let position = file.seek(whence::CURRENT, 0).unwrap_or(u64::MAX);
        println!(
            "posixrun: {} position {position}",
            o.output.as_deref().unwrap_or("")
        );
    }
    match record.reason {
        ExitReason::Exited => {
            println!("posixrun: exit {}", record.code);
            record.code
        }
        other => {
            print!("posixrun: ended {other:?} detail {}\n", record.detail);
            126
        }
    }
}
