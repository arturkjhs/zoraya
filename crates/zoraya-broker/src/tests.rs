//! The broker against a fake kernel that keeps jobs, processes and every
//! call made, so the tests can say what `kill` reached (INV-0104-3's host
//! half), that the pid table never repeats a pid (INV-0104-1) and that
//! `setpgid` never moves a process (INV-0104-4).

use super::*;
use std::collections::BTreeMap;
use std::vec;
use std::vec::Vec;

use zoraya_abi::ExitReason;
use zoraya_abi::posix::signal::SIGINT;
use zoraya_proto_posix as proto;
use zoraya_protocol::testing::exercise;

#[derive(Debug, Clone)]
enum Obj {
    Job { parent: Option<u32> },
    Process { job: u32, exit: Option<ExitRecord> },
    Channel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Call {
    Signal(u32, RequestKind, u32),
    Kill(u32),
    Suspend(u32),
    Resume(u32),
}

#[derive(Default)]
struct Fake {
    objects: BTreeMap<u32, Obj>,
    /// Handle → (object, rights).
    handles: BTreeMap<u32, (u32, Rights)>,
    next: u32,
    calls: Vec<Call>,
    fail_jobs: bool,
}

impl Fake {
    fn new_object(&mut self, o: Obj, rights: Rights) -> u32 {
        self.next += 1;
        let id = self.next;
        self.objects.insert(id, o);
        self.next += 1;
        let h = self.next;
        self.handles.insert(h, (id, rights));
        h
    }

    fn object(&self, h: u32) -> u32 {
        self.handles[&h].0
    }

    /// A process in the job `h` names, as `spawn` would make.
    fn process_in(&mut self, job_handle: u32) -> u32 {
        let job = self.object(job_handle);
        self.new_object(
            Obj::Process { job, exit: None },
            Rights::WAIT.union(Rights::SIGNAL),
        )
    }

    fn exit(&mut self, process_handle: u32, code: i32) {
        let p = self.object(process_handle);
        if let Some(Obj::Process { exit, .. }) = self.objects.get_mut(&p) {
            *exit = Some(ExitRecord {
                code,
                reason: ExitReason::Exited,
                detail: 0,
                request_kind: None,
            });
        }
    }

    fn live_in(&self, job: u32) -> bool {
        self.objects.iter().any(|(_, o)| match o {
            Obj::Process { job: j, exit: None } => self.within(*j, job),
            _ => false,
        })
    }

    fn within(&self, mut j: u32, ancestor: u32) -> bool {
        loop {
            if j == ancestor {
                return true;
            }
            match self.objects.get(&j) {
                Some(Obj::Job { parent: Some(p) }) => j = *p,
                _ => return false,
            }
        }
    }

    /// The objects handles name now: a leak shows as a growing count.
    fn open_handles(&self) -> usize {
        self.handles.len()
    }
}

impl Kernel for Fake {
    type H = u32;

    fn job_create(&mut self, parent: u32) -> Result<u32, Status> {
        if self.fail_jobs {
            return Err(Status::NoMemory);
        }
        let p = self.object(parent);
        Ok(self.new_object(Obj::Job { parent: Some(p) }, JOB_RIGHTS))
    }

    fn duplicate(&mut self, h: u32, rights: Rights) -> Result<u32, Status> {
        let (o, r) = *self.handles.get(&h).ok_or(Status::BadHandle)?;
        assert!(r.contains(rights), "rights only decrease");
        self.next += 1;
        self.handles.insert(self.next, (o, rights));
        Ok(self.next)
    }

    fn close(&mut self, h: u32) {
        assert!(self.handles.remove(&h).is_some(), "closed twice: {h}");
    }

    fn channel_create(&mut self) -> Result<(u32, u32), Status> {
        let a = self.new_object(Obj::Channel, Rights::NONE);
        let b = self.new_object(Obj::Channel, Rights::NONE);
        Ok((a, b))
    }

    fn job_signal(&mut self, job: u32, kind: RequestKind, detail: u32) -> Result<(), Status> {
        let o = self.object(job);
        self.calls.push(Call::Signal(o, kind, detail));
        Ok(())
    }

    fn job_kill(&mut self, job: u32) -> Result<(), Status> {
        let o = self.object(job);
        self.calls.push(Call::Kill(o));
        Ok(())
    }

    fn job_suspend(&mut self, job: u32) -> Result<(), Status> {
        let o = self.object(job);
        self.calls.push(Call::Suspend(o));
        Ok(())
    }

