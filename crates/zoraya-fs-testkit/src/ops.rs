//! Random request sequences run against the engine and the model in
//! lockstep (the model-based tests of INV-0010-1/-2/-5, INV-0013-2/-4,
//! INV-0014-2/-3).

use zoraya_fs::{
    Attr, BlockDevice, FsError, Handle, ObjKind, OpenOptions, PublishMode, RenameMode, Timestamp,
    Volume,
};

use crate::clock::TestClock;
use crate::model::Model;
use crate::rng::Rng;
use crate::state::FsState;

/// One request. Handles are indices into the session's handle list; index 0
/// is the root.
#[derive(Debug, Clone)]
pub enum Op {
    Open {
        dir: usize,
        path: String,
        create: Option<u16>,
        exclusive: bool,
        truncate: bool,
        link_itself: bool,
    },
    Close {
        h: usize,
    },
    Mkdir {
        dir: usize,
        path: String,
        mode: u16,
    },
    Symlink {
        dir: usize,
        path: String,
        target: String,
    },
    Unlink {
        dir: usize,
        path: String,
    },
    Rmdir {
        dir: usize,
        path: String,
    },
    Rename {
        sdir: usize,
        spath: String,
        ddir: usize,
        dpath: String,
        mode: RenameMode,
    },
    SetAttr {
        h: usize,
        mode: Option<u16>,
        mtime: Option<Timestamp>,
    },
    SetSize {
        h: usize,
        size: u64,
    },
    Write {
        h: usize,
        off: u64,
        data: Vec<u8>,
    },
    Read {
        h: usize,
        off: u64,
        len: usize,
    },
    Stat {
        h: usize,
    },
    StatAt {
        dir: usize,
        path: String,
    },
    List {
        h: usize,
    },
    Readlink {
        h: usize,
    },
    CreateUnnamed {
        dir: usize,
        mode: u16,
    },
    Publish {
        h: usize,
        dir: usize,
        path: String,
        mode: PublishMode,
    },
    Sync,
    /// Advance the monotonic clock and let the engine commit if due.
    Tick {
        ns: u64,
    },
}

impl Op {
    /// Whether the request may change the volume.
    pub fn mutates(&self) -> bool {
        !matches!(
            self,
            Op::Read { .. }
                | Op::Stat { .. }
                | Op::StatAt { .. }
                | Op::List { .. }
                | Op::Readlink { .. }
        ) && !matches!(
            self,
            Op::Open {
                create: None,
                truncate: false,
                ..
            }
        )
    }
}

/// A request's successful result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Out {
    Unit,
    Opened { id: u64, kind: ObjKind },
    Written(usize),
    Data(Vec<u8>),
    Attr(Attr),
    List(Vec<(String, u64, ObjKind)>),
    Target(String),
}

/// How the scripted wall clock moves between requests (INV-0014-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockMode {
    /// Forward by a random amount, sometimes zero.
    Advancing,
    /// Never moves.
    Stuck,
    /// Moves in whole seconds only (a coarse clock).
    Coarse,
    /// Sometimes steps backwards.
    Stepping,
}

#[derive(Debug, Clone, Copy)]
struct Slot {
    eng: Handle,
    id: u64,
    kind: ObjKind,
}

/// A volume and the model, driven together.
pub struct Session<D: BlockDevice> {
    pub vol: Volume<D, TestClock>,
    pub model: Model,
    pub clock: TestClock,
    slots: Vec<Slot>,
    pub clock_mode: ClockMode,
    /// Requests that completed (replied), for crash marks.
    pub completed: u64,
    /// Every time the engine assigned, in completion order.
    pub assigned: Vec<Timestamp>,
    /// `(request index, time)` for each request that assigned a time.
    pub assigned_at: Vec<(u64, Timestamp)>,
    /// Ids of objects created, in order.
    pub created: Vec<u64>,
    /// Every object id ever returned.
    pub ids: Vec<u64>,
}

impl<D: BlockDevice> Session<D> {
    pub fn new(mut vol: Volume<D, TestClock>, model: Model, clock: TestClock) -> Self {
        let mut model = model;
        let eng = vol.root();
        let (id, kind) = model.root();
        Session {
            vol,
            model,
            clock,
            slots: vec![Slot { eng, id, kind }],
            clock_mode: ClockMode::Advancing,
            completed: 0,
            assigned: Vec::new(),
            assigned_at: Vec::new(),
            created: Vec::new(),
            ids: Vec::new(),
        }
    }

