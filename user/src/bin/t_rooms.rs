#![no_std]
#![no_main]

//! The room keeper in QEMU (ADR-0269, Q6a): the real `rooms` process under a
//! session job this test makes, a table in an in-memory `fsd`, and this
//! program standing in for `init` (it hands the keeper its channels and
//! reads what the keeper keeps).
//!
//! INV-0269-1 (one job per room, whoever launches; one launcher at a time in
//! an untrusted room), INV-0269-2 (no launch job carries `LIMIT`; what a
//! `Room(R)` channel may send), INV-0269-3 (a restarted keeper stops a room
//! it did not start), INV-0266-1 (the room job carries the line's limits) and
//! INV-0261-8 (four untrusted rooms at their limits, each with a fork bomb and
//! a memory hog, while a `home` program starts 16 processes and maps 16 MiB).
//! Prints the keeper's round trip per launch.

extern crate alloc;

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::quota::JobLimits;
use zoraya_abi::spawn::{Encoder, Transfer};
use zoraya_abi::{
    ChannelEntry, DIRECTORY, DirectoryObject, Handle, JOB, JobFlags, JobObject, MapFlags,
    MemoryFlags, ObjectType, Rights, Status,
};
use zoraya_proto_file::{open_flags, rights};
use zoraya_proto_rooms as pr;
use zoraya_protocol::{Below, Header, Text, decode, encode};
use zoraya_user::files::File;
use zoraya_user::{
    assert_err, channel_create, channel_read, channel_write, clock_monotonic, console_entry, dup,
    handle_close, handle_duplicate, image, job_create, job_get_usage, job_kill, job_set_limits,
    memory_create, memory_map, println, sleep_until, spawn, startup, wait_exit,
};

const MIB: u64 = 1 << 20;
const CHUNK: u64 = 64 << 10;
const REFUSED_EXIT: i32 = 42;
/// The session this test makes (the keeper's slot 5): half of a small guest.
const SESSION_MEM: u64 = 64 * MIB;
const SESSION_PROCS: u32 = 120;

const TABLE: &str = "project --colour 1 --mem 16M +w ~/project\n\
agent-1 --untrusted --mem 8M --procs 12 --cpu 25% --disk 1G --conns 4 --tmp 1M +w ~/sandbox/agent-1\n\
agent-2 --untrusted --mem 8M --procs 12 +w ~/sandbox/agent-2\n\
agent-3 --untrusted --mem 8M --procs 12 +w ~/sandbox/agent-3\n\
agent-4 --untrusted --mem 8M --procs 12 +w ~/sandbox/agent-4\n\
agent-5 --untrusted --mem 8M --procs 12 +w ~/sandbox/agent-5\n\
big --untrusted --mem 40M +w ~/sandbox/big\n";

fn nap(ms: u64) {
    sleep_until(clock_monotonic() + ms * 1_000_000);
}

fn eventually(mut done: impl FnMut() -> bool) -> bool {
    let deadline = clock_monotonic() + 15_000_000_000;
    while !done() {
        if clock_monotonic() > deadline {
            return false;
        }
        nap(5);
    }
    true
}

fn limits(memory: Option<u64>, processes: Option<u32>) -> JobLimits {
    JobLimits {
        memory,
        processes,
        cpu: None,
    }
}

fn child_in(job: Handle, role: &str) -> Result<Handle, Status> {
    let mut buffer = [0; 512];
    let program = image("t_rooms");
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(zoraya_user::dup(
        zoraya_abi::CONSOLE.slot(),
        zoraya_abi::CONSOLE,
        Rights::WRITE.union(Rights::DUPLICATE),
    ));
    // The launch job has every right but LIMIT.
    e.handle(dup(
        JOB.slot(),
        job,
        JobObject::RIGHTS.without(Rights::LIMIT),
    ));
    e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
    e.arg(role.as_bytes());
    let length = e.finish().expect("encode");
    let result = spawn(&buffer[..length]);
    handle_close(program).expect("image");
    result
}

