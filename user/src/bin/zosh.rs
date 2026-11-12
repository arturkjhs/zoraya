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
//! Every program gets three byte streams (ADR-0152): `in`, `out` and `err`.
//! A pipeline is one job with one stream per `|`; every other stream end is
//! the shell's, which relays it from its event loop without ever blocking on
//! it: output to the terminal, a file or a `$(...)`, and input from typed
//! keys, a file or the output of the builtin that started the pipeline.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::rc::Rc;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;
use core::sync::atomic::{AtomicU64, Ordering};

use zoraya_abi::posix::{self, NO_HANDLE};
use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, DEADLINE_INFINITE, DIRECTORY, DirectoryEntry, ExitRecord, Handle, JOB,
    JobFlags, MAX_HANDLES, Mode, ObjectType, POWER, Packet, RequestKind, Rights, STREAM_ATOMIC,
    Status, StreamReadObject, StreamWriteObject, channel_signals, console_signals, job_signals,
    stream_signals,
};
use zoraya_abi::{ChannelEntry, native};
use zoraya_command::Parsed;
use zoraya_describe::{Description, What};
use zoraya_proto_file::{kind as proto_kind, open_flags, rights as file_rights, write_flags};
use zoraya_proto_term::{
    Input as TermInput, MAX_OUTPUT, MAX_TITLE, Output as TermOutput, Size as TermSize,
    Title as TermTitle,
};
use zoraya_protocol::{Data, Header, NO_REPLY, decode, encode};
use zoraya_shell::editor::{self, Assist, Completion, Editor, Key, Options};
use zoraya_shell::eval::{
    self, Exec, Found, Host, Io, LANGUAGE, Member, Pipe, Ran, Shell, Sink, State,
    function_description, pipeline_status,
};
use zoraya_shell::grants;
use zoraya_shell::jobs::{self, At, Exit, Jobs, State as JobState};
use zoraya_shell::record;
use zoraya_shell::syntax;
use zoraya_shell::syntax::Stream;
use zoraya_user::descriptions;
use zoraya_user::files::{ALL, DIRECTORY as OPEN_DIRECTORY, File, FsResult, read_all};
use zoraya_user::posix_child::{self, Broker};
use zoraya_user::{
    channel_create, channel_read, channel_write, clock_monotonic, console_dropped, console_entry,
    console_read, console_write, directory_entry, directory_lookup, dup, handle_close, handle_type,
    job_create, job_kill, job_resume, job_signal, job_suspend, object_get_signals, port_create,
    port_subscribe, port_unsubscribe, port_wait, power_off, power_reset, process_exit,
    process_exit_record, spawn, stream_create, stream_read, stream_write,
};

zoraya_user::describe!("system", "zosh");

/// The connections a POSIX program may hold open at once (ADR-0262 item 2).
const POSIX_NET_BUDGET: u32 = 2;

/// The roots every native program with a network grant may read (ADR-0228
/// item 3; the `tls.roots` setting of ADR-0229 is not built, so the path is
/// fixed).
const TLS_ROOTS: &str = "/system/tls/roots";

const CONSOLE_KEY: u64 = 1;
/// Port keys of jobs start here.
const JOB_KEY: u64 = 1000;
/// Jobs at once.
const MAX_JOBS: usize = 16;
/// How long a job in front may go on after Ctrl-C before the shell says
/// that the next Ctrl-C kills it (ADR-0158 rule 3).
const KILL_OFFER_AFTER: u64 = 1_000_000_000;
/// Programs that start others: they get their job and the directory too.
const NEED_MORE: [&str; 3] = ["fanout", "parent", "posixrun"];
/// Programs that use files: they get a connection at slot 5.
const NEED_VOLUME: [&str; 4] = ["crashwriter", "crashcheck", "fsbench", "posixrun"];
/// Where the configuration lives (ADR-0157).
const SETTINGS: &str = "/config/settings";
/// The room table (ADR-0172 point 1).
const ROOMS: &str = "/config/rooms";
/// The compositor's description, for the desktop's settings (ADR-0169).
const COMPD: &str = include_str!("../../describe/system/compd.cmd");
/// The network service's description, for the `net.*` settings (ADR-0229).
const NETD: &str = include_str!("../../describe/system/netd.cmd");
const START: &str = "/config/start.zosh";
const HISTORY: &str = "/config/history";
/// A line end inside one history entry, as `/config/history` keeps it: the
/// record separator, which no key types.
const ENTRY_BREAK: &str = "\u{1e}";
/// The first line of every interactive shell (ADR-0154 rule 9).
const GREETING: &str = "Type help to begin, or help tour.\n";

/// Builtins this program runs (ADR-0155 item 8), and, until step 7, the
/// file commands.
const BUILTINS: [&str; 17] = [
    "cd", "pwd", "history", "help", "grants", "jobs", "fg", "bg", "wait", "kill", "settings",
    "slots", "uptime", "exit", "poweroff", "reboot", "room",
];
/// File commands still built in: `sync` needs the `sync` right, which no
/// entry of a program's table carries (ADR-0155's open questions).
const FILE_COMMANDS: [&str; 1] = ["sync"];
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
    show_bytes(s.as_bytes());
}

/// [`show`] for bytes: a foreground program's output, passed as it is
/// (ADR-0154 rule 7).
fn show_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    let _ = match terminal() {
        Some(end) => terminal_write(end, bytes),
        None => console_write(CONSOLE, bytes)
            .map(|_| ())
            .map_err(|_| fmt::Error),
    };
}