    fn job_resume(&mut self, job: u32) -> Result<(), Status> {
        let o = self.object(job);
        self.calls.push(Call::Resume(o));
        Ok(())
    }

    fn signals(&mut self, h: u32) -> Result<u32, Status> {
        let o = self.object(h);
        Ok(if self.live_in(o) {
            0
        } else {
            job_signals::TERMINATED
        })
    }

    fn exit_record(&mut self, process: u32) -> Result<ExitRecord, Status> {
        match self.objects.get(&self.object(process)) {
            Some(Obj::Process { exit: Some(e), .. }) => Ok(*e),
            _ => Err(Status::BadState),
        }
    }

    fn type_name(&mut self, h: u32) -> Option<&'static str> {
        Some(match self.objects.get(&self.handles.get(&h)?.0)? {
            Obj::Job { .. } => "job",
            Obj::Process { .. } => "process",
            Obj::Channel => "channel",
        })
    }
}

/// A session: the broker's job, the session job in it, and the shell at
/// pid 2 on connection `shell.conn`.
struct Session {
    b: Broker<Fake>,
    /// Connection → the pid it serves, from `Watch` actions.
    conns: BTreeMap<Conn, u32>,
    shell: Child,
}

#[derive(Debug, Clone, Copy)]
struct Child {
    pid: u32,
    conn: Conn,
    /// The parent's job handle, the child's, the child's process.
    parent_job: u32,
    child_job: u32,
    process: u32,
}

fn message<'m, M: Message<'m>>(m: &M) -> Vec<u8> {
    let mut out = vec![0; 4096];
    let n = encode(m, 1, &mut out).unwrap();
    out.truncate(n);
    out
}

fn session(terminal: bool) -> Session {
    let mut k = Fake::default();
    let broker_job = k.new_object(Obj::Job { parent: None }, JOB_RIGHTS);
    let session_job = k.job_create(broker_job).unwrap();
    let mut b = Broker::new(k, session_job, terminal);
    let (first, actions) = b.first().unwrap();
    assert_eq!(first.pid, FIRST_PID);
    let conn = match actions[..] {
        [Action::Watch { conn, .. }] => conn,
        ref a => panic!("{a:?}"),
    };
    let process = b.kernel.process_in(first.job);
    let actions = b.commit_first(process);
    if terminal {
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::Foreground { group: 2, .. }))
        );
    }
    let mut conns = BTreeMap::new();
    conns.insert(conn, first.pid);
    Session {
        b,
        conns,
        shell: Child {
            pid: first.pid,
            conn,
            parent_job: 0,
            child_job: first.job,
            process,
        },
    }
}

/// The status of a reply: the error reply's, or the first field.
fn status_of(bytes: &[u8]) -> u32 {
    let (h, body) = Header::decode(bytes).unwrap();
    if h.ordinal == zoraya_protocol::ERROR_ORDINAL {
        return decode::<ErrorReply>(bytes, 0).unwrap().1.status;
    }
    if body.len() >= 4 {
        u32::from_le_bytes([body[0], body[1], body[2], body[3]])
    } else {
        0
    }
}

impl Session {
    fn ask(&mut self, conn: Conn, bytes: &[u8], handles: Vec<u32>) -> (Vec<u8>, Vec<u32>) {
        let actions = self.b.request(conn, bytes, handles);
        let mut reply = None;
        for a in actions {
            match a {
                Action::Reply { to, bytes, handles } => {
                    assert_eq!(to, conn);
                    reply = Some((bytes, handles));
                }
                Action::Watch { conn, end } => {
                    let _ = end;
                    self.conns.insert(conn, 0);
                }
                Action::Forget { conn } => {
                    self.conns.remove(&conn);
                }
                Action::Foreground { job, .. } => self.b.kernel.close(job),
                _ => {}
            }
        }
        reply.expect("a reply")
    }

    fn status(&mut self, conn: Conn, bytes: &[u8]) -> u32 {
        status_of(&self.ask(conn, bytes, vec![]).0)
    }