/// A child's roles.
fn hold() -> i32 {
    nap(60_000);
    0
}

fn hog() -> i32 {
    let mut held = 0;
    loop {
        match memory_create(CHUNK, MemoryFlags::NONE) {
            Ok(_) => held += 1,
            Err(Status::OverQuota) => break,
            Err(other) => panic!("memory_create: {other:?} after {held}"),
        }
        assert!(held < 2000, "no limit reached");
    }
    nap(60_000);
    REFUSED_EXIT
}

/// Two copies of itself in its own job, until the job refuses.
fn bomb() -> i32 {
    for _ in 0..2 {
        let mut buffer = [0; 512];
        let program = image("t_rooms");
        let mut e = Encoder::new(&mut buffer, JOB, program);
        e.handle(dup(
            zoraya_abi::CONSOLE.slot(),
            zoraya_abi::CONSOLE,
            Rights::WRITE.union(Rights::DUPLICATE),
        ));
        e.handle(dup(
            JOB.slot(),
            JOB,
            JobObject::RIGHTS.without(Rights::LIMIT),
        ));
        e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
        e.arg(b"bomb");
        let length = e.finish().expect("encode");
        let _ = spawn(&buffer[..length]);
        handle_close(program).expect("image");
    }
    nap(60_000);
    0
}

/// `home`'s program: 16 processes and 16 MiB, in the launch job it was given.
fn home_work(job: Handle) -> bool {
    let mut kids = Vec::new();
    for _ in 0..16 {
        match child_in(job, "hold") {
            Ok(k) => kids.push(k),
            Err(_) => return false,
        }
    }
    let ok = (0..(16 * MIB / CHUNK)).all(|_| memory_create(CHUNK, MemoryFlags::NONE).is_ok());
    for k in kids {
        let _ = handle_close(k);
    }
    ok
}

/// The test's side of the keeper: a channel and the transactions on it.
struct Chan {
    end: Handle,
    next: u32,
    /// `Refused` messages that arrived while waiting for something else.
    refused: Vec<(String, u32, u64, u64)>,
}

enum Answer {
    Launched(Handle),
    Done,
    Denied(u32, String),
    Usage(Vec<pr::UsageRecord>),
}

impl Chan {
    fn new(end: Handle) -> Chan {
        Chan {
            end,
            next: 1,
            refused: Vec::new(),
        }
    }

    fn send<'a, M: zoraya_protocol::Message<'a>>(&mut self, m: &M) -> u32 {
        let mut b = [0u8; 256];
        let tx = self.next;
        self.next += 1;
        let n = encode(m, tx, &mut b).expect("encode");
        channel_write(self.end, &b[..n], &[]).expect("write");
        tx
    }

    /// The next message: its bytes and handle, or `None` at the deadline.
    fn read(&mut self, wait_ms: u64) -> Option<(Vec<u8>, Option<Handle>)> {
        let deadline = clock_monotonic() + wait_ms * 1_000_000;
        loop {
            let mut buf = vec![0u8; 8192];
            let mut handles = [self.end; 1];
            match channel_read(self.end, &mut buf, &mut handles) {
                Ok(sizes) => {
                    buf.truncate(sizes.bytes as usize);
                    return Some((buf, (sizes.handles > 0).then_some(handles[0])));
                }
                Err((Status::WouldBlock, _)) => {
                    if clock_monotonic() > deadline {
                        return None;
                    }
                    nap(2);
                }
                Err(_) => return None,
            }
        }
    }

