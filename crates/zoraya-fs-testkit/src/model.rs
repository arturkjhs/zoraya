//! The reference model of ADR-0010's contract: a namespace and file contents
//! in plain Rust collections, with the request semantics written out
//! directly. It knows nothing of blocks, trees or commits.
//!
//! The model decides the same errors in the same order as the engine (the
//! order a request's checks are made is part of its observable result), and
//! assigns times by ADR-0014 rule 3 from the same scripted wall clock.

use std::collections::BTreeMap;

use zoraya_fs::format::{MAX_FILE_SIZE, MAX_LINK_TARGET, MODE_MASK, ROOT_ID};
use zoraya_fs::{
    Attr, FsError, MAX_ATOMIC_WRITE, ObjKind, PublishMode, RelPath, RenameMode, Timestamp,
};

use crate::state::{FsState, ObjState};

#[derive(Debug, Clone)]
pub struct MObj {
    pub kind: ObjKind,
    pub mode: u16,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    /// File data or link target.
    pub data: Vec<u8>,
    /// Directory entries, in byte order.
    pub entries: BTreeMap<Vec<u8>, u64>,
    pub parent: Option<u64>,
    pub orphan: bool,
}

impl MObj {
    fn size(&self) -> u64 {
        match self.kind {
            ObjKind::Directory => self.entries.len() as u64,
            _ => self.data.len() as u64,
        }
    }
}

/// The model's state.
#[derive(Debug, Clone)]
pub struct Model {
    pub objs: BTreeMap<u64, MObj>,
    pub next_id: u64,
    pub last: Timestamp,
    pub open: BTreeMap<u64, u32>,
    /// The time of the current request, once assigned.
    req: Option<Timestamp>,
    /// The wall clock for the current request.
    wall: Timestamp,
}

type R<T> = Result<T, FsError>;

impl Model {
    /// A fresh volume: an empty root directory with the given mode and time.
    pub fn new(root_mode: u16, now: Timestamp) -> Model {
        let root = MObj {
            kind: ObjKind::Directory,
            mode: root_mode,
            mtime: now,
            ctime: now,
            data: Vec::new(),
            entries: BTreeMap::new(),
            parent: None,
            orphan: false,
        };
        Model {
            objs: BTreeMap::from([(ROOT_ID, root)]),
            next_id: 2,
            last: now,
            open: BTreeMap::new(),
            req: None,
            wall: now,
        }
    }

    /// Starts a request with the wall clock at `wall`.
    pub fn begin(&mut self, wall: Timestamp) {
        self.wall = wall;
        self.req = None;
    }

    fn now(&mut self) -> Timestamp {
        if let Some(t) = self.req {
            return t;
        }
        let t = if self.wall > self.last {
            self.wall
        } else {
            self.last.succ()
        };
        self.last = t;
        self.req = Some(t);
        t
    }

    pub fn obj(&self, id: u64) -> &MObj {
        &self.objs[&id]
    }

    fn obj_mut(&mut self, id: u64) -> &mut MObj {
        self.objs.get_mut(&id).expect("model object")
    }

    pub fn attr(&self, id: u64) -> Attr {
        let o = self.obj(id);
        Attr {
            id,
            kind: o.kind,
            size: o.size(),
            mtime: o.mtime,
            ctime: o.ctime,
            posix_mode: o.mode,
        }
    }

    fn dir(&self, h: (u64, ObjKind)) -> R<u64> {
        if h.1 != ObjKind::Directory {
            Err(FsError::NotDir)
        } else {
            Ok(h.0)
        }
    }

    fn entry(&self, dir: u64, name: &str) -> Option<(u64, ObjKind)> {
        self.obj(dir)
            .entries
            .get(name.as_bytes())
            .map(|&c| (c, self.obj(c).kind))
    }

    fn target(&self, id: u64) -> String {
        String::from_utf8(self.obj(id).data.clone()).expect("model link target")
    }

    fn walk(&self, start: u64, path: &str, n: usize) -> R<u64> {
        let mut cur = start;
        let mut pos = 0;
        for comp in path.split('/').take(n) {
            pos += comp.len() + 1;
            let (child, kind) = self.entry(cur, comp).ok_or(FsError::NotFound)?;
            match kind {
                ObjKind::Directory => cur = child,
                ObjKind::Link => {
                    return Err(FsError::IsLink {
                        remaining: path.get(pos..).unwrap_or("").to_string(),
                        target: self.target(child),
                    });
                }
                ObjKind::File => return Err(FsError::NotDir),
            }
        }
        Ok(cur)
    }

