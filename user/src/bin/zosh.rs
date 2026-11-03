#![no_std]
#![no_main]

//! The native shell (TASK-18, ADR-0150 to ADR-0158). The language, the line
//! editor and the settings file are host-tested crates (`zoraya-shell`,
//! `zoraya-settings`); this program gives them the system: the terminal, the
//! volume, programs from the ramdisk, jobs and the builtins that act on the
//! shell's own authority.
//!
//! With a channel at slot 6 the shell runs in a GUI terminal (ADR-0096): its
//! output and input go over that channel, and it does not read the console.
//!
//! Until TASK-18 step 7 the file commands (`ls`, `cat`, `mkdir`, `rm`, `mv`,
//! `stat`, `sync`) are built in, and programs print to the console: they get
//! no byte streams before step 8 (docs/design.md).

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, DEADLINE_INFINITE, DIRECTORY, DirectoryEntry, ExitRecord, Handle, JOB,
    JobFlags, MAX_HANDLES, Mode, ObjectType, POWER, Packet, RequestKind, Rights, Status,
    channel_signals, console_signals, job_signals,
};
use zoraya_command::Parsed;
use zoraya_describe::{Description, What};
use zoraya_proto_file::{kind as proto_kind, open_flags, write_flags};
use zoraya_proto_term::{Input as TermInput, MAX_OUTPUT, Output as TermOutput, Size as TermSize};
use zoraya_protocol::{Data, Header, NO_REPLY, decode, encode};
use zoraya_shell::editor::{self, Assist, Completion, Editor, Key, Options};
use zoraya_shell::eval::{
    self, Exec, Found, Host, Io, LANGUAGE, Ran, Shell, Sink, State, function_description,
};
use zoraya_shell::record;
use zoraya_shell::syntax;
use zoraya_user::descriptions;
use zoraya_user::files::{ALL, DIRECTORY as OPEN_DIRECTORY, File, FsResult, read_all};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, console_dropped, console_entry, console_read,
    console_write, directory_entry, directory_lookup, dup, handle_close, handle_type, job_create,
    job_kill, job_signal, port_create, port_subscribe, port_unsubscribe, port_wait, power_off,
    process_exit, process_exit_record, spawn,
};

zoraya_user::describe!("system", "zosh");

const CONSOLE_KEY: u64 = 1;
/// Port keys of jobs start here.
const JOB_KEY: u64 = 1000;
/// Jobs at once.
const MAX_JOBS: usize = 16;
/// Ctrl-Cs after which a foreground job that will not end is killed.
const PATIENCE: u32 = 3;
/// Programs that start others: they get their job and the directory too.
const NEED_MORE: [&str; 3] = ["fanout", "parent", "posixrun"];
/// Programs that use files: they get a connection at slot 5.
const NEED_VOLUME: [&str; 4] = ["crashwriter", "crashcheck", "fsbench", "posixrun"];
/// Where the configuration lives (ADR-0157).
const SETTINGS: &str = "/config/settings";
const START: &str = "/config/start.zosh";
const HISTORY: &str = "/config/history";
/// The first line of every interactive shell (ADR-0154 rule 9).
const GREETING: &str = "Type help to begin, or help tour.\n";

/// Builtins this program runs (ADR-0155 item 8), and, until step 7, the
/// file commands.
const BUILTINS: [&str; 16] = [
    "cd", "pwd", "history", "help", "grants", "jobs", "fg", "bg", "wait", "kill", "settings",
    "slots", "uptime", "exit", "poweroff", "reboot",
];
const FILE_COMMANDS: [&str; 7] = ["ls", "cat", "mkdir", "rm", "mv", "stat", "sync"];
/// The help topics (ADR-0156 item 4), in the order `help` lists them.
const TOPICS: [&str; 7] = [
    "language", "grants", "jobs", "keys", "settings", "errors", "tour",
];

/// The exit code when the terminal is gone: `init` tells it from `exit`'s 0,
/// which it answers with a new shell (ADR-0155's Decision).
const TERMINAL_GONE: i32 = 1;

/// The terminal's channel, if the shell has one: 0 is none.
static TERMINAL: AtomicU64 = AtomicU64::new(0);
/// How long output waits for room in the terminal's queue.
const OUTPUT_WAIT_NS: u64 = 5_000_000_000;

fn terminal() -> Option<Handle> {
    Handle::new(TERMINAL.load(Ordering::Relaxed))
}

/// Sends output to the terminal, waiting while its queue is full. A
/// terminal that is gone ends the shell with [`TERMINAL_GONE`]: nobody could
/// see it.
fn terminal_write(end: Handle, bytes: &[u8]) -> fmt::Result {
    let mut out = [0u8; MAX_OUTPUT + 64];
    for chunk in bytes.chunks(MAX_OUTPUT) {
        let m = TermOutput { bytes: Data(chunk) };
        let n = encode(&m, NO_REPLY, &mut out).map_err(|_| fmt::Error)?;
        loop {
            match channel_write(end, &out[..n], &[]) {
                Ok(()) => break,
                Err(Status::WouldBlock) => {
                    let port = port_create().map_err(|_| fmt::Error)?;
                    let room = channel_signals::WRITABLE | channel_signals::PEER_CLOSED;
                    let _ = port_subscribe(port, end, room, 1, Mode::Level);
                    let mut packets = [Packet::default()];
                    let waited = port_wait(port, clock_monotonic() + OUTPUT_WAIT_NS, &mut packets);
                    let _ = handle_close(port);
                    if waited.is_err() {
                        return Err(fmt::Error);
                    }
                }
                Err(Status::PeerClosed) => process_exit(TERMINAL_GONE),
                Err(_) => return Err(fmt::Error),
            }
        }
    }
    Ok(())
}

/// Where the shell writes: its terminal, or the console. The console takes
/// `\n` alone; the kernel adds the `\r`.
fn show(s: &str) {
    if s.is_empty() {
        return;
    }
    let _ = match terminal() {
        Some(end) => terminal_write(end, s.as_bytes()),
        None => console_write(CONSOLE, s.as_bytes())
            .map(|_| ())
            .map_err(|_| fmt::Error),
    };
}