    pub fn handles(&self) -> usize {
        self.slots.len()
    }

    fn slot(&self, i: usize) -> Slot {
        self.slots[i % self.slots.len()]
    }

    fn mh(&self, i: usize) -> (u64, ObjKind) {
        let s = self.slot(i);
        (s.id, s.kind)
    }

    fn step_clock(&mut self, rng: &mut Rng) {
        let t = self.clock.wall_now();
        let nt = match self.clock_mode {
            ClockMode::Advancing => {
                let d = if rng.chance(1, 4) {
                    0
                } else {
                    rng.below(3_000_000_000)
                };
                add_ns(t, d)
            }
            ClockMode::Stuck => t,
            ClockMode::Coarse => {
                if rng.chance(1, 5) {
                    Timestamp {
                        secs: t.secs + 1,
                        nanos: 0,
                    }
                } else {
                    t
                }
            }
            ClockMode::Stepping => {
                if rng.chance(1, 4) {
                    Timestamp {
                        secs: t.secs - rng.below(100) as i64 - 1,
                        nanos: rng.below(1_000_000_000) as u32,
                    }
                } else {
                    add_ns(t, rng.below(2_000_000_000))
                }
            }
        };
        self.clock.set_wall(nt);
    }

    /// Runs `op` on both; returns both results. Panics with the op if they
    /// differ, unless the engine refused with `NoSpace` and changed nothing.
    pub fn step(&mut self, op: &Op, rng: &mut Rng) -> Result<Out, FsError> {
        self.step_clock(rng);
        let wall = self.clock.wall_now();
        let before_model = self.model.clone();
        self.model.begin(wall);
        let last_before = self.model.last;
        let next_before = self.model.next_id;
        let m = self.run_model(op);
        let e = self.run_engine(op);
        if e == Err(FsError::NoSpace) && m.is_ok() {
            // The engine refused for space; the model knows nothing of
            // space. The request must have changed nothing (ADR-0010 rule 6).
            self.model = before_model;
            self.completed += 1;
            return e;
        }
        assert_eq!(e, m, "engine and model differ on {op:?}");
        if self.model.last != last_before {
            self.assigned.push(self.model.last);
            self.assigned_at.push((self.completed, self.model.last));
        }
        if self.model.next_id != next_before {
            self.created.push(next_before);
        }
        match (op, &e) {
            (Op::Open { .. } | Op::CreateUnnamed { .. }, Ok(Out::Opened { id, kind })) => {
                // run_engine pushed the engine handle; record the model side.
                let last = self.slots.last_mut().expect("slot");
                last.id = *id;
                last.kind = *kind;
                self.ids.push(*id);
            }
            (Op::Close { h }, Ok(_)) if h % self.slots.len() != 0 => {
                let i = h % self.slots.len();
                let s = self.slots.remove(i);
                self.model.close(s.id);
            }
            _ => {}
        }
        self.completed += 1;
        e
    }