    fn parent_of<'p>(&self, dir: (u64, ObjKind), path: &'p str) -> R<(u64, &'p str)> {
        let start = self.dir(dir)?;
        let rp = RelPath::new(path)?;
        let n = rp.components().count();
        let parent = self.walk(start, path, n - 1)?;
        Ok((parent, rp.split_last().1.as_str()))
    }

    fn lookup(&self, dir: (u64, ObjKind), path: &str) -> R<(u64, ObjKind)> {
        let (p, name) = self.parent_of(dir, path)?;
        self.entry(p, name).ok_or(FsError::NotFound)
    }

    fn check_mode(mode: u16) -> R<()> {
        if mode & !MODE_MASK != 0 {
            Err(FsError::InvalidArgument)
        } else {
            Ok(())
        }
    }

    fn touch_dir(&mut self, dir: u64, t: Timestamp) {
        let d = self.obj_mut(dir);
        d.mtime = t;
        d.ctime = t;
    }

    fn create_obj(
        &mut self,
        parent: u64,
        name: &str,
        kind: ObjKind,
        mode: u16,
        target: Option<&str>,
    ) -> R<u64> {
        Self::check_mode(mode)?;
        if self.obj(parent).orphan {
            return Err(FsError::NotFound);
        }
        if self.entry(parent, name).is_some() {
            return Err(FsError::Exists);
        }
        let id = self.next_id;
        self.next_id += 1;
        let t = self.now();
        self.objs.insert(
            id,
            MObj {
                kind,
                mode,
                mtime: t,
                ctime: t,
                data: target.map(|s| s.as_bytes().to_vec()).unwrap_or_default(),
                entries: BTreeMap::new(),
                parent: (kind == ObjKind::Directory).then_some(parent),
                orphan: false,
            },
        );
        self.obj_mut(parent)
            .entries
            .insert(name.as_bytes().to_vec(), id);
        self.touch_dir(parent, t);
        Ok(id)
    }

    fn unname(&mut self, id: u64, t: Timestamp) {
        if self.open.get(&id).copied().unwrap_or(0) > 0 {
            let o = self.obj_mut(id);
            o.parent = None;
            o.ctime = t;
            o.orphan = true;
        } else {
            self.objs.remove(&id);
        }
    }

    fn resize(&mut self, id: u64, size: u64) {
        let t = self.now();
        let o = self.obj_mut(id);
        o.data.resize(size as usize, 0);
        o.mtime = t;
        o.ctime = t;
    }

    fn is_within(&self, mut dir: u64, anc: u64) -> bool {
        loop {
            if dir == anc {
                return true;
            }
            match self.obj(dir).parent {
                Some(p) => dir = p,
                None => return false,
            }
        }
    }

    fn opened(&mut self, id: u64) {
        *self.open.entry(id).or_insert(0) += 1;
    }

    // -- Requests. `dir` and `h` are (object id, kind) of a handle.

    pub fn open(
        &mut self,
        dir: (u64, ObjKind),
        path: &str,
        create: Option<u16>,
        exclusive: bool,
        truncate: bool,
        link_itself: bool,
    ) -> R<(u64, ObjKind)> {
        let r = if create.is_some() || truncate {
            let (parent, name) = self.parent_of(dir, path)?;
            match self.entry(parent, name) {
                Some((id, kind)) => {
                    if create.is_some() && exclusive {
                        return Err(FsError::Exists);
                    }
                    if kind == ObjKind::Link && !link_itself {
                        return Err(FsError::IsLink {
                            remaining: String::new(),
                            target: self.target(id),
                        });
                    }
                    if truncate {
                        match kind {
                            ObjKind::Directory => return Err(FsError::IsDir),
                            ObjKind::Link => return Err(FsError::InvalidArgument),
                            ObjKind::File => self.resize(id, 0),
                        }
                    }
                    (id, kind)
                }
                None => {
                    let Some(mode) = create else {
                        return Err(FsError::NotFound);
                    };
                    (
                        self.create_obj(parent, name, ObjKind::File, mode, None)?,
                        ObjKind::File,
                    )
                }
            }
        } else {
            let (id, kind) = self.lookup(dir, path)?;
            if kind == ObjKind::Link && !link_itself {
                return Err(FsError::IsLink {
                    remaining: String::new(),
                    target: self.target(id),
                });
            }
            (id, kind)
        };
        self.opened(r.0);
        Ok(r)
    }

    /// A handle to the root.
    pub fn root(&mut self) -> (u64, ObjKind) {
        self.opened(ROOT_ID);
        (ROOT_ID, ObjKind::Directory)
    }

    pub fn mkdir(&mut self, dir: (u64, ObjKind), path: &str, mode: u16) -> R<()> {
        let (p, name) = self.parent_of(dir, path)?;
        self.create_obj(p, name, ObjKind::Directory, mode, None)
            .map(|_| ())
    }

    pub fn symlink(&mut self, dir: (u64, ObjKind), path: &str, target: &str) -> R<()> {
        if target.len() > MAX_LINK_TARGET {
            return Err(FsError::InvalidArgument);
        }
        let (p, name) = self.parent_of(dir, path)?;
        self.create_obj(p, name, ObjKind::Link, 0o777, Some(target))
            .map(|_| ())
    }

    pub fn unlink(&mut self, dir: (u64, ObjKind), path: &str) -> R<()> {
        let (p, name) = self.parent_of(dir, path)?;
        let (id, kind) = self.entry(p, name).ok_or(FsError::NotFound)?;
        if kind == ObjKind::Directory {
            return Err(FsError::IsDir);
        }
        let t = self.now();
        self.obj_mut(p).entries.remove(name.as_bytes());
        self.touch_dir(p, t);
        self.unname(id, t);
        Ok(())
    }

    pub fn rmdir(&mut self, dir: (u64, ObjKind), path: &str) -> R<()> {
        let (p, name) = self.parent_of(dir, path)?;
        let (id, kind) = self.entry(p, name).ok_or(FsError::NotFound)?;
        if kind != ObjKind::Directory {
            return Err(FsError::NotDir);
        }
        if !self.obj(id).entries.is_empty() {
            return Err(FsError::NotEmpty);
        }
        let t = self.now();
        self.obj_mut(p).entries.remove(name.as_bytes());
        self.touch_dir(p, t);
        self.unname(id, t);
        Ok(())
    }

    pub fn rename(
        &mut self,
        sdir: (u64, ObjKind),
        spath: &str,
        ddir: (u64, ObjKind),
        dpath: &str,
        mode: RenameMode,
    ) -> R<()> {
        let (sp, sn) = self.parent_of(sdir, spath)?;
        let (dp, dn) = self.parent_of(ddir, dpath)?;
        let (sid, skind) = self.entry(sp, sn).ok_or(FsError::NotFound)?;
        if sp == dp && sn == dn {
            return Ok(());
        }
        let dst = self.entry(dp, dn);
        match (mode, dst) {
            (RenameMode::NoReplace, Some(_)) => return Err(FsError::Exists),
            (RenameMode::Exchange, None) => return Err(FsError::NotFound),
            (RenameMode::Replace, Some((did, dkind))) => {
                if skind == ObjKind::Directory && dkind != ObjKind::Directory {
                    return Err(FsError::NotDir);
                }
                if skind != ObjKind::Directory && dkind == ObjKind::Directory {
                    return Err(FsError::IsDir);
                }
                if dkind == ObjKind::Directory && !self.obj(did).entries.is_empty() {
                    return Err(FsError::NotEmpty);
                }
            }
            _ => {}
        }
        if self.obj(dp).orphan {
            return Err(FsError::NotFound);
        }
        if skind == ObjKind::Directory && self.is_within(dp, sid) {
            return Err(FsError::InvalidArgument);
        }
        if mode == RenameMode::Exchange
            && let Some((did, ObjKind::Directory)) = dst
            && self.is_within(sp, did)
        {
            return Err(FsError::InvalidArgument);
        }
        let t = self.now();
        match (mode, dst) {
            (RenameMode::Exchange, Some((did, dkind))) => {
                self.obj_mut(sp).entries.insert(sn.as_bytes().to_vec(), did);
                self.obj_mut(dp).entries.insert(dn.as_bytes().to_vec(), sid);
                for (id, kind, parent) in [(sid, skind, dp), (did, dkind, sp)] {
                    let o = self.obj_mut(id);
                    if kind == ObjKind::Directory {
                        o.parent = Some(parent);
                    }
                    o.ctime = t;
                }
                self.touch_dir(sp, t);
                self.touch_dir(dp, t);
            }
            _ => {
                if let Some((did, _)) = dst {
                    self.unname(did, t);
                }
                self.obj_mut(sp).entries.remove(sn.as_bytes());
                self.obj_mut(dp).entries.insert(dn.as_bytes().to_vec(), sid);
                let o = self.obj_mut(sid);
                if skind == ObjKind::Directory {
                    o.parent = Some(dp);
                }
                o.ctime = t;
                self.touch_dir(sp, t);
                self.touch_dir(dp, t);
            }
        }
        Ok(())
    }

    pub fn set_attr(
        &mut self,
        h: (u64, ObjKind),
        mode: Option<u16>,
        mtime: Option<Timestamp>,
    ) -> R<()> {
        if let Some(m) = mode {
            Self::check_mode(m)?;
        }
        if mtime.is_some_and(|t| !t.is_valid()) {
            return Err(FsError::InvalidArgument);
        }
        let t = self.now();
        let o = self.obj_mut(h.0);
        o.ctime = t;
        if let Some(m) = mode {
            o.mode = m;
        }
        if let Some(m) = mtime {
            o.mtime = m;
        }
        Ok(())
    }

    pub fn set_size(&mut self, h: (u64, ObjKind), size: u64) -> R<()> {
        match h.1 {
            ObjKind::Directory => return Err(FsError::IsDir),
            ObjKind::Link => return Err(FsError::InvalidArgument),
            ObjKind::File => {}
        }
        if size > MAX_FILE_SIZE {
            return Err(FsError::InvalidArgument);
        }
        self.resize(h.0, size);
        Ok(())
    }

    pub fn write(&mut self, h: (u64, ObjKind), off: u64, data: &[u8]) -> R<usize> {
        match h.1 {
            ObjKind::Directory => return Err(FsError::IsDir),
            ObjKind::Link => return Err(FsError::InvalidArgument),
            ObjKind::File => {}
        }
        if data.len() > MAX_ATOMIC_WRITE {
            return Err(FsError::InvalidArgument);
        }
        let end = off
            .checked_add(data.len() as u64)
            .filter(|&e| e <= MAX_FILE_SIZE)
            .ok_or(FsError::InvalidArgument)?;
        if data.is_empty() {
            return Ok(0);
        }
        let t = self.now();
        let o = self.obj_mut(h.0);
        if o.data.len() < end as usize {
            o.data.resize(end as usize, 0);
        }
        o.data[off as usize..end as usize].copy_from_slice(data);
        o.mtime = t;
        o.ctime = t;
        Ok(data.len())
    }

    pub fn read(&self, h: (u64, ObjKind), off: u64, len: usize) -> R<Vec<u8>> {
        if h.1 == ObjKind::Directory {
            return Err(FsError::IsDir);
        }
        let d = &self.obj(h.0).data;
        if off >= d.len() as u64 {
            return Ok(Vec::new());
        }
        let end = (off as usize).saturating_add(len).min(d.len());
        Ok(d[off as usize..end].to_vec())
    }

    pub fn stat_at(&self, dir: (u64, ObjKind), path: &str) -> R<Attr> {
        self.lookup(dir, path).map(|(id, _)| self.attr(id))
    }

    pub fn readlink(&self, h: (u64, ObjKind)) -> R<String> {
        if h.1 != ObjKind::Link {
            return Err(FsError::InvalidArgument);
        }
        Ok(self.target(h.0))
    }

    pub fn list(&self, h: (u64, ObjKind)) -> R<Vec<(String, u64, ObjKind)>> {
        let d = self.dir(h)?;
        Ok(self
            .obj(d)
            .entries
            .iter()
            .map(|(n, &c)| {
                (
                    String::from_utf8(n.clone()).expect("name"),
                    c,
                    self.obj(c).kind,
                )
            })
            .collect())
    }

    pub fn create_unnamed(&mut self, dir: (u64, ObjKind), mode: u16) -> R<(u64, ObjKind)> {
        let d = self.dir(dir)?;
        Self::check_mode(mode)?;
        if self.obj(d).orphan {
            return Err(FsError::NotFound);
        }
        let id = self.next_id;
        self.next_id += 1;
        let t = self.now();
        self.objs.insert(
            id,
            MObj {
                kind: ObjKind::File,
                mode,
                mtime: t,
                ctime: t,
                data: Vec::new(),
                entries: BTreeMap::new(),
                parent: None,
                orphan: true,
            },
        );
        self.opened(id);
        Ok((id, ObjKind::File))
    }

    pub fn publish(
        &mut self,
        h: (u64, ObjKind),
        dir: (u64, ObjKind),
        path: &str,
        mode: PublishMode,
    ) -> R<()> {
        if h.1 != ObjKind::File {
            return Err(FsError::InvalidArgument);
        }
        let (p, name) = self.parent_of(dir, path)?;
        if !self.obj(h.0).orphan {
            return Err(FsError::InvalidArgument);
        }
        if self.obj(p).orphan {
            return Err(FsError::NotFound);
        }
        let dst = self.entry(p, name);
        match (mode, dst) {
            (PublishMode::NoReplace, Some(_)) => return Err(FsError::Exists),
            (PublishMode::Replace, Some((_, ObjKind::Directory))) => return Err(FsError::IsDir),
            _ => {}
        }
        let t = self.now();
        if let Some((did, _)) = dst {
            self.unname(did, t);
        }
        let o = self.obj_mut(h.0);
        o.orphan = false;
        o.ctime = t;
        self.obj_mut(p)
            .entries
            .insert(name.as_bytes().to_vec(), h.0);
        self.touch_dir(p, t);
        Ok(())
    }

    pub fn close(&mut self, id: u64) {
        let n = self.open.get_mut(&id).expect("model handle");
        *n -= 1;
        if *n == 0 {
            self.open.remove(&id);
            if self.objs.get(&id).is_some_and(|o| o.orphan) {
                self.objs.remove(&id);
            }
        }
    }

    /// What a crash keeps of this state: the named objects (ADR-0010 rule 7:
    /// objects with no name are absent after a crash).
    pub fn state(&self) -> FsState {
        let mut out = FsState::new();
        self.collect(ROOT_ID, String::new(), &mut out);
        out
    }

    fn collect(&self, id: u64, path: String, out: &mut FsState) {
        let o = self.obj(id);
        out.insert(
            path.clone(),
            ObjState {
                id,
                kind: o.kind,
                mode: o.mode,
                mtime: o.mtime,
                ctime: o.ctime,
                size: o.size(),
                content: if o.kind == ObjKind::Directory {
                    Vec::new()
                } else {
                    o.data.clone()
                },
            },
        );
        for (n, &c) in &o.entries {
            let name = String::from_utf8(n.clone()).expect("name");
            let p = if path.is_empty() {
                name
            } else {
                format!("{path}/{name}")
            };
            self.collect(c, p, out);
        }
    }

    /// A model holding exactly `state` (as read from a remounted volume),
    /// with the engine's next id and last assigned time.
    pub fn from_state(state: &FsState, next_id: u64, last: Timestamp) -> Model {
        let mut objs = BTreeMap::new();
        for (path, o) in state {
            objs.insert(
                o.id,
                MObj {
                    kind: o.kind,
                    mode: o.mode,
                    mtime: o.mtime,
                    ctime: o.ctime,
                    data: o.content.clone(),
                    entries: BTreeMap::new(),
                    parent: None,
                    orphan: false,
                },
            );
            let _ = path;
        }
        for (path, o) in state {
            if path.is_empty() {
                continue;
            }
            let (dir, name) = match path.rfind('/') {
                Some(i) => (&path[..i], &path[i + 1..]),
                None => ("", path.as_str()),
            };
            let pid = state[dir].id;
            objs.get_mut(&pid)
                .expect("parent")
                .entries
                .insert(name.as_bytes().to_vec(), o.id);
            if o.kind == ObjKind::Directory {
                objs.get_mut(&o.id).expect("dir").parent = Some(pid);
            }
        }
        Model {
            objs,
            next_id,
            last,
            open: BTreeMap::new(),
            req: None,
            wall: last,
        }
    }

    /// Forgets every handle and orphan, as a crash or remount does.
    pub fn after_remount(&mut self, next_id: u64) {
        self.open.clear();
        self.objs.retain(|_, o| !o.orphan);
        self.next_id = next_id;
    }
}
