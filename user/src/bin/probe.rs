#![no_std]
#![no_main]

//! `probe [--read FILE] [--change FILE] [--create FILE] [--remove PATH]
//! [--tree DIR] [PATH...]`: what a command got, for the grant tests
//! (ADR-0153, INV-0153-1, INV-0153-2, the native half of INV-0153-7). It
//! says what it holds at each slot, then for each entry of its start-up
//! record what each request of ADR-0055 gives through that connection,
//! and for each PATH what opening it to read and to change gives through
//! its namespace, twice (the shell reports a refused path once).

extern crate alloc;

use alloc::string::String;

use zoraya_abi::{Handle, MAX_HANDLES, native};
use zoraya_proto_file::{open_flags, rights, write_flags};
use zoraya_user::files::{File, FsResult};
use zoraya_user::{command, handle_type, namespace, println};

zoraya_user::describe!("tests", "probe");

fn word<T>(r: &FsResult<T>) -> String {
    match r {
        Ok(_) => "ok".into(),
        Err(s) => zoraya_command::file_problem(*s),
    }
}

/// Every request through one entry's connection, as `NAME RESULT` pairs.
fn requests(file: &File, kind: native::EntryKind) -> String {
    let mut out = String::new();
    let mut say = |name: &str, r: String| {
        out.push_str(&alloc::format!(" {name}={r}"));
    };
    say("stat", word(&file.stat()));
    say("widen", word(&file.clone_as(rights::ALL)));
    if kind == native::EntryKind::File {
        let mut b = [0u8; 16];
        say("read", word(&file.read(0, &mut b)));
        say("write", word(&file.write(0, write_flags::APPEND, b"")));
        say("sync", word(&file.sync()));
        say("share", word(&file.share(rights::ALL)));
        return out;
    }
    say("list", word(&file.list(&mut |_, _| {})));
    let created = file.open(
        "probe.tmp",
        open_flags::CREATE,
        rights::READ | rights::WRITE,
    );
    say("create", word(&created));
    say("mkdir", word(&file.make_directory("probe.dir")));
    say("remove", word(&file.remove("probe.tmp")));
    say("sync", word(&file.sync()));
    // The rest of ADR-0055's requests (review K10), after the ones above so
    // that their words keep their places.
    say("rename", word(&file.rename("probe.dir", file, "probe.mv")));
    let _ = file.remove("probe.dir");
    let _ = file.remove("probe.mv");
    let token = file.token();
    say("token", word(&token));
    if let Ok(t) = token {
        let _ = zoraya_user::handle_close(t);
    }
    say("share", word(&file.share(rights::ALL)));
    say("narrow", word(&file.narrow("probe.tmp", rights::ALL)));
    let unnamed = file.create_unnamed();
    say("unnamed", word(&unnamed));
    if let Ok(u) = &unnamed {
        say("publish", word(&u.publish(file, "probe.pub")));
        let _ = file.remove("probe.pub");
    }
    out
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let c = command::start(DESCRIPTION);
    let p = c.parse();
    let ns = namespace::get();
    let mut slots = String::from("slots:");
    for n in 1..=u64::from(MAX_HANDLES) {
        if let Some(Ok(t)) = Handle::new(n).map(handle_type) {
            slots.push_str(&alloc::format!(" {n}:{t}"));
        }
    }
    println!("{slots}");
    let Some(ns) = ns else {
        println!("no start-up record");
        return 1;
    };
    for e in &ns.entries {
        let kind = match e.kind {
            native::EntryKind::Directory => "directory",
            native::EntryKind::File => "file",
            native::EntryKind::Missing => "missing",
        };
        let tried = match &e.file {
            Some(f) => requests(f, e.kind),
            None => String::new(),
        };
        println!("entry {} {kind}:{tried}", e.path);
    }
    for path in p.operand_values(&c.d, "PATH") {
        for _ in 0..2 {
            let read = ns.open(path, 0, rights::READ);
            let change = ns.open(path, 0, rights::READ | rights::WRITE);
            println!("path {path}: read={} change={}", word(&read), word(&change));
        }
    }
    0
}