/// Joins `path` to the absolute `cwd` and normalises it lexically: `..`
/// drops the last component and stays at `/` (ADR-0153 rule 6).
fn resolve(cwd: &str, path: &str) -> String {
    let mut parts: Vec<&str> = Vec::new();
    let start = if path.starts_with('/') { "" } else { cwd };
    for p in start.split('/').chain(path.split('/')) {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut s = String::new();
    for p in parts {
        s.push('/');
        s.push_str(p);
    }
    if s.is_empty() {
        s.push('/');
    }
    s
}

/// `/a/b` as `("/a", "b")`.
fn split_last(abs: &str) -> (&str, &str) {
    match abs.rfind('/') {
        Some(0) => ("/", &abs[1..]),
        Some(i) => (&abs[..i], &abs[i + 1..]),
        None => ("/", abs),
    }
}

fn problem(status: u32) -> String {
    zoraya_command::file_problem(status)
}

/// A job started from a line.
struct Job {
    number: u32,
    job: Handle,
    process: Handle,
    line: String,
}

/// An output file a redirection opened.
struct Output {
    file: File,
}

struct Z {
    root: Option<File>,
    /// The current directory's absolute path.
    cwd: String,
    port: Handle,
    jobs: Vec<Job>,
    notices: Vec<String>,
    descriptions: BTreeMap<String, Rc<Description>>,
    found: BTreeMap<String, Found>,
    outputs: BTreeMap<u32, Output>,
    next_output: u32,
    settings: zoraya_settings::Values,
    declared: Vec<zoraya_describe::Setting>,
    /// The history the editor keeps, for `history`; `clear_history` asks
    /// the main loop to empty the editor's.
    history: Vec<String>,
    clear_history: bool,
    /// Keys read while the shell itself was busy.
    pending_interrupt: bool,
    dropped: u64,
    /// The last line asked `exit` with jobs running.
    exit_warned: bool,
    rows: usize,
    columns: usize,
    /// Keys read but not yet used: typed after an Enter, for the next prompt.
    pending: Vec<u8>,
}

impl Z {
    fn setting(&self, name: &str) -> String {
        self.settings.get(name).unwrap_or("").to_string()
    }

    fn home(&self) -> String {
        self.setting("shell.home")
    }

    /// The prompt: `shell.prompt` with `{cwd}` the current directory, `~`
    /// for home.
    fn prompt(&self) -> String {
        let home = self.home();
        let shown = if self.cwd == home {
            "~".to_string()
        } else if let Some(rest) = self.cwd.strip_prefix(&home).filter(|r| r.starts_with('/')) {
            format!("~{rest}")
        } else {
            self.cwd.clone()
        };
        self.setting("shell.prompt").replace("{cwd}", &shown)
    }

    fn abs(&self, path: &str) -> String {
        resolve(&self.cwd, path)
    }

    fn open(&self, path: &str, flags: u32) -> FsResult<File> {
        let root = self.root.as_ref().ok_or(zoraya_command::FILE_GONE)?;
        let abs = self.abs(path);
        match abs.strip_prefix('/') {
            Some("") | None => root.clone_as(ALL),
            Some(rel) => root.open(rel, flags, ALL),
        }
    }

    /// The directory holding `path`, and its last name.
    fn parent(&self, path: &str) -> FsResult<(File, String)> {
        let abs = self.abs(path);
        let (dir, name) = split_last(&abs);
        if name.is_empty() {
            return Err(zoraya_proto_file::status::INVALID_NAME);
        }
        Ok((self.open(dir, OPEN_DIRECTORY)?, name.into()))
    }

    fn entries(&self, path: &str) -> FsResult<Vec<(String, bool)>> {
        let dir = self.open(path, OPEN_DIRECTORY)?;
        let mut out: Vec<(String, bool)> = Vec::new();
        dir.list(&mut |name, kind| out.push((name.into(), kind == proto_kind::DIRECTORY)))?;
        out.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        Ok(out)
    }

    fn read_text(&self, path: &str) -> Result<String, String> {
        let f = self.open(path, 0).map_err(problem)?;
        let bytes = read_all(&f).map_err(problem)?;
        String::from_utf8(bytes).map_err(|_| "not UTF-8 text".into())
    }

    fn write_file(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let flags = open_flags::CREATE | open_flags::TRUNCATE;
        let f = self.open(path, flags).map_err(problem)?;
        if !bytes.is_empty() {
            f.write(0, 0, bytes).map_err(problem)?;
        }
        Ok(())
    }

    fn description(&mut self, name: &str) -> Option<Rc<Description>> {
        if let Some(d) = self.descriptions.get(name) {
            return Some(d.clone());
        }
        let groups = [
            descriptions::BUILTINS,
            descriptions::COMMANDS,
            descriptions::DEMOS,
            descriptions::TOPICS,
            descriptions::SYSTEM,
        ];
        let text = groups
            .iter()
            .flat_map(|g| g.iter())
            .find(|(n, _)| *n == name)
            .map(|(_, t)| *t)?;
        let d = Rc::new(zoraya_describe::parse(text.as_bytes()).ok()?);
        self.descriptions.insert(name.into(), d.clone());
        Some(d)
    }

    // --- Jobs ---

    fn free_number(&self) -> u32 {
        (1..)
            .find(|n| self.jobs.iter().all(|j| j.number != *n))
            .unwrap_or(1)
    }

    fn spawn_in_job(&self, image: Handle, exec: &Exec<'_>) -> Result<(Handle, Handle), Status> {
        let job = job_create(JOB, JobFlags::NONE)?;
        let mut buffer = alloc::vec![0u8; 16 * 1024];
        let mut e = Encoder::new(&mut buffer, job, image);
        let volume = if NEED_VOLUME.contains(&exec.name) {
            self.root.as_ref().and_then(|v| v.clone_as(ALL).ok())
        } else {
            None
        };
        if let Some(volume) = &volume {
            e.handle(HandleEntry {
                slot: 5,
                handle: volume.end,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            });
        }
        if NEED_MORE.contains(&exec.name) {
            let rights = Rights::WRITE.union(Rights::DUPLICATE);
            e.handle(dup(CONSOLE.slot(), CONSOLE, rights));
            e.handle(dup(JOB.slot(), job, Rights::CREATE_PROCESS));
            e.handle(dup(DIRECTORY.slot(), DIRECTORY, Rights::LOOKUP));
        } else {
            e.handle(console_entry());
        }
        for a in exec.args {
            e.arg(a.text.as_bytes());
        }
        for v in exec.env {
            e.env(v.as_bytes());
        }
        let result = match e.finish() {
            Some(length) => spawn(&buffer[..length]),
            None => Err(Status::InvalidArgument),
        };
        match result {
            Ok(process) => {
                core::mem::forget(volume);
                Ok((job, process))
            }
            Err(e) => {
                let _ = handle_close(job);
                Err(e)
            }
        }
    }

    /// The job with port key `key` ended: its record, and the job removed.
    fn take_ended(&mut self, key: u64) -> Option<(Job, ExitRecord)> {
        let at = self
            .jobs
            .iter()
            .position(|j| JOB_KEY + u64::from(j.number) == key)?;
        let job = self.jobs.remove(at);
        let _ = port_unsubscribe(self.port, key);
        let record = process_exit_record(job.process).unwrap_or_else(|_| record::exited(0));
        let _ = handle_close(job.process);
        let _ = handle_close(job.job);
        Some((job, record))
    }

    /// A background job's end, in ADR-0158 rule 6's words.
    fn notice(job: &Job, record: &ExitRecord) -> String {
        if record::is_success(record) {
            format!("[{}] done: {}\n", job.number, job.line)
        } else {
            format!("[{}] {}: {}\n", job.number, job.line, record::what(record))
        }
    }

    /// Reads what keys are waiting, without blocking. Ctrl-C while the
    /// shell itself is busy is remembered for `interrupted`.
    fn read_keys(&mut self) -> Vec<u8> {
        let mut keys = core::mem::take(&mut self.pending);
        if let Some(end) = terminal() {
            let mut message = [0u8; 512];
            let mut handles = [end; 1];
            loop {
                let length = match channel_read(end, &mut message, &mut handles) {
                    Ok(sizes) => {
                        for h in &handles[..sizes.handles as usize] {
                            let _ = handle_close(*h);
                        }
                        sizes.bytes as usize
                    }
                    Err((Status::WouldBlock, _)) => break,
                    Err(_) => process_exit(TERMINAL_GONE),
                };
                let bytes = &message[..length];
                match Header::decode(bytes).map(|(h, _)| h.ordinal) {
                    Ok(2) => {
                        if let Ok((_, m)) = decode::<TermInput<'_>>(bytes, 0) {
                            if m.dropped > 0 {
                                self.notices
                                    .push(format!("[input: {} bytes dropped]\n", m.dropped));
                            }
                            keys.extend_from_slice(m.bytes.0);
                        }
                    }
                    Ok(3) => {
                        if let Ok((_, m)) = decode::<TermSize>(bytes, 0) {
                            self.columns = (m.columns as usize).clamp(1, 500);
                            self.rows = (m.rows as usize).clamp(1, 500);
                        }
                    }
                    _ => {}
                }
            }
        } else {
            let mut bytes = [0; 64];
            loop {
                let count = console_read(CONSOLE, &mut bytes).unwrap_or(0);
                if count == 0 {
                    break;
                }
                keys.extend_from_slice(&bytes[..count]);
            }
            let dropped = console_dropped(CONSOLE).unwrap_or(0);
            if dropped != self.dropped {
                self.notices.push(format!(
                    "[input: {} bytes dropped]\n",
                    dropped - self.dropped
                ));
                self.dropped = dropped;
            }
        }
        keys
    }

    /// Waits for a foreground job, passing it Ctrl-C and Ctrl-\. Other
    /// keys are dropped: there is no stream to give them to (step 8).
    fn wait_foreground(&mut self, number: u32) -> (ExitRecord, bool) {
        let mut interrupts = 0;
        let mut ctrl_c = false;
        loop {
            // Keys typed ahead of the job reach it first.
            let mut keys = core::mem::take(&mut self.pending);
            if keys.is_empty() {
                let mut packets = [Packet::default(); 8];
                let Ok(count) = port_wait(self.port, DEADLINE_INFINITE, &mut packets) else {
                    continue;
                };
                for packet in &packets[..count] {
                    if packet.key == CONSOLE_KEY {
                        keys.extend(self.read_keys());
                    } else if let Some((job, record)) = self.take_ended(packet.key) {
                        if job.number == number {
                            return (record, ctrl_c);
                        }
                        self.notices.push(Z::notice(&job, &record));
                    }
                }
            }
            let Some(job) = self.jobs.iter().find(|j| j.number == number).map(|j| j.job) else {
                continue;
            };
            for key in keys {
                match key {
                    editor::CTRL_C => {
                        interrupts += 1;
                        ctrl_c = true;
                        show("^C\n");
                        if interrupts >= PATIENCE {
                            let _ = job_kill(job);
                        } else {
                            let _ = job_signal(job, RequestKind::Interrupt, 0);
                        }
                    }
                    editor::CTRL_BACKSLASH => {
                        show("^\\\n");
                        let _ = job_signal(job, RequestKind::Quit, 0);
                    }
                    // No stream to give other keys to yet (step 8).
                    _ => {}
                }
            }
        }
    }

    /// Waits until job `number` ends or `deadline` passes; reports ends.
    fn wait_for(&mut self, number: Option<u32>, deadline: u64) -> bool {
        loop {
            let done = match number {
                Some(n) => self.jobs.iter().all(|j| j.number != n),
                None => self.jobs.is_empty(),
            };
            if done {
                return true;
            }
            let mut packets = [Packet::default(); 8];
            let count = match port_wait(self.port, deadline, &mut packets) {
                Ok(c) => c,
                Err(_) => return false,
            };
            for packet in &packets[..count] {
                if packet.key == CONSOLE_KEY {
                    if self.read_keys().contains(&editor::CTRL_C) {
                        show("^C\n");
                        return false;
                    }
                } else if let Some((job, record)) = self.take_ended(packet.key) {
                    show(&Z::notice(&job, &record));
                }
            }
        }
    }

    /// `%N`, or the newest job.
    fn which_job(&self, word: Option<&str>) -> Result<u32, String> {
        match word {
            None => self
                .jobs
                .iter()
                .map(|j| j.number)
                .max()
                .ok_or_else(|| "no jobs".into()),
            Some(w) => {
                let n = w
                    .strip_prefix('%')
                    .and_then(zoraya_describe::parse_number)
                    .filter(|n| *n > 0)
                    .ok_or_else(|| format!("{w}: not a job; write %N"))?;
                let n = n as u32;
                if self.jobs.iter().any(|j| j.number == n) {
                    Ok(n)
                } else {
                    Err(format!("{w}: no such job"))
                }
            }
        }
    }

    // --- Builtins ---

    fn file_command(&mut self, name: &str, d: &Description, p: &Parsed<'_>, io: &mut Io) -> i32 {
        let ramdisk = name == "ls"
            && p.operand(d, "DIR")
                .is_some_and(|dir| self.abs(dir) == "/zoraya/bin");
        if ramdisk {
            // The ramdisk's programs, until programs come from the volume
            // (ADR-0108): descriptions are not programs.
            for n in ramdisk_programs() {
                io.out.push_str(&n);
                io.out.push('\n');
            }
            return 0;
        }
        if self.root.is_none() {
            io.err.push_str(&zoraya_command::error(
                name,
                None,
                "there is no volume",
                &[],
            ));
            return 1;
        }
        let mut failed = false;
        let mut fail = |io: &mut Io, subject: &str, status: u32, hints: &[&str]| {
            io.err.push_str(&zoraya_command::error(
                name,
                Some(subject),
                &problem(status),
                hints,
            ));
            failed = true;
        };
        match name {
            "ls" => {
                let dir = p.operand(d, "DIR").unwrap_or(".");
                match self.entries(dir) {
                    Ok(list) => {
                        for (n, is_dir) in list {
                            io.out.push_str(&editor::printable(&n));
                            io.out.push_str(if is_dir { "/\n" } else { "\n" });
                        }
                    }
                    Err(s) => fail(io, dir, s, &[]),
                }
            }
            "cat" => {
                for file in p.operand_values(d, "FILE") {
                    let read = self.open(file, 0).and_then(|f| read_all(&f));
                    match read {
                        Ok(bytes) => match core::str::from_utf8(&bytes) {
                            Ok(text) => {
                                io.out.push_str(text);
                                if !text.is_empty() && !text.ends_with('\n') {
                                    io.out.push('\n');
                                }
                            }
                            Err(_) => io
                                .out
                                .push_str(&format!("{} bytes of binary\n", bytes.len())),
                        },
                        Err(s) => fail(io, file, s, &[]),
                    }
                }
            }
            "mkdir" => {
                for dir in p.operand_values(d, "DIR") {
                    let made = self.parent(dir).and_then(|(f, n)| f.make_directory(&n));
                    if let Err(s) = made {
                        fail(io, dir, s, &[]);
                    }
                }
            }
            "rm" => {
                let recursive = p.flag(d, "recursive");
                for path in p.operand_values(d, "PATH") {
                    let result = if recursive {
                        self.remove_tree(path)
                    } else {
                        self.parent(path).and_then(|(f, n)| f.remove(&n))
                    };
                    if let Err(s) = result {
                        let hint = format!("rm --recursive {path} removes it and everything in it");
                        let hints: &[&str] = if s == zoraya_proto_file::status::NOT_EMPTY {
                            &[&hint]
                        } else {
                            &[]
                        };
                        fail(io, path, s, hints);
                    }
                }
            }
            "mv" => {
                let (from, to) = (
                    p.operand(d, "FROM").unwrap_or(""),
                    p.operand(d, "TO").unwrap_or(""),
                );
                let moved = self.parent(from).and_then(|(a, an)| {
                    let (b, bn) = self.parent(to)?;
                    a.rename(&an, &b, &bn)
                });
                if let Err(s) = moved {
                    fail(io, from, s, &[]);
                }
            }
            "stat" => {
                for path in p.operand_values(d, "PATH") {
                    match self.open(path, 0).and_then(|f| f.stat()) {
                        Ok(s) => {
                            let kind = match s.kind {
                                proto_kind::DIRECTORY => "directory",
                                proto_kind::LINK => "link",
                                _ => "file",
                            };
                            io.out.push_str(&format!(
                                "{kind} size {} mtime {} ctime {} mode {:o}\n",
                                s.size,
                                s.modified / 1_000_000_000,
                                s.changed / 1_000_000_000,
                                s.mode
                            ));
                        }
                        Err(s) => fail(io, path, s, &[]),
                    }
                }
            }
            "sync" => {
                if let Some(Err(s)) = self.root.as_ref().map(File::sync) {
                    io.err
                        .push_str(&zoraya_command::error("sync", None, &problem(s), &[]));
                    failed = true;
                }
            }
            _ => {}
        }
        i32::from(failed)
    }

    fn remove_tree(&self, path: &str) -> FsResult<()> {
        let abs = self.abs(path);
        if let Ok(list) = self.entries(&abs) {
            for (name, _) in list {
                self.remove_tree(&format!("{abs}/{name}"))?;
            }
        }
        let (dir, name) = self.parent(&abs)?;
        dir.remove(&name)
    }

    fn help(&mut self, state: &State, d: &Description, p: &Parsed<'_>, io: &mut Io) -> i32 {
        if let Some(word) = p.value(d, "search") {
            let mut found = Vec::new();
            for (name, summary) in self.commands(state) {
                if name.contains(word) || summary.contains(word) {
                    found.push((name, summary));
                }
            }
            for t in TOPICS {
                if let Some(d) = self.description(t)
                    && (t.contains(word) || d.summary.contains(word))
                {
                    found.push((t.into(), d.summary.clone()));
                }
            }
            if found.is_empty() {
                io.err.push_str(&zoraya_command::error(
                    "help",
                    Some(word),
                    "no page matches",
                    &[],
                ));
                return 1;
            }
            let text = self.listing(state, &found, "");
            io.out.push_str(&text);
            return 0;
        }
        let Some(name) = p.operand(d, "NAME") else {
            io.out.push_str("Topics: ");
            io.out.push_str(&TOPICS.join(" "));
            io.out.push_str("\nCommands:\n");
            let list = self.commands(state);
            let text = self.listing(state, &list, "  ");
            io.out.push_str(&text);
            let demos: Vec<&str> = descriptions::DEMOS.iter().map(|(n, _)| *n).collect();
            io.out
                .push_str(&format!("Demonstrations: {}\n", demos.join(" ")));
            return 0;
        };
        let (page_d, first) = if let Some(f) = state.function(name) {
            let origin = state.function_origin(name).unwrap_or("the prompt");
            (
                Rc::new(function_description(&f)),
                format!("{name} is a function, from {origin}"),
            )
        } else if LANGUAGE.contains(&name) || BUILTINS.contains(&name) {
            let d = match eval::language_description(name) {
                Some(d) => Rc::new(d),
                None => match self.description(name) {
                    Some(d) => d,
                    None => return 1,
                },
            };
            (d, format!("{name} is built into the shell"))
        } else if FILE_COMMANDS.contains(&name) {
            match self.description(name) {
                Some(d) => (
                    d,
                    format!("{name} is built into the shell until it becomes a program"),
                ),
                None => return 1,
            }
        } else if let Some(d) = self.description(name) {
            let first = match d.what {
                What::Topic => format!("{name} is a help topic"),
                What::Command => format!("{name} is a program, /zoraya/bin/{name}"),
            };
            (d, first)
        } else {
            let mut hints = Vec::new();
            let mut names: Vec<String> = self.commands(state).into_iter().map(|(n, _)| n).collect();
            names.extend(TOPICS.iter().map(|t| t.to_string()));
            let near = eval::nearest(name, &names);
            let hint = format!("did you mean {}?", near.join(" or "));
            if !near.is_empty() {
                hints.push(hint.as_str());
            }
            io.err.push_str(&zoraya_command::error(
                "help",
                Some(name),
                "no such page",
                &hints,
            ));
            return 1;
        };
        let page = zoraya_command::page(&page_d, &first);
        if io.terminal {
            self.pager(&page);
        } else {
            io.out.push_str(&page);
        }
        0
    }

    /// Lines `NAME  SUMMARY`, names padded to the longest command name and
    /// two spaces, whatever is listed: `help` and `help --search` align.
    fn listing(&mut self, state: &State, items: &[(String, String)], indent: &str) -> String {
        let width = self
            .commands(state)
            .iter()
            .map(|(n, _)| n.chars().count())
            .max()
            .unwrap_or(0)
            + 2;
        let mut s = String::new();
        for (name, summary) in items {
            s.push_str(indent);
            s.push_str(name);
            for _ in name.chars().count()..width {
                s.push(' ');
            }
            s.push_str(summary);
            s.push('\n');
        }
        s
    }

    /// Every command `help` lists, with its summary, sorted.
    fn commands(&mut self, state: &State) -> Vec<(String, String)> {
        let mut names: Vec<&str> = BUILTINS.to_vec();
        names.extend(LANGUAGE);
        names.extend(FILE_COMMANDS);
        names.extend(descriptions::COMMANDS.iter().map(|(n, _)| *n));
        names.sort_unstable();
        names.dedup();
        let mut out: Vec<(String, String)> = names
            .into_iter()
            .filter_map(|n| {
                let d = eval::language_description(n)
                    .map(Rc::new)
                    .or_else(|| self.description(n))?;
                Some((n.to_string(), d.summary.clone()))
            })
            .collect();
        for f in state.function_names() {
            if let Some(func) = state.function(&f) {
                out.push((f.clone(), function_description(&func).summary));
            }
        }
        out.sort();
        out
    }

    /// Shows `page` a screen at a time (ADR-0156 item 5).
    fn pager(&mut self, page: &str) {
        let rows = self.rows.max(2) - 1;
        let lines: Vec<&str> = page.lines().collect();
        let mut at = 0;
        while at < lines.len() {
            let end = (at + rows).min(lines.len());
            let mut text = String::new();
            for l in &lines[at..end] {
                text.push_str(l);
                text.push('\n');
            }
            show(&text);
            at = end;
            if at >= lines.len() {
                break;
            }
            show("-- space: next page, q: quit --");
            let key = self.wait_key();
            show("\r\x1b[K");
            if key == b'q' || key == editor::CTRL_C {
                break;
            }
        }
    }

    /// Blocks for one key.
    fn wait_key(&mut self) -> u8 {
        loop {
            let mut packets = [Packet::default(); 8];
            let Ok(count) = port_wait(self.port, DEADLINE_INFINITE, &mut packets) else {
                continue;
            };
            for packet in &packets[..count] {
                if packet.key == CONSOLE_KEY {
                    if let Some(k) = self.read_keys().first() {
                        return *k;
                    }
                } else if let Some((job, record)) = self.take_ended(packet.key) {
                    self.notices.push(Z::notice(&job, &record));
                }
            }
        }
    }

    fn settings_builtin(&mut self, d: &Description, p: &Parsed<'_>, io: &mut Io) -> i32 {
        let bytes = self
            .open(SETTINGS, 0)
            .and_then(|f| read_all(&f))
            .unwrap_or_default();
        let declared = self.declared.clone();
        let fail = |io: &mut Io, subject: &str, problem: &str, hints: &[&str]| {
            io.err.push_str(&zoraya_command::error(
                "settings",
                Some(subject),
                problem,
                hints,
            ));
            1
        };
        let show_one = |io: &mut Io, s: &zoraya_describe::Setting, value: &str| {
            let default = if value == s.default {
                "   (default)"
            } else {
                ""
            };
            io.out.push_str(&format!(
                "{}{default}\n  {}: {}\n",
                zoraya_settings::line(&s.name, value),
                s.ty.word(),
                s.text
            ));
        };
        if p.flag(d, "check") {
            let v = zoraya_settings::read(&bytes, &declared);
            for pr in &v.problems {
                io.err.push_str(&format!(
                    "settings: {SETTINGS} line {}: {}\n",
                    pr.line, pr.problem
                ));
            }
            return i32::from(!v.problems.is_empty());
        }
        if let Some(name) = p.value(d, "reset") {
            if !declared.iter().any(|s| s.name == name) {
                return fail(io, name, "no such setting", &[]);
            }
            let new = zoraya_settings::reset(&bytes, name);
            if let Err(e) = self.write_settings(&new) {
                return fail(io, SETTINGS, &e, &[]);
            }
            return 0;
        }
        let all = p.flag(d, "all");
        let name = p.operand(d, "NAME");
        let value = p.operand(d, "VALUE");
        match (name, value) {
            (None, _) => {
                let v = zoraya_settings::read(&bytes, &declared);
                for s in &declared {
                    let current = v.get(&s.name).unwrap_or(&s.default);
                    if all || current != s.default {
                        io.out.push_str(&zoraya_settings::line(&s.name, current));
                        io.out.push('\n');
                    }
                }
                0
            }
            (Some(name), None) => {
                let Some(s) = declared.iter().find(|s| s.name == name) else {
                    return self.unknown_setting(io, name);
                };
                let v = zoraya_settings::read(&bytes, &declared);
                show_one(io, s, v.get(name).unwrap_or(&s.default));
                0
            }
            (Some(name), Some(value)) => match zoraya_settings::set(&bytes, &declared, name, value)
            {
                Ok(new) => match self.write_settings(&new) {
                    Ok(()) => 0,
                    Err(e) => fail(io, SETTINGS, &e, &[]),
                },
                Err(zoraya_settings::Refusal::Unknown(_)) => self.unknown_setting(io, name),
                Err(zoraya_settings::Refusal::Value(problem)) => fail(io, name, &problem, &[]),
            },
        }
    }

    fn unknown_setting(&self, io: &mut Io, name: &str) -> i32 {
        let near = zoraya_settings::nearest(name, &self.declared);
        let hint = format!("did you mean {}?", near.join(" or "));
        let hints: Vec<&str> = if near.is_empty() {
            Vec::new()
        } else {
            alloc::vec![&hint]
        };
        io.err.push_str(&zoraya_command::error(
            "settings",
            Some(name),
            "no such setting",
            &hints,
        ));
        1
    }

    /// Writes the settings file and applies it at once (ADR-0157 item 4).
    fn write_settings(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.write_file(SETTINGS, bytes)?;
        self.settings = zoraya_settings::read(bytes, &self.declared);
        Ok(())
    }

    fn grants(&mut self, p: &Parsed<'_>, io: &mut Io) -> i32 {
        let words: Vec<&str> = p.operands.iter().map(|(_, w)| *w).collect();
        let mut words = words.as_slice();
        while let [g @ ("+r" | "+w"), _, rest @ ..] = words {
            let _ = g;
            words = rest;
        }
        let Some(name) = words.first() else {
            io.err.push_str(&zoraya_command::error(
                "grants",
                None,
                "missing operand LINE",
                &[],
            ));
            return 2;
        };
        let shown = |s: &str| format!("  {s}\n");
        if FILE_COMMANDS.contains(name) || BUILTINS.contains(name) || LANGUAGE.contains(name) {
            io.out.push_str(&format!(
                "{name} is built into the shell: it acts with the shell's own access.\n"
            ));
            return 0;
        }
        io.out.push_str(&format!("{name} gets:\n"));
        io.out.push_str(&shown(
            "console              print (programs have no streams before TASK-18 step 8)",
        ));
        io.out.push_str(&shown("its own job"));
        if NEED_MORE.contains(name) {
            io.out
                .push_str(&shown("the ramdisk          start programs"));
        }
        if NEED_VOLUME.contains(name) {
            io.out.push_str(&shown(
                "/                    the whole volume (a test program)",
            ));
        }
        io.out
            .push_str("  (files named on its line are given from TASK-18 step 7)\n");
        0
    }
}