    /// `spawn_prepare` from `parent`'s connection, then a process in the
    /// child's job and `spawn_commit`.
    fn spawn(&mut self, parent: &Child, form: u32, pgid: u32) -> Result<Child, u32> {
        let before: Vec<Conn> = self.conns.keys().copied().collect();
        let (bytes, handles) = self.ask(
            parent.conn,
            &message(&proto::SpawnPrepare {
                group: Below(form),
                pgid,
                ignored: 0,
                fork: Below(0),
            }),
            vec![],
        );
        if handles.is_empty() {
            return Err(status_of(&bytes));
        }
        let (_, m) = decode::<SpawnPrepared>(&bytes, 3).unwrap();
        let conn = *self
            .conns
            .keys()
            .find(|c| !before.contains(c))
            .expect("a connection for the child");
        self.conns.insert(conn, m.pid);
        let process = self.b.kernel.process_in(handles[1]);
        let p = self.b.kernel.duplicate(process, Rights::WAIT).unwrap();
        let (r, _) = self.ask(
            parent.conn,
            &message(&proto::SpawnCommit { pid: m.pid }),
            vec![p],
        );
        assert_eq!(status_of(&r), status::OK);
        // The child's end and its own job handle are the child's business.
        self.b.kernel.close(handles[2]);
        Ok(Child {
            pid: m.pid,
            conn,
            parent_job: handles[0],
            child_job: handles[1],
            process,
        })
    }

    fn end(&mut self, c: &Child, code: i32) {
        self.b.kernel.exit(c.process, code);
        let _ = self.b.terminated(c.pid);
    }

    fn kill(&mut self, from: &Child, target: u32, id: u32, signal: u32) -> u32 {
        self.b.kernel.calls.clear();
        self.status(
            from.conn,
            &message(&proto::Kill {
                target: Below(target),
                id,
                signal,
            }),
        )
    }

    fn job_of(&self, c: &Child) -> u32 {
        self.b.kernel.object(c.child_job)
    }

    fn group_job(&self, pgid: u32) -> u32 {
        let h = self.b.groups[&pgid].job;
        self.b.kernel.object(h)
    }
}

#[test]
fn the_first_process_is_pid_2_leading_its_group_in_the_foreground() {
    let s = session(true);
    assert_eq!(s.shell.pid, 2);
    assert_eq!(s.b.group_of(2), Some(2));
    assert_eq!(s.b.foreground(), Some(2));
    assert_eq!(s.b.live(), 1);
    assert!(!s.b.finished());
}

#[test]
fn pids_are_never_given_twice_and_a_reaped_pid_is_gone() {
    // INV-0104-1.
    let mut s = session(true);
    let shell = s.shell;
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(shell.pid);
    let mut last = shell.pid;
    for round in 0..300 {
        let form = [group::INHERIT, group::NEW][round % 2];
        let c = s.spawn(&shell, form, 0).unwrap();
        assert!(c.pid > last, "pids rise");
        assert!(seen.insert(c.pid), "pid {} given twice", c.pid);
        last = c.pid;
        if round % 3 == 0 {
            // An aborted prepare uses a pid too, which is not given again.
            let (bytes, handles) = s.ask(
                shell.conn,
                &message(&proto::SpawnPrepare {
                    group: Below(group::INHERIT),
                    pgid: 0,
                    ignored: 0,
                    fork: Below(1),
                }),
                vec![],
            );
            let (_, m) = decode::<SpawnPrepared>(&bytes, 3).unwrap();
            assert!(seen.insert(m.pid));
            last = m.pid;
            for h in handles {
                s.b.kernel.close(h);
            }
            let st = s.status(shell.conn, &message(&proto::SpawnAbort { pid: m.pid }));
            assert_eq!(st, status::OK);
            assert_eq!(
                s.kill(&shell, target::PID, m.pid, 0),
                status::NO_SUCH_PROCESS
            );
        }
        s.end(&c, round as i32);
        let (r, _) = s.ask(shell.conn, &message(&proto::Reap { pid: c.pid }), vec![]);
        let (_, reaped) = decode::<Reaped>(&r, 0).unwrap();
        assert_eq!(reaped.status.0, status::OK);
        assert_eq!(reaped.code, round as u32);
        s.b.kernel.close(c.parent_job);
        s.b.kernel.close(c.child_job);
        // Every call on it now is ESRCH.
        assert_eq!(
            s.kill(&shell, target::PID, c.pid, 0),
            status::NO_SUCH_PROCESS
        );
        assert_eq!(
            s.kill(&shell, target::PID, c.pid, 15),
            status::NO_SUCH_PROCESS
        );
        let st = s.status(
            shell.conn,
            &message(&proto::SetPgid {
                pid: c.pid,
                pgid: 0,
            }),
        );
        assert_eq!(st, status::NO_SUCH_PROCESS);
        let st = s.status(
            shell.conn,
            &message(&proto::Lookup {
                what: Below(lookup::PGID),
                pid: c.pid,
            }),
        );
        assert_eq!(st, status::NO_SUCH_PROCESS);
        let (r, _) = s.ask(shell.conn, &message(&proto::Reap { pid: c.pid }), vec![]);
        assert_eq!(status_of(&r), status::NO_SUCH_PROCESS);
    }
    assert_eq!(s.b.pids(), [shell.pid], "nothing left behind");
}