    fn run_model(&mut self, op: &Op) -> Result<Out, FsError> {
        let mh = |s: &Self, i: usize| s.mh(i);
        match op {
            Op::Open {
                dir,
                path,
                create,
                exclusive,
                truncate,
                link_itself,
            } => {
                let d = mh(self, *dir);
                self.model
                    .open(d, path, *create, *exclusive, *truncate, *link_itself)
                    .map(|(id, kind)| Out::Opened { id, kind })
            }
            // The root slot is never closed; the model side of a close is
            // applied in `step` once the engine succeeded.
            Op::Close { .. } => Ok(Out::Unit),
            Op::Mkdir { dir, path, mode } => {
                let d = mh(self, *dir);
                self.model.mkdir(d, path, *mode).map(|_| Out::Unit)
            }
            Op::Symlink { dir, path, target } => {
                let d = mh(self, *dir);
                self.model.symlink(d, path, target).map(|_| Out::Unit)
            }
            Op::Unlink { dir, path } => {
                let d = mh(self, *dir);
                self.model.unlink(d, path).map(|_| Out::Unit)
            }
            Op::Rmdir { dir, path } => {
                let d = mh(self, *dir);
                self.model.rmdir(d, path).map(|_| Out::Unit)
            }
            Op::Rename {
                sdir,
                spath,
                ddir,
                dpath,
                mode,
            } => {
                let (s, d) = (mh(self, *sdir), mh(self, *ddir));
                self.model
                    .rename(s, spath, d, dpath, *mode)
                    .map(|_| Out::Unit)
            }
            Op::SetAttr { h, mode, mtime } => {
                let x = mh(self, *h);
                self.model.set_attr(x, *mode, *mtime).map(|_| Out::Unit)
            }
            Op::SetSize { h, size } => {
                let x = mh(self, *h);
                self.model.set_size(x, *size).map(|_| Out::Unit)
            }
            Op::Write { h, off, data } => {
                let x = mh(self, *h);
                self.model.write(x, *off, data).map(Out::Written)
            }
            Op::Read { h, off, len } => self.model.read(mh(self, *h), *off, *len).map(Out::Data),
            Op::Stat { h } => Ok(Out::Attr(self.model.attr(mh(self, *h).0))),
            Op::StatAt { dir, path } => self.model.stat_at(mh(self, *dir), path).map(Out::Attr),
            Op::List { h } => self.model.list(mh(self, *h)).map(Out::List),
            Op::Readlink { h } => self.model.readlink(mh(self, *h)).map(Out::Target),
            Op::CreateUnnamed { dir, mode } => {
                let d = mh(self, *dir);
                self.model
                    .create_unnamed(d, *mode)
                    .map(|(id, kind)| Out::Opened { id, kind })
            }
            Op::Publish { h, dir, path, mode } => {
                let (x, d) = (mh(self, *h), mh(self, *dir));
                self.model.publish(x, d, path, *mode).map(|_| Out::Unit)
            }
            Op::Sync | Op::Tick { .. } => Ok(Out::Unit),
        }
    }

    /// Runs `op` on the engine only (for fault tests, where the model no
    /// longer applies). Keeps the handle list in step.
    pub fn step_engine(&mut self, op: &Op, rng: &mut Rng) -> Result<Out, FsError> {
        self.step_clock(rng);
        let r = self.run_engine(op);
        if let (Op::Close { h }, Ok(_) | Err(FsError::StorageFailed)) = (op, &r)
            && h % self.slots.len() != 0
        {
            self.slots.remove(h % self.slots.len());
        }
        self.completed += 1;
        r
    }