impl Host for Z {
    fn lookup(&mut self, name: &str) -> Found {
        if let Some(f) = self.found.get(name) {
            return *f;
        }
        let f = if BUILTINS.contains(&name) || FILE_COMMANDS.contains(&name) {
            Found::Builtin
        } else if name.is_empty()
            || name.ends_with(".cmd")
            || name.contains('/')
            || name.len() > zoraya_ramdisk_name_size()
        {
            Found::Nothing
        } else {
            match directory_lookup(DIRECTORY, name) {
                Ok(image) => {
                    let _ = handle_close(image);
                    Found::Program
                }
                Err(_) => Found::Nothing,
            }
        };
        self.found.insert(name.into(), f);
        f
    }

    fn description(&mut self, name: &str) -> Option<Rc<Description>> {
        Z::description(self, name)
    }

    fn names(&mut self) -> Vec<String> {
        let mut n: Vec<String> = BUILTINS
            .iter()
            .chain(&FILE_COMMANDS)
            .map(|s| s.to_string())
            .collect();
        n.extend(descriptions::COMMANDS.iter().map(|(s, _)| s.to_string()));
        n.extend(descriptions::DEMOS.iter().map(|(s, _)| s.to_string()));
        n
    }

    fn builtin(
        &mut self,
        state: &mut State,
        name: &str,
        d: &Description,
        p: &Parsed<'_>,
        io: &mut Io,
    ) -> i32 {
        match name {
            n if FILE_COMMANDS.contains(&n) => self.file_command(n, d, p, io),
            "cd" => {
                let home = self.home();
                let target = p.operand(d, "DIR").unwrap_or(&home);
                let abs = self.abs(target);
                match self.open(&abs, OPEN_DIRECTORY) {
                    Ok(_) => {
                        self.cwd = abs;
                        0
                    }
                    Err(s) => {
                        io.err.push_str(&zoraya_command::error(
                            "cd",
                            Some(target),
                            &problem(s),
                            &[],
                        ));
                        1
                    }
                }
            }
            "pwd" => {
                io.out.push_str(&self.cwd);
                io.out.push('\n');
                0
            }
            "history" => {
                if p.flag(d, "clear") {
                    self.history.clear();
                    self.clear_history = true;
                    let _ = self.write_file(HISTORY, b"");
                } else {
                    for h in &self.history {
                        io.out.push_str(&editor::printable(h));
                        io.out.push('\n');
                    }
                }
                0
            }
            "help" => self.help(state, d, p, io),
            "grants" => self.grants(p, io),
            "jobs" => {
                for j in &self.jobs {
                    io.out
                        .push_str(&format!("[{}]  running   {}\n", j.number, j.line));
                }
                0
            }
            "wait" => {
                let which = match p.operand(d, "JOB") {
                    None => None,
                    Some(w) => match self.which_job(Some(w)) {
                        Ok(n) => Some(n),
                        Err(e) => {
                            io.err
                                .push_str(&zoraya_command::error("wait", None, &e, &[]));
                            return 1;
                        }
                    },
                };
                for n in core::mem::take(&mut self.notices) {
                    show(&n);
                }
                i32::from(!self.wait_for(which, DEADLINE_INFINITE))
            }
            "fg" => {
                let n = match self.which_job(p.operand(d, "JOB")) {
                    Ok(n) => n,
                    Err(_) => {
                        io.err.push_str(&zoraya_command::error(
                            "fg",
                            None,
                            "no job to continue",
                            &[],
                        ));
                        return 1;
                    }
                };
                let line = self
                    .jobs
                    .iter()
                    .find(|j| j.number == n)
                    .map(|j| j.line.clone())
                    .unwrap_or_default();
                show(&format!("{line}\n"));
                let (record, ctrl_c) = self.wait_foreground(n);
                if let Some(l) = record::screen_line(&line, &record, ctrl_c) {
                    show(&format!("{l}\n"));
                }
                i32::from(!record::is_success(&record))
            }
            "bg" => {
                io.err.push_str(&zoraya_command::error(
                    "bg",
                    None,
                    "no stopped job",
                    &["Ctrl-Z arrives with TASK-18 step 9"],
                ));
                1
            }
            "kill" => {
                let n = match self.which_job(p.operand(d, "JOB")) {
                    Ok(n) => n,
                    Err(e) => {
                        io.err
                            .push_str(&zoraya_command::error("kill", None, &e, &[]));
                        return 1;
                    }
                };
                let Some(job) = self.jobs.iter().find(|j| j.number == n).map(|j| j.job) else {
                    return 1;
                };
                let kind = if p.flag(d, "interrupt") {
                    Some(RequestKind::Interrupt)
                } else if p.flag(d, "hangup") {
                    Some(RequestKind::Hangup)
                } else if p.flag(d, "quit") {
                    Some(RequestKind::Quit)
                } else if p.flag(d, "kill") {
                    None
                } else {
                    Some(RequestKind::Terminate)
                };
                let _ = match kind {
                    Some(k) => job_signal(job, k, 0),
                    None => job_kill(job),
                };
                for n in core::mem::take(&mut self.notices) {
                    show(&n);
                }
                if !self.wait_for(Some(n), clock_monotonic() + 2_000_000_000) {
                    show(&format!("[{n}] still running; kill --kill %{n} ends it\n"));
                }
                0
            }
            "settings" => self.settings_builtin(d, p, io),
            "slots" => {
                io.out.push_str("slots:");
                for slot in 1..=u64::from(MAX_HANDLES) {
                    if let Some(Ok(t)) = Handle::new(slot).map(handle_type) {
                        io.out.push_str(&format!(" {slot}:{t}"));
                    }
                }
                io.out.push('\n');
                0
            }
            "uptime" => {
                let nanos = clock_monotonic();
                io.out.push_str(&format!(
                    "up {}.{:03} s\n",
                    nanos / 1_000_000_000,
                    nanos / 1_000_000 % 1000
                ));
                0
            }
            "exit" => {
                if !self.jobs.is_empty() && !self.exit_warned {
                    self.exit_warned = true;
                    let n = self.jobs.len();
                    io.err.push_str(&if n == 1 {
                        "1 job is still running; exit again to end it\n".to_string()
                    } else {
                        format!("{n} jobs are still running; exit again to end them\n")
                    });
                    return 1;
                }
                for j in &self.jobs {
                    let _ = job_kill(j.job);
                }
                show(&io.out);
                process_exit(0)
            }
            "poweroff" => {
                if let Some(root) = &self.root
                    && let Err(s) = root.sync()
                {
                    io.err.push_str(&zoraya_command::error(
                        "poweroff",
                        Some("sync"),
                        &problem(s),
                        &[],
                    ));
                }
                show(&core::mem::take(&mut io.err));
                let status = power_off(POWER);
                io.err.push_str(&zoraya_command::error(
                    "poweroff",
                    None,
                    &zoraya_command::status_problem(status),
                    &[],
                ));
                1
            }
            "reboot" => {
                io.err.push_str(&zoraya_command::error(
                    "reboot",
                    None,
                    "not available until the kernel has a reset call",
                    &["poweroff turns the machine off; ADR-0159 proposes the reset call"],
                ));
                1
            }
            _ => 2,
        }
    }

