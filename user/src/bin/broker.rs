#![no_std]
#![no_main]

//! The POSIX broker (ADR-0104) as a process: the session's pids, groups and
//! terminal foreground, over `zoraya-broker`'s logic. `init` starts it in a
//! `posix` job with the root connection at slot 5, `ttyd`'s control channel
//! at 6, a terminal connection at 7 and the terminal's output stream's write
//! end at 8 (ADR-0105 item 1), its own job at `JOB` and the ramdisk at
//! `DIRECTORY` for `fsd`. It starts the temporary directory service, reads
//! `/etc/zoraya/session`, spawns the session's first process as pid 2, and
//! serves until that process is reaped and no other pid is live; then it
//! prints its statistics and ends, which ends the session.
//!
//! Arguments: `--time-base N`, the boot's base (ADR-0191); `--no-shell`, the
//! mode with no first shell (ADR-0153's note), in which slot 9 is a channel
//! on which the broker sends a client connection and the temporary
//! service's root, and serves for ever.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::posix::{self as abi, EntryKind, FdKind, NO_HANDLE, SELF_MESSAGE, slot};
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelEntry, DEADLINE_INFINITE, DIRECTORY, Disposition, ExitRecord, Handle, JOB,
    JobFlags, MapFlags, MemoryFlags, Mode, OBJECT_TYPES, Packet, RequestKind, Rights, Status,
    channel_signals, job_signals, process_signals,
};
use zoraya_broker::{Action, Broker, Conn, Kernel, session};
use zoraya_proto_tty as tp;
use zoraya_protocol::{Header, MAX_WIRE_MESSAGE, Message, NO_REPLY, decode, encode};
use zoraya_user::files::{File, read_all};
use zoraya_user::{
    channel_create, channel_read, channel_write, directory_lookup, handle_close, handle_duplicate,
    handle_type, image_create, job_create, job_kill, job_resume, job_signal, job_suspend,
    memory_create, memory_map, memory_unmap, object_get_signals, port_create, port_subscribe,
    port_unsubscribe, port_wait, println, process_exit_record, spawn, startup,
};

const ROOT: u32 = 5;
const CONTROL: u32 = 6;
const TERMINAL: u32 = 7;
const OUTPUT: u32 = 8;
const HANDOFF: u32 = 9;

const KEY_CONTROL: u64 = 1;
const KEY_TMPD: u64 = 2;
const KEY_CONN: u64 = 1 << 32;
const KEY_JOB: u64 = 2 << 32;

/// The temporary volume's size (ADR-0107 item 12, `[assumption]`).
const TMP_MIB: &str = "16";

fn slot_handle(n: u32) -> Handle {
    Handle::from_parts(n, 0).expect("a slot")
}

/// The rights a handle of its type can carry: what a moved entry lists.
fn rights_of(h: Handle) -> Rights {
    let name = handle_type(h).unwrap_or("");
    OBJECT_TYPES
        .iter()
        .find(|t| t.name == name)
        .map_or(Rights::NONE, |t| t.rights)
}

struct Rt;

impl Kernel for Rt {
    type H = Handle;

    fn job_create(&mut self, parent: Handle) -> Result<Handle, Status> {
        job_create(parent, JobFlags::POSIX)
    }
    fn duplicate(&mut self, h: Handle, rights: Rights) -> Result<Handle, Status> {
        handle_duplicate(h, rights)
    }
    fn close(&mut self, h: Handle) {
        let _ = handle_close(h);
    }
    fn channel_create(&mut self) -> Result<(Handle, Handle), Status> {
        channel_create()
    }
    fn job_signal(&mut self, job: Handle, kind: RequestKind, detail: u32) -> Result<(), Status> {
        job_signal(job, kind, detail)
    }
    fn job_kill(&mut self, job: Handle) -> Result<(), Status> {
        job_kill(job)
    }
    fn job_suspend(&mut self, job: Handle) -> Result<(), Status> {
        job_suspend(job)
    }
    fn job_resume(&mut self, job: Handle) -> Result<(), Status> {
        job_resume(job)
    }
    fn signals(&mut self, h: Handle) -> Result<u32, Status> {
        object_get_signals(h)
    }
    fn exit_record(&mut self, process: Handle) -> Result<ExitRecord, Status> {
        process_exit_record(process)
    }
    fn type_name(&mut self, h: Handle) -> Option<&'static str> {
        handle_type(h).ok()
    }
}