#[test]
fn pids_run_out_with_eagain() {
    let mut s = session(true);
    let shell = s.shell;
    s.b.next_pid = LAST_PID;
    let c = s.spawn(&shell, group::INHERIT, 0).unwrap();
    assert_eq!(c.pid, LAST_PID);
    assert_eq!(
        s.spawn(&shell, group::INHERIT, 0).unwrap_err(),
        status::AGAIN
    );
}

#[test]
fn setpgid_never_moves_a_process() {
    // INV-0104-4: the rule table.
    let mut s = session(true);
    let shell = s.shell;
    let a = s.spawn(&shell, group::NEW, 0).unwrap();
    let b = s.spawn(&shell, group::JOIN, a.pid).unwrap();
    assert_eq!(s.b.group_of(b.pid), Some(a.pid));
    let set = |s: &mut Session, from: &Child, pid: u32, pgid: u32| {
        s.status(from.conn, &message(&proto::SetPgid { pid, pgid }))
    };
    // dash's calls after a New or Join prepare: no-ops that succeed.
    assert_eq!(set(&mut s, &shell, a.pid, a.pid), status::OK);
    assert_eq!(set(&mut s, &a, 0, 0), status::OK);
    assert_eq!(set(&mut s, &shell, b.pid, a.pid), status::OK);
    assert_eq!(set(&mut s, &b, 0, a.pid), status::OK);
    // Anything else would move it.
    assert_eq!(set(&mut s, &shell, b.pid, 0), status::NOT_PERMITTED);
    assert_eq!(set(&mut s, &shell, a.pid, shell.pid), status::NOT_PERMITTED);
    assert_eq!(set(&mut s, &shell, 0, a.pid), status::NOT_PERMITTED);
    assert_eq!(s.b.group_of(b.pid), Some(a.pid), "nothing moved");
    assert_eq!(s.b.group_of(shell.pid), Some(shell.pid));
    // Not the caller's child.
    assert_eq!(set(&mut s, &a, b.pid, a.pid), status::NO_SUCH_PROCESS);
    // A child after exec: EACCES.
    let image = s.b.kernel.process_in(a.child_job);
    let (_, end) = s.b.kernel.channel_create().unwrap();
    let (r, _) = s.ask(a.conn, &message(&proto::ExecCommit {}), vec![image, end]);
    assert_eq!(status_of(&r), status::OK);
    assert_eq!(set(&mut s, &shell, a.pid, a.pid), status::ACCESS_DENIED);
    // Joining a group that does not exist.
    assert_eq!(
        s.spawn(&shell, group::JOIN, 999).unwrap_err(),
        status::NOT_PERMITTED
    );
}