    fn program(&mut self, exec: &Exec<'_>) -> Ran {
        if !exec.redirects.is_empty() || !exec.grants.is_empty() {
            let what = if exec.grants.is_empty() {
                "redirection"
            } else {
                "a grant"
            };
            let msg = format!(
                "zosh: {}: {what} of a program needs byte streams or grants, which arrive with TASK-18 steps 7 and 8\n",
                exec.name
            );
            show(&msg);
            return Ran::Refused(1);
        }
        if self.jobs.len() >= MAX_JOBS {
            show("zosh: too many jobs\n");
            return Ran::Refused(1);
        }
        let image = match directory_lookup(DIRECTORY, exec.name) {
            Ok(i) => i,
            Err(s) => {
                show(&zoraya_command::error(
                    "zosh",
                    Some(exec.name),
                    &zoraya_command::status_problem(s),
                    &[],
                ));
                return Ran::Refused(1);
            }
        };
        let started = self.spawn_in_job(image, exec);
        let _ = handle_close(image);
        let (job, process) = match started {
            Ok(both) => both,
            Err(s) => {
                show(&zoraya_command::error(
                    "zosh",
                    Some(exec.name),
                    &zoraya_command::status_problem(s),
                    &[],
                ));
                return Ran::Refused(1);
            }
        };
        let number = self.free_number();
        let key = JOB_KEY + u64::from(number);
        if port_subscribe(self.port, job, job_signals::TERMINATED, key, Mode::Level).is_err() {
            let _ = job_kill(job);
            let _ = handle_close(process);
            let _ = handle_close(job);
            return Ran::Refused(1);
        }
        let line = exec.line.trim_end_matches(['&', ' ']).to_string();
        self.jobs.push(Job {
            number,
            job,
            process,
            line: line.clone(),
        });
        if exec.background {
            show(&format!("[{number}] {line}\n"));
            return Ran::Background(number);
        }
        let (record, ctrl_c) = self.wait_foreground(number);
        Ran::Ended { record, ctrl_c }
    }