/// The value after `name` among the arguments.
fn option(name: &[u8]) -> Option<u64> {
    let mut argv = startup().argv;
    while let Some(a) = argv.next() {
        if a == name {
            return core::str::from_utf8(argv.next()?).ok()?.parse().ok();
        }
    }
    None
}

fn flag(name: &[u8]) -> bool {
    startup().argv.any(|a| a == name)
}

/// Waits for one message on `end` into `bytes`, with its handles.
fn read_one(end: Handle, bytes: &mut [u8], handles: &mut Vec<Handle>) -> Option<usize> {
    let port = port_create().ok()?;
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let r = port_subscribe(port, end, wanted, 1, Mode::Level).and_then(|()| {
        let mut packets = [Packet::default()];
        port_wait(port, DEADLINE_INFINITE, &mut packets)
    });
    let _ = handle_close(port);
    r.ok()?;
    let mut got = [end; 64];
    match channel_read(end, bytes, &mut got) {
        Ok(sizes) => {
            handles.extend_from_slice(&got[..sizes.handles as usize]);
            Some(sizes.bytes as usize)
        }
        Err(_) => None,
    }
}

/// A terminal connection for the first process's group (ADR-0105 item 4).
fn clone_tty(terminal: Handle, group: u32) -> Option<Handle> {
    let (mine, theirs) = channel_create().ok()?;
    let mut out = [0u8; 64];
    let n = encode(&tp::Clone { group }, 1, &mut out).ok()?;
    let entry = [ChannelEntry {
        handle: theirs,
        rights: rights_of(theirs),
        transfer: Transfer::Move,
    }];
    channel_write(terminal, &out[..n], &entry).ok()?;
    let mut reply = [0u8; 256];
    let mut handles = Vec::new();
    let n = read_one(terminal, &mut reply, &mut handles)?;
    (zoraya_tty::reply_status(&reply[..n]) == tp::status::OK).then_some(mine)
}

/// The program's image, `/bin/sh` through its link (ADR-0108).
fn load_image(root: &File, path: &str) -> Option<Handle> {
    let file = root
        .open_followed(
            path,
            0,
            zoraya_proto_file::rights::READ | zoraya_proto_file::rights::CLONE,
        )
        .ok()?;
    let length = file.stat().ok()?.size;
    let size = length.next_multiple_of(4096).max(4096);
    let object = memory_create(size, MemoryFlags::NONE).ok()?;
    let address = memory_map(object, 0, size, MapFlags::WRITE).ok()?;
    let mut at = 0u64;
    let mut ok = true;
    while at < length {
        let n = ((length - at) as usize).min(zoraya_proto_file::MAX_INLINE);
        // SAFETY: this process's mapping of `size` bytes, kept until the
        // unmap below, and `at + n` stays within `length`.
        let out = unsafe { core::slice::from_raw_parts_mut((address + at) as *mut u8, n) };
        match file.read(at, out) {
            Ok(0) | Err(_) => {
                ok = false;
                break;
            }
            Ok(got) => at += got as u64,
        }
    }
    let _ = memory_unmap(address, size);
    let image = if ok {
        image_create(object, length).ok()
    } else {
        None
    };
    let _ = handle_close(object);
    image
}

