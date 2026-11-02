#![no_std]
#![no_main]

//! The console shell (TASK-11). One port waits for the console and for every
//! job. Each program gets a sub-job and only the handles listed here.

use alloc::string::String;
use core::fmt::{self, Write};

use zoraya_abi::spawn::Encoder;
use zoraya_abi::{
    CONSOLE, DEADLINE_INFINITE, DIRECTORY, DirectoryEntry, ExitRecord, Handle, JOB, JobFlags,
    MAX_HANDLES, Mode, POWER, Packet, RequestKind, Rights, Status, console_signals, job_signals,
};
use zoraya_proto_file::{kind as proto_kind, open_flags, write_flags};
use zoraya_shell::{
    CTRL_BACKSLASH, CTRL_C, Editor, Key, Report, Words, job_number, request_flag, split,
};
use zoraya_user::files::{
    ALL, DIRECTORY as OPEN_DIRECTORY, File, FsResult, read_all, split_path, status_name,
};
use zoraya_user::{
    clock_monotonic, console_dropped, console_entry, console_read, console_write, directory_entry,
    directory_lookup, dup, handle_close, handle_type, job_create, job_kill, job_signal,
    port_create, port_subscribe, port_unsubscribe, port_wait, power_off, print, println,
    process_exit, process_exit_record, spawn,
};

extern crate alloc;

const PROMPT: &str = "zosh$ ";
const CONSOLE_KEY: u64 = 1;
/// Jobs at once.
const MAX_JOBS: usize = 8;
const NAME_SIZE: usize = 32;
/// Ctrl-Cs after which a foreground job that will not end is killed.
const PATIENCE: u32 = 3;
/// Programs that start others: they get their job and the directory too.
const NEED_MORE: [&str; 2] = ["fanout", "parent"];

struct Console;

impl Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        // Only the kernel's newline handling applies, so `\n` is enough.
        console_write(CONSOLE, s.as_bytes()).map_err(|_| fmt::Error)?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct Job {
    number: u32,
    job: Handle,
    process: Handle,
    name: [u8; NAME_SIZE],
    name_length: usize,
}

impl Job {
    fn name(&self) -> &str {
        core::str::from_utf8(&self.name[..self.name_length]).unwrap_or("?")
    }
}

/// What the prompt is waiting for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Waiting {
    Prompt,
    /// A foreground job, by number.
    Foreground(u32),
    /// `wait %n`: Ctrl-C ends the wait, not the job.
    One(u32),
    /// `wait`.
    All,
}

/// The volume: a root connection, the current directory and its path.
struct Volume {
    root: File,
    current: File,
    path: String,
}