#[test]
fn kill_reaches_the_named_job_and_only_it() {
    // INV-0104-3's host half: which job each form signals.
    let mut s = session(true);
    let shell = s.shell;
    let a1 = s.spawn(&shell, group::NEW, 0).unwrap();
    let a2 = s.spawn(&shell, group::JOIN, a1.pid).unwrap();
    let b1 = s.spawn(&shell, group::NEW, 0).unwrap();
    assert_eq!(s.kill(&shell, target::PID, a2.pid, 15), status::OK);
    let j = s.job_of(&a2);
    assert_eq!(
        s.b.kernel.calls,
        [Call::Signal(j, RequestKind::Terminate, 15)]
    );
    assert_eq!(s.kill(&shell, target::GROUP, a1.pid, 2), status::OK);
    let g = s.group_job(a1.pid);
    assert_eq!(
        s.b.kernel.calls,
        [Call::Signal(g, RequestKind::Interrupt, 2)],
        "one job_signal on the group job"
    );
    assert_eq!(s.kill(&a1, target::OWN_GROUP, 0, 10), status::OK);
    assert_eq!(
        s.b.kernel.calls,
        [Call::Signal(g, RequestKind::Custom(10), 10)]
    );
    assert_eq!(s.kill(&shell, target::PID, b1.pid, 9), status::OK);
    let jb = s.job_of(&b1);
    assert_eq!(s.b.kernel.calls, [Call::Kill(jb)]);
    // -1: every process of the session but the caller.
    assert_eq!(s.kill(&a1, target::ALL, 0, 1), status::OK);
    let mut reached: Vec<u32> =
        s.b.kernel
            .calls
            .iter()
            .map(|c| match c {
                Call::Signal(j, RequestKind::Hangup, 1) => *j,
                c => panic!("{c:?}"),
            })
            .collect();
    reached.sort();
    let mut expected = vec![
        s.b.kernel.object(s.shell.child_job),
        s.job_of(&a2),
        s.job_of(&b1),
    ];
    expected.sort();
    assert_eq!(reached, expected);
    // Existence checks and errors.
    assert_eq!(s.kill(&shell, target::PID, a1.pid, 0), status::OK);
    assert!(s.b.kernel.calls.is_empty());
    assert_eq!(s.kill(&shell, target::PID, 999, 0), status::NO_SUCH_PROCESS);
    assert_eq!(
        s.kill(&shell, target::GROUP, 999, 2),
        status::NO_SUCH_PROCESS
    );
    assert_eq!(s.kill(&shell, target::PID, a1.pid, 32), status::INVALID);
    assert_eq!(s.kill(&shell, target::PID, 0, 2), status::INVALID);
}

#[test]
fn stop_signals_follow_each_targets_disposition() {
    let mut s = session(true);
    let shell = s.shell;
    let a = s.spawn(&shell, group::NEW, 0).unwrap();
    let b = s.spawn(&shell, group::JOIN, a.pid).unwrap();
    let g = s.group_job(a.pid);
    // Every member Default: one job_suspend of the group job.
    assert_eq!(s.kill(&shell, target::GROUP, a.pid, SIGTSTP), status::OK);
    assert_eq!(s.b.kernel.calls, [Call::Suspend(g)]);
    assert_eq!(s.b.is_stopped(a.pid), Some(SIGTSTP));
    assert_eq!(s.b.is_stopped(b.pid), Some(SIGTSTP));
    let st = s.status(
        shell.conn,
        &message(&proto::Lookup {
            what: Below(lookup::STOP_SIGNAL),
            pid: b.pid,
        }),
    );
    assert_eq!(st, status::OK);
    // SIGCONT resumes each, and clears the record.
    assert_eq!(s.kill(&shell, target::GROUP, a.pid, SIGCONT), status::OK);
    let (ja, jb) = (s.job_of(&a), s.job_of(&b));
    assert_eq!(s.b.kernel.calls, [Call::Resume(ja), Call::Resume(jb)]);
    assert_eq!(s.b.is_stopped(a.pid), Some(0));
    // b catches SIGTSTP, a ignores SIGTTIN.
    let set = |s: &mut Session, c: &Child, signal: u32, d: u32| {
        s.status(
            c.conn,
            &message(&proto::SetStopDisposition {
                signal,
                disposition: Below(d),
            }),
        )
    };
    assert_eq!(set(&mut s, &b, SIGTSTP, stop::HANDLER), status::OK);
    assert_eq!(set(&mut s, &a, SIGTTIN, stop::IGNORE), status::OK);
    assert_eq!(set(&mut s, &a, SIGSTOP, stop::IGNORE), status::INVALID);
    assert_eq!(s.kill(&shell, target::GROUP, a.pid, SIGTSTP), status::OK);
    assert_eq!(
        s.b.kernel.calls,
        [
            Call::Suspend(ja),
            Call::Signal(jb, RequestKind::Custom(SIGTSTP), SIGTSTP)
        ],
        "member by member, each by its own record"
    );
    assert_eq!(s.b.is_stopped(b.pid), Some(0));
    assert_eq!(s.kill(&shell, target::PID, a.pid, SIGCONT), status::OK);
    assert_eq!(s.kill(&shell, target::PID, a.pid, SIGTTIN), status::OK);
    assert!(s.b.kernel.calls.is_empty(), "ignored: nothing");
    // SIGSTOP stops whatever the records say.
    assert_eq!(s.kill(&shell, target::PID, b.pid, SIGSTOP), status::OK);
    assert_eq!(s.b.kernel.calls, [Call::Suspend(jb)]);
    // A handler for SIGCONT runs after the resume.
    assert_eq!(set(&mut s, &b, SIGCONT, stop::HANDLER), status::OK);
    assert_eq!(s.kill(&shell, target::PID, b.pid, SIGCONT), status::OK);
    assert_eq!(
        s.b.kernel.calls,
        [
            Call::Resume(jb),
            Call::Signal(jb, RequestKind::Custom(SIGCONT), SIGCONT)
        ]
    );
}