/// Starts the temporary directory service (ADR-0107 item 12) in `job`: the
/// root connection's client end.
fn start_tmpd(job: Handle, base: u64) -> Option<(Handle, Handle)> {
    let image = directory_lookup(DIRECTORY, "fsd")
        .map_err(|s| println!("broker: fsd: {s:?}"))
        .ok()?;
    let (client, server) = channel_create().ok()?;
    let mut buffer = vec![0u8; 1024];
    let mut e = Encoder::new(&mut buffer, job, image);
    e.handle(zoraya_user::dup(CONSOLE.slot(), CONSOLE, Rights::WRITE));
    e.handle(HandleEntry {
        slot: 6,
        handle: server,
        rights: rights_of(server),
        transfer: Transfer::Move,
    });
    let base = alloc::format!("{base}");
    for a in [
        &b"fsd"[..],
        b"--memory",
        TMP_MIB.as_bytes(),
        b"--base",
        base.as_bytes(),
    ] {
        e.arg(a);
    }
    let length = e.finish()?;
    let process = spawn(&buffer[..length])
        .map_err(|s| println!("broker: spawn fsd: {s:?}"))
        .ok();
    let _ = handle_close(image);
    Some((client, process?))
}

/// The session's first process: its record and its spawn (ADR-0104 item
/// 12, ADR-0107's "who builds it").
#[allow(clippy::too_many_arguments)] // Reason: one per part of the session.
fn spawn_first(
    first: &zoraya_broker::First<Handle>,
    image: Handle,
    root: &File,
    tmp: &File,
    program: &str,
    s: &session::Session,
    base: u64,
) -> Option<Handle> {
    let terminal = slot_handle(TERMINAL);
    let output = slot_handle(OUTPUT);
    let group = first.pid;
    // The message's handles: the three fds' terminal connections, then the
    // entries'.
    let mut handles = Vec::new();
    for _ in 0..3 {
        handles.push(clone_tty(terminal, group)?);
    }
    let tmp_root = tmp.clone_as(zoraya_proto_file::rights::ALL).ok()?;
    handles.push(tmp_root.end);
    core::mem::forget(tmp_root);
    handles.push(clone_tty(terminal, group)?);
    handles.push(handle_duplicate(image, rights_of(image)).ok()?);
    let root_clone = root.clone_as(zoraya_proto_file::rights::ALL).ok()?;
    handles.push(root_clone.end);
    core::mem::forget(root_clone);
    let fds: Vec<abi::Fd> = (0..3)
        .map(|fd| abi::Fd {
            fd,
            kind: FdKind::Tty,
            flags: 0,
            tty: fd as u8,
        })
        .collect();
    // Longest prefix first.
    let entries = [
        abi::Entry {
            prefix: "/proc/self/exe",
            kind: EntryKind::Image,
            handle: 5,
        },
        abi::Entry {
            prefix: "/dev/null",
            kind: EntryKind::Null,
            handle: NO_HANDLE,
        },
        abi::Entry {
            prefix: "/dev/tty",
            kind: EntryKind::Terminal,
            handle: 4,
        },
        abi::Entry {
            prefix: "/tmp",
            kind: EntryKind::Tmp,
            handle: 3,
        },
        abi::Entry {
            prefix: "/",
            kind: EntryKind::Directory,
            handle: 6,
        },
    ];
    let facts = abi::Facts {
        pid: first.pid,
        ppid: zoraya_broker::BROKER_PID,
        pgid: first.pid,
        sid: first.pid,
        uid: 1000,
        gid: 1000,
        umask: 0o022,
        ignored: 0,
        realtime_base: base,
    };
    let mut record = vec![0u8; MAX_WIRE_MESSAGE];
    let n = abi::encode(
        &facts,
        0,
        program,
        4,
        "",
        &fds,
        &entries,
        handles.len(),
        &mut record,
    )?;
    let (boot, boot_child) = channel_create().ok()?;
    let mut buffer = vec![0u8; zoraya_abi::spawn::MAX_SPAWN_SIZE];
    let mut e = Encoder::new(&mut buffer, first.job, image);
    // Every signal its default (ADR-0106 item 1): the ones that ignore by
    // default ignored in the kernel.
    for n in 1..=abi::signal::MAX {
        if let Some(kind) = abi::signal::kind(n) {
            let d = if abi::signal::default_terminates(n) {
                Disposition::Terminate
            } else {
                Disposition::Ignore
            };
            e.disposition(kind, d);
        }
    }
    for fd in 0..3 {
        e.handle(HandleEntry {
            slot: slot::of_fd(fd),
            handle: output,
            rights: rights_of(output),
            transfer: Transfer::Duplicate,
        });
    }
    for (at, h, rights) in [
        (slot::BOOTSTRAP.slot(), boot_child, rights_of(boot_child)),
        (
            slot::BROKER.slot(),
            first.broker_end,
            rights_of(first.broker_end),
        ),
        (
            slot::PROCESS_JOB.slot(),
            first.job,
            zoraya_broker::CHILD_JOB_RIGHTS,
        ),
    ] {
        e.handle(HandleEntry {
            slot: at,
            handle: h,
            rights,
            transfer: Transfer::Move,
        });
    }
    for w in &s.command {
        e.arg(w.as_bytes());
    }
    for v in session::environment(s) {
        e.env(v.as_bytes());
    }
    let length = e.finish()?;
    let process = spawn(&buffer[..length])
        .map_err(|s| println!("broker: spawn {program}: {s:?}"))
        .ok()?;
    let moved: Vec<ChannelEntry> = handles
        .iter()
        .map(|&h| ChannelEntry {
            handle: h,
            rights: rights_of(h),
            transfer: Transfer::Move,
        })
        .collect();
    channel_write(boot, &record[..n], &moved).ok()?;
    // TRANSFER to move it; the child receives WAIT alone.
    let me = handle_duplicate(process, Rights::WAIT.union(Rights::TRANSFER)).ok()?;
    let entry = [ChannelEntry {
        handle: me,
        rights: Rights::WAIT,
        transfer: Transfer::Move,
    }];
    channel_write(boot, SELF_MESSAGE, &entry).ok()?;
    let _ = handle_close(boot);
    Some(process)
}