    fn note(&mut self, bytes: &[u8]) -> bool {
        if let Ok((_, r)) = decode::<pr::Refused<'_>>(bytes, 0) {
            self.refused
                .push((String::from(r.room.0), r.field.0, r.used, r.limit));
            return true;
        }
        false
    }

    fn answer(&mut self, tx: u32) -> Answer {
        loop {
            let (bytes, handle) = self.read(5_000).expect("an answer");
            if self.note(&bytes) {
                continue;
            }
            let (h, _) = Header::decode(&bytes).expect("header");
            assert_eq!(h.transaction, tx, "{h:?}");
            return match h.ordinal {
                64 => {
                    decode::<pr::Launched>(&bytes, 1).expect("launched");
                    Answer::Launched(handle.expect("a launch job"))
                }
                65 => Answer::Done,
                66 => {
                    let (_, d) = decode::<pr::Denied<'_>>(&bytes, 0).expect("denied");
                    Answer::Denied(d.status.0, String::from(d.text.0))
                }
                67 => {
                    let (_, u) = decode::<pr::UsageList<'_>>(&bytes, 0).expect("usage");
                    let records = u
                        .records
                        .0
                        .chunks(pr::RECORD_SIZE)
                        .map(|c| pr::UsageRecord::decode(c).expect("record"))
                        .collect();
                    Answer::Usage(records)
                }
                other => panic!("ordinal {other}"),
            };
        }
    }

    fn launch(&mut self, room: &str) -> Result<Handle, (u32, String)> {
        let tx = self.send(&pr::Launch { room: Text(room) });
        match self.answer(tx) {
            Answer::Launched(j) => Ok(j),
            Answer::Denied(s, t) => Err((s, t)),
            _ => panic!("launch"),
        }
    }

    fn ok(&mut self, tx: u32) -> Result<(), (u32, String)> {
        match self.answer(tx) {
            Answer::Done => Ok(()),
            Answer::Denied(s, t) => Err((s, t)),
            _ => panic!("done"),
        }
    }

    fn usage(&mut self, room: &str) -> Vec<pr::UsageRecord> {
        let tx = self.send(&pr::Usage { room: Text(room) });
        match self.answer(tx) {
            Answer::Usage(r) => r,
            Answer::Denied(s, t) => panic!("usage denied {s} {t}"),
            _ => panic!("usage"),
        }
    }
}

/// What this program, as `init`, holds of the keeper.
struct Keeper {
    process: Handle,
    job: Handle,
    init: Handle,
}

fn start_keeper(session: Handle, config: &File, init_end: Handle, theirs: Handle) -> Keeper {
    let _ = init_end;
    let job = job_create(JOB, JobFlags::NONE).expect("keeper job");
    let program = image("rooms");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, job, program);
    e.handle(console_entry());
    e.handle(dup(JOB.slot(), job, JobObject::RIGHTS));
    e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
    e.handle(dup(5, session, JobObject::RIGHTS));
    e.handle(zoraya_abi::spawn::HandleEntry {
        slot: 6,
        handle: theirs,
        rights: zoraya_abi::ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    });
    let conn = config
        .clone_as(rights::READ | rights::CLONE)
        .expect("clone");
    e.handle(zoraya_abi::spawn::HandleEntry {
        slot: 7,
        handle: conn.end,
        rights: zoraya_abi::ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    });
    core::mem::forget(conn);
    let length = e.finish().expect("encode");
    let process = spawn(&buffer[..length]).expect("spawn rooms");
    handle_close(program).expect("image");
    Keeper {
        process,
        job,
        init: init_end,
    }
}

/// A new `Manage`-like channel of `kind`, attached to the keeper over `init`.
fn attach(keeper: &Keeper, kind: u32, room: &str) -> Chan {
    let (mine, theirs) = channel_create().expect("channel");
    let mut b = [0u8; 128];
    let n = encode(
        &pr::Attach {
            kind: Below(kind),
            room: Text(room),
        },
        0,
        &mut b,
    )
    .expect("encode");
    let entry = ChannelEntry {
        handle: theirs,
        rights: zoraya_abi::ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    };
    channel_write(keeper.init, &b[..n], &[entry]).expect("attach");
    Chan::new(mine)
}

