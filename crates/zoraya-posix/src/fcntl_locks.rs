//! `fcntl`'s record-lock commands on a process (ADR-0319): the part of
//! `struct flock` that needs the descriptor table and the file's identity.

#![forbid(unsafe_code)]

use crate::errno::*;
use crate::process::{Desc, Posix, o};
use crate::records::{self as r, FLOCK_SIZE, Full, Owner};
use crate::sys::Sys;

impl<S: Sys> Posix<S> {
    /// `F_GETLK`, `F_SETLK`, `F_SETLKW` and the `F_OFD_` forms on `fd`, with
    /// the `struct flock` the caller gave (`F_GETLK` fills it in).
    ///
    /// A wait command is made as the command that does not wait: a conflict
    /// is `EAGAIN`, and the caller sleeps and asks again (the lock is the
    /// library's, a thread that holds one gives it up by running).
    pub fn fcntl_lock(
        &mut self,
        fd: i32,
        command: i32,
        flock: &mut [u8; FLOCK_SIZE],
    ) -> Result<(), i32> {
        let ofd = command >= r::F_OFD_GETLK;
        let get = matches!(command, r::F_GETLK | r::F_OFD_GETLK);
        let kind = r::flock_type(flock);
        if !matches!(kind, r::F_RDLCK | r::F_WRLCK | r::F_UNLCK) {
            return Err(EINVAL);
        }
        // An OFD lock has no owner pid to name; `l_pid` must say so.
        if ofd && r::flock_pid(flock) != 0 {
            return Err(EINVAL);
        }
        // What the lock needs of the descriptor: a read lock a readable one.
        let (conn, access) = match self.fds.get(fd) {
            None => return Err(EBADF),
            Some(&Desc::Object { conn, access, .. }) => (conn, access),
            Some(_) => return Err(EINVAL),
        };
        if !get
            && ((kind == r::F_RDLCK && access == o::WRONLY)
                || (kind == r::F_WRLCK && access == o::RDONLY))
        {
            return Err(EBADF);
        }
        let (_, meta) = self.stat_conn(conn)?;
        let base = match r::flock_whence(flock) {
            0 => 0,
            1 => self.lseek(fd, 0, 1)?,
            2 => i64::try_from(meta.size).map_err(|_| EINVAL)?,
            _ => return Err(EINVAL),
        };
        let (start, end) =
            r::range(base, r::flock_start(flock), r::flock_len(flock)).ok_or(EINVAL)?;
        let file = (meta.volume, meta.id);
        let owner = if ofd {
            Owner::Description(conn)
        } else {
            Owner::Process
        };
        let write = kind == r::F_WRLCK;
        if get {
            match self.locks.conflict(file, owner, write, start, end) {
                Some(c) => {
                    let pid = match c.owner {
                        Owner::Process => self.facts.pid as i32,
                        Owner::Description(_) => -1,
                    };
                    r::put_conflict(flock, c.write, c.start, c.end, pid);
                }
                None => r::put_free(flock),
            }
            return Ok(());
        }
        if kind == r::F_UNLCK {
            self.locks.unlock(file, owner, start, end);
            return Ok(());
        }
        self.locks
            .lock(file, owner, write, start, end)
            .map_err(|e| match e {
                Full::Conflict => EAGAIN,
                Full::Table => ENOLCK,
            })
    }

    /// `close` of `fd` ends every POSIX lock of the process on its file,
    /// whichever fd took them. A stat on the connection finds the file; it is
    /// made only while the process holds a lock.
    pub(crate) fn release_process_locks(&mut self, fd: i32) {
        if self.locks.is_empty() {
            return;
        }
        if let Some(&Desc::Object { conn, .. }) = self.fds.get(fd)
            && let Ok((_, meta)) = self.stat_conn(conn)
        {
            self.locks.release_process((meta.volume, meta.id));
        }
    }
}

impl<S: Sys> Posix<S> {
    /// Whether the process holds no record lock (for tests).
    pub fn locks_is_empty(&self) -> bool {
        self.locks.is_empty()
    }
}