/// The broker's port and its connections.
struct Server {
    port: Handle,
    control: Handle,
    ends: BTreeMap<Conn, Handle>,
    /// `Suspend` messages that came while waiting for a reply.
    queued: Vec<(u32, u32)>,
    transaction: u32,
    out: Vec<u8>,
}

impl Server {
    fn act(&mut self, core: &mut Broker<Rt>, actions: Vec<Action<Handle>>) {
        for a in actions {
            match a {
                Action::Reply { to, bytes, handles } => {
                    // A prepare's reply moves the two job handles at their
                    // own rights (ADR-0104 item 4), then a channel end.
                    let prepared = [
                        zoraya_broker::PARENT_JOB_RIGHTS,
                        zoraya_broker::CHILD_JOB_RIGHTS,
                    ];
                    let entries: Vec<ChannelEntry> = handles
                        .iter()
                        .enumerate()
                        .map(|(i, &h)| ChannelEntry {
                            handle: h,
                            rights: if handles.len() == 3 && i < 2 {
                                prepared[i]
                            } else {
                                rights_of(h)
                            },
                            transfer: Transfer::Move,
                        })
                        .collect();
                    let sent = self
                        .ends
                        .get(&to)
                        .is_some_and(|&end| channel_write(end, &bytes, &entries).is_ok());
                    if !sent {
                        for h in handles {
                            let _ = handle_close(h);
                        }
                    }
                }
                Action::Watch { conn, end } => {
                    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
                    let key = KEY_CONN + conn as u64;
                    if port_subscribe(self.port, end, wanted, key, Mode::Level).is_ok() {
                        self.ends.insert(conn, end);
                    } else {
                        let _ = handle_close(end);
                        let more = core.closed(conn);
                        self.act(core, more);
                    }
                }
                Action::Forget { conn } => {
                    if let Some(end) = self.ends.remove(&conn) {
                        let _ = port_unsubscribe(self.port, KEY_CONN + conn as u64);
                        let _ = handle_close(end);
                    }
                }
                Action::WatchJob { pid, job } => {
                    let key = KEY_JOB + u64::from(pid);
                    let _ =
                        port_subscribe(self.port, job, job_signals::TERMINATED, key, Mode::Level);
                }
                Action::UnwatchJob { pid } => {
                    let _ = port_unsubscribe(self.port, KEY_JOB + u64::from(pid));
                }
                Action::Foreground { group, job } => self.foreground(group, job),
            }
        }
    }