    fn list(&mut self, dir: &str) -> Option<Vec<(String, bool)>> {
        let path = if dir.is_empty() { "." } else { dir };
        self.entries(path).ok()
    }

    fn read(&mut self, path: &str) -> Result<String, String> {
        self.read_text(path)
    }

    fn kind(&mut self, path: &str) -> Option<bool> {
        let s = self.open(path, 0).and_then(|f| f.stat()).ok()?;
        Some(s.kind == proto_kind::DIRECTORY)
    }

    fn open_output(&mut self, path: &str, append: bool) -> Result<u32, String> {
        let flags = open_flags::CREATE | if append { 0 } else { open_flags::TRUNCATE };
        let file = self.open(path, flags).map_err(problem)?;
        self.next_output += 1;
        self.outputs.insert(self.next_output, Output { file });
        Ok(self.next_output)
    }

    fn write(&mut self, sink: Sink, text: &str) {
        match sink {
            Sink::File(t) => {
                if let Some(o) = self.outputs.get(&t) {
                    let _ = o.file.write(0, write_flags::APPEND, text.as_bytes());
                }
            }
            _ => show(text),
        }
    }

    fn close_output(&mut self, token: u32) {
        self.outputs.remove(&token);
    }

    fn home(&mut self) -> String {
        Z::home(self)
    }