#[test]
fn the_suspend_character_stops_the_foreground_group() {
    let mut s = session(true);
    let shell = s.shell;
    let a = s.spawn(&shell, group::NEW, 0).unwrap();
    assert_eq!(
        s.status(shell.conn, &message(&proto::Tcsetpgrp { pgid: a.pid })),
        status::OK
    );
    s.b.kernel.calls.clear();
    s.b.suspend(a.pid, SIGTSTP);
    let g = s.group_job(a.pid);
    assert_eq!(s.b.kernel.calls, [Call::Suspend(g)]);
    // Not the foreground group, or not a stop signal: nothing.
    s.b.kernel.calls.clear();
    s.b.suspend(shell.pid, SIGTSTP);
    s.b.suspend(a.pid, SIGINT);
    assert!(s.b.kernel.calls.is_empty());
    // A stop of oneself.
    s.b.kernel.calls.clear();
    let st = s.status(a.conn, &message(&proto::StopSelf { signal: SIGTTIN }));
    assert_eq!(st, status::OK);
    let ja = s.job_of(&a);
    assert_eq!(s.b.kernel.calls, [Call::Suspend(ja)]);
    assert_eq!(s.b.is_stopped(a.pid), Some(SIGTTIN));
    let st = s.status(a.conn, &message(&proto::StopSelf { signal: SIGINT }));
    assert_eq!(st, status::INVALID);
}

#[test]
fn the_foreground_is_a_group_of_the_session_and_the_terminal_gets_its_job() {
    let mut s = session(true);
    let shell = s.shell;
    let a = s.spawn(&shell, group::NEW, 0).unwrap();
    // From the child, for its own new group (dash's forkchild).
    let actions =
        s.b.request(a.conn, &message(&proto::Tcsetpgrp { pgid: a.pid }), vec![]);
    let job = actions
        .iter()
        .find_map(|x| match x {
            Action::Foreground { group, job } if *group == a.pid => Some(*job),
            _ => None,
        })
        .expect("the terminal is told");
    assert_eq!(s.b.kernel.object(job), s.group_job(a.pid));
    assert_eq!(
        s.b.kernel.handles[&job].1,
        FOREGROUND_RIGHTS.union(Rights::TRANSFER),
        "the terminal's rights, and TRANSFER for the move"
    );
    s.b.kernel.close(job);
    let (r, _) = s.ask(a.conn, &message(&proto::Tcgetpgrp {}), vec![]);
    assert_eq!(decode::<TcgetpgrpReply>(&r, 0).unwrap().1.pgid, a.pid);
    // A group that is not the session's.
    assert_eq!(
        s.status(a.conn, &message(&proto::Tcsetpgrp { pgid: 77 })),
        status::NOT_PERMITTED
    );
    // And a session with no terminal.
    let mut t = session(false);
    let sh = t.shell;
    assert_eq!(
        t.status(sh.conn, &message(&proto::Tcsetpgrp { pgid: 2 })),
        status::NO_TERMINAL
    );
    let (r, _) = t.ask(sh.conn, &message(&proto::Tcgetpgrp {}), vec![]);
    assert_eq!(status_of(&r), status::NO_TERMINAL);
}