    fn run_engine(&mut self, op: &Op) -> Result<Out, FsError> {
        let v = &mut self.vol;
        let eh = |s: &[Slot], i: usize| s[i % s.len()].eng;
        let sl = &self.slots;
        match op {
            Op::Open {
                dir,
                path,
                create,
                exclusive,
                truncate,
                link_itself,
            } => {
                let o = OpenOptions {
                    create: *create,
                    exclusive: *exclusive,
                    truncate: *truncate,
                    link_itself: *link_itself,
                };
                match v.open(eh(sl, *dir), path, o) {
                    Ok(h) => {
                        let a = v.stat(h)?;
                        self.slots.push(Slot {
                            eng: h,
                            id: a.id,
                            kind: a.kind,
                        });
                        Ok(Out::Opened {
                            id: a.id,
                            kind: a.kind,
                        })
                    }
                    Err(e) => Err(e),
                }
            }
            Op::Close { h } => {
                let i = h % sl.len();
                if i == 0 {
                    return Ok(Out::Unit);
                }
                v.close(sl[i].eng).map(|_| Out::Unit)
            }
            Op::Mkdir { dir, path, mode } => v.mkdir(eh(sl, *dir), path, *mode).map(|_| Out::Unit),
            Op::Symlink { dir, path, target } => {
                v.symlink(eh(sl, *dir), path, target).map(|_| Out::Unit)
            }
            Op::Unlink { dir, path } => v.unlink(eh(sl, *dir), path).map(|_| Out::Unit),
            Op::Rmdir { dir, path } => v.rmdir(eh(sl, *dir), path).map(|_| Out::Unit),
            Op::Rename {
                sdir,
                spath,
                ddir,
                dpath,
                mode,
            } => v
                .rename(eh(sl, *sdir), spath, eh(sl, *ddir), dpath, *mode)
                .map(|_| Out::Unit),
            Op::SetAttr { h, mode, mtime } => {
                v.set_attr(eh(sl, *h), *mode, *mtime).map(|_| Out::Unit)
            }
            Op::SetSize { h, size } => v.set_size(eh(sl, *h), *size).map(|_| Out::Unit),
            Op::Write { h, off, data } => v.write(eh(sl, *h), *off, data).map(Out::Written),
            Op::Read { h, off, len } => {
                let mut buf = vec![0u8; *len];
                v.read(eh(sl, *h), *off, &mut buf).map(|n| {
                    buf.truncate(n);
                    Out::Data(buf)
                })
            }
            Op::Stat { h } => v.stat(eh(sl, *h)).map(Out::Attr),
            Op::StatAt { dir, path } => v.stat_at(eh(sl, *dir), path).map(Out::Attr),
            Op::List { h } => {
                let h = eh(sl, *h);
                let mut out = Vec::new();
                let mut after: Option<String> = None;
                loop {
                    match v.readdir(h, after.as_deref()) {
                        Ok(Some(e)) => {
                            after = Some(e.name.clone());
                            out.push((e.name, e.id, e.kind));
                        }
                        Ok(None) => break Ok(Out::List(out)),
                        Err(e) => break Err(e),
                    }
                }
            }
            Op::Readlink { h } => v.readlink(eh(sl, *h)).map(Out::Target),
            Op::CreateUnnamed { dir, mode } => match v.create_unnamed(eh(sl, *dir), *mode) {
                Ok(h) => {
                    let a = v.stat(h)?;
                    self.slots.push(Slot {
                        eng: h,
                        id: a.id,
                        kind: a.kind,
                    });
                    Ok(Out::Opened {
                        id: a.id,
                        kind: a.kind,
                    })
                }
                Err(e) => Err(e),
            },
            Op::Publish { h, dir, path, mode } => v
                .publish(eh(sl, *h), eh(sl, *dir), path, *mode)
                .map(|_| Out::Unit),
            Op::Sync => v.sync().map(|_| Out::Unit),
            Op::Tick { ns } => {
                self.clock.advance_mono(*ns);
                v.tick().map(|_| Out::Unit)
            }
        }
    }

    /// The engine's observable state and the model's must be equal.
    pub fn compare(&mut self) -> FsState {
        let e = crate::state::engine_state(&mut self.vol).expect("engine state");
        let m = self.model.state();
        if e != m {
            for (k, v) in &m {
                match e.get(k) {
                    None => eprintln!("missing in engine: {k:?}"),
                    Some(x) if x != v => {
                        eprintln!("differs at {k:?}:\n engine {x:?}\n model  {v:?}")
                    }
                    _ => {}
                }
            }
            for k in e.keys() {
                if !m.contains_key(k) {
                    eprintln!("extra in engine: {k:?}");
                }
            }
            panic!("engine state differs from the model");
        }
        let (counted, walked) = self.vol.debug_dirty_counts();
        assert_eq!(counted, walked, "dirty block accounting");
        m
    }
}

pub fn add_ns(t: Timestamp, ns: u64) -> Timestamp {
    let total = u64::from(t.nanos) + ns;
    Timestamp {
        secs: t.secs + (total / 1_000_000_000) as i64,
        nanos: (total % 1_000_000_000) as u32,
    }
}

/// Generation of random requests.
#[derive(Debug, Clone, Copy)]
pub struct Gen {
    /// Largest write in bytes.
    pub max_write: usize,
    /// Largest file offset.
    pub max_off: u64,
    /// Weight of invalid names and paths.
    pub invalid: u64,
    /// Weight of `Sync`.
    pub sync: u64,
    /// Whether to include `Tick`.
    pub tick: bool,
}

impl Default for Gen {
    fn default() -> Self {
        Gen {
            max_write: 9000,
            max_off: 20_000,
            invalid: 2,
            sync: 2,
            tick: true,
        }
    }
}

const NAMES: &[&str] = &[
    "a",
    "b",
    "c",
    "d",
    "A",
    "\u{e9}",
    "e\u{301}",
    "long-name-0123456789",
];