    fn interrupted(&mut self) -> bool {
        let mut keys = self.read_keys();
        // Keys after the last Ctrl-C wait for the prompt; those before it
        // go with what it ends.
        if let Some(at) = keys.iter().rposition(|k| *k == editor::CTRL_C) {
            keys.drain(..=at);
            self.pending_interrupt = true;
        }
        self.pending = keys;
        core::mem::take(&mut self.pending_interrupt)
    }
}

fn zoraya_ramdisk_name_size() -> usize {
    32
}

/// The programs in the ramdisk, sorted, without description entries.
fn ramdisk_programs() -> Vec<String> {
    let mut names = Vec::new();
    let mut record = [0; 128];
    let mut cookie = 0;
    while let Ok(size) = directory_entry(DIRECTORY, cookie, &mut record) {
        let Some(entry) = DirectoryEntry::decode(&record[..size.min(record.len())]) else {
            break;
        };
        if size == 0 {
            break;
        }
        if let Ok(name) = core::str::from_utf8(entry.name)
            && !name.ends_with(".cmd")
        {
            names.push(name.to_string());
        }
        if entry.next == 0 {
            break;
        }
        cookie = entry.next;
    }
    names.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    names
}

/// The editor's view of the shell.
struct Helper<'a> {
    shell: &'a mut Shell<Z>,
}