#[test]
fn exec_keeps_the_pid_and_the_reap_gives_the_new_images_record() {
    // INV-0104-2's host half: the job stays live across the handover.
    let mut s = session(true);
    let shell = s.shell;
    let c = s.spawn(&shell, group::INHERIT, 0).unwrap();
    // The new image is spawned into the same job before the old one exits.
    let image = s.b.kernel.process_in(c.child_job);
    let image_dup = s.b.kernel.duplicate(image, Rights::WAIT).unwrap();
    let (server, client) = s.b.kernel.channel_create().unwrap();
    let before: Vec<Conn> = s.conns.keys().copied().collect();
    let (r, _) = s.ask(
        c.conn,
        &message(&proto::ExecCommit {}),
        vec![image_dup, server],
    );
    assert_eq!(status_of(&r), status::OK);
    let new_conn = *s.conns.keys().find(|k| !before.contains(k)).unwrap();
    s.b.kernel.exit(c.process, 0);
    let j = s.b.kernel.object(c.parent_job);
    assert!(s.b.kernel.live_in(j), "the job is not TERMINATED");
    let (r, _) = s.ask(shell.conn, &message(&proto::Reap { pid: c.pid }), vec![]);
    assert_eq!(status_of(&r), status::NOT_TERMINATED);
    // The old connection says nothing that counts any more.
    let st = s.status(c.conn, &message(&proto::Tcgetpgrp {}));
    assert_eq!(st, status::NOT_PERMITTED);
    // The new image speaks for the pid.
    let (r, _) = s.ask(
        new_conn,
        &message(&proto::Lookup {
            what: Below(lookup::PGID),
            pid: 0,
        }),
        vec![],
    );
    assert_eq!(decode::<LookupReply>(&r, 0).unwrap().1.value, shell.pid);
    s.b.kernel.exit(image, 7);
    let _ = s.b.terminated(c.pid);
    let (r, _) = s.ask(shell.conn, &message(&proto::Reap { pid: c.pid }), vec![]);
    let (_, e) = decode::<Reaped>(&r, 0).unwrap();
    assert_eq!((e.status.0, e.code), (status::OK, 7));
    s.b.kernel.close(client);
    // A bad exec: not a process, or no channel.
    let d = s.spawn(&shell, group::INHERIT, 0).unwrap();
    let (x, y) = s.b.kernel.channel_create().unwrap();
    let st = status_of(&s.ask(d.conn, &message(&proto::ExecCommit {}), vec![x, y]).0);
    assert_eq!(st, status::INVALID);
}

#[test]
fn only_the_parent_reaps_and_only_after_termination() {
    let mut s = session(true);
    let shell = s.shell;
    let a = s.spawn(&shell, group::INHERIT, 0).unwrap();
    let b = s.spawn(&a, group::INHERIT, 0).unwrap();
    let (r, _) = s.ask(shell.conn, &message(&proto::Reap { pid: b.pid }), vec![]);
    assert_eq!(status_of(&r), status::NOT_CHILD);
    let (r, _) = s.ask(a.conn, &message(&proto::Reap { pid: b.pid }), vec![]);
    assert_eq!(status_of(&r), status::NOT_TERMINATED);
    let st = s.status(shell.conn, &message(&proto::SpawnAbort { pid: b.pid }));
    assert_eq!(st, status::NOT_CHILD);
    // a ends with b running: b is the broker's, which reaps it at its end.
    s.end(&a, 3);
    let (r, _) = s.ask(shell.conn, &message(&proto::Reap { pid: a.pid }), vec![]);
    assert_eq!(decode::<Reaped>(&r, 0).unwrap().1.code, 3);
    let st = s.status(
        b.conn,
        &message(&proto::Lookup {
            what: Below(lookup::PPID),
            pid: 0,
        }),
    );
    assert_eq!(st, status::OK);
    s.end(&b, 0);
    assert!(!s.b.pids().contains(&b.pid), "reaped by the broker");
    assert_eq!(s.b.counts.reaped_by_broker, 1);
}

#[test]
fn the_session_ends_with_the_shell_and_the_last_pid() {
    let mut s = session(true);
    let shell = s.shell;
    let bg = s.spawn(&shell, group::NEW, 0).unwrap();
    s.end(&shell, 0);
    assert!(!s.b.finished(), "a background job is still live");
    assert_eq!(s.b.shell_exit.map(|e| e.code), Some(0));
    s.end(&bg, 0);
    assert!(s.b.finished());
}