fn name(rng: &mut Rng) -> String {
    rng.pick(NAMES).to_string()
}

fn path(rng: &mut Rng, g: &Gen) -> String {
    if rng.below(100) < g.invalid {
        let bad = [
            String::new(),
            "/a".into(),
            "a/".into(),
            "a//b".into(),
            ".".into(),
            "a/..".into(),
            "../a".into(),
            "x\u{7}".into(),
            "x\u{85}y".into(),
            "x\u{7f}".into(),
            "n".repeat(256),
        ];
        return rng.pick(&bad).clone();
    }
    let depth = 1 + rng.below(3) as usize;
    (0..depth).map(|_| name(rng)).collect::<Vec<_>>().join("/")
}

/// A random request for a session with `handles` handles.
pub fn random_op(rng: &mut Rng, handles: usize, g: &Gen) -> Op {
    let h = |rng: &mut Rng| rng.below(handles as u64) as usize;
    let mode = |rng: &mut Rng| {
        if rng.chance(1, 30) {
            0o17777
        } else {
            (rng.below(0o10000)) as u16
        }
    };
    match rng.below(100) {
        0..=14 => Op::Open {
            dir: h(rng),
            path: path(rng, g),
            create: if rng.chance(2, 3) {
                Some(mode(rng))
            } else {
                None
            },
            exclusive: rng.chance(1, 3),
            truncate: rng.chance(1, 6),
            link_itself: rng.chance(1, 4),
        },
        15..=22 => Op::Close { h: h(rng) },
        23..=30 => Op::Mkdir {
            dir: h(rng),
            path: path(rng, g),
            mode: mode(rng),
        },
        31..=33 => Op::Symlink {
            dir: h(rng),
            path: path(rng, g),
            target: if rng.chance(1, 2) {
                path(rng, g)
            } else {
                "../x/y".into()
            },
        },
        34..=39 => Op::Unlink {
            dir: h(rng),
            path: path(rng, g),
        },
        40..=43 => Op::Rmdir {
            dir: h(rng),
            path: path(rng, g),
        },
        44..=51 => Op::Rename {
            sdir: h(rng),
            spath: path(rng, g),
            ddir: h(rng),
            dpath: path(rng, g),
            mode: *rng.pick(&[
                RenameMode::Replace,
                RenameMode::NoReplace,
                RenameMode::Exchange,
            ]),
        },
        52..=54 => Op::SetAttr {
            h: h(rng),
            mode: rng.chance(1, 2).then(|| mode(rng)),
            mtime: rng.chance(1, 2).then(|| Timestamp {
                secs: rng.next_u64() as i64 >> rng.below(64),
                nanos: if rng.chance(1, 20) {
                    1_000_000_000 + rng.below(100) as u32
                } else {
                    rng.below(1_000_000_000) as u32
                },
            }),
        },
        55..=58 => Op::SetSize {
            h: h(rng),
            size: rng.below(g.max_off),
        },
        59..=72 => {
            let len = rng.below(g.max_write as u64 + 1) as usize;
            let mut data = vec![0u8; len];
            rng.fill(&mut data);
            Op::Write {
                h: h(rng),
                off: rng.below(g.max_off),
                data,
            }
        }
        73..=78 => Op::Read {
            h: h(rng),
            off: rng.below(g.max_off),
            len: rng.below(g.max_write as u64) as usize,
        },
        79..=80 => Op::Stat { h: h(rng) },
        81..=82 => Op::StatAt {
            dir: h(rng),
            path: path(rng, g),
        },
        83..=85 => Op::List { h: h(rng) },
        86 => Op::Readlink { h: h(rng) },
        87..=89 => Op::CreateUnnamed {
            dir: h(rng),
            mode: mode(rng),
        },
        90..=93 => Op::Publish {
            h: h(rng),
            dir: h(rng),
            path: path(rng, g),
            mode: *rng.pick(&[PublishMode::Replace, PublishMode::NoReplace]),
        },
        n if n < 94 + g.sync => Op::Sync,
        _ if g.tick => Op::Tick {
            ns: rng.below(3 * zoraya_fs::T_COMMIT_NS / 2),
        },
        _ => Op::Stat { h: h(rng) },
    }
}