impl Volume {
    /// The directory `path` starts from, and the path from there. A leading
    /// `/` starts at the root: `..` is the shell's, and only in `cd`.
    fn base<'a>(&self, path: &'a str) -> (&File, &'a str) {
        match path.strip_prefix('/') {
            Some(rest) => (&self.root, rest),
            None => (&self.current, path),
        }
    }

    /// Opens the directory holding `path`, and gives its last name.
    fn parent<'a>(&self, path: &'a str) -> FsResult<(File, &'a str)> {
        let (base, rest) = self.base(path);
        let (dir, name) = split_path(rest);
        let parent = if dir.is_empty() {
            base.clone_as(ALL)?
        } else {
            base.open(dir, OPEN_DIRECTORY, ALL)?
        };
        Ok((parent, name))
    }

    fn directory(&self, path: &str) -> FsResult<File> {
        let (base, rest) = self.base(path);
        if rest.is_empty() {
            base.clone_as(ALL)
        } else {
            base.open(rest, OPEN_DIRECTORY, ALL)
        }
    }

    fn list(&self, path: Option<&str>) -> FsResult<()> {
        let dir = self.directory(path.unwrap_or(""))?;
        dir.list(&mut |name, kind| {
            let mark = if kind == proto_kind::DIRECTORY {
                "/"
            } else {
                ""
            };
            println!("{name}{mark}");
        })
    }

    fn cat(&self, path: &str) -> FsResult<()> {
        let (base, rest) = self.base(path);
        let file = base.open(rest, 0, ALL)?;
        let data = read_all(&file)?;
        match core::str::from_utf8(&data) {
            Ok(text) => {
                for chunk in text.as_bytes().chunks(4096) {
                    if let Ok(part) = core::str::from_utf8(chunk) {
                        print!("{part}");
                    }
                }
                if !text.is_empty() && !text.ends_with('\n') {
                    println!();
                }
            }
            Err(_) => println!("{} bytes of binary", data.len()),
        }
        Ok(())
    }

    fn store(&self, path: &str, flags: u32, write: u32, words: &[&str]) -> FsResult<()> {
        let (base, rest) = self.base(path);
        let file = base.open(rest, flags, ALL)?;
        let mut line = String::new();
        for (i, word) in words.iter().enumerate() {
            if i > 0 {
                line.push(' ');
            }
            line.push_str(word);
        }
        line.push('\n');
        file.write(0, write, line.as_bytes()).map(|_| ())
    }

    fn mkdir(&self, path: &str) -> FsResult<()> {
        let (parent, name) = self.parent(path)?;
        parent.make_directory(name)
    }

    fn remove(&self, path: &str) -> FsResult<()> {
        let (parent, name) = self.parent(path)?;
        parent.remove(name)
    }

    fn rename(&self, old: &str, new: &str) -> FsResult<()> {
        let (from, old_name) = self.parent(old)?;
        let (to, new_name) = self.parent(new)?;
        from.rename(old_name, &to, new_name)
    }

    fn stat(&self, path: &str) -> FsResult<()> {
        let (base, rest) = self.base(path);
        let file = if rest.is_empty() {
            base.clone_as(ALL)?
        } else {
            base.open(rest, 0, ALL)?
        };
        let s = file.stat()?;
        let kind = match s.kind {
            proto_kind::DIRECTORY => "directory",
            proto_kind::LINK => "link",
            _ => "file",
        };
        println!(
            "{kind} size {} mtime {} ctime {} mode {:o}",
            s.size,
            s.modified / 1_000_000_000,
            s.changed / 1_000_000_000,
            s.mode
        );
        Ok(())
    }

    /// `cd`: `..` is taken off the path here, and the path opened from the root.
    fn change(&mut self, target: &str) -> FsResult<()> {
        let mut path = if target.starts_with('/') {
            String::new()
        } else {
            self.path.clone()
        };
        for part in target.split('/').filter(|p| !p.is_empty()) {
            if part == ".." {
                path.truncate(path.rfind('/').unwrap_or(0));
            } else {
                path.push('/');
                path.push_str(part);
            }
        }
        let next = match path.strip_prefix('/') {
            Some(relative) => self.root.open(relative, OPEN_DIRECTORY, ALL)?,
            None => self.root.clone_as(ALL)?,
        };
        self.current = next;
        self.path = path;
        Ok(())
    }
}

fn report(what: &str, result: FsResult<()>) {
    if let Err(status) = result {
        println!("zosh: {what}: {}", status_name(status));
    }
}

struct Shell {
    volume: Option<Volume>,
    port: Handle,
    jobs: [Option<Job>; MAX_JOBS],
    next_number: u32,
    waiting: Waiting,
    interrupts: u32,
    last: Option<ExitRecord>,
    dropped: u64,
    editor: Editor,
}

impl Shell {
    fn key_of(number: u32) -> u64 {
        u64::from(number) + 1
    }

    fn start(&mut self, words: &[&str], background: bool) {
        let name = words[0];
        let Some(free) = self.jobs.iter().position(Option::is_none) else {
            println!("zosh: too many jobs");
            return;
        };
        let image = match directory_lookup(DIRECTORY, name) {
            Ok(image) => image,
            Err(Status::NotFound) => {
                println!("zosh: no such program: {name}");
                return;
            }
            Err(e) => {
                println!("zosh: {name}: {e:?}");
                return;
            }
        };
        let started = self.spawn_in_job(image, words);
        let _ = handle_close(image);
        let (job, process) = match started {
            Ok(both) => both,
            Err(e) => {
                println!("zosh: {name}: {e:?}");
                return;
            }
        };
        let number = self.next_number;
        self.next_number += 1;
        let key = Shell::key_of(number);
        if let Err(e) = port_subscribe(self.port, job, job_signals::TERMINATED, key, Mode::Level) {
            println!("zosh: {name}: {e:?}");
            let _ = job_kill(job);
            let _ = handle_close(process);
            let _ = handle_close(job);
            return;
        }
        let mut entry = Job {
            number,
            job,
            process,
            name: [0; NAME_SIZE],
            name_length: name.len().min(NAME_SIZE),
        };
        entry.name[..entry.name_length].copy_from_slice(&name.as_bytes()[..entry.name_length]);
        self.jobs[free] = Some(entry);
        if background {
            println!("[{number}] {name}");
        } else {
            self.waiting = Waiting::Foreground(number);
            self.interrupts = 0;
        }
    }