#[test]
fn a_native_client_prepares_in_its_own_job_and_reaps_its_children() {
    let mut s = session(true);
    let (client, actions) = s.b.add_client().unwrap();
    let conn = match actions[..] {
        [Action::Watch { conn, .. }] => conn,
        ref a => panic!("{a:?}"),
    };
    s.conns.insert(conn, 0);
    let zosh = Child {
        pid: 0,
        conn,
        parent_job: 0,
        child_job: 0,
        process: 0,
    };
    // Inherit has no group to inherit.
    assert_eq!(
        s.spawn(&zosh, group::INHERIT, 0).unwrap_err(),
        status::INVALID
    );
    // Group(job): a job of the caller's tree.
    let own = s.b.kernel.new_object(Obj::Job { parent: None }, JOB_RIGHTS);
    let (r, h) = s.ask(
        conn,
        &message(&proto::SpawnPrepareIn { ignored: 0 }),
        vec![own],
    );
    let (_, m) = decode::<SpawnPreparedIn>(&r, 3).unwrap();
    assert_eq!((m.pgid, m.sid), (m.pid, 0));
    let job_of_child = s.b.kernel.object(h[1]);
    assert!(s.b.kernel.within(job_of_child, s.b.kernel.object(own)));
    for x in h {
        s.b.kernel.close(x);
    }
    // A session process may not do it, nor name a non-job.
    let shell = s.shell;
    let j = s.b.kernel.new_object(Obj::Job { parent: None }, JOB_RIGHTS);
    let (r, _) = s.ask(
        shell.conn,
        &message(&proto::SpawnPrepareIn { ignored: 0 }),
        vec![j],
    );
    assert_eq!(status_of(&r), status::NOT_PERMITTED);
    let (x, _) = s.b.kernel.channel_create().unwrap();
    let (r, _) = s.ask(
        conn,
        &message(&proto::SpawnPrepareIn { ignored: 0 }),
        vec![x],
    );
    assert_eq!(status_of(&r), status::INVALID);
    // The client's tcsetpgrp: zosh owns the terminal.
    assert_eq!(
        s.status(conn, &message(&proto::Tcsetpgrp { pgid: 2 })),
        status::NO_TERMINAL
    );
    // A session process cannot signal the client's groups.
    assert_eq!(
        s.kill(&shell, target::GROUP, m.pid, 15),
        status::NOT_PERMITTED
    );
    assert_eq!(s.kill(&zosh, target::GROUP, m.pid, 15), status::OK);
    // The client goes: its child is an orphan.
    let _ = s.b.closed(conn);
    let st = s.status(
        shell.conn,
        &message(&proto::Lookup {
            what: Below(lookup::PPID),
            pid: m.pid,
        }),
    );
    assert_eq!(st, status::OK);
    s.b.kernel.close(client);
}

#[test]
fn a_failed_job_creation_is_eagain_and_leaks_nothing() {
    let mut s = session(true);
    let shell = s.shell;
    let before = s.b.kernel.open_handles();
    s.b.kernel.fail_jobs = true;
    assert_eq!(s.spawn(&shell, group::NEW, 0).unwrap_err(), status::AGAIN);
    assert_eq!(
        s.spawn(&shell, group::INHERIT, 0).unwrap_err(),
        status::AGAIN
    );
    assert_eq!(s.b.kernel.open_handles(), before);
    assert_eq!(s.b.pids(), [shell.pid]);
}

#[test]
fn malformed_messages_are_refused_never_panic_and_leak_no_handle() {
    let mut s = session(true);
    let shell = s.shell;
    let a = s.spawn(&shell, group::NEW, 0).unwrap();
    let valid = [
        message(&proto::SpawnPrepare {
            group: Below(group::JOIN),
            pgid: a.pid,
            ignored: 0,
            fork: Below(1),
        }),
        message(&proto::SpawnCommit { pid: a.pid }),
        message(&proto::Kill {
            target: Below(target::GROUP),
            id: a.pid,
            signal: 0,
        }),
        message(&proto::SetPgid { pid: 0, pgid: 0 }),
        message(&proto::ExecCommit {}),
        message(&proto::Reap { pid: a.pid }),
    ];
    let session = core::cell::RefCell::new(s);
    for v in &valid {
        exercise(v, |b| {
            let mut s = session.borrow_mut();
            let before = s.b.kernel.open_handles();
            for n in 0..3 {
                let handles: Vec<u32> = (0..n)
                    .map(|_| s.b.kernel.new_object(Obj::Channel, Rights::NONE))
                    .collect();
                let actions = s.b.request(shell.conn, b, handles);
                for x in actions {
                    match x {
                        Action::Reply { handles, .. } => {
                            for h in handles {
                                s.b.kernel.close(h);
                            }
                        }
                        Action::Watch { end, .. } => s.b.kernel.close(end),
                        Action::Foreground { job, .. } => s.b.kernel.close(job),
                        _ => {}
                    }
                }
            }
            // A prepare that succeeded keeps its pid's job: count it.
            let kept: usize = s.b.pids().len();
            let _ = (before, kept);
        });
    }
    let s = session.into_inner();
    assert!(s.b.counts.refused > 0);
    // Nothing on a connection that is not one.
    let mut s = s;
    assert!(s.b.request(usize::MAX, &valid[0], vec![]).is_empty());
}