    /// A control request to `ttyd`, and its reply; a `Suspend` that comes
    /// meanwhile waits in `queued`.
    fn control_call<'m, M: Message<'m>>(&mut self, m: &M, moved: &[(Handle, Rights)]) {
        self.transaction = self.transaction.wrapping_add(1).max(1);
        let t = self.transaction;
        let Ok(n) = encode(m, t, &mut self.out) else {
            return;
        };
        let entries: Vec<ChannelEntry> = moved
            .iter()
            .map(|&(h, rights)| ChannelEntry {
                handle: h,
                rights,
                transfer: Transfer::Move,
            })
            .collect();
        if channel_write(self.control, &self.out[..n], &entries).is_err() {
            for &(h, _) in moved {
                let _ = handle_close(h);
            }
            return;
        }
        let mut bytes = vec![0u8; MAX_WIRE_MESSAGE];
        loop {
            let mut handles = Vec::new();
            let Some(n) = read_one(self.control, &mut bytes, &mut handles) else {
                return;
            };
            for h in handles {
                let _ = handle_close(h);
            }
            match Header::decode(&bytes[..n]) {
                Ok((h, _)) if h.ordinal == tp::Suspend::ORDINAL => {
                    if let Ok((_, s)) = decode::<tp::Suspend>(&bytes[..n], 0) {
                        self.queued.push((s.group, s.signal));
                    }
                }
                Ok((h, _)) if h.transaction == t => return,
                _ => {}
            }
        }
    }

    fn foreground(&mut self, group: u32, job: Handle) {
        self.control_call(
            &tp::Foreground { group },
            &[(job, zoraya_broker::FOREGROUND_RIGHTS)],
        );
    }
}