    fn spawn_in_job(&self, image: Handle, words: &[&str]) -> Result<(Handle, Handle), Status> {
        let job = job_create(JOB, JobFlags::NONE)?;
        let mut buffer = [0; 1024];
        let mut e = Encoder::new(&mut buffer, job, image);
        if NEED_MORE.contains(&words[0]) {
            // They hand the console on to their children.
            let rights = Rights::WRITE.union(Rights::DUPLICATE);
            e.handle(dup(CONSOLE.slot(), CONSOLE, rights));
            e.handle(dup(JOB.slot(), job, Rights::CREATE_PROCESS));
            e.handle(dup(DIRECTORY.slot(), DIRECTORY, Rights::LOOKUP));
        } else {
            e.handle(console_entry());
        }
        for word in &words[1..] {
            e.arg(word.as_bytes());
        }
        let result = match e.finish() {
            Some(length) => spawn(&buffer[..length]),
            None => Err(Status::InvalidArgument),
        };
        match result {
            Ok(process) => Ok((job, process)),
            Err(e) => {
                let _ = handle_close(job);
                Err(e)
            }
        }
    }

    fn command(&mut self, line: &str) {
        let parsed: Words<'_> = match split(line) {
            Ok(words) => words,
            Err(_) => {
                println!("zosh: too many words");
                return;
            }
        };
        let words = parsed.as_slice();
        const BUILT_IN: [&str; 10] = [
            "help",
            "slots",
            "ls-ramdisk",
            "echo",
            "uptime",
            "exit-code",
            "jobs",
            "wait",
            "kill",
            "poweroff",
        ];
        if parsed.background && words.first().is_some_and(|w| BUILT_IN.contains(w)) {
            println!("zosh: & is for programs");
            return;
        }
        match words {
            [] => {}
            ["help"] => help(),
            ["ls-ramdisk"] => list(),
            [
                "ls" | "cat" | "write" | "append" | "mkdir" | "rm" | "mv" | "stat" | "sync" | "cd"
                | "pwd",
                ..,
            ] => self.file_command(words),
            ["echo", rest @ ..] => {
                for (i, word) in rest.iter().enumerate() {
                    print!("{}{word}", if i == 0 { "" } else { " " });
                }
                println!();
            }
            ["uptime"] => {
                let nanos = clock_monotonic();
                println!(
                    "up {}.{:03} s",
                    nanos / 1_000_000_000,
                    nanos / 1_000_000 % 1000
                );
            }
            ["exit-code"] => match self.last {
                Some(record) => println!("{}", Report(record)),
                None => println!("no program has ended"),
            },
            ["jobs"] => self.list_jobs(),
            ["slots"] => {
                print!("slots:");
                for slot in 1..=u64::from(MAX_HANDLES) {
                    if let Some(Ok(name)) = Handle::new(slot).map(handle_type) {
                        print!(" {slot}:{name}");
                    }
                }
                println!();
            }
            ["wait"] => {
                if self.jobs.iter().any(Option::is_some) {
                    self.waiting = Waiting::All;
                }
            }
            ["wait", which] => match self.find(which) {
                Some(number) => self.waiting = Waiting::One(number),
                None => println!("zosh: no such job: {which}"),
            },
            ["wait", ..] => println!("usage: wait [%n]"),
            ["kill", which] => self.signal(which, RequestKind::Terminate),
            ["kill", flag, which] => match request_flag(flag) {
                Some(kind) => self.signal(which, kind),
                None => println!("usage: kill [-int|-term|-hup|-quit] %n"),
            },
            ["kill", ..] => println!("usage: kill [-int|-term|-hup|-quit] %n"),
            // `exit <n>` is the program. Alone, it ends the shell (ADR-0063).
            ["exit"] => process_exit(0),
            ["poweroff"] => {
                if let Some(volume) = &self.volume {
                    report("sync", volume.root.sync());
                }
                let status = power_off(POWER);
                println!("zosh: poweroff: {status:?}");
            }
            ["run"] => println!("usage: run <program> [args...]"),
            ["run", program @ ..] => self.start(program, parsed.background),
            program => self.start(program, parsed.background),
        }
    }

    fn file_command(&mut self, words: &[&str]) {
        let Some(volume) = self.volume.as_mut() else {
            println!("zosh: no volume");
            return;
        };
        let (name, usage) = (words[0], "usage: ");
        let create = open_flags::CREATE;
        let result = match words {
            ["ls"] => volume.list(None),
            ["ls", dir] => volume.list(Some(dir)),
            ["cat", file] => volume.cat(file),
            ["write", file, text @ ..] => {
                volume.store(file, create | open_flags::TRUNCATE, 0, text)
            }
            ["append", file, text @ ..] => volume.store(file, create, write_flags::APPEND, text),
            ["mkdir", dir] => volume.mkdir(dir),
            ["rm", path] => volume.remove(path),
            ["mv", old, new] => volume.rename(old, new),
            ["stat", path] => volume.stat(path),
            ["sync"] => volume.root.sync(),
            ["cd"] => volume.change("/"),
            ["cd", dir] => volume.change(dir),
            ["pwd"] => {
                println!(
                    "{}",
                    if volume.path.is_empty() {
                        "/"
                    } else {
                        &volume.path
                    }
                );
                Ok(())
            }
            _ => {
                println!("{usage}{name} ...");
                Ok(())
            }
        };
        report(name, result);
    }

    fn find(&self, word: &str) -> Option<u32> {
        let number = job_number(word)?;
        self.jobs
            .iter()
            .flatten()
            .find(|j| j.number == number)
            .map(|j| j.number)
    }

    fn list_jobs(&self) {
        let mut any = false;
        for job in self.jobs.iter().flatten() {
            println!("[{}] running {}", job.number, job.name());
            any = true;
        }
        if !any {
            println!("no jobs");
        }
    }

    fn signal(&self, which: &str, kind: RequestKind) {
        let found = self.find(which).and_then(|number| {
            self.jobs
                .iter()
                .flatten()
                .find(|j| j.number == number)
                .copied()
        });
        match found {
            Some(job) => {
                if let Err(e) = job_signal(job.job, kind, 0) {
                    println!("zosh: kill: {e:?}");
                }
            }
            None => println!("zosh: no such job: {which}"),
        }
    }

    fn foreground(&self) -> Option<Job> {
        let Waiting::Foreground(number) = self.waiting else {
            return None;
        };
        self.jobs
            .iter()
            .flatten()
            .find(|j| j.number == number)
            .copied()
    }

    /// One byte while something is running: only two keys mean anything.
    /// Everything else is dropped, so nothing typed reaches the next prompt.
    fn running_key(&mut self, byte: u8) {
        match (byte, self.foreground()) {
            (CTRL_C, Some(job)) => {
                self.interrupts += 1;
                println!("^C");
                if self.interrupts >= PATIENCE {
                    let _ = job_kill(job.job);
                } else {
                    let _ = job_signal(job.job, RequestKind::Interrupt, 0);
                }
            }
            (CTRL_BACKSLASH, Some(job)) => {
                println!("^\\");
                let _ = job_signal(job.job, RequestKind::Quit, 0);
            }
            (CTRL_C, None) => {
                println!("^C");
                self.waiting = Waiting::Prompt;
                print!("{PROMPT}");
            }
            _ => {}
        }
    }

    fn prompt_key(&mut self, byte: u8) {
        match self.editor.key(byte, &mut Console) {
            Ok(Key::Line) => {
                let mut line = [0; zoraya_shell::LINE_SIZE];
                let text = self.editor.line();
                let length = text.len();
                line[..length].copy_from_slice(text.as_bytes());
                self.editor.finish();
                let text = core::str::from_utf8(&line[..length]).unwrap_or("");
                self.command(text);
                if self.waiting == Waiting::Prompt {
                    print!("{PROMPT}");
                }
            }
            Ok(Key::Interrupt) => print!("{PROMPT}"),
            Ok(Key::Edited) | Err(_) => {}
        }
    }

    fn input(&mut self) {
        let mut bytes = [0; 64];
        loop {
            let count = console_read(CONSOLE, &mut bytes).unwrap_or(0);
            if count == 0 {
                break;
            }
            for &byte in &bytes[..count] {
                if self.waiting == Waiting::Prompt {
                    self.prompt_key(byte);
                } else {
                    self.running_key(byte);
                }
            }
        }
        let dropped = console_dropped(CONSOLE).unwrap_or(0);
        if dropped != self.dropped {
            println!("[input: {} bytes dropped]", dropped - self.dropped);
            self.dropped = dropped;
            if self.waiting == Waiting::Prompt {
                print!("{PROMPT}");
            }
        }
    }

    /// A job's tree has no live process left.
    fn ended(&mut self, key: u64) {
        let found = self
            .jobs
            .iter()
            .position(|j| j.is_some_and(|j| Shell::key_of(j.number) == key));
        let Some(slot) = found else { return };
        let Some(job) = self.jobs[slot].take() else {
            return;
        };
        let _ = port_unsubscribe(self.port, key);
        let record = process_exit_record(job.process);
        let _ = handle_close(job.process);
        let _ = handle_close(job.job);
        let foreground = self.waiting == Waiting::Foreground(job.number);
        if let Ok(record) = record {
            self.last = Some(record);
            let report = Report(record);
            if !foreground {
                println!("[{}] done {} {}", job.number, job.name(), report);
                if self.waiting == Waiting::Prompt {
                    print!("{PROMPT}{}", self.editor.line());
                }
            } else if !report.is_clean() {
                println!("{report}");
            }
        }
        let back = match self.waiting {
            Waiting::Foreground(n) | Waiting::One(n) => n == job.number,
            Waiting::All => self.jobs.iter().all(Option::is_none),
            Waiting::Prompt => false,
        };
        if back {
            self.waiting = Waiting::Prompt;
            print!("{PROMPT}");
        }
    }
}

