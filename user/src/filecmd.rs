//! The file commands as programs (ADR-0155 rule 8, ADR-0153): each is
//! `zoraya-commands` over the namespace `zosh` gave it, and reaches only
//! what its line designates.

use alloc::string::String;
use alloc::vec::Vec;

use zoraya_commands::{Files, Kind, Stat};
use zoraya_proto_file::{kind as proto_kind, open_flags, rights};

use crate::files::read_all;
use crate::namespace::{self, Namespace};
use crate::{command, eprint, input_read, print};

struct Table(&'static Namespace);

fn kind_of(kind: u32) -> Kind {
    match kind {
        proto_kind::DIRECTORY => Kind::Directory,
        proto_kind::LINK => Kind::Link,
        _ => Kind::File,
    }
}

impl Files for Table {
    fn list(&mut self, dir: &str) -> Result<Vec<(String, Kind)>, u32> {
        let d = self.0.open(dir, open_flags::DIRECTORY, rights::READ)?;
        let mut out = Vec::new();
        d.list(&mut |name, kind| out.push((name.into(), kind_of(kind))))?;
        Ok(out)
    }

    fn read(&mut self, path: &str) -> Result<Vec<u8>, u32> {
        read_all(&self.0.open(path, 0, rights::READ)?)
    }

    fn create(&mut self, path: &str, bytes: &[u8], replace: bool) -> Result<(), u32> {
        let how = if replace {
            open_flags::TRUNCATE
        } else {
            open_flags::EXCLUSIVE
        };
        let f = self
            .0
            .open(path, open_flags::CREATE | how, rights::READ | rights::WRITE)?;
        if !bytes.is_empty() {
            f.write(0, 0, bytes)?;
        }
        Ok(())
    }

    fn make_directory(&mut self, path: &str) -> Result<(), u32> {
        self.0
            .with_parent(path, rights::CREATE, |d, n| d.make_directory(n))
    }

    fn remove(&mut self, path: &str) -> Result<(), u32> {
        self.0.with_parent(path, rights::REMOVE, |d, n| d.remove(n))
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), u32> {
        let want = rights::RENAME | rights::CREATE | rights::REMOVE;
        self.0.with_parent(from, want, |a, an| {
            self.0.with_parent(to, want, |b, bn| a.rename(an, b, bn))
        })
    }

    fn stat(&mut self, path: &str) -> Result<Stat, u32> {
        let s = self.0.open(path, 0, rights::READ)?.stat()?;
        Ok(Stat {
            kind: kind_of(s.kind),
            size: s.size,
            modified: s.modified,
            changed: s.changed,
            mode: s.mode,
        })
    }

    fn sync(&mut self) -> Result<(), u32> {
        Err(zoraya_proto_file::status::ACCESS_DENIED)
    }

    fn read_input(&mut self, buffer: &mut [u8]) -> Result<usize, u32> {
        input_read(buffer).map_err(|_| zoraya_proto_file::status::STORAGE_FAILED)
    }

    fn print(&mut self, text: &str) -> bool {
        print!("{text}");
        true
    }
}

/// The whole of a file command's `main`.
pub fn main(name: &str, description: &str) -> i32 {
    let c = command::start(description);
    let p = c.parse();
    let Some(ns) = namespace::get() else {
        c.error(None, "it was given no files: run it from zosh");
        return i32::from(zoraya_command::failure_code(&c.d));
    };
    let (code, o) = zoraya_commands::run(name, &c.d, &p, &mut Table(ns));
    print!("{}", o.out);
    eprint!("{}", o.err);
    i32::from(code)
}