impl Assist for Helper<'_> {
    fn known(&mut self, name: &str) -> bool {
        self.shell.find(name) != Found::Nothing
    }

    fn incomplete(&mut self, text: &str) -> bool {
        matches!(syntax::parse(text), Err(syntax::Error::Incomplete))
    }

    fn complete(&mut self, line: &str, cursor: usize) -> Completion {
        let before = &line[..cursor];
        let start = before.rfind([' ', '\t']).map_or(0, |n| n + 1);
        let word = &before[start..];
        let segment_start = before.rfind([';', '|', '&', '{', '}']).map_or(0, |n| n + 1);
        let words: Vec<&str> = before[segment_start..start].split_whitespace().collect();
        let mut c = Completion {
            start,
            ..Completion::default()
        };
        let command_position = words.is_empty()
            || words
                .last()
                .is_some_and(|w| matches!(*w, "if" | "while" | "!" | "else" | "try"));
        if let Some(var) = word.strip_prefix('$') {
            for n in self.shell.state.variable_names() {
                if n.starts_with(var) {
                    c.candidates.push((format!("${n}"), None));
                }
            }
            return c;
        }
        if let Some(_n) = word.strip_prefix('%') {
            for j in &self.shell.host.jobs {
                let t = format!("%{}", j.number);
                if t.starts_with(word) {
                    c.candidates.push((t, Some(j.line.clone())));
                }
            }
            return c;
        }
        if command_position {
            let state = core::mem::take(&mut self.shell.state);
            let mut names: Vec<(String, String)> = self.shell.host.commands(&state);
            self.shell.state = state;
            for (n, _) in descriptions::DEMOS {
                names.push((n.to_string(), String::new()));
            }
            names.sort();
            names.dedup_by(|a, b| a.0 == b.0);
            for (n, _) in names {
                if n.starts_with(word) {
                    c.candidates.push((n, None));
                }
            }
            return c;
        }
        let command = words
            .iter()
            .find(|w| !w.starts_with('+') && !w.contains('='))
            .copied();
        if word.starts_with('-')
            && let Some(cmd) = command
        {
            let d = if let Some(f) = self.shell.state.function(cmd) {
                Some(Rc::new(function_description(&f)))
            } else {
                self.shell.builtin_description(cmd)
            };
            if let Some(d) = d {
                for (opt, text) in zoraya_command::completion_options(&d) {
                    if opt.starts_with(word) {
                        c.candidates.push((opt, Some(text)));
                    }
                }
            }
            return c;
        }
        let only_dirs = command == Some("cd");
        let (dir, prefix) = match word.rfind('/') {
            Some(i) => (&word[..=i], &word[i + 1..]),
            None => ("", word),
        };
        let path = if dir.is_empty() { "." } else { dir };
        let path = if let Some(rest) = path.strip_prefix('~') {
            format!("{}{rest}", self.shell.host.home())
        } else {
            path.to_string()
        };
        if let Ok(entries) = self.shell.host.entries(&path) {
            for (name, is_dir) in entries {
                if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.'))
                {
                    continue;
                }
                if only_dirs && !is_dir {
                    continue;
                }
                let mut t = format!("{dir}{name}");
                if is_dir {
                    t.push('/');
                }
                c.candidates.push((t, None));
            }
        }
        c
    }
}