fn help() {
    println!("ls [dir]  cat  write  append  mkdir  rm  mv  stat  sync  cd  pwd  ls-ramdisk");
    println!("run <program> [args...]  <program> [args...]  <command> &");
    println!("jobs  wait [%n]  kill [-int|-term|-hup|-quit] %n");
    println!("echo  uptime  exit-code  slots  exit  poweroff  help");
    println!("Ctrl-C interrupts the foreground job, or clears the line. Ctrl-\\ sends Quit.");
}

fn list() {
    let mut record = [0; 128];
    let mut cookie = 0;
    loop {
        let size = match directory_entry(DIRECTORY, cookie, &mut record) {
            Ok(0) => return,
            Ok(size) => size,
            Err(e) => {
                println!("zosh: ls: {e:?}");
                return;
            }
        };
        let Some(entry) = DirectoryEntry::decode(&record[..size]) else {
            return;
        };
        println!("{}", core::str::from_utf8(entry.name).unwrap_or("?"));
        if entry.next == 0 {
            return;
        }
        cookie = entry.next;
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let port = port_create().expect("port");
    port_subscribe(
        port,
        CONSOLE,
        console_signals::READABLE,
        CONSOLE_KEY,
        Mode::Level,
    )
    .expect("subscribe to the console");
    let root = Handle::new(5).filter(|h| handle_type(*h) == Ok("channel"));
    let volume = root.and_then(|end| {
        let root = File { end };
        let current = root.clone_as(ALL).ok()?;
        Some(Volume {
            root,
            current,
            path: String::new(),
        })
    });
    let mut shell = Shell {
        volume,
        port,
        jobs: [None; MAX_JOBS],
        next_number: 1,
        waiting: Waiting::Prompt,
        interrupts: 0,
        last: None,
        dropped: 0,
        editor: Editor::new(),
    };
    println!("zosh: type help");
    print!("{PROMPT}");
    let mut packets = [Packet::default(); 8];
    loop {
        let count = match port_wait(port, DEADLINE_INFINITE, &mut packets) {
            Ok(count) => count,
            Err(_) => continue,
        };
        for packet in &packets[..count] {
            if packet.key == CONSOLE_KEY {
                shell.input();
            } else {
                shell.ended(packet.key);
            }
        }
    }
}