fn processes_of(chan: &mut Chan, room: &str) -> u64 {
    chan.usage(room).first().map_or(0, |r| r.procs_used)
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"hold") => return hold(),
        Some(b"hog") => return hog(),
        Some(b"bomb") => return bomb(),
        _ => {}
    }
    // The table, in a memory volume.
    let (root_client, root_server) = channel_create().expect("channel");
    let fsd_job = job_create(JOB, JobFlags::NONE).expect("job");
    {
        let program = image("fsd");
        let mut buffer = [0; 512];
        let mut e = Encoder::new(&mut buffer, fsd_job, program);
        e.handle(console_entry());
        e.handle(zoraya_abi::spawn::HandleEntry {
            slot: 6,
            handle: root_server,
            rights: zoraya_abi::ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        });
        e.arg(b"--memory");
        e.arg(b"2");
        let length = e.finish().expect("encode");
        spawn(&buffer[..length]).expect("fsd");
        handle_close(program).expect("image");
    }
    let config = File { end: root_client };
    let write_table = |text: &str| {
        let f = config
            .open(
                "rooms",
                open_flags::CREATE | open_flags::TRUNCATE,
                rights::ALL,
            )
            .expect("rooms file");
        f.write(0, 0, text.as_bytes()).expect("write");
    };
    write_table(TABLE);

    // The session this test stands as `init` for.
    let session = job_create(JOB, JobFlags::NONE).expect("session");
    job_set_limits(session, &limits(Some(SESSION_MEM), Some(SESSION_PROCS))).expect("limits");

    let (init_mine, init_theirs) = channel_create().expect("channel");
    let mut keeper = start_keeper(session, &config, init_mine, init_theirs);
    let mut desktop = attach(&keeper, pr::attach::MANAGE_DESKTOP, "");
    let mut console = attach(&keeper, pr::attach::MANAGE_CONSOLE, "");
    let mut usage_chan = attach(&keeper, pr::attach::USAGE, "");
    let mut agent1_chan = attach(&keeper, pr::attach::ROOM, "agent-1");

    // The round trip per launch (the plan's measurement), over `home`.
    {
        const N: u64 = 50;
        let start = clock_monotonic();
        for _ in 0..N {
            let j = desktop.launch("home").expect("home");
            handle_close(j).expect("close");
        }
        let per = (clock_monotonic() - start) / N / 1000;
        println!("t_rooms: launch round trip {per} us (average of {N}, HVF or TCG as run)");
    }

    // INV-0269-1 (trusted): the desktop's and the console's launches are one
    // job, with the line's limits (INV-0266-1).
    let a = desktop.launch("project").expect("desktop launch");
    let b = console.launch("project").expect("console launch");
    let pa = child_in(a, "hold").expect("child a");
    let pb = child_in(b, "hold").expect("child b");
    let records = desktop.usage("project");
    assert_eq!(records.len(), 1, "one job for the room");
    assert_eq!(records[0].procs_used, 2, "both launches are in it");
    assert_eq!(records[0].mem_limit, 16 * MIB);
    // Each launch job sees the same room above it: both children count in
    // the room's use, and the two launch jobs' own use adds up to it.
    let ua = job_get_usage(a, false).expect("usage").processes.used;
    let ub = job_get_usage(b, false).expect("usage").processes.used;
    assert_eq!((ua, ub), (1, 1));
    // INV-0269-2: no launch job carries LIMIT.
    assert_err!(
        job_set_limits(a, &limits(Some(MIB), None)),
        Status::AccessDenied
    );
    assert_err!(
        job_set_limits(b, &limits(None, Some(1))),
        Status::AccessDenied
    );
    // The handle itself has no LIMIT and cannot be duplicated into one that does.
    assert_err!(handle_duplicate(a, JobObject::RIGHTS), Status::AccessDenied);
    let weak = handle_duplicate(a, JobObject::RIGHTS.without(Rights::LIMIT)).expect("dup");
    assert_err!(
        job_set_limits(weak, &limits(Some(MIB), None)),
        Status::AccessDenied
    );
    handle_close(weak).expect("close");
    println!("t_rooms: INV-0269-1 (trusted): one job, two launchers; INV-0269-2: no LIMIT");

    // INV-0269-1 (untrusted): one Manage holder at a time.
    let agent = console.launch("agent-1").expect("console enters agent-1");
    assert_eq!(
        desktop.launch("agent-1").unwrap_err(),
        (pr::status::BUSY, String::from("running from the console"))
    );
    let tx = console.send(&pr::Leave {
        room: Text("agent-1"),
    });
    console.ok(tx).expect("leave");
    let held = child_in(agent, "hold").expect("child in agent-1");
    // The room has a process: still the console's.
    assert_eq!(desktop.launch("agent-1").unwrap_err().0, pr::status::BUSY);
    // All six words reach the record (INV-0266-1), the job its three.
    let rec = console.usage("agent-1")[0];
    assert_eq!(rec.mem_limit, 8 * MIB);
    assert_eq!(rec.procs_limit, 12);
    assert_eq!(rec.cpu_limit, 250);
    assert_eq!(
        (rec.disk_limit, rec.conns_limit, rec.tmp_limit),
        (1 << 30, 4, MIB)
    );
    job_kill(agent).expect("end the room's process");
    wait_exit(held);
    assert!(eventually(|| processes_of(&mut console, "agent-1") == 0));
    handle_close(held).expect("close");
    handle_close(agent).expect("close");
    let again = desktop
        .launch("agent-1")
        .expect("the desktop, after the room ended");
    handle_close(again).expect("close");
    let tx = desktop.send(&pr::Leave {
        room: Text("agent-1"),
    });
    desktop.ok(tx).expect("leave");
    println!("t_rooms: INV-0269-1 (untrusted): one launcher at a time");

    // A Room(R) channel: Launch and Usage of R, nothing else.
    {
        let tx = agent1_chan.send(&pr::Stop {
            room: Text("agent-1"),
        });
        assert_eq!(agent1_chan.ok(tx).unwrap_err().0, pr::status::NOT_ALLOWED);
        let tx = agent1_chan.send(&pr::Set {
            room: Text("agent-1"),
        });
        assert_eq!(agent1_chan.ok(tx).unwrap_err().0, pr::status::NOT_ALLOWED);
        assert_eq!(
            agent1_chan.launch("project").unwrap_err().0,
            pr::status::NOT_ALLOWED
        );
        let tx = agent1_chan.send(&pr::Usage { room: Text("") });
        assert!(matches!(
            agent1_chan.answer(tx),
            Answer::Denied(s, _) if s == pr::status::NOT_ALLOWED
        ));
        let own = agent1_chan.usage("agent-1");
        assert_eq!(own.len(), 1);
        let j = agent1_chan.launch("agent-1").expect("its own room");
        handle_close(j).expect("close");
        let tx = usage_chan.send(&pr::Stop {
            room: Text("project"),
        });
        assert_eq!(usage_chan.ok(tx).unwrap_err().0, pr::status::NOT_ALLOWED);
        assert!(!usage_chan.usage("").is_empty());
    }
    println!("t_rooms: INV-0269-2: a Room(R) channel and a Usage channel send only what they may");

    // A hog in agent-1 gets OverQuota, exits by itself, and the keeper tells
    // the room's channel, the overview's and the console's (ADR-0263 point 3).
    {
        let j = console.launch("agent-1").expect("launch");
        let hog = child_in(j, "hog").expect("hog");
        // It holds 8 MiB and then waits: the refusal has happened by then.
        assert!(eventually(|| console.usage("agent-1")[0].mem_refused > 0));
        let mut seen = (false, false, false);
        let deadline = clock_monotonic() + 10_000_000_000;
        while clock_monotonic() < deadline && !(seen.0 && seen.1 && seen.2) {
            for (i, c) in [&mut agent1_chan, &mut usage_chan, &mut console]
                .into_iter()
                .enumerate()
            {
                while let Some((bytes, _)) = c.read(20) {
                    c.note(&bytes);
                }
                let hit = c
                    .refused
                    .iter()
                    .any(|r| r.0 == "agent-1" && r.1 == pr::field::MEMORY);
                match i {
                    0 => seen.0 = hit,
                    1 => seen.1 = hit,
                    _ => seen.2 = hit,
                }
            }
        }
        assert!(seen.0 && seen.1 && seen.2, "Refused on all three: {seen:?}");
        let (_, _, used, limit) = agent1_chan
            .refused
            .iter()
            .find(|r| r.0 == "agent-1")
            .cloned()
            .expect("notice");
        assert_eq!(limit, 8 * MIB);
        assert!(used <= 8 * MIB);
        job_kill(j).expect("kill");
        wait_exit(hog);
        handle_close(hog).expect("close");
        handle_close(j).expect("close");
        assert!(eventually(|| processes_of(&mut console, "agent-1") == 0));
        let tx = console.send(&pr::Leave {
            room: Text("agent-1"),
        });
        console.ok(tx).expect("leave");
    }
    println!("t_rooms: a hog in agent-1 is refused and the keeper's Refused reaches its channels");

    // `room set`: the new line reaches the running room's job; what is above
    // the untrusted job is refused and changes nothing.
    {
        let j = console.launch("agent-1").expect("launch");
        write_table(&TABLE.replace(
            "agent-1 --untrusted --mem 8M",
            "agent-1 --untrusted --mem 6M",
        ));
        let tx = console.send(&pr::Set {
            room: Text("agent-1"),
        });
        console.ok(tx).expect("set");
        assert_eq!(console.usage("agent-1")[0].mem_limit, 6 * MIB);
        write_table(&TABLE.replace(
            "agent-1 --untrusted --mem 8M",
            "agent-1 --untrusted --mem 40M",
        ));
        let tx = console.send(&pr::Set {
            room: Text("agent-1"),
        });
        let e = console.ok(tx).unwrap_err();
        assert_eq!(e.0, pr::status::BAD_LINE, "{e:?}");
        assert!(e.1.contains("above the untrusted rooms' 32M"), "{e:?}");
        assert_eq!(console.usage("agent-1")[0].mem_limit, 6 * MIB, "unchanged");
        write_table(TABLE);
        let tx = console.send(&pr::Set {
            room: Text("agent-1"),
        });
        console.ok(tx).expect("set back");
        assert_eq!(console.usage("agent-1")[0].mem_limit, 8 * MIB);
        handle_close(j).expect("close");
        let tx = console.send(&pr::Leave {
            room: Text("agent-1"),
        });
        console.ok(tx).expect("leave");
    }
    // A line above the untrusted job never makes a job.
    assert_eq!(console.launch("big").unwrap_err().0, pr::status::BAD_LINE);
    assert_eq!(
        console.launch("nobody").unwrap_err().0,
        pr::status::NO_SUCH_ROOM
    );
    println!("t_rooms: INV-0266-1/-2: Set applies the line; above the untrusted job is refused");

    // INV-0261-8: four untrusted rooms, each with a bomb and a hog, while a
    // `home` program starts 16 processes and maps 16 MiB.
    {
        let mut rooms = Vec::new();
        for (i, name) in ["agent-1", "agent-2", "agent-3", "agent-4"]
            .iter()
            .enumerate()
        {
            let chan = if i % 2 == 0 {
                &mut desktop
            } else {
                &mut console
            };
            let j = chan.launch(name).expect("room");
            rooms.push((
                name,
                j,
                child_in(j, "bomb").expect("bomb"),
                child_in(j, "hog").expect("hog"),
            ));
        }
        // A fifth is refused.
        assert_eq!(
            desktop.launch("agent-5").unwrap_err(),
            (
                pr::status::TOO_MANY,
                String::from("four untrusted rooms are running")
            )
        );
        for (name, ..) in &rooms {
            let n = **name;
            assert!(
                eventually(|| {
                    let r = desktop.usage(n)[0];
                    r.mem_refused > 0 || r.procs_refused > 0
                }),
                "{n} reached a limit"
            );
        }
        for (name, ..) in &rooms {
            let r = desktop.usage(name)[0];
            assert!(r.procs_used <= 12 && r.mem_used <= 8 * MIB, "{r:?}");
            println!(
                "t_rooms: {name}: {} processes of {}, {} KiB of {} KiB, refused {} + {}",
                r.procs_used,
                r.procs_limit,
                r.mem_used >> 10,
                r.mem_limit >> 10,
                r.procs_refused,
                r.mem_refused
            );
        }
        // `home` through the keeper, into the session's launch job.
        let home = desktop.launch("home").expect("home");
        let ok = home_work(home);
        assert!(ok, "home could not start 16 processes and map 16 MiB");
        for (_, j, bomb, hog) in rooms {
            job_kill(j).expect("kill");
            wait_exit(bomb);
            wait_exit(hog);
            handle_close(bomb).expect("close");
            handle_close(hog).expect("close");
            handle_close(j).expect("close");
        }
        job_kill(home).expect("kill home's children");
    }
    println!(
        "t_rooms: INV-0261-8: four untrusted rooms at their limits; home started 16 processes and mapped 16 MiB"
    );

    // INV-0269-3: the keeper ends with a room running; init starts a new one
    // with what the old one kept, and Stop ends the room.
    {
        let j = console.launch("agent-2").expect("launch");
        let victim = child_in(j, "hold").expect("child");
        // What the keeper told init: a duplicate of every job it made.
        let mut kept: Vec<(String, bool, Handle)> = Vec::new();
        let mut keep = Chan::new(keeper.init);
        while let Some((bytes, handle)) = keep.read(100) {
            if let (Ok((_, m)), Some(h)) = (decode::<pr::Keep<'_>>(&bytes, 1), handle) {
                kept.push((String::from(m.room.0), m.untrusted.0 == 1, h));
            }
        }
        assert!(kept.iter().any(|k| k.1), "the untrusted job was kept");
        assert!(kept.iter().any(|k| k.0 == "agent-2"), "the room was kept");
        // The keeper dies; the room does not.
        job_kill(keeper.job).expect("kill the keeper");
        wait_exit(keeper.process);
        assert_eq!(processes_of_job(j), 1);
        assert_err!(channel_write(console.end, b"x", &[]), Status::PeerClosed);
        // A new keeper, with the old one's jobs queued before it starts.
        let (init_mine, init_theirs) = channel_create().expect("channel");
        for (room, untrusted, h) in &kept {
            let mut b = [0u8; 128];
            let n = encode(
                &pr::Keep {
                    untrusted: Below(u32::from(*untrusted)),
                    room: Text(if *untrusted { "" } else { room }),
                },
                0,
                &mut b,
            )
            .expect("encode");
            let entry = ChannelEntry {
                handle: *h,
                rights: JobObject::RIGHTS,
                transfer: Transfer::Duplicate,
            };
            channel_write(init_mine, &b[..n], &[entry]).expect("keep");
        }
        let second = start_keeper(session, &config, init_mine, init_theirs);
        let mut console2 = attach(&second, pr::attach::MANAGE_CONSOLE, "");
        // It knows the room: usage shows it, a second job is not made.
        let rec = console2.usage("agent-2");
        assert_eq!(rec.len(), 1, "the room is known after a restart");
        assert_eq!(rec[0].procs_used, 1);
        let tx = console2.send(&pr::Stop {
            room: Text("agent-2"),
        });
        console2.ok(tx).expect("stop");
        wait_exit(victim);
        assert!(
            eventually(|| processes_of_job(j) == 0),
            "Stop ended the room"
        );
        handle_close(victim).expect("close");
        handle_close(j).expect("close");
        keeper = second;
    }
    println!("t_rooms: INV-0269-3: a restarted keeper stops a room it did not start");

    let _ = (
        pa,
        pb,
        MapFlags::READ,
        memory_map as fn(_, _, _, _) -> _,
        format!(""),
    );
    job_kill(keeper.job).expect("end the keeper");
    println!("t_rooms: all passed");
    0
}

fn processes_of_job(job: Handle) -> u64 {
    job_get_usage(job, false).map_or(0, |u| u.processes.used)
}