fn statistics(core: &Broker<Rt>) {
    let s = core.statistics();
    println!(
        "BROKER-STATS prepares={} commits={} aborts={} execs={} reaps={} kills={} others={} refused={} reaped_by_broker={} next_pid={}",
        s.prepares,
        s.commits,
        s.aborts,
        s.execs,
        s.reaps,
        s.kills,
        s.others,
        s.refused,
        core.counts.reaped_by_broker,
        s.next_pid
    );
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let base = option(b"--time-base").unwrap_or(0);
    let no_shell = flag(b"--no-shell");
    let Ok(session_job) = job_create(JOB, JobFlags::POSIX) else {
        println!("broker: no session job");
        return 1;
    };
    let Ok(tmp_job) = job_create(JOB, JobFlags::NONE) else {
        println!("broker: no job for the temporary service");
        return 1;
    };
    let Some((tmp_end, tmpd)) = start_tmpd(tmp_job, base) else {
        println!("broker: the temporary directory service did not start");
        return 1;
    };
    let tmp = File { end: tmp_end };
    let root = File {
        end: slot_handle(ROOT),
    };
    let control = slot_handle(CONTROL);
    let port = port_create().expect("port");
    let mut server = Server {
        port,
        control,
        ends: BTreeMap::new(),
        queued: Vec::new(),
        transaction: 0,
        out: vec![0; MAX_WIRE_MESSAGE],
    };
    let mut core = Broker::new(Rt, session_job, !no_shell);
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let _ = port_subscribe(port, control, wanted, KEY_CONTROL, Mode::Level);
    let _ = port_subscribe(
        port,
        tmpd,
        process_signals::TERMINATED,
        KEY_TMPD,
        Mode::Level,
    );
    if no_shell {
        let Ok((client, actions)) = core.add_client() else {
            println!("broker: no client connection");
            return 1;
        };
        server.act(&mut core, actions);
        let tmp_root = tmp.clone_as(zoraya_proto_file::rights::ALL).ok();
        let mut entries = vec![ChannelEntry {
            handle: client,
            rights: rights_of(client),
            transfer: Transfer::Move,
        }];
        if let Some(t) = tmp_root {
            entries.push(ChannelEntry {
                handle: t.end,
                rights: rights_of(t.end),
                transfer: Transfer::Move,
            });
            core::mem::forget(t);
        }
        let _ = channel_write(slot_handle(HANDOFF), b"ZPBC", &entries);
    } else {
        let text = root
            .open("etc/zoraya/session", 0, zoraya_proto_file::rights::READ)
            .ok()
            .and_then(|f| read_all(&f).ok())
            .unwrap_or_default();
        let text = String::from_utf8(text).unwrap_or_default();
        let s = match session::parse(&text) {
            Ok(s) => s,
            Err(e) => {
                println!("broker: /etc/zoraya/session line {}: {}", e.line, e.reason);
                session::Session::default()
            }
        };
        let program = session::program(&s);
        let Some(image) = load_image(&root, &program) else {
            println!("broker: {program}: cannot be loaded");
            return 1;
        };
        let (first, actions) = match core.first() {
            Ok(x) => x,
            Err(_) => return 1,
        };
        server.act(&mut core, actions);
        // The session job, for hangup (ADR-0105 item 3).
        if let Ok(job) = handle_duplicate(session_job, zoraya_broker::FOREGROUND_RIGHTS) {
            server.control_call(&tp::Session {}, &[(job, zoraya_broker::FOREGROUND_RIGHTS)]);
        }
        let Some(process) = spawn_first(&first, image, &root, &tmp, &program, &s, base) else {
            println!("broker: {program} did not start");
            core.abort_first();
            return 1;
        };
        let _ = handle_close(image);
        let actions = core.commit_first(process);
        server.act(&mut core, actions);
    }
    let mut bytes = vec![0u8; MAX_WIRE_MESSAGE];
    loop {
        let queued = core::mem::take(&mut server.queued);
        for (group, signal) in queued {
            core.suspend(group, signal);
        }
        if core.finished() {
            if let Some(e) = core.shell_exit {
                println!("broker: the session's first process ended: {e:?}");
            }
            statistics(&core);
            let _ = job_kill(tmp_job);
            return 0;
        }
        let mut packets = [Packet::default()];
        if port_wait(port, DEADLINE_INFINITE, &mut packets).is_err() {
            return 1;
        }
        let key = packets[0].key;
        if key == KEY_CONTROL {
            let mut handles = Vec::new();
            let mut got = [control; 4];
            match channel_read(control, &mut bytes, &mut got) {
                Ok(sizes) => {
                    handles.extend_from_slice(&got[..sizes.handles as usize]);
                    let n = sizes.bytes as usize;
                    if let Ok((_, s)) = decode::<tp::Suspend>(&bytes[..n], 0)
                        && Header::decode(&bytes[..n]).is_ok_and(|(h, _)| h.transaction == NO_REPLY)
                    {
                        core.suspend(s.group, s.signal);
                    }
                }
                Err((Status::PeerClosed, _)) => {
                    let _ = port_unsubscribe(port, KEY_CONTROL);
                }
                Err(_) => {}
            }
            for h in handles {
                let _ = handle_close(h);
            }
        } else if key == KEY_TMPD {
            println!("broker: the temporary directory service ended");
            let _ = port_unsubscribe(port, KEY_TMPD);
        } else if key >= KEY_JOB {
            let pid = (key - KEY_JOB) as u32;
            let actions = core.terminated(pid);
            server.act(&mut core, actions);
        } else if key >= KEY_CONN {
            let conn = (key - KEY_CONN) as Conn;
            let Some(&end) = server.ends.get(&conn) else {
                continue;
            };
            let mut got = [end; 64];
            match channel_read(end, &mut bytes, &mut got) {
                Ok(sizes) => {
                    let handles = got[..sizes.handles as usize].to_vec();
                    let actions = core.request(conn, &bytes[..sizes.bytes as usize], handles);
                    server.act(&mut core, actions);
                }
                Err((Status::WouldBlock, _)) => {}
                Err(_) => {
                    let actions = core.closed(conn);
                    server.act(&mut core, actions);
                }
            }
        }
    }
}