/// A background job's output as the terminal gets it (ADR-0154 rule 7):
/// every control character but `\n` and `\t` becomes `?`.
fn tamed(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// Removes `name` from `dir`, and everything in it first.
fn remove_tree(dir: &File, name: &str) {
    if let Ok(sub) = dir.open(name, open_flags::DIRECTORY, ALL) {
        let mut names: Vec<String> = Vec::new();
        let _ = sub.list(&mut |n, _| names.push(n.into()));
        for n in names {
            remove_tree(&sub, &n);
        }
    }
    let _ = dir.remove(name);
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

/// A job's handles and what the shell keeps of it; its number, line and
/// state are its [`jobs::Entry`]'s.
struct Job {
    job: Handle,
    /// One per element of the pipeline, in order.
    processes: Vec<Handle>,
    /// Each element's name, whether it is a question command, and the read
    /// end of its `REPORT` channel (ADR-0153 rule 11).
    elements: Vec<(String, bool, Option<Handle>)>,
    /// What its output gave a `$(...)`.
    captured: Vec<u8>,
    /// Its POSIX elements (ADR-0153 rule 10): pid, group and process job.
    posix: Vec<(u32, u32, Handle)>,
    /// Its native elements' own jobs, inside the line's (ADR-0200 item 2).
    native: Vec<Handle>,
    /// The line's own directory on the temporary service, when a POSIX
    /// element has one.
    tmp: Option<String>,
}

impl Drop for Job {
    /// A background job's reports are not read (ADR-0158 rule 6 reports its
    /// end only): the channels go with it.
    fn drop(&mut self) {
        for (_, _, report) in &mut self.elements {
            if let Some(r) = report.take() {
                let _ = handle_close(r);
            }
        }
        for (_, _, job) in self.posix.drain(..) {
            let _ = handle_close(job);
        }
        for job in self.native.drain(..) {
            let _ = handle_close(job);
        }
    }
}

/// Where the keys typed to a job are in an escape sequence, which is
/// skipped.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Escape {
    No,
    /// After `ESC`.
    Started,
    /// `ESC [`, until a final byte.
    Csi,
    /// `ESC O`, for one more byte.
    Ss3,
}

/// One element of a pipeline, ready to start.
enum Ready {
    /// A native program: its image and start-up record.
    Native { image: Handle, table: Option<Table> },
    /// A POSIX program (ADR-0153 rule 10): its image, absolute path, the
    /// table of its bootstrap record with the current directory's index,
    /// and its report channel's ends.
    Posix {
        image: Handle,
        exe: String,
        entries: Vec<posix_child::Entry>,
        cwd_entry: u32,
        /// The current directory below that entry.
        cwd: String,
        report: (Handle, Handle),
        /// A connection to `netd` for the program (ADR-0225 item 2), made
        /// with `narrow` from a literal `url` operand.
        net: Option<Handle>,
    },
}

impl Ready {
    /// Gives back what was made for an element that will not start.
    fn give_back(self) {
        match self {
            Ready::Native { image, table } => {
                let _ = handle_close(image);
                if let Some(t) = table {
                    let _ = handle_close(t.startup);
                    let _ = handle_close(t.report);
                }
            }
            Ready::Posix {
                image,
                entries,
                report,
                net,
                ..
            } => {
                for h in [image, report.0, report.1]
                    .into_iter()
                    .chain(net)
                    .chain(entries.iter().filter_map(|e| e.handle))
                {
                    let _ = handle_close(h);
                }
            }
        }
    }
}

/// The description a POSIX program without one of the image's is planned
/// by: the current directory and the line's grants, nothing designated.
const UNDESCRIBED: &str = "name     posix-program
summary  a POSIX program without a description of the system image's
usage    posix-program [ARG...]
operand  [ARG...]  text  its arguments
";

/// How waiting for a job in front ended.
enum Waited {
    /// It ended: its entry, its records, and whether Ctrl-C interrupted it.
    Ended(Box<jobs::Entry<Job>>, Vec<ExitRecord>, bool),
    /// Ctrl-Z stopped it; it stays in the table as job `n`.
    Stopped(u32),
}

/// Port keys of the shell's stream ends: above every job's.
const STREAM_KEY: u64 = 1 << 32;
/// Most bytes one relay passes before the loop looks at other events.
const RELAY_CHUNK: usize = 4096;
/// Most bytes of a background job's unfinished line the shell holds.
const MAX_HELD_LINE: usize = 4096;
/// Most bytes a `$(...)` keeps of a program's output.
const MAX_CAPTURE: usize = syntax::MAX_SOURCE;

/// Where a relayed stream's bytes go.
enum Dest {
    Terminal,
    File(File),
    Capture,
}

/// A stream a job writes and the shell reads.
struct Relay {
    key: u64,
    job: u32,
    end: Handle,
    dest: Dest,
    /// A background job's line so far: its output is shown in whole lines.
    partial: Vec<u8>,
}

/// What the shell writes into a job's stream.
enum Source {
    /// The output of the builtin or function that started the pipeline.
    Text,
    /// A file, from this offset.
    File(File, u64),
    /// The keys typed while the job is in front.
    Keys,
}

/// A stream the shell writes and a job reads.
struct Feed {
    key: u64,
    job: u32,
    end: Handle,
    source: Source,
    pending: Vec<u8>,
    at: usize,
    /// No more will come: close once `pending` is written.
    closing: bool,
    /// The signals subscribed now.
    armed: u32,
}

/// A program's start-up channel, and the report channel's end the shell
/// reads after the job.
struct Table {
    startup: Handle,
    report: Handle,
}

/// An output file a redirection opened.
struct Output {
    file: File,
}

/// What the shell holds of the volume (ADR-0153; ADR-0242's condition):
/// the root with its rights (the console's shell), or the entries of the
/// start-up record a terminal's shell is given: its room's grants, or
/// home's. A path no entry covers is not the shell's to open or to give.
enum Held {
    Root(File),
    Table(Vec<HeldEntry>),
}

struct HeldEntry {
    /// Absolute and normal.
    path: String,
    file: File,
    /// What the connection allows: a tree entry's rights, or read's.
    rights: u32,
    dir: bool,
}

/// The rights of a `+w` entry (ADR-0153's `tree`) and of a `+r` one.
const TREE_RIGHTS: u32 = file_rights::ALL & !file_rights::PUBLISH;
const READ_RIGHTS: u32 = file_rights::READ | file_rights::CLONE;

impl Held {
    /// The table of a start-up record: each entry's rights found once, by
    /// asking for a tree entry's.
    fn table(ns: zoraya_user::namespace::Namespace) -> Held {
        let mut entries = Vec::new();
        for e in ns.entries {
            let Some(file) = e.file else { continue };
            let rights = match file.clone_as(TREE_RIGHTS) {
                Ok(_) => TREE_RIGHTS,
                Err(_) => READ_RIGHTS,
            };
            let dir = e.kind == native::EntryKind::Directory;
            entries.push(HeldEntry {
                path: e.path,
                file,
                rights,
                dir,
            });
        }
        Held::Table(entries)
    }

    /// The entry covering the absolute `abs`, and the path below it.
    fn covering(&self, abs: &str) -> Option<(&HeldEntry, String)> {
        let Held::Table(entries) = self else {
            return None;
        };
        let (i, rest) = zoraya_resolve::longest(entries.iter().map(|e| e.path.as_str()), abs)?;
        Some((&entries[i], String::from(rest)))
    }

    /// Whether the shell holds `abs`, to write when `write` (INV-0172-4).
    fn covers(&self, abs: &str, write: bool) -> bool {
        match self {
            Held::Root(_) => true,
            Held::Table(_) => self.covering(abs).is_some_and(|(e, rest)| {
                (e.dir || rest.is_empty()) && (!write || e.rights & file_rights::WRITE != 0)
            }),
        }
    }

    /// Opens `rel` (from `/`) with at most `wanted`: within the covering
    /// entry's rights in a table.
    fn open(&self, rel: &str, flags: u32, wanted: u32) -> FsResult<File> {
        match self {
            Held::Root(f) => f.open(rel, flags, wanted),
            Held::Table(_) => {
                let abs = format!("/{}", rel.trim_start_matches('/'));
                let (e, rest) = self
                    .covering(&abs)
                    .ok_or(zoraya_proto_file::status::ACCESS_DENIED)?;
                let w = wanted & e.rights;
                if rest.is_empty() {
                    e.file.clone_as(w)
                } else if e.dir {
                    e.file.open(&rest, flags, w)
                } else {
                    Err(zoraya_proto_file::status::ACCESS_DENIED)
                }
            }
        }
    }

    fn clone_as(&self, wanted: u32) -> FsResult<File> {
        match self {
            Held::Root(f) => f.clone_as(wanted),
            Held::Table(_) => self.open("", OPEN_DIRECTORY, wanted),
        }
    }

    /// As `open`, following links. A table follows them inside the
    /// covering entry only, through its own connection: a relative link
    /// (the toolchain's `tr -> busybox`) resolves as on the volume; an
    /// absolute one resolves from the entry, so it may not be found, and
    /// never leads out of the entry.
    fn open_followed(&self, rel: &str, flags: u32, wanted: u32) -> FsResult<File> {
        match self {
            Held::Root(f) => f.open_followed(rel, flags, wanted),
            Held::Table(_) => {
                let abs = format!("/{}", rel.trim_start_matches('/'));
                match self.covering(&abs) {
                    Some((e, rest)) if e.dir && !rest.is_empty() => {
                        e.file.open_followed(&rest, flags, wanted & e.rights)
                    }
                    _ => self.open(rel, flags, wanted),
                }
            }
        }
    }

    fn link_target(&self, abs: &str) -> FsResult<Option<String>> {
        match self {
            Held::Root(f) => f.link_target(abs),
            Held::Table(_) => Ok(None),
        }
    }

    fn sync(&self) -> FsResult<()> {
        match self {
            Held::Root(f) => f.sync(),
            Held::Table(entries) => {
                for e in entries
                    .iter()
                    .filter(|e| e.rights & file_rights::WRITE != 0)
                {
                    e.file.sync()?;
                }
                Ok(())
            }
        }
    }

    /// The volume's time base (ADR-0191), from any connection to it.
    fn time_base(&self) -> FsResult<u64> {
        match self {
            Held::Root(f) => f.time_base(),
            Held::Table(entries) => entries
                .first()
                .ok_or(zoraya_proto_file::status::ACCESS_DENIED)?
                .file
                .time_base(),
        }
    }

    /// The root, only when held whole.
    fn root(&self) -> Option<&File> {
        match self {
            Held::Root(f) => Some(f),
            Held::Table(_) => None,
        }
    }
}

struct Z {
    root: Option<Held>,
    /// The current directory's absolute path.
    cwd: String,
    port: Handle,
    /// The jobs, and the notices waiting for the next prompt (ADR-0158).
    jobs: Jobs<Job>,
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
    rows: usize,
    columns: usize,
    /// Keys read but not yet used: typed after an Enter, for the next prompt.
    pending: Vec<u8>,
    relays: Vec<Relay>,
    feeds: Vec<Feed>,
    next_stream: u64,
    /// The job in front, which gets the keys.
    foreground: Option<u32>,
    /// A line is being edited: background output goes above it.
    at_prompt: bool,
    /// The line typed to a foreground job so far, and an escape sequence
    /// being skipped.
    typed: Vec<u8>,
    escape: Escape,
    /// The session's broker beside the shell and its temporary service's
    /// root (ADR-0153 rule 10), from `init` at slots 7 and 8.
    broker: Option<Broker>,
    /// The shell's root network connection (slot 9, ADR-0221 item 4), from
    /// which a command that needs the network's status gets a narrowed one.
    net: Option<Handle>,
    /// The shell's connection to the random service (slot 10, ADR-0227
    /// item 3), from which each native program gets its own.
    random: Option<Handle>,
    tmp: Option<File>,
    next_tmp: u32,
    /// The `/tmp` directory of the line being started, once one is made.
    line_tmp_name: Option<String>,
    /// The boot's time base (ADR-0191), for POSIX programs' records.
    realtime_base: u64,
    /// Commands found on the volume through `shell.path`, by absolute path.
    on_volume: BTreeMap<String, String>,
    /// The room the shell is in (ADR-0172 point 3); `None` is `home`.
    room: Option<zoraya_settings::rooms::Room>,
    /// The room keeper (ADR-0269): slot 11, and whether it is a `Manage`
    /// channel (the console's shell: slot 12 is the way to a new one) or a
    /// `Room(R)` channel (a shell the panel launched in a room).
    keeper: Option<zoraya_user::roomsclient::Keeper>,
    keeper_manage: bool,
    /// The launch job of the room entered from here (a `Manage` shell): the
    /// parent of every line's job, so that the line counts against the room.
    room_job: Option<Handle>,
    /// The room's refusal counters (memory, processes) at the end of the last
    /// line, for the quota hint.
    refused_seen: (u64, u64),
}

impl Z {
    fn setting(&self, name: &str) -> String {
        self.settings.get(name).unwrap_or("").to_string()
    }

    fn home(&self) -> String {
        self.setting("shell.home")
    }

    /// The current directory as the prompt shows it: `~` for home.
    fn shown_cwd(&self) -> String {
        let home = self.home();
        if self.cwd == home {
            "~".to_string()
        } else if let Some(rest) = self.cwd.strip_prefix(&home).filter(|r| r.starts_with('/')) {
            format!("~{rest}")
        } else {
            self.cwd.clone()
        }
    }

    /// The prompt: `shell.prompt` with `{cwd}` the current directory, `~`
    /// for home.
    fn prompt(&self) -> String {
        // ADR-0172 point 3: `{room}` is the room's name, empty in `home`;
        // a prompt without it shows the room in front.
        let room = self.room.as_ref().map_or("", |r| r.name.as_str());
        let p = self.setting("shell.prompt");
        let p = if !room.is_empty() && !p.contains("{room}") {
            format!("{room} {p}")
        } else {
            p
        };
        p.replace("{room}", room)
            .replace("{cwd}", &self.shown_cwd())
    }

    /// Tells a GUI terminal the directory for its window's title (ADR-0168),
    /// out of band; on the console there is nothing to tell.
    fn send_title(&self) {
        let Some(end) = terminal() else { return };
        let shown = self.shown_cwd();
        let mut cut = shown.len().min(MAX_TITLE);
        while !shown.is_char_boundary(cut) {
            cut -= 1;
        }
        let m = TermTitle {
            text: Data(&shown.as_bytes()[..cut]),
        };
        let mut out = [0u8; MAX_TITLE + 64];
        if let Ok(n) = encode(&m, NO_REPLY, &mut out) {
            let _ = channel_write(end, &out[..n], &[]);
        }
    }

    fn abs(&self, path: &str) -> String {
        resolve(&self.cwd, path)
    }

    /// ADR-0153's hint for `abs`, which `who` would read or (`write`)
    /// change and this shell was not given (ADR-0242's condition); `None`
    /// when the shell holds it.
    fn not_given(&self, who: &str, abs: &str, write: bool, line: Option<&str>) -> Option<String> {
        let held = self.root.as_ref()?;
        if held.covers(abs, write) {
            return None;
        }
        let home = Z::home(self);
        let dir = if write { split_last(abs).0 } else { abs };
        let (verb, grant) = if write {
            ("change", "+w")
        } else {
            ("read", "+r")
        };
        let path = grants::shown(abs, &home);
        let dir = grants::shown(if dir.is_empty() { "/" } else { dir }, &home);
        let allow = match line {
            Some(l) => format!("{grant} {dir} {l}"),
            None => format!("put {grant} {dir} before {who} in the line"),
        };
        let mut s = format!(
            "zosh: {path}: not given to this shell\n  hint: {who} tried to {verb} {path}, which it was not given\n  to allow it: {allow}\n"
        );
        if let Some(r) = &self.room {
            s.push_str(&format!(
                "  this shell holds room {}'s grants and the toolchain: room shows them\n",
                r.name
            ));
        }
        Some(s)
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

    /// Writes `path` whole as a new unnamed file and publishes it over the
    /// old one in one commit (ADR-0157 item 4, ADR-0010 rule 7): a crash
    /// leaves the old file or the new, never a part.
    fn publish_file(&self, path: &str, bytes: &[u8]) -> Result<(), String> {
        let abs = self.abs(path);
        let (dir, name) = split_last(&abs);
        let dir = self.open(dir, OPEN_DIRECTORY).map_err(problem)?;
        let published = dir.create_unnamed().and_then(|file| {
            if !bytes.is_empty() {
                file.write(0, 0, bytes)?;
            }
            file.publish(&dir, name)
        });
        match published {
            // A tree entry cannot publish (ADR-0153's table): a shell that
            // holds one (ADR-0242's condition) writes the file in place.
            Err(zoraya_proto_file::status::ACCESS_DENIED) => {
                let file = self
                    .open(&abs, open_flags::CREATE | open_flags::TRUNCATE)
                    .map_err(problem)?;
                if !bytes.is_empty() {
                    file.write(0, 0, bytes).map_err(problem)?;
                }
                Ok(())
            }
            other => other.map_err(problem),
        }
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
            descriptions::TESTS,
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

    // --- Grants (ADR-0153) ---

    /// The description of program `name`, and whether it is the system
    /// image's own: only those designate (rule 9, rule 10, INV-0153-6).
    fn program_description(&mut self, name: &str) -> Option<(Rc<Description>, bool)> {
        // Test programs are described too, though help lists none of them.
        let ours = descriptions::COMMANDS
            .iter()
            .chain(descriptions::DEMOS)
            .chain(descriptions::TESTS)
            .any(|(n, _)| *n == name);
        if !ours {
            return None;
        }
        self.description(name).map(|d| (d, true))
    }

    /// The plan for a line running `exec`, or why it may not run.
    fn plan_for(&mut self, exec: &Exec<'_>) -> Result<Option<Vec<grants::Planned>>, String> {
        let Some((d, base)) = self.program_description(exec.name) else {
            if !exec.grants.is_empty() {
                let e = zoraya_command::error(
                    "zosh",
                    Some(exec.name),
                    "it reads no namespace, so a grant would give it nothing",
                    &["a test program gets only what TASK-11's shell gave it"],
                );
                return Err(e);
            }
            return Ok(None);
        };
        self.plan_with(exec, &d, base).map(Some)
    }

    /// The network connection the line gives its command (ADR-0224 item 2,
    /// `zoraya_shell::netgrant`): a value where a literal `url` is needed
    /// refuses the line.
    fn net_grant(&mut self, exec: &Exec<'_>) -> Result<Option<(u32, Vec<u8>)>, String> {
        let Some((d, base)) = self.program_description(exec.name) else {
            return Ok(None);
        };
        Self::net_grant_with(exec, &d, base, &self.cwd)
    }

    /// `net_grant` for a description given: the image's POSIX programs'
    /// as well (N4).
    fn net_grant_with(
        exec: &Exec<'_>,
        d: &Description,
        base: bool,
        cwd: &str,
    ) -> Result<Option<(u32, Vec<u8>)>, String> {
        let texts: Vec<&str> = exec.args.iter().map(|a| a.text.as_str()).collect();
        let parsed = match zoraya_command::parse(d, &texts) {
            Ok(zoraya_command::Outcome::Run(p)) => p,
            _ => zoraya_command::Parsed::default(),
        };
        let line = grants::Line {
            description: d,
            parsed: &parsed,
            args: exec.args,
            cwd,
            grants: exec.grants,
            base_system: base,
        };
        zoraya_shell::netgrant::net_for(&line).map_err(|r| {
            let hints: Vec<&str> = r.hint.iter().map(String::as_str).collect();
            zoraya_command::error("zosh", Some(&r.subject), &r.problem, &hints)
        })
    }

    /// The connection a POSIX program of the image gets for a literal `url`
    /// operand (ADR-0224 item 2, ADR-0225 item 2; N4): `narrow` from the
    /// shell's own, with `reach` and the scope of that host and port. None
    /// for a program the image does not describe, a line with no such
    /// operand, or a session without a network.
    fn posix_net(&mut self, exec: &Exec<'_>, path: &str) -> Result<Option<Handle>, String> {
        let trusted: Vec<Description> = descriptions::POSIX
            .iter()
            .filter_map(|(_, text)| zoraya_describe::parse(text.as_bytes()).ok())
            .collect();
        let Some(d) = grants::trusted_description(&trusted, path) else {
            return Ok(None);
        };
        let Some((rights, scope)) = Self::net_grant_with(exec, d, true, &self.cwd)? else {
            return Ok(None);
        };
        // Each POSIX program counts against an account of its own, carved
        // from the shell's (ADR-0262 item 2; Q3): what it opens does not use
        // up the shell's, or another program's, connections.
        Ok(self.net.and_then(|root| {
            zoraya_user::netclient::narrow_account(root, rights, POSIX_NET_BUDGET, &scope).ok()
        }))
    }

    /// The plan of a line running `exec` under description `d` (rules 1
    /// to 5); `base`: it is the system image's own.
    fn plan_with(
        &mut self,
        exec: &Exec<'_>,
        d: &Description,
        base: bool,
    ) -> Result<Vec<grants::Planned>, String> {
        let texts: Vec<&str> = exec.args.iter().map(|a| a.text.as_str()).collect();
        let parsed = match zoraya_command::parse(d, &texts) {
            Ok(zoraya_command::Outcome::Run(p)) => p,
            // `--help` or a usage mistake: the program says so; it needs
            // nothing but the current directory.
            _ => zoraya_command::Parsed::default(),
        };
        let home = Z::home(self);
        let line = grants::Line {
            description: d,
            parsed: &parsed,
            args: exec.args,
            cwd: &self.cwd,
            grants: exec.grants,
            base_system: base,
        };
        let planned = grants::plan(&line).and_then(|p| {
            // Rule 5: designated paths through link objects.
            let root = self.root.as_ref();
            grants::through_links(p, &self.cwd, &home, |abs| {
                root.and_then(|r| r.link_target(abs).ok().flatten())
            })
        });
        match planned {
            Ok(mut p) => {
                self.add_room_grants(&mut p);
                Ok(p)
            }
            Err(r) => {
                let hints: Vec<&str> = r.hint.iter().map(String::as_str).collect();
                Err(zoraya_command::error(
                    "zosh",
                    Some(&r.subject),
                    &r.problem,
                    &hints,
                ))
            }
        }
    }

    /// One merged entry's connection, from the shell's own root: `None`
    /// for a path that does not exist (a missing entry, rule 2).
    fn open_entry(&self, m: &grants::Merged) -> Result<Option<(File, native::EntryKind)>, String> {
        let fail = |s: u32| zoraya_command::error("zosh", Some(&m.path), &problem(s), &[]);
        let write = m.rights & file_rights::WRITE != 0;
        let abs = match &m.name {
            Some(name) => resolve(&m.path, name),
            None => m.path.clone(),
        };
        if let Some(hint) = self.not_given("the command", &abs, write, None) {
            return Err(hint);
        }
        if let Some(name) = &m.name {
            let dir = match self.open(&m.path, OPEN_DIRECTORY) {
                Ok(dir) => dir,
                // A name to make in a directory that is not there.
                Err(zoraya_proto_file::status::NOT_FOUND) => return Ok(None),
                Err(s) => return Err(fail(s)),
            };
            let narrowed = dir.narrow(name, m.rights).map_err(fail)?;
            return Ok(Some((narrowed, native::EntryKind::Directory)));
        }
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| fail(zoraya_command::FILE_GONE))?;
        let opened = match m.path.strip_prefix('/') {
            Some("") | None => root.clone_as(m.rights),
            Some(rel) => root.open(rel, 0, m.rights),
        };
        match opened {
            Ok(file) => {
                let kind = match file.stat() {
                    Ok(s) if s.kind == proto_kind::DIRECTORY => native::EntryKind::Directory,
                    Ok(_) => native::EntryKind::File,
                    Err(s) => return Err(fail(s)),
                };
                Ok(Some((file, kind)))
            }
            Err(zoraya_proto_file::status::NOT_FOUND) => Ok(None),
            Err(s) => Err(fail(s)),
        }
    }

    /// Opens every entry of `plan` and sends the start-up record (rule 8)
    /// with them on a new channel, whose other end the program gets.
    fn table(
        &self,
        plan: &[grants::Planned],
        net: Option<(u32, Vec<u8>)>,
    ) -> Result<Table, String> {
        let merged = grants::merge(plan);
        let gone = || zoraya_command::error("zosh", None, "too many entries or handles", &[]);
        let mut handles: Vec<ChannelEntry> = Vec::new();
        let mut kinds = Vec::new();
        for m in &merged {
            match self.open_entry(m) {
                Ok(Some((file, kind))) => {
                    handles.push(ChannelEntry {
                        handle: file.end,
                        rights: ChannelObject::RIGHTS,
                        transfer: Transfer::Move,
                    });
                    core::mem::forget(file);
                    kinds.push((kind, (handles.len() - 1) as u8));
                }
                Ok(None) => kinds.push((native::EntryKind::Missing, NO_HANDLE)),
                Err(e) => {
                    for h in &handles {
                        let _ = handle_close(h.handle);
                    }
                    return Err(e);
                }
            }
        }
        let close_all = |handles: &[ChannelEntry]| {
            for h in handles {
                let _ = handle_close(h.handle);
            }
        };
        let Ok((report_ours, report_theirs)) = channel_create() else {
            close_all(&handles);
            return Err(gone());
        };
        handles.push(ChannelEntry {
            handle: report_theirs,
            rights: ChannelObject::RIGHTS,
            transfer: Transfer::Move,
        });
        let mut slots = native::Slots {
            report: (handles.len() - 1) as u8,
            ..native::Slots::NONE
        };
        // A network connection for a base-system command, made from the
        // shell's own with `narrow`: `needs net-status` (ADR-0224 item 2)
        // gets the `status` right and no scope, so it can ask the state and
        // nothing else; a `url` operand (item 3, N3) gets `reach` and a
        // scope of exactly that host and port. Without a network, or if
        // `netd` refuses, the command gets none and says so.
        if let Some((rights, scope)) = net
            && let Some(root) = self.net
            && let Ok(connection) = zoraya_user::netclient::narrow(root, rights, &scope)
        {
            handles.push(ChannelEntry {
                handle: connection,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            });
            slots.net = (handles.len() - 1) as u8;
        }
        // Every program gets its own connection to the random service,
        // made through the shell's (ADR-0227 item 3): randomness designates
        // nothing, so there is no grant word. Without the service, none.
        if let Some(root) = self.random
            && let Some(connection) = zoraya_user::random::share(root)
        {
            handles.push(ChannelEntry {
                handle: connection,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            });
            slots.random = (handles.len() - 1) as u8;
        }
        let entries: Vec<native::Entry<'_>> = merged
            .iter()
            .zip(&kinds)
            .map(|(m, (kind, handle))| native::Entry {
                path: &m.path,
                kind: *kind,
                handle: *handle,
            })
            .collect();
        let mut bytes = alloc::vec![0u8; 64 * 1024];
        let Some(n) = native::encode(
            &self.cwd,
            &slots,
            &entries,
            handles.len(),
            self.realtime_base,
            &mut bytes,
        ) else {
            close_all(&handles);
            let _ = handle_close(report_ours);
            return Err(gone());
        };
        let Ok((ours, theirs)) = channel_create() else {
            close_all(&handles);
            let _ = handle_close(report_ours);
            return Err(gone());
        };
        let sent = channel_write(ours, &bytes[..n], &handles);
        let _ = handle_close(ours);
        if sent.is_err() {
            close_all(&handles);
            let _ = handle_close(report_ours);
            let _ = handle_close(theirs);
            return Err(gone());
        }
        Ok(Table {
            startup: theirs,
            report: report_ours,
        })
    }

    /// Rule 11: after a job, each refused path with the grant that allows it.
    fn hints(&self, report: Handle, name: &str, line: Option<&str>) {
        let mut seen: Vec<(String, bool)> = Vec::new();
        let mut bytes = alloc::vec![0u8; 8192];
        let mut handles = [report; 1];
        while let Ok(sizes) = channel_read(report, &mut bytes, &mut handles) {
            for h in &handles[..sizes.handles as usize] {
                let _ = handle_close(*h);
            }
            if let Some(r) = posix::Report::decode(&bytes[..sizes.bytes as usize]) {
                let write = r.access == posix::Access::Write;
                if !seen.iter().any(|(p, _)| p == r.path) {
                    seen.push((r.path.into(), write));
                }
            }
        }
        let _ = handle_close(report);
        let home = self.home();
        let mut dirs: Vec<String> = Vec::new();
        let mut text = String::new();
        for (path, write) in &seen {
            let dir = split_last(path).0.to_string();
            if dirs.contains(&dir) {
                continue;
            }
            dirs.push(dir.clone());
            if dirs.len() > 3 {
                continue;
            }
            let (verb, grant) = if *write {
                ("change", "+w")
            } else {
                ("read", "+r")
            };
            let shown = grants::shown(&dir, &home);
            let allow = match line {
                Some(line) => format!("{grant} {shown} {line}"),
                None => format!("put {grant} {shown} before {name} in the line"),
            };
            text.push_str(&format!(
                "  hint: {name} tried to {verb} {}, which it was not given\n  to allow it: {allow}\n",
                grants::shown(path, &home),
            ));
        }
        if dirs.len() > 3 {
            text.push_str(&format!("  and {} more\n", dirs.len() - 3));
        }
        show(&text);
    }

    /// One element's image and start-up table, and what it is given in
    /// `jobs`' words; or why it may not run.
    fn ready(&mut self, exec: &Exec<'_>) -> Result<(Ready, String), String> {
        if let Some(path) = self.on_volume.get(exec.name).cloned() {
            return self.ready_from_volume(exec, &path);
        }
        let mut given = String::new();
        let net = self.net_grant(exec)?;
        let table = match self.plan_for(exec)? {
            None => None,
            Some(plan) if self.root.is_none() && !plan.is_empty() => {
                return Err(zoraya_command::error(
                    exec.name,
                    None,
                    "there is no volume",
                    &[],
                ));
            }
            Some(plan) => {
                // ADR-0242's condition: an entry this shell was not given
                // is refused with ADR-0153's hint, before anything runs.
                // The line as the person wrote it: paths with `~`.
                let home = self.home();
                let line = core::iter::once(String::from(exec.name))
                    .chain(exec.args.iter().map(|a| grants::shown(&a.text, &home)))
                    .collect::<Vec<String>>()
                    .join(" ");
                for m in grants::merge(&plan) {
                    let write = m.rights & file_rights::WRITE != 0;
                    let abs = match &m.name {
                        Some(name) => resolve(&m.path, name),
                        None => m.path.clone(),
                    };
                    if let Some(hint) = self.not_given(exec.name, &abs, write, Some(&line)) {
                        return Err(hint);
                    }
                }
                // The roots for TLS come with a network that can be reached
                // (ADR-0228 item 3, ADR-0153 rule 1): read only, not checked
                // against the terminal's table because the network grant
                // implies it, and shown by `grants` as "with the network".
                let mut plan = plan;
                if net
                    .as_ref()
                    .is_some_and(|(rights, _)| rights & zoraya_abi::net::rights::REACH != 0)
                {
                    plan.push(grants::Planned {
                        path: String::from(TLS_ROOTS),
                        access: grants::Access::Read,
                        reason: String::from("with the network"),
                    });
                }
                given = grants::summary(&plan, &self.home());
                Some(self.table(&plan, net)?)
            }
        };
        match directory_lookup(DIRECTORY, exec.name) {
            Ok(image) => Ok((Ready::Native { image, table }, given)),
            Err(s) => {
                if let Some(t) = table {
                    let _ = handle_close(t.startup);
                    let _ = handle_close(t.report);
                }
                Err(zoraya_command::error(
                    "zosh",
                    Some(exec.name),
                    &zoraya_command::status_problem(s),
                    &[],
                ))
            }
        }
    }

    /// A program of the volume: native when it carries the mark (rule 9),
    /// with the current directory only, since no description of the
    /// image's names it; else a POSIX program through the broker.
    fn ready_from_volume(
        &mut self,
        exec: &Exec<'_>,
        path: &str,
    ) -> Result<(Ready, String), String> {
        let fail = |problem: &str| zoraya_command::error("zosh", Some(exec.name), problem, &[]);
        let root = self
            .root
            .as_ref()
            .ok_or_else(|| fail("there is no volume"))?;
        let file = root
            .open_followed(
                path.trim_start_matches('/'),
                0,
                file_rights::READ | file_rights::CLONE,
            )
            .map_err(|s| fail(&problem(s)))?;
        let (image, native) =
            posix_child::load(&file).map_err(|s| fail(&zoraya_command::status_problem(s)))?;
        drop(file);
        let undescribed = zoraya_describe::parse(UNDESCRIBED.as_bytes()).map_err(|_| fail("?"))?;
        if native {
            let plan = self.plan_with(exec, &undescribed, false);
            let table = plan.and_then(|p| self.table(&p, None));
            return match table {
                Ok(t) => Ok((
                    Ready::Native {
                        image,
                        table: Some(t),
                    },
                    String::new(),
                )),
                Err(e) => {
                    let _ = handle_close(image);
                    Err(e)
                }
            };
        }
        if self.broker.is_none() {
            let _ = handle_close(image);
            return Err(zoraya_command::error(
                "zosh",
                Some(exec.name),
                "a POSIX program, and this session has no broker to run it",
                &["the broker runs beside zosh on a volume with /bin (ADR-0153 rule 10)"],
            ));
        }
        let planned = self
            .posix_plan(exec, path)
            .and_then(|plan| self.posix_entries(exec, image, &plan).map(|t| (t, plan)));
        let ((entries, cwd_entry, cwd), plan) = match planned {
            Ok(x) => x,
            Err(e) => {
                let _ = handle_close(image);
                return Err(e);
            }
        };
        let report = match channel_create() {
            Ok(r) => r,
            Err(s) => {
                let _ = handle_close(image);
                for e in &entries {
                    if let Some(h) = e.handle {
                        let _ = handle_close(h);
                    }
                }
                return Err(fail(&zoraya_command::status_problem(s)));
            }
        };
        // A literal `url` operand of a POSIX program the image describes
        // designates, as for a native one (ADR-0224 item 2): the program
        // gets a connection to `netd` that reaches that host and port.
        let net = match self.posix_net(exec, path) {
            Ok(n) => n,
            Err(e) => {
                for h in [image, report.0, report.1]
                    .into_iter()
                    .chain(entries.iter().filter_map(|e| e.handle))
                {
                    let _ = handle_close(h);
                }
                return Err(e);
            }
        };
        let given = grants::summary(&plan, &self.home());
        Ok((
            Ready::Posix {
                image,
                exe: path.into(),
                entries,
                cwd_entry,
                cwd,
                report,
                net,
            },
            given,
        ))
    }

    /// A POSIX program's plan: by the description of the system image's
    /// that names `path` (INV-0153-6), or the current directory and the
    /// line's grants alone.
    fn posix_plan(&mut self, exec: &Exec<'_>, path: &str) -> Result<Vec<grants::Planned>, String> {
        let trusted: Vec<Description> = descriptions::POSIX
            .iter()
            .filter_map(|(_, text)| zoraya_describe::parse(text.as_bytes()).ok())
            .collect();
        match grants::trusted_description(&trusted, path) {
            Some(d) => {
                let d = d.clone();
                self.plan_with(exec, &d, true)
            }
            None => {
                let undescribed = zoraya_describe::parse(UNDESCRIBED.as_bytes())
                    .map_err(|_| String::from("zosh: a description of its own is refused\n"))?;
                self.plan_with(exec, &undescribed, false)
            }
        }
    }

    /// The table of a POSIX program's bootstrap record (ADR-0153 rule 10):
    /// what its plan designates, the current directory among it, then the
    /// toolchain set (`/bin`, `/usr`, `/etc` to read, `/dev/null`,
    /// `/proc/self/exe`) and the line's `/tmp`, longest prefix first. A
    /// file is given through a directory that covers it: the record has
    /// directory entries only.
    fn posix_entries(
        &mut self,
        exec: &Exec<'_>,
        image: Handle,
        plan: &[grants::Planned],
    ) -> Result<(Vec<posix_child::Entry>, u32, String), String> {
        use zoraya_abi::posix::EntryKind as P;
        let merged = grants::merge(plan);
        let mut entries: Vec<posix_child::Entry> = Vec::new();
        let close = |entries: &[posix_child::Entry]| {
            for e in entries {
                if let Some(h) = e.handle {
                    let _ = handle_close(h);
                }
            }
        };
        let mut files: Vec<String> = Vec::new();
        for m in &merged {
            match self.open_entry(m) {
                Ok(Some((file, native::EntryKind::Directory))) => {
                    entries.push(posix_child::Entry {
                        prefix: m.path.clone(),
                        kind: P::Directory,
                        handle: Some(file.end),
                    });
                    core::mem::forget(file);
                }
                Ok(Some((_, _))) => files.push(m.path.clone()),
                // Missing: the program finds it missing.
                Ok(None) => {}
                Err(e) => {
                    close(&entries);
                    return Err(e);
                }
            }
        }
        let root = self.root.as_ref();
        for dir in ["/bin", "/usr", "/etc"] {
            if entries.iter().any(|e| e.prefix == dir) {
                continue;
            }
            let wanted = file_rights::READ | file_rights::CLONE;
            if let Some(f) = root.and_then(|r| {
                r.open(dir.trim_start_matches('/'), open_flags::DIRECTORY, wanted)
                    .ok()
            }) {
                entries.push(posix_child::Entry {
                    prefix: dir.into(),
                    kind: P::Directory,
                    handle: Some(f.end),
                });
                core::mem::forget(f);
            }
        }
        if let Some(tmp) = self.line_tmp()
            && !entries.iter().any(|e| e.prefix == "/tmp")
        {
            entries.push(posix_child::Entry {
                prefix: "/tmp".into(),
                kind: P::Tmp,
                handle: Some(tmp.end),
            });
            core::mem::forget(tmp);
        }
        entries.push(posix_child::Entry {
            prefix: "/dev/null".into(),
            kind: P::Null,
            handle: None,
        });
        if let Ok(copy) = zoraya_user::handle_duplicate(image, zoraya_abi::ImageObject::RIGHTS) {
            entries.push(posix_child::Entry {
                prefix: "/proc/self/exe".into(),
                kind: P::Image,
                handle: Some(copy),
            });
        }
        // A file must be reached through a directory entry.
        for f in &files {
            let covered = entries
                .iter()
                .any(|e| e.kind == P::Directory && zoraya_resolve::covers(&e.prefix, f));
            if !covered {
                close(&entries);
                let shown = grants::shown(f, &self.home());
                return Err(zoraya_command::error(
                    "zosh",
                    Some(exec.name),
                    &format!("{shown}: a POSIX program is given directories only"),
                    &["give the directory that holds it, with +r or +w"],
                ));
            }
        }
        entries.sort_by_key(|e| core::cmp::Reverse(e.prefix.len()));
        // The current directory is its own entry, or below the one that
        // covers it (review S3: an entry inside a wider one is left out).
        let prefixes: Vec<&str> = entries.iter().map(|e| e.prefix.as_str()).collect();
        let found = zoraya_resolve::longest(prefixes.iter().copied(), &self.cwd)
            .filter(|(i, _)| entries[*i].kind == P::Directory)
            .map(|(i, rest)| (i, String::from(rest)));
        let Some((cwd_entry, cwd_rest)) = found else {
            close(&entries);
            return Err(zoraya_command::error(
                "zosh",
                Some(exec.name),
                "the current directory cannot be given",
                &[],
            ));
        };
        Ok((entries, cwd_entry as u32, cwd_rest))
    }

    /// The line's `/tmp`, made for a line that will not run, goes.
    fn drop_line_tmp(&mut self) {
        if let (Some(t), Some(name)) = (&self.tmp, self.line_tmp_name.take()) {
            remove_tree(t, &name);
        }
    }

    /// Reaps the POSIX pids of a killed line once each process job has
    /// ended (up to a second each; the broker refuses one still running).
    fn reap_killed(&mut self, posix: &[(u32, u32, Handle)]) {
        let Some(broker) = self.broker.as_ref() else {
            return;
        };
        for (pid, _, job) in posix {
            let deadline = clock_monotonic() + 1_000_000_000;
            while !object_get_signals(*job).is_ok_and(|s| s & job_signals::TERMINATED != 0)
                && clock_monotonic() < deadline
            {
                zoraya_user::sleep_until(clock_monotonic() + 10_000_000);
            }
            let _ = broker.reap(*pid);
        }
    }

    /// The line's directory on the temporary service: made on first use
    /// for the line being started (`self.line_tmp_name`), and connected.
    fn line_tmp(&mut self) -> Option<File> {
        let tmp = self.tmp.as_ref()?;
        if self.line_tmp_name.is_none() {
            self.next_tmp += 1;
            let name = format!("line-{}", self.next_tmp);
            tmp.make_directory(&name).ok()?;
            self.line_tmp_name = Some(name);
        }
        let name = self.line_tmp_name.as_deref()?;
        tmp.open(name, open_flags::DIRECTORY, ALL).ok()
    }

    /// The output destination a sink names.
    fn dest(&self, sink: Sink) -> Result<Dest, String> {
        match sink {
            Sink::Terminal => Ok(Dest::Terminal),
            Sink::Capture => Ok(Dest::Capture),
            Sink::File(token) => {
                let o = self
                    .outputs
                    .get(&token)
                    .ok_or_else(|| String::from("zosh: an output file is gone\n"))?;
                let file = o
                    .file
                    .clone_as(file_rights::READ | file_rights::WRITE)
                    .map_err(|s| zoraya_command::error("zosh", None, &problem(s), &[]))?;
                Ok(Dest::File(file))
            }
        }
    }

    /// Each element's `in`, `out` and `err` (ADR-0152 rules 1 to 3, ADR-0158
    /// rule 2). Every end made for the children is added to `given`, for
    /// the shell to close once they are started.
    fn streams(
        &mut self,
        number: u32,
        pipe: &Pipe<'_>,
        given: &mut Vec<Handle>,
    ) -> Result<Vec<[Handle; 3]>, String> {
        let fail = |s: Status| {
            zoraya_command::error("zosh", None, &zoraya_command::status_problem(s), &[])
        };
        let n = pipe.programs.len();
        let mut pipes = Vec::new();
        for _ in 1..n {
            let (read, write) = stream_create().map_err(fail)?;
            given.push(read);
            given.push(write);
            pipes.push((read, write));
        }
        // The pipeline's own out and err: one relay when both go one way.
        let out = self.relay(number, self.dest(pipe.out)?).map_err(fail)?;
        given.push(out);
        let err = if pipe.err == pipe.out {
            out
        } else {
            let e = self.relay(number, self.dest(pipe.err)?).map_err(fail)?;
            given.push(e);
            e
        };
        let first = match pipe.input {
            Some(text) => self.feed(number, Source::Text, text.as_bytes().to_vec()),
            None if pipe.background => Z::empty_input(),
            None => self.feed(number, Source::Keys, Vec::new()),
        }
        .map_err(fail)?;
        given.push(first);
        let mut all = Vec::new();
        for (i, exec) in pipe.programs.iter().enumerate() {
            let mut ends = [
                if i == 0 { first } else { pipes[i - 1].0 },
                if i + 1 < n { pipes[i].1 } else { out },
                if exec.err_too && i + 1 < n {
                    pipes[i].1
                } else {
                    err
                },
            ];
            // Rule 3: the shell opens the file itself, before the command
            // starts, and relays it.
            for (stream, append, path) in exec.redirects {
                let opened = if *stream == Stream::In {
                    let file = self
                        .open(path, 0)
                        .map_err(|s| zoraya_command::error("zosh", Some(path), &problem(s), &[]))?;
                    self.feed(number, Source::File(file, 0), Vec::new())
                } else {
                    let flags = open_flags::CREATE | if *append { 0 } else { open_flags::TRUNCATE };
                    let file = self
                        .open(path, flags)
                        .map_err(|s| zoraya_command::error("zosh", Some(path), &problem(s), &[]))?;
                    self.relay(number, Dest::File(file))
                }
                .map_err(fail)?;
                given.push(opened);
                match stream {
                    Stream::In => ends[0] = opened,
                    Stream::Out => ends[1] = opened,
                    Stream::Err => ends[2] = opened,
                    Stream::Both => {
                        ends[1] = opened;
                        ends[2] = opened;
                    }
                }
            }
            all.push(ends);
        }
        Ok(all)
    }

    // --- Jobs ---

    /// Spawns one element into `job`, with its three stream ends.
    fn spawn_in_job(
        &self,
        job: Handle,
        image: Handle,
        exec: &Exec<'_>,
        startup: Option<Handle>,
        streams: [Handle; 3],
    ) -> Result<Handle, Status> {
        let mut buffer = alloc::vec![0u8; 16 * 1024];
        let mut e = Encoder::new(&mut buffer, job, image);
        let volume = if NEED_VOLUME.contains(&exec.name) {
            self.root
                .as_ref()
                .and_then(Held::root)
                .and_then(|v| v.clone_as(ALL).ok())
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
        } else if startup.is_some() {
            // ADR-0153 rule 1: its own job, to start helpers inside it, and
            // no console.
            e.handle(dup(JOB.slot(), job, Rights::CREATE_PROCESS));
        } else {
            // A test program gets what TASK-11's shell gave it.
            e.handle(console_entry());
        }
        if let Some(end) = startup {
            e.handle(HandleEntry {
                slot: native::slot::STARTUP.slot(),
                handle: end,
                rights: ChannelObject::RIGHTS,
                transfer: Transfer::Move,
            });
        }
        let [input, output, error] = streams;
        e.handle(dup(
            native::slot::IN.slot(),
            input,
            StreamReadObject::RIGHTS,
        ));
        e.handle(dup(
            native::slot::OUT.slot(),
            output,
            StreamWriteObject::RIGHTS,
        ));
        e.handle(dup(
            native::slot::ERR.slot(),
            error,
            StreamWriteObject::RIGHTS,
        ));
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
        if result.is_ok() {
            core::mem::forget(volume);
        }
        result
    }

    // --- Streams (ADR-0152) ---

    fn stream_key(&mut self) -> u64 {
        self.next_stream += 1;
        STREAM_KEY + self.next_stream
    }

    /// A stream job `number` writes and the shell relays to `dest`; gives
    /// the write end, for the shell to close once the job has it.
    fn relay(&mut self, number: u32, dest: Dest) -> Result<Handle, Status> {
        let (read, write) = stream_create()?;
        let key = self.stream_key();
        let wanted = stream_signals::READABLE | stream_signals::PEER_CLOSED;
        if let Err(e) = port_subscribe(self.port, read, wanted, key, Mode::Level) {
            let _ = handle_close(read);
            let _ = handle_close(write);
            return Err(e);
        }
        self.relays.push(Relay {
            key,
            job: number,
            end: read,
            dest,
            partial: Vec::new(),
        });
        Ok(write)
    }

    /// A stream the shell writes from `source` and job `number` reads;
    /// gives the read end.
    fn feed(&mut self, number: u32, source: Source, text: Vec<u8>) -> Result<Handle, Status> {
        let (read, write) = stream_create()?;
        let key = self.stream_key();
        let closing = matches!(source, Source::Text);
        self.feeds.push(Feed {
            key,
            job: number,
            end: write,
            source,
            pending: text,
            at: 0,
            closing,
            armed: 0,
        });
        let at = self.feeds.len() - 1;
        self.pump_feed(at);
        Ok(read)
    }

    /// An `in` at its end already (ADR-0158 rule 2).
    fn empty_input() -> Result<Handle, Status> {
        let (read, write) = stream_create()?;
        let _ = handle_close(write);
        Ok(read)
    }

    /// Passes what relay `at` holds: one chunk, or with `all` everything
    /// until its writers are gone. Gives false once the relay is closed.
    fn pump_relay(&mut self, at: usize, all: bool) -> bool {
        let mut bytes = [0u8; RELAY_CHUNK];
        loop {
            let r = &self.relays[at];
            let n = match stream_read(r.end, &mut bytes) {
                Ok(0) | Err(Status::PeerClosed) => {
                    let r = self.relays.remove(at);
                    let _ = handle_close(r.end);
                    if !r.partial.is_empty() {
                        let mut rest = r.partial;
                        rest.push(b'\n');
                        self.background_output(&rest);
                    }
                    return false;
                }
                Ok(n) => n,
                Err(_) => return true,
            };
            let front = self.foreground == Some(self.relays[at].job);
            match &mut self.relays[at].dest {
                Dest::Terminal if front => {
                    // What it printed in the background comes first.
                    let held = core::mem::take(&mut self.relays[at].partial);
                    show(&tamed(&held));
                    show_bytes(&bytes[..n]);
                }
                Dest::Terminal => {
                    let r = &mut self.relays[at];
                    r.partial.extend_from_slice(&bytes[..n]);
                    let cut = match r.partial.iter().rposition(|b| *b == b'\n') {
                        Some(cut) => Some(cut + 1),
                        // A line longer than this is shown in pieces, so
                        // that the shell does not hold it all.
                        None if r.partial.len() >= MAX_HELD_LINE => {
                            Some(r.partial.len() - zoraya_commands::incomplete_tail(&r.partial))
                        }
                        None => None,
                    };
                    if let Some(cut) = cut {
                        let mut lines: Vec<u8> = r.partial.drain(..cut).collect();
                        if lines.last() != Some(&b'\n') {
                            lines.push(b'\n');
                        }
                        self.background_output(&lines);
                    }
                }
                Dest::File(file) => {
                    let _ = file.write(0, write_flags::APPEND, &bytes[..n]);
                }
                Dest::Capture => {
                    let number = self.relays[at].job;
                    if let Some(j) = self.jobs.get_mut(number) {
                        let captured = &mut j.payload.captured;
                        let room = MAX_CAPTURE.saturating_sub(captured.len());
                        captured.extend_from_slice(&bytes[..n.min(room)]);
                    }
                }
            }
            if !all {
                return true;
            }
        }
    }

    /// A background job's lines: above the line being edited, or now.
    fn background_output(&mut self, bytes: &[u8]) {
        let text = tamed(bytes);
        if self.at_prompt {
            self.jobs.queue(text);
        } else {
            show(&text);
        }
    }

    /// Subscribes feed `at` to `wanted`, if it is not already.
    fn arm(&mut self, at: usize, wanted: u32) {
        let f = &mut self.feeds[at];
        if f.armed == wanted {
            return;
        }
        let _ = port_unsubscribe(self.port, f.key);
        if port_subscribe(self.port, f.end, wanted, f.key, Mode::Level).is_ok() {
            f.armed = wanted;
        } else {
            f.armed = 0;
        }
    }

    fn close_feed(&mut self, at: usize) {
        let f = self.feeds.remove(at);
        let _ = handle_close(f.end);
    }

    /// Writes what feed `at` has, as far as the stream takes it without
    /// waiting (INV-0152-2).
    fn pump_feed(&mut self, at: usize) {
        loop {
            let f = &mut self.feeds[at];
            if f.at >= f.pending.len() {
                f.pending.clear();
                f.at = 0;
                if let Source::File(file, offset) = &mut f.source {
                    let mut chunk = alloc::vec![0u8; STREAM_ATOMIC];
                    match file.read(*offset, &mut chunk) {
                        Ok(n) if n > 0 => {
                            chunk.truncate(n);
                            *offset += n as u64;
                            f.pending = chunk;
                        }
                        _ => f.closing = true,
                    }
                }
                if f.pending.is_empty() {
                    if f.closing {
                        self.close_feed(at);
                    } else {
                        // Keys: nothing to write until some are typed.
                        self.arm(at, stream_signals::PEER_CLOSED);
                    }
                    return;
                }
            }
            let f = &mut self.feeds[at];
            let end = (f.at + STREAM_ATOMIC).min(f.pending.len());
            match stream_write(f.end, &f.pending[f.at..end]) {
                Ok(n) => f.at += n,
                Err(Status::WouldBlock) => {
                    self.arm(at, stream_signals::WRITABLE | stream_signals::PEER_CLOSED);
                    return;
                }
                // The reader is gone: nothing more is wanted.
                Err(_) => {
                    self.close_feed(at);
                    return;
                }
            }
        }
    }

    /// One port packet: a stream's, or a job's end, which is reported as
    /// `at` says (ADR-0158 rule 6). Gives the job in front when it ended.
    fn event(&mut self, packet: &Packet, at: At) -> Option<(jobs::Entry<Job>, Vec<ExitRecord>)> {
        if packet.key >= STREAM_KEY {
            if let Some(at) = self.relays.iter().position(|r| r.key == packet.key) {
                self.pump_relay(at, false);
            } else if let Some(at) = self.feeds.iter().position(|f| f.key == packet.key) {
                if packet.signals & stream_signals::PEER_CLOSED != 0 {
                    self.close_feed(at);
                } else {
                    self.pump_feed(at);
                }
            }
            return None;
        }
        self.take_ended(packet.key, at)
    }

    /// A key for the foreground job's `in`, as a terminal in canonical mode
    /// gives it: echoed, a line at a time, Backspace editing it, Ctrl-D
    /// ending the input. Gives what to echo.
    fn type_key(&mut self, number: u32, key: u8, echo: &mut Vec<u8>) {
        let Some(at) = self
            .feeds
            .iter()
            .position(|f| f.job == number && matches!(f.source, Source::Keys))
        else {
            return;
        };
        // `ESC [ ... final` and `ESC O x`: cursor and function keys have no
        // meaning here.
        match self.escape {
            Escape::No => {}
            Escape::Started => {
                self.escape = match key {
                    b'[' => Escape::Csi,
                    b'O' => Escape::Ss3,
                    _ => Escape::No,
                };
                return;
            }
            Escape::Csi => {
                if (0x40..=0x7e).contains(&key) {
                    self.escape = Escape::No;
                }
                return;
            }
            Escape::Ss3 => {
                self.escape = Escape::No;
                return;
            }
        }
        match key {
            b'\r' | b'\n' => {
                echo.push(b'\n');
                self.typed.push(b'\n');
                let line = core::mem::take(&mut self.typed);
                self.feeds[at].pending.extend_from_slice(&line);
                self.pump_feed(at);
            }
            0x7f | 0x08 => {
                // One character back: its continuation bytes too.
                while let Some(b) = self.typed.pop() {
                    if b & 0xc0 != 0x80 {
                        echo.extend_from_slice(b"\x08 \x08");
                        break;
                    }
                }
            }
            editor::CTRL_D => {
                let line = core::mem::take(&mut self.typed);
                let f = &mut self.feeds[at];
                if line.is_empty() {
                    f.closing = true;
                } else {
                    f.pending.extend_from_slice(&line);
                }
                self.pump_feed(at);
            }
            0x1b => self.escape = Escape::Started,
            b'\t' => {
                self.typed.push(key);
                echo.push(key);
            }
            k if k >= 0x20 => {
                self.typed.push(k);
                echo.push(k);
            }
            _ => {}
        }
    }

    /// Ctrl-Z (ADR-0158 rule 4, ADR-0200 item 2): `job_suspend` of each
    /// native element's own job, and `SIGTSTP` through the broker to each
    /// POSIX element, which applies its disposition (a handler runs first).
    /// Stopped once every element is, within half a second.
    fn stop_job(&mut self, number: u32) -> bool {
        let Some(e) = self.jobs.get(number) else {
            return false;
        };
        let p = &e.payload;
        for j in &p.native {
            let _ = job_suspend(*j);
        }
        if !p.posix.is_empty() {
            let Some(broker) = self.broker.as_ref() else {
                return false;
            };
            for (_, pgid, _) in &p.posix {
                let _ = broker.signal_group(*pgid, zoraya_abi::posix::signal::SIGTSTP);
            }
        }
        let deadline = clock_monotonic() + 500_000_000;
        loop {
            let stopped = self
                .jobs
                .get(number)
                .is_some_and(|e| Z::all_stopped(&e.payload));
            // What it wrote before it stopped (a POSIX handler restoring the
            // terminal) is shown before the shell says it stopped (review K9).
            self.drain_relays(number);
            if stopped {
                return true;
            }
            if clock_monotonic() > deadline {
                return false;
            }
            zoraya_user::sleep_until(clock_monotonic() + 10_000_000);
        }
    }

    /// Passes everything job `number`'s relays hold now, without waiting.
    fn drain_relays(&mut self, number: u32) {
        let mut at = 0;
        while at < self.relays.len() {
            if self.relays[at].job == number && !self.pump_relay(at, true) {
                // Closed and removed: the next one is now at `at`.
                continue;
            }
            at += 1;
        }
    }

    /// Every element of the job is stopped: its own job, or a POSIX one's
    /// process job, `SUSPENDED`.
    fn all_stopped(p: &Job) -> bool {
        let suspended =
            |j: Handle| object_get_signals(j).is_ok_and(|s| s & job_signals::SUSPENDED != 0);
        (!p.native.is_empty() || !p.posix.is_empty())
            && p.native.iter().all(|j| suspended(*j))
            && p.posix.iter().all(|(_, _, j)| suspended(*j))
    }

    /// Continues job `number`: `job_resume` of its jobs, and `SIGCONT` to its
    /// POSIX groups through the broker.
    fn resume_job(&mut self, number: u32) {
        let Some(e) = self.jobs.get(number) else {
            return;
        };
        let _ = job_resume(e.payload.job);
        for j in &e.payload.native {
            let _ = job_resume(*j);
        }
        if let Some(broker) = self.broker.as_ref() {
            for (_, pgid, _) in &e.payload.posix {
                let _ = broker.signal_group(*pgid, zoraya_abi::posix::signal::SIGCONT);
            }
        }
    }

    /// Job `number`'s input from the keys ends, after what was typed for it
    /// (ADR-0158 rule 2: a job in the background reads no key).
    fn close_keys(&mut self, number: u32) {
        while let Some(at) = self
            .feeds
            .iter()
            .position(|f| f.job == number && matches!(f.source, Source::Keys))
        {
            let f = &mut self.feeds[at];
            f.source = Source::Text;
            f.closing = true;
            self.pump_feed(at);
        }
    }

    /// Each job's state as the kernel has it: `SUSPENDED` on its job
    /// (ADR-0105 item 8).
    fn refresh_states(&mut self) {
        let suspended =
            |j: Handle| object_get_signals(j).is_ok_and(|s| s & job_signals::SUSPENDED != 0);
        for e in self.jobs.iter_mut() {
            let p = &e.payload;
            // Stopped element by element (ADR-0200 item 2).
            e.state = if suspended(p.job) || Z::all_stopped(p) {
                JobState::Stopped
            } else {
                JobState::Running
            };
        }
    }

    /// Job `number`'s streams: the relays emptied, the feeds closed.
    fn finish_streams(&mut self, number: u32) {
        while let Some(at) = self.relays.iter().position(|r| r.job == number) {
            if self.pump_relay(at, true) {
                // A writer outlived the job (it cannot: the job held them
                // all); stop rather than wait.
                let r = self.relays.remove(at);
                let _ = handle_close(r.end);
            }
        }
        while let Some(at) = self.feeds.iter().position(|f| f.job == number) {
            self.close_feed(at);
        }
    }

    /// The job with port key `key` ended: its streams emptied, its records
    /// read and its handles closed. The job in front is removed and given
    /// back, for its line to report; any other is reported as `at` says.
    fn take_ended(&mut self, key: u64, at: At) -> Option<(jobs::Entry<Job>, Vec<ExitRecord>)> {
        let number = self
            .jobs
            .iter()
            .find(|e| JOB_KEY + u64::from(e.number) == key)?
            .number;
        self.finish_streams(number);
        let _ = port_unsubscribe(self.port, key);
        let job = &self.jobs.get(number)?.payload;
        let records: Vec<ExitRecord> = job
            .processes
            .iter()
            .map(|p| {
                let r = process_exit_record(*p).unwrap_or_else(|_| record::exited(0));
                let _ = handle_close(*p);
                r
            })
            .collect();
        let status = Z::status_of(job, &records);
        // Its POSIX pids go, and the line's /tmp (ADR-0153 rule 10).
        let pids: Vec<u32> = job.posix.iter().map(|(pid, _, _)| *pid).collect();
        let tmp = job.tmp.clone();
        if let Some(b) = &self.broker {
            for pid in pids {
                let _ = b.reap(pid);
            }
        }
        if let (Some(t), Some(name)) = (&self.tmp, tmp) {
            remove_tree(t, &name);
        }
        let (entry, now) = if self.foreground == Some(number) {
            (self.jobs.remove(number)?, None)
        } else {
            self.jobs.ended(number, &status, at)?
        };
        let _ = handle_close(entry.payload.job);
        if let Some(text) = now {
            show(&text);
        }
        (self.foreground == Some(number)).then_some((entry, records))
    }

    /// The status of a job's pipeline (ADR-0152 rule 6).
    fn status_of(job: &Job, records: &[ExitRecord]) -> ExitRecord {
        let members: Vec<Member> = records
            .iter()
            .zip(&job.elements)
            .map(|(record, (_, question, _))| Member {
                record: *record,
                question: *question,
            })
            .collect();
        pipeline_status(&members).record
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
                                self.jobs
                                    .queue(format!("[input: {} bytes dropped]\n", m.dropped));
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
                self.jobs.queue(format!(
                    "[input: {} bytes dropped]\n",
                    dropped - self.dropped
                ));
                self.dropped = dropped;
            }
        }
        keys
    }

    /// Waits for job `number` in front, giving it the keys (ADR-0158 rule
    /// 3): Ctrl-C and Ctrl-\\ as job signals, Ctrl-Z as `job_suspend`, the
    /// rest to its `in`. A Ctrl-C the job outlives by a second makes the
    /// next one a kill, once the shell has said so.
    fn wait_foreground(&mut self, number: u32) -> Waited {
        let mut ctrl_c = false;
        let mut offer_at: Option<u64> = None;
        let mut offered = false;
        self.foreground = Some(number);
        self.typed.clear();
        self.escape = Escape::No;
        let waited = 'wait: loop {
            // Keys typed ahead of the job reach it first.
            let mut keys = core::mem::take(&mut self.pending);
            if keys.is_empty() {
                let mut packets = [Packet::default(); 8];
                let deadline = offer_at.unwrap_or(DEADLINE_INFINITE);
                let Ok(count) = port_wait(self.port, deadline, &mut packets) else {
                    // The job outlived its Ctrl-C by a second.
                    if offer_at.take().is_some() && self.jobs.get(number).is_some() {
                        show("(Ctrl-C again to kill it)\n");
                        offered = true;
                    }
                    continue;
                };
                for packet in &packets[..count] {
                    if packet.key == CONSOLE_KEY {
                        keys.extend(self.read_keys());
                    } else if let Some((entry, records)) = self.event(packet, At::Elsewhere) {
                        break 'wait Waited::Ended(Box::new(entry), records, ctrl_c);
                    }
                }
            }
            let Some(job) = self.jobs.get(number).map(|e| e.payload.job) else {
                continue;
            };
            let mut echo = Vec::new();
            for (i, &key) in keys.iter().enumerate() {
                match key {
                    editor::CTRL_C => {
                        ctrl_c = true;
                        show_bytes(&echo);
                        echo.clear();
                        show("^C\n");
                        if offered {
                            let _ = job_kill(job);
                        } else {
                            let _ = job_signal(job, RequestKind::Interrupt, 0);
                            offer_at.get_or_insert(clock_monotonic() + KILL_OFFER_AFTER);
                        }
                    }
                    editor::CTRL_BACKSLASH => {
                        show_bytes(&echo);
                        echo.clear();
                        show("^\\\n");
                        let _ = job_signal(job, RequestKind::Quit, 0);
                    }
                    editor::CTRL_Z => {
                        show_bytes(&echo);
                        echo.clear();
                        show("^Z\n");
                        let stopped = self.stop_job(number);
                        if let Some(e) = self.jobs.get_mut(number) {
                            if stopped {
                                e.state = JobState::Stopped;
                                show(&format!("[{number}] stopped: {}\n", e.line));
                                // What was typed after it is the prompt's.
                                self.pending = keys[i + 1..].to_vec();
                                break 'wait Waited::Stopped(number);
                            }
                            // ADR-0158 rule 4: it stays in front.
                            show(&format!("[{number}] did not stop: {}\n", e.line));
                        }
                    }
                    k => self.type_key(number, k, &mut echo),
                }
            }
            show_bytes(&echo);
        };
        self.foreground = None;
        self.typed.clear();
        waited
    }

    /// A job in front ended: its hints read (ADR-0153 rule 11), and its
    /// records and output for the line.
    fn ended_in_front(
        &mut self,
        mut entry: jobs::Entry<Job>,
        records: Vec<ExitRecord>,
        ctrl_c: bool,
    ) -> Ran {
        let line = entry.line.clone();
        // A grant belongs before its own element (rule 3, review S11): a
        // pipeline's hint names the element instead of the whole line.
        let alone = entry.payload.elements.len() == 1;
        for (name, _, report) in &mut entry.payload.elements {
            if let Some(r) = report.take() {
                self.hints(r, name, alone.then_some(line.as_str()));
            }
        }
        let captured = String::from_utf8_lossy(&entry.payload.captured).into_owned();
        Ran::Ended {
            records,
            ctrl_c,
            captured,
        }
    }

    /// Waits until job `number` ends, or every running job when `None`,
    /// or `deadline` passes; reports ends as they come (ADR-0158 rule 6).
    fn wait_for(&mut self, number: Option<u32>, deadline: u64) -> bool {
        loop {
            let done = match number {
                Some(n) => self.jobs.get(n).is_none(),
                None => self.jobs.iter().all(|e| e.state == JobState::Stopped),
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
                } else {
                    self.event(packet, At::Reporting);
                }
            }
        }
    }

    /// `%N`, or the newest job.
    fn which_job(&self, word: Option<&str>) -> Result<u32, String> {
        self.jobs.which(word)
    }

    // --- Builtins ---

    fn file_command(&mut self, name: &str, d: &Description, p: &Parsed<'_>, io: &mut Io) -> i32 {
        if self.root.is_none() {
            io.err.push_str(&zoraya_command::error(
                name,
                None,
                "there is no volume",
                &[],
            ));
            return i32::from(zoraya_command::failure_code(d));
        }
        let (code, o) = zoraya_commands::run(name, d, p, &mut Volume(self));
        io.out.push_str(&o.out);
        io.out.push_str(&String::from_utf8_lossy(&o.raw));
        io.err.push_str(&o.err);
        i32::from(code)
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
            let mut page = format!("Topics: {}\nCommands:\n", TOPICS.join(" "));
            let list = self.commands(state);
            page.push_str(&self.listing(state, &list, "  "));
            let demos: Vec<&str> = descriptions::DEMOS.iter().map(|(n, _)| *n).collect();
            page.push_str(&format!("Demonstrations: {}\n", demos.join(" ")));
            // A screen at a time, as every help page (ADR-0156 item 5): a
            // window has no scrollback to find the top in.
            if io.terminal {
                self.pager(&page);
            } else {
                io.out.push_str(&page);
            }
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
        } else if FILE_COMMANDS.contains(&name) && name == "sync" {
            match self.description(name) {
                Some(d) => (d, format!("{name} is built into the shell")),
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
                } else {
                    self.event(packet, At::Elsewhere);
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
                zoraya_settings::typed_line(&s.name, value, &s.ty),
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
        let values = p.operand_values(d, "VALUE");
        let at = p.value(d, "at");
        // A list (ADR-0171): its items, +ITEM, -ITEM.
        if let Some(name) = name
            && let Some(s) = declared.iter().find(|s| s.name == name)
            && matches!(s.ty, zoraya_describe::SettingType::List { .. })
        {
            let v = zoraya_settings::read(&bytes, &declared);
            let current = v.get(name).unwrap_or(&s.default).to_string();
            if values.is_empty() {
                show_one(io, s, &current);
                let items = zoraya_describe::list_items(&current).unwrap_or_default();
                for (i, item) in items.iter().enumerate() {
                    io.out
                        .push_str(&format!("  {i} {}\n", zoraya_describe::quote_value(item)));
                }
                return 0;
            }
            use zoraya_settings::ListEdit;
            let edit = match (values.as_slice(), at) {
                ([one], None) if one.len() > 1 && one.starts_with('+') => {
                    ListEdit::Append(one[1..].into())
                }
                ([one], Some(n)) if one.len() > 1 && one.starts_with('+') => {
                    match n.parse::<usize>() {
                        Ok(n) => ListEdit::Insert(one[1..].into(), n),
                        Err(_) => return fail(io, name, &format!("--at {n} is not an index"), &[]),
                    }
                }
                ([one], None) if one.len() > 1 && one.starts_with('-') => {
                    ListEdit::Remove(one[1..].into())
                }
                (items, None) => {
                    ListEdit::Replace(items.iter().map(|i| String::from(*i)).collect())
                }
                (_, Some(_)) => return fail(io, name, "--at goes with one +ITEM", &[]),
            };
            let new = match zoraya_settings::edit_list(&current, edit, &s.ty) {
                Ok(new) => new,
                Err(zoraya_settings::Refusal::Value(problem)) => {
                    return fail(io, name, &problem, &[]);
                }
                Err(zoraya_settings::Refusal::Unknown(_)) => return self.unknown_setting(io, name),
            };
            return match zoraya_settings::set(&bytes, &declared, name, &new) {
                Ok(f) => match self.write_settings(&f) {
                    Ok(()) => 0,
                    Err(e) => fail(io, SETTINGS, &e, &[]),
                },
                Err(zoraya_settings::Refusal::Value(problem)) => fail(io, name, &problem, &[]),
                Err(zoraya_settings::Refusal::Unknown(_)) => self.unknown_setting(io, name),
            };
        }
        if at.is_some() {
            return fail(io, "--at", "goes with a list's +ITEM", &[]);
        }
        if values.len() > 1 {
            return fail(
                io,
                name.unwrap_or(""),
                "takes one value; quote a value with spaces",
                &[],
            );
        }
        let value = values.first().copied();
        match (name, value) {
            (None, _) => {
                let v = zoraya_settings::read(&bytes, &declared);
                for s in &declared {
                    let current = v.get(&s.name).unwrap_or(&s.default);
                    if all || current != s.default {
                        io.out
                            .push_str(&zoraya_settings::typed_line(&s.name, current, &s.ty));
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

    /// A room grant's path, absolute: `~` is the home directory.
    fn room_path(&self, path: &str) -> String {
        if path == "~" {
            self.home()
        } else if let Some(rest) = path.strip_prefix("~/") {
            format!("{}/{rest}", self.home().trim_end_matches('/'))
        } else {
            path.into()
        }
    }

    /// In a room, every line receives the room's grants as if written
    /// before the command (ADR-0172 point 3), shown by `grants` with
    /// `(room NAME)`.
    fn add_room_grants(&self, plan: &mut Vec<grants::Planned>) {
        let Some(room) = &self.room else { return };
        for g in &room.grants {
            plan.push(grants::Planned {
                path: self.room_path(&g.path),
                access: if g.write {
                    grants::Access::Tree
                } else {
                    grants::Access::Read
                },
                reason: format!("room {}", room.name),
            });
        }
    }

    fn read_rooms(&self) -> zoraya_settings::rooms::Rooms {
        let bytes = self
            .open(ROOMS, 0)
            .and_then(|f| read_all(&f))
            .unwrap_or_default();
        zoraya_settings::rooms::read(&bytes)
    }

    /// Enters `name` (ADR-0172 point 3): the prompt shows it, the current
    /// directory becomes its first `+w` directory, and every line gets its
    /// grants. `home` leaves.
    fn enter_room(&mut self, name: &str) -> Result<(), String> {
        if name == zoraya_settings::rooms::HOME {
            self.leave_room_job();
            self.room = None;
            return Ok(());
        }
        let rooms = self.read_rooms();
        // A room's own shell cannot read the table: its room is what it
        // holds (ADR-0242's condition).
        let home = self.home();
        let held = match &self.root {
            Some(Held::Table(entries)) if rooms.rooms.is_empty() => {
                Some(zoraya_settings::rooms::Room {
                    name: name.into(),
                    untrusted: false,
                    colour: 0,
                    limits: zoraya_settings::rooms::Limits::default(),
                    grants: entries
                        .iter()
                        .map(|e| zoraya_settings::rooms::Grant {
                            write: e.rights & file_rights::WRITE != 0,
                            path: grants::shown(&e.path, &home),
                        })
                        .collect(),
                })
            }
            _ => None,
        };
        let Some(room) = rooms.get(name).cloned().or(held) else {
            return Err(zoraya_command::error(
                "room",
                Some(name),
                "no such room",
                &["room lists them; room new makes one"],
            ));
        };
        if room.untrusted {
            // Nothing runs untrusted until quotas exist (review 7d, 1).
            // untrusted-gate: refusal (ADR-0268, `make check-untrusted-gate`)
            return Err(zoraya_command::error(
                "room",
                Some(name),
                "untrusted rooms wait for quotas",
                &[
                    "ADR-0056: until a room's memory and processes can be limited, no program runs untrusted",
                ],
            ));
        }
        // The room's one job (ADR-0269 point 4): asked for before anything
        // else changes, so that a refusal leaves the shell where it was.
        self.join_room_job(&room)?;
        if let Some(g) = room.grants.iter().find(|g| g.write) {
            let dir = self.room_path(&g.path);
            if self.open(&dir, OPEN_DIRECTORY).is_ok() {
                self.cwd = dir;
                self.send_title();
            }
        }
        self.room = Some(room);
        self.refused_seen = self.room_refusals();
        Ok(())
    }

    /// A launch job in `room` from the keeper, for a shell that holds a
    /// `Manage` channel; the job the shell was in before is given back. A shell
    /// the panel launched in a room is in it already; a shell with no keeper
    /// enters a trusted room as before and refuses an untrusted one.
    fn join_room_job(&mut self, room: &zoraya_settings::rooms::Room) -> Result<(), String> {
        use zoraya_user::roomsclient::Fail;
        let needs = |why: &str, hints: &[&str]| {
            Err(zoraya_command::error("room", Some(&room.name), why, hints))
        };
        // Without a launch job the line's --mem, --procs and --cpu would not
        // be applied, and nothing would say so: refused, not entered.
        let limited = room.limits.needs_job();
        let without_keeper = || -> Result<(), String> {
            if room.untrusted {
                needs("an untrusted room needs the room keeper", &[])
            } else if limited {
                needs(
                    "its limits need the room keeper, which does not answer",
                    &["room set NAME --mem none ... removes them"],
                )
            } else {
                Ok(())
            }
        };
        if !self.keeper_manage {
            return if self.room.is_none() {
                without_keeper()
            } else {
                Ok(())
            };
        }
        let Some(k) = self.keeper.as_mut() else {
            return without_keeper();
        };
        match k.launch(&room.name) {
            Ok(job) => {
                self.leave_room_job();
                self.room_job = Some(job);
                Ok(())
            }
            Err(Fail::Denied(status, text)) => {
                let hint = if status == zoraya_proto_rooms::status::BUSY {
                    "use its terminal, or end it first: room remove NAME --kill"
                        .replace("NAME", &room.name)
                } else {
                    String::from("room lists the rooms")
                };
                needs(&text, &[hint.as_str()])
            }
            Err(Fail::Gone) => without_keeper(),
        }
    }

    /// Gives the launch job back: the keeper may let the room go to the other
    /// launcher once its processes have ended.
    fn leave_room_job(&mut self) {
        if let Some(job) = self.room_job.take() {
            let _ = handle_close(job);
            if let (Some(k), Some(r)) = (self.keeper.as_mut(), self.room.as_ref()) {
                let _ = k.leave(&r.name);
            }
        }
    }

    /// The room's refusal counters (memory, processes), from the keeper; zero
    /// without one.
    fn room_refusals(&mut self) -> (u64, u64) {
        let Some(name) = self.room.as_ref().map(|r| r.name.clone()) else {
            return (0, 0);
        };
        match self.keeper.as_mut().map(|k| k.usage(&name)) {
            Some(Ok(records)) => records
                .first()
                .map_or((0, 0), |r| (r.mem_refused, r.procs_refused)),
            _ => (0, 0),
        }
    }

    /// After a line (ADR-0263 point 3, INV-0263-4): at most one hint, naming
    /// the field the room refused and the room.
    fn quota_hint(&mut self) {
        use zoraya_settings::rooms::{quota_hint, refused_field};
        let Some(name) = self.room.as_ref().map(|r| r.name.clone()) else {
            return;
        };
        let before = self.refused_seen;
        let Some(Ok(records)) = self.keeper.as_mut().map(|k| k.usage(&name)) else {
            return;
        };
        let Some(r) = records.first() else { return };
        let after = (r.mem_refused, r.procs_refused);
        self.refused_seen = after;
        match refused_field(before, after) {
            Some(zoraya_settings::rooms::Refused::Memory) => show(&quota_hint(
                &name,
                zoraya_settings::rooms::Refused::Memory,
                r.mem_limit,
                self.keeper_manage,
            )),
            Some(zoraya_settings::rooms::Refused::Processes) => show(&quota_hint(
                &name,
                zoraya_settings::rooms::Refused::Processes,
                r.procs_limit,
                self.keeper_manage,
            )),
            None => {}
        }
    }

    /// The limit words of `room new` and `room set` into `limits`; the
    /// first bad value is the error. `none` removes a word of a trusted room.
    fn limit_options(
        d: &Description,
        p: &Parsed<'_>,
        limits: &mut zoraya_settings::rooms::Limits,
        untrusted: bool,
    ) -> Result<(), String> {
        for (word, name) in [
            ("--mem", "mem"),
            ("--cpu", "cpu"),
            ("--procs", "procs"),
            ("--disk", "disk"),
            ("--conns", "conns"),
            ("--tmp", "tmp"),
        ] {
            if let Some(v) = p.value(d, name) {
                limits.set(word, v, untrusted)?;
            }
        }
        Ok(())
    }

    /// `--disk` on each `+w` directory of the room (ADR-0265, ADR-0266
    /// point 6); a directory that is not there is said, not made.
    fn apply_disk(&mut self, room: &zoraya_settings::rooms::Room, io: &mut Io) {
        let Some(bytes) = room.limits.disk else {
            return;
        };
        for g in room.grants.iter().filter(|g| g.write) {
            let path = self.room_path(&g.path);
            match self.parent(&path) {
                Ok((dir, leaf)) => {
                    let mut client = zoraya_fsclient::Client::new(zoraya_user::app::Files);
                    if client.set_limit(dir.end, &leaf, Some(bytes)).is_err() {
                        io.err.push_str(&format!(
                            "room: {}: --disk: could not limit {}\n",
                            room.name,
                            g.words()
                        ));
                    }
                }
                Err(_) => io.err.push_str(&format!(
                    "room: {}: --disk: {} is not there\n",
                    room.name,
                    g.words()
                )),
            }
        }
    }

    /// `room set NAME WORD VALUE...` (ADR-0266 point 6): the line is
    /// changed and, if the room runs, the keeper applies it to the room's job
    /// at once. A refusal by the keeper (a limit above the parent's) puts the
    /// old line back. `--disk` is applied to the room's `+w` directories.
    fn room_set(&mut self, d: &Description, p: &Parsed<'_>, name: &str, io: &mut Io) -> i32 {
        use zoraya_settings::rooms;
        let fail = |io: &mut Io, why: &str, hints: &[&str]| {
            io.err
                .push_str(&zoraya_command::error("room", Some(name), why, hints));
            1
        };
        // A room's own shell holds no `Set` (ADR-0269): it says where.
        if self.room.is_some() && !self.keeper_manage {
            return fail(
                io,
                "from home: room set cannot be run inside a room",
                &["leave with exit, or run it in a shell in home"],
            );
        }
        let old = self
            .open(ROOMS, 0)
            .and_then(|f| read_all(&f))
            .unwrap_or_default();
        let Some(mut room) = rooms::read(&old).get(name).cloned() else {
            return fail(io, "no such room", &["room lists them; room new makes one"]);
        };
        let before = room.limits;
        let old_room = room.clone();
        if let Err(e) = Self::limit_options(d, p, &mut room.limits, room.untrusted) {
            return fail(io, &e, &[]);
        }
        if room.limits == before {
            return fail(
                io,
                "nothing to change",
                &["room set NAME --mem 128M --cpu 10%"],
            );
        }
        if room.untrusted {
            // An untrusted line always has all six (INV-0266-1). The defaults
            // come from the limits of this shell's own job (the session's, or
            // below it); with none known the words stay out and the keeper,
            // which reads the session job, fills them when it launches or
            // sets the room. No machine is made up here.
            let usage = zoraya_user::job_get_usage(JOB, false).ok();
            if let Some(m) = usage.and_then(|u| {
                Some(rooms::Machine {
                    session_mem: u.memory.limit?,
                    session_procs: u.processes.limit?.min(u64::from(u32::MAX)) as u32,
                    free_disk: 8 << 30,
                })
            }) {
                room.limits.fill_defaults(&m);
            }
        }
        let new = match rooms::replace(&old, &room) {
            Ok(b) => b,
            Err(_) => return fail(io, "the room's line cannot be rewritten", &[]),
        };
        if let Err(e) = self.publish_file(ROOMS, &new) {
            return fail(io, &e, &[]);
        }
        if let Some(k) = self.keeper.as_mut()
            && self.keeper_manage
            && let Err(e) = k.set(name)
        {
            // Put the old line back, and only it: the table is read again,
            // so that another edit made since is not lost (a gap remains
            // between that read and the write; there is no file lock).
            let now = self
                .open(ROOMS, 0)
                .and_then(|f| read_all(&f))
                .unwrap_or_default();
            if let Ok(back) = rooms::replace(&now, &old_room) {
                let _ = self.publish_file(ROOMS, &back);
            }
            let why = match e {
                zoraya_user::roomsclient::Fail::Denied(_, t) => t,
                zoraya_user::roomsclient::Fail::Gone => {
                    String::from("the room keeper does not answer")
                }
            };
            return fail(io, &why, &[]);
        }
        if room.limits.disk != before.disk {
            self.apply_disk(&room, io);
        }
        io.out.push_str(&format!("{}\n", room.line()));
        0
    }

    /// `room` (ADR-0172 point 3): list, enter, leave, make and remove
    /// rooms, in `/config/rooms`.
    fn room_builtin(&mut self, d: &Description, p: &Parsed<'_>, io: &mut Io) -> i32 {
        use zoraya_settings::rooms::{self, Grant, Room};
        let fail = |io: &mut Io, subject: Option<&str>, problem: &str, hints: &[&str]| {
            io.err
                .push_str(&zoraya_command::error("room", subject, problem, hints));
            1
        };
        let words = p.operand_values(d, "WORDS");
        if p.flag(d, "leave") {
            self.leave_room_job();
            self.room = None;
            return 0;
        }
        let current = self.room.as_ref().map(|r| r.name.clone());
        match words.as_slice() {
            [] => {
                let mut table = self.read_rooms();
                // A room's own shell cannot read the table: it lists the
                // room it holds (ADR-0242's condition).
                if let Some(r) = &self.room
                    && table.get(&r.name).is_none()
                {
                    table.rooms.push(r.clone());
                }
                let mark = |n: &str| {
                    if current.as_deref() == Some(n) {
                        "* "
                    } else {
                        "  "
                    }
                };
                io.out.push_str(&format!("{}home\n", mark("home")));
                for r in &table.rooms {
                    let mut line = format!("{}{}", mark(&r.name), r.name);
                    if r.untrusted {
                        line.push_str(" untrusted");
                    }
                    let limits = r.limits.words();
                    if !limits.is_empty() {
                        line.push_str("  ");
                        line.push_str(&limits);
                    }
                    for g in &r.grants {
                        line.push_str("  ");
                        line.push_str(&g.words());
                    }
                    io.out.push_str(&line);
                    io.out.push('\n');
                }
                for pr in &table.problems {
                    io.err
                        .push_str(&format!("room: {ROOMS} line {}: {}\n", pr.line, pr.problem));
                }
                0
            }
            ["new", name, rest @ ..] => {
                if p.flag(d, "untrusted") {
                    // untrusted-gate: refusal (ADR-0268, `make check-untrusted-gate`)
                    return fail(
                        io,
                        Some(name),
                        "untrusted rooms wait for quotas",
                        &[
                            "ADR-0056: until a room's memory and processes can be limited, no program runs untrusted",
                        ],
                    );
                }
                if !rooms::is_room_name(name) {
                    return fail(
                        io,
                        Some(name),
                        "not a room name",
                        &["a lower-case letter, then letters, digits or -"],
                    );
                }
                if rest.is_empty() || rest.len() % 2 != 0 {
                    return fail(
                        io,
                        Some(name),
                        "give the grants as +r PATH or +w PATH",
                        &["room new project +w ~/project +r ~/include"],
                    );
                }
                let mut given = Vec::new();
                for pair in rest.chunks(2) {
                    let write = match pair[0] {
                        "+w" => true,
                        "+r" => false,
                        other => return fail(io, Some(other), "not +r or +w", &[]),
                    };
                    // The shell has read `~` already (ADR-0150 rule 8): a path
                    // under home is written back with it, as the mockups and
                    // grants show it.
                    let path = grants::shown(pair[1], &self.home());
                    if !rooms::is_grant_path(&path) {
                        return fail(
                            io,
                            Some(pair[1]),
                            "a grant's path starts with ~ or /, with no . or ..",
                            &[],
                        );
                    }
                    // INV-0172-4: a room holds only what its creator holds.
                    // The shell in a terminal holds the volume's root
                    // (ADR-0168, INV-0168-2); a shell without it holds no
                    // path to give.
                    let abs = self.room_path(&path);
                    if !self.root.as_ref().is_some_and(|h| h.covers(&abs, write)) {
                        return fail(
                            io,
                            Some(pair[1]),
                            "outside what this shell holds",
                            &["grants shows what a line here is given"],
                        );
                    }
                    given.push(Grant { write, path });
                }
                let bytes = self
                    .open(ROOMS, 0)
                    .and_then(|f| read_all(&f))
                    .unwrap_or_default();
                let table = rooms::read(&bytes);
                // The six words (ADR-0266 point 1); a trusted room has the
                // ones given. `--tmp` is for untrusted rooms only.
                let mut limits = rooms::Limits::default();
                if let Err(e) = Self::limit_options(d, p, &mut limits, false) {
                    return fail(io, Some(name), &e, &[]);
                }
                let room = Room {
                    name: (*name).into(),
                    untrusted: false,
                    colour: table.free_colour(),
                    limits,
                    grants: given,
                };
                match rooms::add(&bytes, &room) {
                    Ok(out) => match self.publish_file(ROOMS, &out) {
                        Ok(()) => {
                            self.apply_disk(&room, io);
                            io.out.push_str(&format!("{}\n", room.line()));
                            0
                        }
                        Err(e) => fail(io, Some(ROOMS), &e, &[]),
                    },
                    Err(rooms::RoomRefusal::Exists) => {
                        fail(io, Some(name), "a room of that name exists", &[])
                    }
                    Err(_) => fail(
                        io,
                        Some(name),
                        "the room table is full or the room is too large",
                        &[],
                    ),
                }
            }
            ["remove", name] => {
                // `--kill` ends the room's processes first (ADR-0269
                // point 2's `Stop`), also one a keeper started before it
                // was restarted (INV-0269-3).
                if p.flag(d, "kill") {
                    // The line first: `home` and a name that is no room kill
                    // nothing (`home` is the session's own terminals).
                    let table = self
                        .open(ROOMS, 0)
                        .and_then(|f| read_all(&f))
                        .unwrap_or_default();
                    if *name == zoraya_settings::rooms::HOME
                        || zoraya_settings::rooms::read(&table).get(name).is_none()
                    {
                        return fail(io, Some(name), "no such room", &[]);
                    }
                    match self.keeper.as_mut().map(|k| k.stop(name)) {
                        Some(Ok(())) | None => {}
                        Some(Err(zoraya_user::roomsclient::Fail::Denied(
                            zoraya_proto_rooms::status::NOT_RUNNING,
                            _,
                        ))) => {}
                        Some(Err(e)) => {
                            let why = match e {
                                zoraya_user::roomsclient::Fail::Denied(_, t) => t,
                                zoraya_user::roomsclient::Fail::Gone => {
                                    String::from("the room keeper does not answer")
                                }
                            };
                            return fail(io, Some(name), &why, &[]);
                        }
                    }
                }
                let bytes = self
                    .open(ROOMS, 0)
                    .and_then(|f| read_all(&f))
                    .unwrap_or_default();
                match rooms::remove(&bytes, name) {
                    Ok(out) => match self.publish_file(ROOMS, &out) {
                        Ok(()) => {
                            if current.as_deref() == Some(*name) {
                                self.leave_room_job();
                                self.room = None;
                            }
                            0
                        }
                        Err(e) => fail(io, Some(ROOMS), &e, &[]),
                    },
                    Err(_) => fail(io, Some(name), "no such room", &[]),
                }
            }
            ["set", name] => self.room_set(d, p, name, io),
            [name] => match self.enter_room(name) {
                Ok(()) => 0,
                Err(e) => {
                    io.err.push_str(&e);
                    1
                }
            },
            [name, ..] => fail(
                io,
                Some(name),
                "one line in a room is not built yet",
                &["enter it with room NAME, run the line, and room --leave"],
            ),
        }
    }

    /// Writes the settings file and applies it at once (ADR-0157 item 4).
    fn write_settings(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.publish_file(SETTINGS, bytes)?;
        self.settings = zoraya_settings::read(bytes, &self.declared);
        Ok(())
    }

    /// `grants LINE...`: what the command on the line would receive, and
    /// nothing run (ADR-0153 rule 7, INV-0153-4).
    /// `grants LINE` (rule 7): what running the line would give, planned
    /// as a run plans it (review S6).
    fn grants_report(&mut self, exec: &Exec<'_>, io: &mut Io) -> i32 {
        let name = exec.name;
        if BUILTINS.contains(&name) || LANGUAGE.contains(&name) || FILE_COMMANDS.contains(&name) {
            io.out.push_str(&format!(
                "{name} is built into the shell: it acts with the shell's own access\n"
            ));
            return 0;
        }
        // A program of the volume: a POSIX one is shown with its toolchain
        // set and its line's /tmp; a native one as a run gives it.
        if self.lookup(name) == Found::Program
            && let Some(path) = self.on_volume.get(name).cloned()
        {
            let native = self.root.as_ref().is_some_and(|r| {
                r.open_followed(path.trim_start_matches('/'), 0, file_rights::READ)
                    .and_then(|f| read_all(&f))
                    .is_ok_and(|bytes| zoraya_elf::is_native(&bytes))
            });
            let plan = if native {
                zoraya_describe::parse(UNDESCRIBED.as_bytes())
                    .map_err(|_| String::from("zosh: a description of its own is refused\n"))
                    .and_then(|d| self.plan_with(exec, &d, false))
            } else {
                self.posix_plan(exec, &path)
            };
            return match plan {
                Err(e) => {
                    io.err.push_str(&e);
                    1
                }
                Ok(plan) => {
                    let home = self.home();
                    io.out
                        .push_str(&grants::report(name, &plan, &home, "the terminal", !native));
                    0
                }
            };
        }
        match self.plan_for(exec) {
            Err(e) => {
                io.err.push_str(&e);
                1
            }
            Ok(None) => {
                io.out.push_str(&format!(
                    "{name} gets the console and its own job, and no files: it has no description\n"
                ));
                0
            }
            Ok(Some(plan)) => {
                let home = self.home();
                io.out
                    .push_str(&grants::report(name, &plan, &home, "the terminal", false));
                0
            }
        }
    }
}

impl Z {
    /// Where `name` is on the volume, by `shell.path` (its `/zoraya/bin`
    /// is the ramdisk) or, with a `/`, as a path.
    fn find_on_volume(&mut self, name: &str) -> Option<String> {
        if name.is_empty() || name.ends_with(".cmd") || name.ends_with('/') {
            return None;
        }
        let candidates: Vec<String> = if name.contains('/') {
            alloc::vec![self.abs(name)]
        } else {
            self.setting("shell.path")
                .split_whitespace()
                .filter(|d| *d != "/zoraya/bin")
                .map(|d| resolve(d, name))
                .collect()
        };
        let root = self.root.as_ref()?;
        candidates.into_iter().find(|abs| {
            root.open_followed(abs.trim_start_matches('/'), 0, file_rights::READ)
                .and_then(|f| f.stat())
                .is_ok_and(|st| st.kind != proto_kind::DIRECTORY)
        })
    }

    /// A builtin, or a program of the ramdisk.
    fn lookup_image(&mut self, name: &str) -> Found {
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
}

impl Host for Z {
    fn cwd(&mut self) -> String {
        self.cwd.clone()
    }

    fn grants(&mut self, _: &mut State, exec: &Exec<'_>, io: &mut Io) -> i32 {
        let code = self.grants_report(exec, io);
        // ADR-0266 point 7: in a room, the room's limits, in the words of its
        // line, from the keeper's record (the truth), else from the line.
        if code == 0
            && let Some(name) = self.room.as_ref().map(|r| r.name.clone())
        {
            let mut limits = self.room.as_ref().map(|r| r.limits).unwrap_or_default();
            if let Some(Ok(records)) = self.keeper.as_mut().map(|k| k.usage(&name))
                && let Some(r) = records.first()
            {
                limits = zoraya_rooms::record_limits(r);
            }
            let words = limits.words();
            if !words.is_empty() {
                io.out.push_str(&format!("limits {words}  (room {name})\n"));
            }
        }
        code
    }

    fn lookup(&mut self, name: &str) -> Found {
        let f = self.lookup_image(name);
        // The volume is looked at each time: what a line builds, the next
        // runs (`tcc hello.c -o hello`, then `./hello`).
        self.on_volume.remove(name);
        if f == Found::Nothing
            && let Some(path) = self.find_on_volume(name)
        {
            self.on_volume.insert(name.into(), path);
            return Found::Program;
        }
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
                        self.send_title();
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
            "jobs" => {
                self.refresh_states();
                io.out.push_str(&self.jobs.list());
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
                self.refresh_states();
                if let Some(n) = which
                    && self
                        .jobs
                        .get(n)
                        .is_some_and(|e| e.state == JobState::Stopped)
                {
                    let problem = format!("%{n} is stopped");
                    let hint = format!("fg %{n} or bg %{n} continues it");
                    io.err
                        .push_str(&zoraya_command::error("wait", None, &problem, &[&hint]));
                    return 1;
                }
                show(&self.jobs.take_notices());
                i32::from(!self.wait_for(which, DEADLINE_INFINITE))
            }
            "fg" => {
                let n = match self.which_job(p.operand(d, "JOB")) {
                    Ok(n) => n,
                    Err(e) => {
                        let problem = if p.operand(d, "JOB").is_some() {
                            e
                        } else {
                            "no job to continue".into()
                        };
                        io.err
                            .push_str(&zoraya_command::error("fg", None, &problem, &[]));
                        return 1;
                    }
                };
                let Some(e) = self.jobs.get_mut(n) else {
                    return 1;
                };
                let line = e.line.clone();
                e.state = JobState::Running;
                show(&format!("{line}\n"));
                self.resume_job(n);
                match self.wait_foreground(n) {
                    Waited::Stopped(_) => 1,
                    Waited::Ended(entry, records, ctrl_c) => {
                        let record = Z::status_of(&entry.payload, &records);
                        let _ = self.ended_in_front(*entry, records, ctrl_c);
                        if let Some(l) = record::screen_line(&line, &record, ctrl_c) {
                            show(&format!("{l}\n"));
                        }
                        i32::from(!record::is_success(&record))
                    }
                }
            }
            "bg" => {
                self.refresh_states();
                let n = match p.operand(d, "JOB") {
                    None => match self.jobs.newest_stopped() {
                        Some(n) => n,
                        None => {
                            io.err.push_str(&zoraya_command::error(
                                "bg",
                                None,
                                "no stopped job",
                                &[],
                            ));
                            return 1;
                        }
                    },
                    Some(w) => match self.which_job(Some(w)) {
                        Ok(n) => n,
                        Err(e) => {
                            io.err.push_str(&zoraya_command::error("bg", None, &e, &[]));
                            return 1;
                        }
                    },
                };
                let Some(e) = self.jobs.get_mut(n) else {
                    return 1;
                };
                if e.state == JobState::Running {
                    let problem = format!("%{n} is already running");
                    io.err
                        .push_str(&zoraya_command::error("bg", None, &problem, &[]));
                    return 1;
                }
                e.state = JobState::Running;
                let line = e.line.clone();
                // In the background its input is at its end (rule 2).
                self.close_keys(n);
                self.resume_job(n);
                io.out.push_str(&format!("[{n}] {line} &\n"));
                0
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
                let Some(job) = self.jobs.get(n).map(|e| e.payload.job) else {
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
                match kind {
                    Some(k) => {
                        let _ = job_signal(job, k, 0);
                        // A stopped job takes the request once it runs.
                        self.refresh_states();
                        if let Some(e) = self.jobs.get_mut(n)
                            && e.state == JobState::Stopped
                        {
                            e.state = JobState::Running;
                            self.close_keys(n);
                            self.resume_job(n);
                        }
                    }
                    None => {
                        let _ = job_kill(job);
                    }
                }
                show(&self.jobs.take_notices());
                if !self.wait_for(Some(n), clock_monotonic() + 2_000_000_000) {
                    show(&format!("[{n}] still running; kill --kill %{n} ends it\n"));
                }
                0
            }
            "settings" => self.settings_builtin(d, p, io),
            "room" => self.room_builtin(d, p, io),
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
                if let Exit::Warn(text) = self.jobs.exit() {
                    io.err.push_str(&text);
                    return 1;
                }
                for e in self.jobs.iter() {
                    let _ = job_kill(e.payload.job);
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
            // ADR-0159: as poweroff, with a reset; it returns only when the
            // machine has none.
            "reboot" => {
                if let Some(root) = &self.root
                    && let Err(s) = root.sync()
                {
                    io.err.push_str(&zoraya_command::error(
                        "reboot",
                        Some("sync"),
                        &problem(s),
                        &[],
                    ));
                }
                show(&core::mem::take(&mut io.err));
                let status = power_reset(POWER);
                io.err.push_str(&zoraya_command::error(
                    "reboot",
                    None,
                    &zoraya_command::status_problem(status),
                    &[],
                ));
                1
            }
            _ => 2,
        }
    }

    fn program(&mut self, pipe: &Pipe<'_>) -> Ran {
        // `ls /zoraya/bin` lists the ramdisk (ADR-0155 rule 8), which no
        // table can give a program.
        if let [exec] = pipe.programs
            && exec.name == "ls"
            && exec.redirects.is_empty()
            && exec.args.len() == 1
            && self.abs(&exec.args[0].text) == "/zoraya/bin"
        {
            let mut text = String::new();
            for n in ramdisk_programs() {
                text.push_str(&n);
                text.push('\n');
            }
            let mut captured = String::new();
            match pipe.out {
                Sink::Capture => captured = text,
                other => Host::write(self, other, &text),
            }
            return Ran::Ended {
                records: alloc::vec![record::exited(0)],
                ctrl_c: false,
                captured,
            };
        }
        if self.jobs.len() >= MAX_JOBS {
            show("zosh: too many jobs\n");
            return Ran::Refused(1);
        }
        // Every element's table and image before anything starts.
        let mut ready: Vec<Ready> = Vec::new();
        let mut given_all: Vec<String> = Vec::new();
        let give_back = |ready: Vec<Ready>| {
            for r in ready {
                r.give_back();
            }
        };
        // A line's /tmp is made by its first POSIX element.
        self.line_tmp_name = None;
        for exec in pipe.programs {
            match self.ready(exec) {
                Ok((r, given)) => {
                    ready.push(r);
                    if !given.is_empty() {
                        given_all.push(given);
                    }
                }
                Err(e) => {
                    show(&e);
                    give_back(ready);
                    self.drop_line_tmp();
                    return Ran::Refused(1);
                }
            }
        }
        let number = self.jobs.free_number();
        let job = match job_create(self.room_job.unwrap_or(JOB), JobFlags::NONE) {
            Ok(j) => j,
            Err(s) => {
                show(&zoraya_command::error(
                    "zosh",
                    None,
                    &zoraya_command::status_problem(s),
                    &[],
                ));
                give_back(ready);
                self.drop_line_tmp();
                return Ran::Refused(1);
            }
        };
        let mut given = Vec::new();
        let streams = self.streams(number, pipe, &mut given);
        let mut processes = Vec::new();
        let mut elements = Vec::new();
        let mut posix = Vec::new();
        let mut native = Vec::new();
        let mut failed = streams.as_ref().err().cloned();
        if let Ok(streams) = &streams {
            for ((exec, r), ends) in pipe.programs.iter().zip(ready).zip(streams) {
                let (image, table) = match r {
                    Ready::Native { image, table } => (image, table),
                    Ready::Posix {
                        image,
                        exe,
                        entries,
                        cwd_entry,
                        cwd,
                        report,
                        net,
                    } => {
                        let question = false;
                        elements.push((exec.name.to_string(), question, Some(report.0)));
                        if failed.is_some() {
                            for h in [image, report.1]
                                .into_iter()
                                .chain(net)
                                .chain(entries.iter().filter_map(|e| e.handle))
                            {
                                let _ = handle_close(h);
                            }
                            continue;
                        }
                        let mut args: Vec<&[u8]> = alloc::vec![exec.name.as_bytes()];
                        args.extend(exec.args.iter().map(|a| a.text.as_bytes()));
                        // PATH and HOME for a POSIX program unless the line
                        // gives them (ADR-0104 item 12 gives a session's).
                        let mut env: Vec<String> = exec.env.to_vec();
                        let home = format!("HOME={}", self.home());
                        for (name, value) in
                            [("PATH=", "PATH=/bin:/usr/bin"), ("HOME=", home.as_str())]
                        {
                            if !env.iter().any(|v| v.starts_with(name)) {
                                env.push(value.into());
                            }
                        }
                        let env: Vec<&[u8]> = env.iter().map(|v| v.as_bytes()).collect();
                        let launch = posix_child::Launch {
                            line_job: job,
                            image,
                            exe: &exe,
                            entries,
                            cwd_entry,
                            cwd: &cwd,
                            args: &args,
                            env: &env,
                            streams: *ends,
                            report: report.1,
                            net,
                            realtime_base: self.realtime_base,
                        };
                        let broker = self.broker.as_ref().expect("checked by ready");
                        match posix_child::launch(broker, launch) {
                            Ok(c) => {
                                processes.push(c.process);
                                posix.push((c.pid, c.pgid, c.job));
                            }
                            Err(s) => {
                                failed = Some(zoraya_command::error(
                                    "zosh",
                                    Some(exec.name),
                                    &zoraya_command::status_problem(s),
                                    &[],
                                ));
                            }
                        }
                        let _ = handle_close(image);
                        continue;
                    }
                };
                let startup = table.as_ref().map(|t| t.startup);
                // A job of its own inside the line's (ADR-0200 item 2), so
                // that Ctrl-Z can stop it apart from the POSIX elements.
                let own = if failed.is_none() {
                    job_create(job, JobFlags::NONE)
                } else {
                    Err(Status::BadState)
                };
                let started = match own {
                    Ok(own) => {
                        native.push(own);
                        self.spawn_in_job(own, image, exec, startup, *ends)
                    }
                    Err(s) => Err(s),
                };
                let _ = handle_close(image);
                let report = table.map(|t| t.report);
                let question = self.description(exec.name).is_some_and(|d| d.question);
                elements.push((exec.name.to_string(), question, report));
                match started {
                    Ok(p) => processes.push(p),
                    Err(s) if failed.is_none() => {
                        if let Some(t) = startup {
                            let _ = handle_close(t);
                        }
                        failed = Some(zoraya_command::error(
                            "zosh",
                            Some(exec.name),
                            &zoraya_command::status_problem(s),
                            &[],
                        ));
                    }
                    Err(_) => {
                        if let Some(t) = startup {
                            let _ = handle_close(t);
                        }
                    }
                }
            }
        } else {
            give_back(ready);
        }
        // INV-0152-1: the shell keeps none of the ends it gave.
        for h in given {
            let _ = handle_close(h);
        }
        let key = JOB_KEY + u64::from(number);
        if failed.is_none()
            && port_subscribe(self.port, job, job_signals::TERMINATED, key, Mode::Level).is_err()
        {
            failed = Some("zosh: too many jobs\n".into());
        }
        let record = Job {
            job,
            processes,
            elements,
            captured: Vec::new(),
            posix,
            native,
            tmp: self.line_tmp_name.take(),
        };
        if let Some(e) = failed {
            let _ = job_kill(job);
            // Its POSIX pids go once their jobs have ended, and its /tmp
            // (review S12).
            self.reap_killed(&record.posix);
            for p in &record.processes {
                let _ = handle_close(*p);
            }
            let _ = handle_close(job);
            self.finish_streams(number);
            if let (Some(t), Some(name)) = (&self.tmp, &record.tmp) {
                remove_tree(t, name);
            }
            show(&e);
            return Ran::Refused(1);
        }
        let added = self.jobs.add(pipe.line, given_all.join(", "), record);
        debug_assert_eq!(added, number, "the streams were made for this number");
        if pipe.background {
            let line = self
                .jobs
                .get(number)
                .map(|e| e.line.clone())
                .unwrap_or_default();
            show(&format!("[{number}] {line}\n"));
            return Ran::Background(number);
        }
        match self.wait_foreground(number) {
            Waited::Stopped(n) => Ran::Stopped(n),
            Waited::Ended(entry, records, ctrl_c) => self.ended_in_front(*entry, records, ctrl_c),
        }
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
        let abs = self.abs(path);
        if let Some(hint) = self.not_given("zosh", &abs, true, None) {
            return Err(hint);
        }
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

/// The volume as the file commands see it, with the shell's own access
/// until the commands are programs (TASK-18 step 7).
struct Volume<'a>(&'a Z);

impl zoraya_commands::Files for Volume<'_> {
    fn list(&mut self, dir: &str) -> Result<Vec<(String, zoraya_commands::Kind)>, u32> {
        let dir = self.0.open(dir, OPEN_DIRECTORY)?;
        let mut out = Vec::new();
        dir.list(&mut |name, kind| out.push((name.into(), kind_of(kind))))?;
        Ok(out)
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, u32> {
        read_all(&self.0.open(path, 0)?)
    }

    fn create(&mut self, path: &str, bytes: &[u8], replace: bool) -> Result<(), u32> {
        let exclusive = if replace {
            open_flags::TRUNCATE
        } else {
            open_flags::EXCLUSIVE
        };
        let file = self.0.open(path, open_flags::CREATE | exclusive)?;
        if !bytes.is_empty() {
            file.write(0, 0, bytes)?;
        }
        Ok(())
    }

    fn make_directory(&mut self, path: &str) -> Result<(), u32> {
        let (dir, name) = self.0.parent(path)?;
        dir.make_directory(&name)
    }

    fn remove(&mut self, path: &str) -> Result<(), u32> {
        let (dir, name) = self.0.parent(path)?;
        dir.remove(&name)
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), u32> {
        let (a, an) = self.0.parent(from)?;
        let (b, bn) = self.0.parent(to)?;
        a.rename(&an, &b, &bn)
    }

    fn stat(&mut self, path: &str) -> Result<zoraya_commands::Stat, u32> {
        let s = self.0.open(path, 0)?.stat()?;
        Ok(zoraya_commands::Stat {
            kind: kind_of(s.kind),
            size: s.size,
            modified: s.modified,
            changed: s.changed,
            mode: s.mode,
        })
    }

    fn sync(&mut self) -> Result<(), u32> {
        self.0
            .root
            .as_ref()
            .ok_or(zoraya_command::FILE_GONE)?
            .sync()
    }
}

fn kind_of(kind: u32) -> zoraya_commands::Kind {
    match kind {
        proto_kind::DIRECTORY => zoraya_commands::Kind::Directory,
        proto_kind::LINK => zoraya_commands::Kind::Link,
        _ => zoraya_commands::Kind::File,
    }
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
            for e in self.shell.host.jobs.iter() {
                let t = format!("%{}", e.number);
                if t.starts_with(word) {
                    c.candidates.push((t, Some(e.line.clone())));
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
    // The root at slot 5 (the console's shell), or a start-up record of
    // what a terminal's shell holds (ADR-0242's condition).
    let table = zoraya_user::namespace::take();
    let start_cwd = table.as_ref().map(|t| t.cwd.clone());
    let root = Handle::new(5)
        .filter(|h| handle_type(*h) == Ok("channel"))
        .map(|end| Held::Root(File { end }))
        .or_else(|| table.map(Held::table));
    // The shell's settings and the desktop's, which the compositor
    // declares (ADR-0169): `settings` knows both.
    let mut declared = c.d.settings.clone();
    for text in [COMPD, NETD] {
        if let Ok(d) = zoraya_describe::parse(text.as_bytes()) {
            declared.extend(d.settings);
        }
    }
    let mut z = Z {
        root,
        cwd: "/".into(),
        port,
        jobs: Jobs::default(),
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
        rows: 24,
        columns: 80,
        pending: Vec::new(),
        relays: Vec::new(),
        feeds: Vec::new(),
        next_stream: 0,
        foreground: None,
        at_prompt: false,
        typed: Vec::new(),
        escape: Escape::No,
        broker: Handle::new(7)
            .filter(|h| handle_type(*h) == Ok("channel"))
            .map(Broker::new),
        net: Handle::new(9).filter(|h| handle_type(*h) == Ok("channel")),
        random: Handle::new(10).filter(|h| handle_type(*h) == Ok("channel")),
        tmp: Handle::new(8)
            .filter(|h| handle_type(*h) == Ok("channel"))
            .map(|end| File { end }),
        next_tmp: 0,
        line_tmp_name: None,
        realtime_base: 0,
        on_volume: BTreeMap::new(),
        room: None,
        keeper: Handle::new(11)
            .filter(|h| handle_type(*h) == Ok("channel"))
            .and_then(|end| {
                zoraya_user::roomsclient::Keeper::new(
                    end,
                    Handle::new(12).filter(|h| handle_type(*h) == Ok("channel")),
                )
            }),
        keeper_manage: Handle::new(12).is_some_and(|h| handle_type(h) == Ok("channel")),
        room_job: None,
        refused_seen: (0, 0),
    };
    z.realtime_base = z
        .root
        .as_ref()
        .and_then(|r| r.time_base().ok())
        .unwrap_or(0);
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
        } else if let Some(cwd) = start_cwd {
            // A room's shell holds no home: it starts where its record says.
            z.cwd = cwd;
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
        editor.set_history(text.lines().map(|l| l.replace(ENTRY_BREAK, "\n")).collect());
    }
    // The panel's launch in a room (ADR-0172 point 4).
    if let Some(room) = c.parse().value(&c.d, "room")
        && let Err(e) = z.enter_room(room)
    {
        show(&e);
    }
    z.send_title();
    let mut shell = Shell::new(z);
    if shell.host.root.is_some()
        && let Ok(text) = shell.host.read_text(START)
    {
        shell.run_script(START, &text, Vec::new());
    }
    show(GREETING);
    let mut out = String::new();
    loop {
        // Ends read since the last prompt, before it (ADR-0158 rule 6).
        show(&shell.host.jobs.take_notices());
        shell.host.history = editor.history().to_vec();
        editor.options = options(&shell.host);
        let prompt = shell.host.prompt();
        out.clear();
        editor.start(&prompt, &mut out);
        show(&out);
        shell.host.at_prompt = true;
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
                    } else {
                        shell.host.event(packet, At::Elsewhere);
                    }
                }
            }
            if shell.host.jobs.has_notices() {
                // What comes while a line is edited goes above it, and the
                // line is drawn again below (INV-0158-4).
                out.clear();
                let text = shell.host.jobs.take_notices();
                editor.above(&text, &mut out);
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
        shell.host.at_prompt = false;
        let Some(entry) = entry else { continue };
        if entry.trim().is_empty() {
            continue;
        }
        // A second exit counts only right after the first (ADR-0158 rule 7).
        if entry.trim() != "exit" {
            shell.host.jobs.other_line();
        }
        run_entry(&mut shell, &entry);
        shell.host.quota_hint();
        // The line joins the history once it has run, so that `history`
        // lists what ran before it (the tour's example 21).
        if core::mem::take(&mut shell.host.clear_history) {
            editor.set_history(Vec::new());
        } else {
            editor.remember(&entry);
        }
        shell.host.history = editor.history().to_vec();
        if shell.host.setting("shell.history") == "yes" && shell.host.root.is_some() {
            // One line a file line: a line end inside an entry (a block
            // typed over several lines) is kept as ENTRY_BREAK (review S17).
            let lines: Vec<String> = editor
                .history()
                .iter()
                .map(|e| e.replace('\n', ENTRY_BREAK))
                .collect();
            let mut text = lines.join("\n");
            text.push('\n');
            let _ = shell.host.write_file(HISTORY, text.as_bytes());
        }
    }
}