fn options(z: &Z) -> Options {
    let number = |name: &str, default: usize| {
        zoraya_describe::parse_number(&z.setting(name)).map_or(default, |n| n.max(0) as usize)
    };
    Options {
        width: if terminal().is_some() {
            z.columns
        } else {
            number("console.columns", 80)
        },
        suggestions: z.setting("shell.suggestions") == "yes",
        colour: z.setting("shell.colour") == "yes",
        history_size: if z.setting("shell.history") == "yes" {
            number("shell.history-size", 1000)
        } else {
            0
        },
    }
}

/// What the editor wrote for a burst of keys. A redraw of the row (`\r`
/// and no line end) replaces the whole row, so of several in a row only
/// the last needs sending: on the serial line a full redraw per key would
/// cost a line of bytes for every key a script or a paste sends.
#[derive(Default)]
struct Screen {
    held: String,
}

impl Screen {
    fn is_redraw(s: &str) -> bool {
        s.starts_with('\r') && !s.contains('\n')
    }

    fn add(&mut self, s: &str) {
        if Screen::is_redraw(s) && Screen::is_redraw(&self.held) {
            self.held.clear();
        } else {
            self.flush();
        }
        self.held.push_str(s);
    }

    fn flush(&mut self) {
        show(&self.held);
        self.held.clear();
    }
}

/// Runs one entry typed at the prompt.
fn run_entry(shell: &mut Shell<Z>, entry: &str) {
    match syntax::parse(entry) {
        Ok(block) => {
            shell.run_line(entry, &block);
        }
        Err(syntax::Error::Incomplete) => show("zosh: the line ends too soon\n"),
        Err(syntax::Error::Invalid { at, problem, hint }) => {
            show(&editor::parse_error(entry, at, &problem, hint.as_deref()));
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = zoraya_user::command::start(DESCRIPTION);
    c.parse();
    let port = port_create().expect("port");
    match Handle::new(6).filter(|h| handle_type(*h) == Ok("channel")) {
        Some(end) => {
            TERMINAL.store(end.raw(), Ordering::Relaxed);
            let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
            port_subscribe(port, end, wanted, CONSOLE_KEY, Mode::Level)
                .expect("subscribe to the terminal");
        }
        None => port_subscribe(
            port,
            CONSOLE,
            console_signals::READABLE,
            CONSOLE_KEY,
            Mode::Level,
        )
        .expect("subscribe to the console"),
    }
    let root = Handle::new(5)
        .filter(|h| handle_type(*h) == Ok("channel"))
        .map(|end| File { end });
    let declared = c.d.settings.clone();
    let mut z = Z {
        root,
        cwd: "/".into(),
        port,
        jobs: Vec::new(),
        notices: Vec::new(),
        descriptions: BTreeMap::new(),
        found: BTreeMap::new(),
        outputs: BTreeMap::new(),
        next_output: 0,
        settings: zoraya_settings::read(b"", &declared),
        declared,
        history: Vec::new(),
        clear_history: false,
        pending_interrupt: false,
        dropped: 0,
        exit_warned: false,
        rows: 24,
        columns: 80,
        pending: Vec::new(),
    };
    // Settings (ADR-0157 item 5): each problem once, and the defaults apply.
    if z.root.is_some() {
        if let Ok(bytes) = z.open(SETTINGS, 0).and_then(|f| read_all(&f)) {
            z.settings = zoraya_settings::read(&bytes, &z.declared);
            for p in &z.settings.problems {
                show(&format!(
                    "zosh: {SETTINGS} line {}: {}\n",
                    p.line, p.problem
                ));
            }
        }
        let home = z.home();
        if z.open(&home, OPEN_DIRECTORY).is_ok() {
            z.cwd = home;
        } else {
            show(&format!("zosh: {home}: not found; starting in /\n"));
        }
    } else {
        show("zosh: there is no volume: the file commands will fail\n");
    }
    // Let a GUI terminal's Size arrive before the first prompt.
    let _ = z.read_keys();
    let mut editor = Editor::new(options(&z));
    if z.setting("shell.history") == "yes"
        && let Ok(text) = z.read_text(HISTORY)
    {
        editor.set_history(text.lines().map(String::from).collect());
    }
    let mut shell = Shell::new(z);
    if shell.host.root.is_some()
        && let Ok(text) = shell.host.read_text(START)
    {
        shell.run_script(START, &text, Vec::new());
    }
    show(GREETING);
    let mut out = String::new();
    loop {
        for n in core::mem::take(&mut shell.host.notices) {
            show(&n);
        }
        shell.host.history = editor.history().to_vec();
        editor.options = options(&shell.host);
        let prompt = shell.host.prompt();
        out.clear();
        editor.start(&prompt, &mut out);
        show(&out);
        let entry = 'read: loop {
            // Keys typed ahead of this prompt come first.
            let mut keys = core::mem::take(&mut shell.host.pending);
            if keys.is_empty() {
                let mut packets = [Packet::default(); 8];
                let Ok(count) = port_wait(port, DEADLINE_INFINITE, &mut packets) else {
                    continue;
                };
                for packet in &packets[..count] {
                    if packet.key == CONSOLE_KEY {
                        keys.extend(shell.host.read_keys());
                    } else if let Some((job, record)) = shell.host.take_ended(packet.key) {
                        shell.host.notices.push(Z::notice(&job, &record));
                    }
                }
            }
            if !shell.host.notices.is_empty() {
                // Output that comes while a line is edited goes above it.
                out.clear();
                editor.hide(&mut out);
                for n in core::mem::take(&mut shell.host.notices) {
                    out.push_str(&n);
                }
                editor.redraw(&mut out);
                show(&out);
            }
            let mut screen = Screen::default();
            for (i, k) in keys.iter().enumerate() {
                out.clear();
                let key = editor.key(*k, &mut out, &mut Helper { shell: &mut shell });
                screen.add(&out);
                match key {
                    Key::Line(entry) => {
                        screen.flush();
                        shell.host.pending = keys[i + 1..].to_vec();
                        break 'read Some(entry);
                    }
                    Key::Interrupt => {
                        screen.flush();
                        shell.host.pending = keys[i + 1..].to_vec();
                        break 'read None;
                    }
                    Key::Edited => {}
                }
            }
            screen.flush();
        };
        let Some(entry) = entry else { continue };
        if entry.trim().is_empty() {
            continue;
        }
        editor.remember(&entry);
        shell.host.history = editor.history().to_vec();
        if shell.host.setting("shell.history") == "yes" && shell.host.root.is_some() {
            let mut text = editor.history().join("\n");
            text.push('\n');
            let _ = shell.host.write_file(HISTORY, text.as_bytes());
        }
        // A second exit counts only right after the first (ADR-0158 rule 7).
        if entry.trim() != "exit" {
            shell.host.exit_warned = false;
        }
        run_entry(&mut shell, &entry);
        if core::mem::take(&mut shell.host.clear_history) {
            editor.set_history(Vec::new());
        }
    }
}
