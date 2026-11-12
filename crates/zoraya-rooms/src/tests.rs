use super::*;
use alloc::collections::BTreeMap;
use std::vec;
use zoraya_proto_rooms::alloc_free::RoomName;
use zoraya_proto_rooms::{Leave as LeaveMsg, Set as SetMsg};
use zoraya_settings::rooms::{self, Machine};

/// A model of the kernel's jobs: a tree, limits, a count of processes the
/// test sets, and a refused counter.
#[derive(Default)]
struct Model {
    jobs: BTreeMap<JobId, ModelJob>,
    next: JobId,
    table: String,
    now: u64,
    session_mem: u64,
    refuse_create: bool,
}

#[derive(Default, Clone)]
struct ModelJob {
    parent: Option<JobId>,
    limits: JobLimits,
    procs: u64,
    refused_mem: u64,
    refused_procs: u64,
    killed: bool,
    closed: bool,
    has_limit_right: bool,
}

impl Env for Model {
    fn create_job(&mut self, parent: Parent, limits: &JobLimits) -> Result<JobId, Status> {
        if self.refuse_create {
            return Err(Status::NoMemory);
        }
        self.next += 1;
        let parent = match parent {
            Parent::Session => 0,
            Parent::Job(j) => j,
        };
        self.jobs.insert(
            self.next,
            ModelJob {
                parent: Some(parent),
                limits: *limits,
                has_limit_right: true,
                ..ModelJob::default()
            },
        );
        Ok(self.next)
    }
    fn launch_job(&mut self, room: JobId) -> Result<JobId, Status> {
        self.next += 1;
        self.jobs.insert(
            self.next,
            ModelJob {
                parent: Some(room),
                has_limit_right: false,
                ..ModelJob::default()
            },
        );
        Ok(self.next)
    }
    fn set_limits(&mut self, job: JobId, limits: &JobLimits) -> Result<(), Status> {
        let j = self.jobs.get_mut(&job).ok_or(Status::BadHandle)?;
        if !j.has_limit_right {
            return Err(Status::AccessDenied);
        }
        j.limits = *limits;
        Ok(())
    }
    fn usage(&mut self, job: JobId, clear: bool) -> Option<JobUsage> {
        let j = self.jobs.get_mut(&job)?;
        let mut u = JobUsage::default();
        u.memory.limit = j.limits.memory;
        u.processes.limit = j.limits.processes.map(u64::from);
        u.processes.used = j.procs;
        u.memory.refused = j.refused_mem;
        u.processes.refused = j.refused_procs;
        u.cpu.limit = j.limits.cpu;
        let _ = clear;
        Some(u)
    }
    fn kill(&mut self, job: JobId) {
        if let Some(j) = self.jobs.get_mut(&job) {
            j.killed = true;
            j.procs = 0;
        }
    }
    fn close(&mut self, job: JobId) {
        if let Some(j) = self.jobs.get_mut(&job) {
            j.closed = true;
        }
    }
    fn now_ms(&mut self) -> u64 {
        self.now
    }
    fn room_line(&mut self, name: &str) -> Result<Option<Room>, TableError> {
        if self.table == "BROKEN" {
            return Err(TableError);
        }
        Ok(rooms::read(self.table.as_bytes()).get(name).cloned())
    }
    fn machine(&mut self) -> Machine {
        Machine {
            session_mem: self.session_mem,
            session_procs: 200,
            free_disk: 1 << 40,
        }
    }
}

const TABLE: &str = "project --colour 1 +w ~/project\n\
build --colour 2 --cpu 50% --mem 64M --disk 2G +w ~/build\n\
agent-1 --untrusted +w ~/sandbox/agent-1\n\
agent-2 --untrusted --mem 16M +w ~/sandbox/agent-2\n\
agent-3 --untrusted +w ~/sandbox/agent-3\n\
agent-4 --untrusted +w ~/sandbox/agent-4\n\
agent-5 --untrusted +w ~/sandbox/agent-5\n\
big --untrusted --mem 400M +w ~/sandbox/big\n";

fn keeper() -> Keeper<Model> {
    // Job 0 is the session, job 1 the untrusted job the keeper makes.
    let mut model = Model {
        table: TABLE.into(),
        session_mem: 356 << 20,
        ..Model::default()
    };
    model.jobs.insert(0, ModelJob::default());
    Keeper::new(model, None)
}

const DESKTOP: ChannelKind = ChannelKind::Manage(Launcher::Desktop);
const CONSOLE: ChannelKind = ChannelKind::Manage(Launcher::Console);

fn msg<'a, M: zoraya_protocol::Message<'a>>(m: &M) -> std::vec::Vec<u8> {
    let mut b = vec![0u8; 512];
    let n = encode(m, 5, &mut b).unwrap();
    b.truncate(n);
    b
}

fn ask(
    k: &mut Keeper<Model>,
    chan: ChanId,
    kind: &ChannelKind,
    bytes: &[u8],
) -> (Reply, std::vec::Vec<u8>) {
    let mut out = vec![0u8; OUT_SIZE];
    let r = k.handle(chan, kind, bytes, 0, &mut out);
    (r, out)
}

fn launch(
    k: &mut Keeper<Model>,
    chan: ChanId,
    kind: &ChannelKind,
    room: &str,
) -> Result<JobId, (u32, String)> {
    let (r, out) = ask(k, chan, kind, &msg(&Launch { room: Text(room) }));
    match r {
        Reply::SendJob(n, job) => {
            assert!(decode::<Launched>(&out[..n], 1).is_ok());
            Ok(job)
        }
        Reply::Send(n) => {
            let (_, d) = decode::<Denied<'_>>(&out[..n], 0).unwrap();
            Err((d.status.0, d.text.0.into()))
        }
        Reply::Close | Reply::Open(..) => Err((99, "closed".into())),
    }
}

fn simple(
    k: &mut Keeper<Model>,
    chan: ChanId,
    kind: &ChannelKind,
    bytes: &[u8],
) -> Result<(), (u32, String)> {
    let (r, out) = ask(k, chan, kind, bytes);
    match r {
        Reply::Send(n) => match decode::<Done>(&out[..n], 0) {
            Ok(_) => Ok(()),
            Err(_) => {
                let (_, d) = decode::<Denied<'_>>(&out[..n], 0).unwrap();
                Err((d.status.0, d.text.0.into()))
            }
        },
        _ => Err((99, "closed".into())),
    }
}

#[test]
fn the_keeper_makes_the_untrusted_job_with_adr_0266_ceilings_and_tells_init() {
    let mut k = keeper();
    let u = k.env().jobs.get(&1).unwrap().clone();
    assert_eq!(u.limits.memory, Some(178 << 20));
    assert_eq!(u.limits.processes, Some(96));
    assert_eq!(u.limits.cpu, Some(500));
    assert_eq!(
        k.take_events(),
        vec![Event::Created {
            room: String::new(),
            job: 1,
            untrusted: true,
            launcher: None,
        }]
    );
    // A keeper restarted with init's duplicate makes none.
    let mut model = Model {
        table: TABLE.into(),
        session_mem: 356 << 20,
        ..Model::default()
    };
    model.jobs.insert(7, ModelJob::default());
    let mut k = Keeper::new(model, Some(7));
    assert!(k.take_events().is_empty());
    assert_eq!(k.env().jobs.len(), 1);
}

#[test]
fn one_job_per_room_whoever_launches() {
    let mut k = keeper();
    let a = launch(&mut k, 1, &DESKTOP, "project").unwrap();
    let b = launch(&mut k, 2, &CONSOLE, "project").unwrap();
    assert_ne!(a, b, "two launch jobs");
    // Both are sub-jobs of the one room job, and neither carries LIMIT.
    let ja = k.env().jobs[&a].clone();
    let jb = k.env().jobs[&b].clone();
    assert_eq!(ja.parent, jb.parent);
    assert!(!ja.has_limit_right && !jb.has_limit_right);
    let room_job = ja.parent.unwrap();
    assert_eq!(k.job_of("project"), Some(room_job));
    assert_eq!(k.running(), vec!["project".to_string()]);
    // The room job is under the session, with the (empty) limits of the line.
    assert_eq!(k.env().jobs[&room_job].parent, Some(0));
    assert_eq!(k.env().jobs[&room_job].limits, JobLimits::default());
}

#[test]
fn a_rooms_limits_come_from_its_line() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "build").unwrap();
    let j = k.job_of("build").unwrap();
    let l = k.env().jobs[&j].limits;
    assert_eq!(
        (l.memory, l.cpu, l.processes),
        (Some(64 << 20), Some(500), None)
    );
}

#[test]
fn an_untrusted_room_has_all_its_defaults_under_the_untrusted_job() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    let j = k.job_of("agent-1").unwrap();
    let job = k.env().jobs[&j].clone();
    assert_eq!(job.parent, Some(1));
    // A quarter of the session's 356 MiB, 25%, 32 processes (INV-0266-1).
    assert_eq!(job.limits.memory, Some(89 << 20));
    assert_eq!(job.limits.cpu, Some(250));
    assert_eq!(job.limits.processes, Some(32));
    let (r, out) = ask(
        &mut k,
        1,
        &DESKTOP,
        &msg(&Usage {
            room: Text("agent-1"),
        }),
    );
    let Reply::Send(n) = r else { panic!() };
    let (_, list) = decode::<UsageList<'_>>(&out[..n], 0).unwrap();
    assert_eq!(list.count.0, 1);
    let rec = UsageRecord::decode(list.records.0).unwrap();
    assert_eq!(rec.name(), "agent-1");
    assert_eq!(rec.flags & flags::UNTRUSTED, flags::UNTRUSTED);
    // All six words, the same as the line fills (INV-0266-1, INV-0266-3).
    assert_eq!(
        record_limits(&rec).words(),
        "--mem 89M --cpu 25% --procs 32 --disk 1G --conns 16 --tmp 16M"
    );
    assert!(record_limits(&rec).is_complete());
}

#[test]
fn a_limit_above_the_untrusted_job_is_refused_and_nothing_is_made() {
    let mut k = keeper();
    let before = k.env().jobs.len();
    assert_eq!(
        launch(&mut k, 1, &DESKTOP, "big").unwrap_err().0,
        status::BAD_LINE
    );
    assert_eq!(k.env().jobs.len(), before);
    assert!(k.running().is_empty());
    assert!(launch(&mut k, 1, &DESKTOP, "nobody").unwrap_err().0 == status::NO_SUCH_ROOM);
    k.env().table = "BROKEN".into();
    assert_eq!(
        launch(&mut k, 1, &DESKTOP, "project").unwrap_err().0,
        status::NO_TABLE
    );
}

#[test]
fn an_untrusted_room_is_launched_into_by_one_manage_holder_at_a_time() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    let busy = launch(&mut k, 2, &CONSOLE, "agent-1").unwrap_err();
    assert_eq!(busy, (status::BUSY, "running from the desktop".to_string()));
    // The same holder may launch again.
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    // The panel is done with its spawn (Leave) but the room has a process.
    let room = k.job_of("agent-1").unwrap();
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    k.env().jobs.get_mut(&room).unwrap().procs = 1;
    assert_eq!(
        launch(&mut k, 2, &CONSOLE, "agent-1").unwrap_err(),
        (status::BUSY, "running from the desktop".to_string())
    );
    // The processes end: the console may enter.
    k.env().jobs.get_mut(&room).unwrap().procs = 0;
    launch(&mut k, 2, &CONSOLE, "agent-1").unwrap();
    assert_eq!(k.running().len(), 1, "still one job for the room");
    assert_eq!(k.job_of("agent-1"), Some(room));
    // And now the desktop is refused, in words naming the console.
    assert_eq!(
        launch(&mut k, 1, &DESKTOP, "agent-1").unwrap_err(),
        (status::BUSY, "running from the console".to_string())
    );
    // A channel that closes gives up its lease.
    k.channel_closed(2, &CONSOLE);
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
}

#[test]
fn a_trusted_room_is_shared_by_both_launchers() {
    let mut k = keeper();
    let a = launch(&mut k, 1, &DESKTOP, "project").unwrap();
    let pj = k.job_of("project").unwrap();
    k.env().jobs.get_mut(&pj).unwrap().procs = 3;
    let b = launch(&mut k, 2, &CONSOLE, "project").unwrap();
    assert_ne!(a, b);
    assert_eq!(k.running().len(), 1);
}

#[test]
fn four_untrusted_rooms_at_most() {
    let mut k = keeper();
    for (i, room) in ["agent-1", "agent-2", "agent-3", "agent-4"]
        .iter()
        .enumerate()
    {
        launch(&mut k, 10 + i, &DESKTOP, room).unwrap();
    }
    assert_eq!(k.untrusted_running(), 4);
    assert_eq!(
        launch(&mut k, 20, &DESKTOP, "agent-5").unwrap_err(),
        (
            status::TOO_MANY,
            "four untrusted rooms are running".to_string()
        )
    );
    // A trusted room is not counted.
    launch(&mut k, 20, &DESKTOP, "project").unwrap();
    // A running one is still reachable by its holder.
    launch(&mut k, 10, &DESKTOP, "agent-1").unwrap();
    // One ends and is collected: the fifth may run.
    simple(
        &mut k,
        10,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    k.env().now += IDLE_MS;
    launch(&mut k, 20, &DESKTOP, "agent-5").unwrap();
    assert_eq!(k.untrusted_running(), 4);
    assert!(k.running().iter().all(|r| r != "agent-1"));
    let events = k.take_events();
    assert!(events.contains(&Event::Gone {
        room: "agent-1".into()
    }));
}

#[test]
fn no_room_is_collected_while_it_has_processes_or_a_lease() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    k.env().now += 10 * IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "leased");
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    let room = k.job_of("agent-1").unwrap();
    k.env().jobs.get_mut(&room).unwrap().procs = 2;
    k.env().now += 10 * IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "live processes");
    k.env().jobs.get_mut(&room).unwrap().procs = 0;
    k.collect_idle();
    assert!(k.running().is_empty(), "no process, no lease, long idle");
    assert!(k.env().jobs[&room].closed);
    // A launcher that has just left keeps the room for IDLE_MS: it may be
    // between a Launch and its spawn.
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    k.collect_idle();
    assert_eq!(k.running().len(), 1);
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert!(k.running().is_empty());
}

#[test]
fn a_lease_is_per_channel_and_one_launcher_leaving_keeps_the_others() {
    // Finding 1: the console enters `project`, the desktop launches into it
    // and leaves: the console still holds its launch job.
    let mut k = keeper();
    launch(&mut k, 2, &CONSOLE, "project").unwrap();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("project"),
        }),
    )
    .unwrap();
    k.env().now += 10 * IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "the console still holds a lease");
    // A closed channel removes only its own lease.
    k.channel_closed(1, &DESKTOP);
    k.collect_idle();
    assert_eq!(k.running().len(), 1);
    k.channel_closed(2, &CONSOLE);
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert!(k.running().is_empty(), "no lease left");
}

#[test]
fn which_messages_each_channel_kind_may_send() {
    // INV-0269-2: a Room(R) channel is refused every message but
    // Launch { R } and Usage { R }; a Usage channel only reads.
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    launch(&mut k, 1, &DESKTOP, "build").unwrap();
    let room = |n: &str| ChannelKind::Room(RoomName::new(n).unwrap());
    let project = room("project");
    let name = |n| Text::<24>(n);
    let all: [(&str, std::vec::Vec<u8>); 7] = [
        (
            "launch",
            msg(&Launch {
                room: name("project"),
            }),
        ),
        (
            "leave",
            msg(&LeaveMsg {
                room: name("project"),
            }),
        ),
        (
            "set",
            msg(&SetMsg {
                room: name("project"),
            }),
        ),
        (
            "usage",
            msg(&Usage {
                room: name("project"),
            }),
        ),
        (
            "report",
            msg(&Report {
                service: Below(0),
                used: 1,
                limit: 2,
                refused: 0,
                room: name("project"),
            }),
        ),
        (
            "stop",
            msg(&Stop {
                room: name("project"),
            }),
        ),
        (
            "open",
            msg(&zoraya_proto_rooms::Open {
                kind: Below(0),
                room: name("project"),
            }),
        ),
    ];
    for (what, bytes) in &all {
        let (r, out) = ask(&mut k, 3, &project, bytes);
        let ok = matches!(*what, "launch" | "usage");
        match r {
            Reply::SendJob(..) => assert!(ok && *what == "launch"),
            Reply::Send(n) if ok => {
                assert!(decode::<UsageList<'_>>(&out[..n], 0).is_ok(), "{what}");
            }
            Reply::Send(n) => {
                let (_, d) = decode::<Denied<'_>>(&out[..n], 0).unwrap();
                assert_eq!(d.status.0, status::NOT_ALLOWED, "{what}");
            }
            Reply::Close | Reply::Open(..) => panic!("{what}"),
        }
    }
    assert_eq!(k.running().len(), 2, "stop was not done");
    // Another room's name on a Room(project) channel.
    let (r, out) = ask(
        &mut k,
        3,
        &project,
        &msg(&Launch {
            room: name("build"),
        }),
    );
    let Reply::Send(n) = r else { panic!() };
    assert_eq!(
        decode::<Denied<'_>>(&out[..n], 0).unwrap().1.status.0,
        status::NOT_ALLOWED
    );
    let (r, _) = ask(&mut k, 3, &project, &msg(&Usage { room: name("") }));
    assert!(matches!(r, Reply::Send(_)));
    // The empty name is every room: refused on a Room channel.
    let (r, out) = ask(&mut k, 3, &project, &msg(&Usage { room: name("") }));
    let Reply::Send(n) = r else { panic!() };
    assert_eq!(
        decode::<Denied<'_>>(&out[..n], 0).unwrap().1.status.0,
        status::NOT_ALLOWED
    );
    // A Usage channel reads any room and does nothing else.
    for (what, bytes) in &all {
        let (r, out) = ask(&mut k, 4, &ChannelKind::Usage, bytes);
        let Reply::Send(n) = r else { panic!("{what}") };
        if *what == "usage" {
            assert!(decode::<UsageList<'_>>(&out[..n], 0).is_ok());
        } else {
            assert_eq!(
                decode::<Denied<'_>>(&out[..n], 0).unwrap().1.status.0,
                status::NOT_ALLOWED,
                "{what}"
            );
        }
    }
    assert_eq!(k.running().len(), 2);
    // Replies, init's messages and unknown ordinals close the channel.
    for ord in [0u32, 16, 64, 80, 96, 97, 98, 1000] {
        let mut b = msg(&Done {});
        b[4..8].copy_from_slice(&ord.to_le_bytes());
        for kind in [&DESKTOP, &project, &ChannelKind::Usage] {
            let (r, _) = ask(&mut k, 3, kind, &b);
            assert_eq!(r, Reply::Close, "ordinal {ord}");
        }
    }
    // An unused request ordinal is denied, not served.
    for ord in [8u32, 15] {
        let mut b = msg(&Done {});
        b[4..8].copy_from_slice(&ord.to_le_bytes());
        let (r, out) = ask(&mut k, 3, &DESKTOP, &b);
        let Reply::Send(n) = r else { panic!() };
        assert_eq!(
            decode::<Denied<'_>>(&out[..n], 0).unwrap().1.status.0,
            status::NOT_ALLOWED
        );
    }
    let (r, _) = ask(&mut k, 3, &DESKTOP, &[1, 2, 3]);
    assert_eq!(r, Reply::Close);
}

#[test]
fn no_launch_job_carries_limit() {
    let mut k = keeper();
    for (i, room) in ["project", "build", "agent-1"].iter().enumerate() {
        let j = launch(&mut k, i, &DESKTOP, room).unwrap();
        assert!(!k.env().jobs[&j].has_limit_right, "{room}");
        assert!(k.env().set_limits(j, &JobLimits::default()).is_err());
    }
}

#[test]
fn set_applies_the_new_line_and_refuses_what_is_above_its_parent() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    let j = k.job_of("agent-1").unwrap();
    k.env().table = TABLE.replace(
        "agent-1 --untrusted",
        "agent-1 --untrusted --mem 128M --cpu 10%",
    );
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&SetMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    let l = k.env().jobs[&j].limits;
    assert_eq!(
        (l.memory, l.cpu, l.processes),
        (Some(128 << 20), Some(100), Some(32))
    );
    // 512M is above the untrusted rooms' 178M.
    k.env().table = TABLE.replace("agent-1 --untrusted", "agent-1 --untrusted --mem 512M");
    let e = simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&SetMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap_err();
    assert_eq!(e.0, status::BAD_LINE);
    assert!(e.1.contains("above the untrusted rooms' 178M"), "{e:?}");
    assert_eq!(k.env().jobs[&j].limits.memory, Some(128 << 20), "unchanged");
    // A room that does not run: nothing to apply, but the ceilings are
    // checked all the same (ADR-0293 Option A).
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&SetMsg {
            room: Text("project"),
        }),
    )
    .unwrap();
    k.env().table = TABLE.replace("agent-3 --untrusted", "agent-3 --untrusted --mem 512M");
    let e = simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&SetMsg {
            room: Text("agent-3"),
        }),
    )
    .unwrap_err();
    assert_eq!(e.0, status::BAD_LINE, "{e:?}");
}

#[test]
fn stop_kills_the_room_and_a_second_stop_finds_nothing() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    let j = k.job_of("agent-1").unwrap();
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&Stop {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    assert!(k.env().jobs[&j].killed);
    assert_eq!(
        simple(
            &mut k,
            1,
            &DESKTOP,
            &msg(&Stop {
                room: Text("agent-1")
            })
        )
        .unwrap_err()
        .0,
        status::NOT_RUNNING
    );
    // The room can be started again, with a new job.
    launch(&mut k, 2, &CONSOLE, "agent-1").unwrap();
    assert_ne!(k.job_of("agent-1"), Some(j));
}

#[test]
fn a_restarted_keeper_stops_a_room_it_did_not_start() {
    // INV-0269-3, the keeper's half: init hands the jobs back with `resume`.
    let mut first = keeper();
    launch(&mut first, 1, &DESKTOP, "agent-1").unwrap();
    let j = first.job_of("agent-1").unwrap();
    let model = core::mem::take(first.env());
    let mut second = Keeper::new(model, Some(1));
    assert!(second.running().is_empty());
    second.resume("agent-1", j, true, Some(Launcher::Desktop));
    assert_eq!(second.untrusted_running(), 1);
    simple(
        &mut second,
        9,
        &CONSOLE,
        &msg(&Stop {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    assert!(second.env().jobs[&j].killed);
}

fn restarted(holder: Option<Launcher>, procs: u64) -> (Keeper<Model>, JobId) {
    let mut first = keeper();
    launch(&mut first, 1, &DESKTOP, "agent-1").unwrap();
    let j = first.job_of("agent-1").unwrap();
    first.env().jobs.get_mut(&j).unwrap().procs = procs;
    let model = core::mem::take(first.env());
    let mut second = Keeper::new(model, Some(1));
    second.resume("agent-1", j, true, holder);
    (second, j)
}

#[test]
fn a_resumed_room_keeps_its_holders_kind_and_is_leased_until_it_leaves() {
    // Findings 2 and 3. The holder is known by kind, not by channel slot, and
    // slot numbers are reused after a restart.
    let (mut k, j) = restarted(Some(Launcher::Desktop), 1);
    // The console, even on the slot the desktop had, is refused.
    assert_eq!(
        launch(&mut k, 1, &CONSOLE, "agent-1").unwrap_err(),
        (status::BUSY, "running from the desktop".to_string())
    );
    // The leased room is not let go while its holder may come back.
    k.env().jobs.get_mut(&j).unwrap().procs = 0;
    k.env().now += 2 * IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "leased by its old holder");
    assert_eq!(
        launch(&mut k, 2, &CONSOLE, "agent-1").unwrap_err().0,
        status::BUSY,
        "still leased"
    );
    // The holder (a new channel) leaves: the lease ends.
    simple(
        &mut k,
        3,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert!(k.running().is_empty());
    // Or its channel closes.
    let (mut k, _) = restarted(Some(Launcher::Console), 0);
    k.channel_closed(4, &CONSOLE);
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert!(k.running().is_empty());
    // Or it never comes back: no time limit ends the lease (round 3,
    // finding 1: its client reconnects only at its next call, which may be
    // long after the restart). Stop, Leave, a launch or the closing of a
    // `Manage` channel of its kind end it.
    let (mut k, _) = restarted(Some(Launcher::Console), 0);
    k.env().now += 100 * IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "the lease does not lapse on time");
    // The same kind of holder may launch again, and then holds a lease.
    let (mut k, _) = restarted(Some(Launcher::Desktop), 2);
    launch(&mut k, 5, &DESKTOP, "agent-1").unwrap();
}

#[test]
fn a_resumed_room_with_processes_and_no_known_holder_is_busy() {
    let (mut k, j) = restarted(None, 2);
    for (chan, kind) in [(1, &DESKTOP), (2, &CONSOLE)] {
        let e = launch(&mut k, chan, kind, "agent-1").unwrap_err();
        assert_eq!(e.0, status::BUSY, "{e:?}");
    }
    k.env().jobs.get_mut(&j).unwrap().procs = 0;
    launch(&mut k, 2, &CONSOLE, "agent-1").unwrap();
}

#[test]
fn a_slot_reused_by_the_other_launcher_does_not_inherit_the_room() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    let j = k.job_of("agent-1").unwrap();
    k.env().jobs.get_mut(&j).unwrap().procs = 1;
    k.channel_closed(1, &DESKTOP);
    assert_eq!(
        launch(&mut k, 1, &CONSOLE, "agent-1").unwrap_err().0,
        status::BUSY
    );
}

#[test]
fn a_resumed_room_does_not_report_old_refusals_again() {
    let mut first = keeper();
    launch(&mut first, 1, &DESKTOP, "agent-1").unwrap();
    let j = first.job_of("agent-1").unwrap();
    first.env().jobs.get_mut(&j).unwrap().refused_mem = 7;
    let model = core::mem::take(first.env());
    let mut second = Keeper::new(model, Some(1));
    second.resume("agent-1", j, true, None);
    assert!(second.refusals().is_empty());
    second.env().jobs.get_mut(&j).unwrap().refused_mem = 8;
    assert_eq!(second.refusals().len(), 1);
}

#[test]
fn the_keeper_tells_init_who_holds_a_room_when_it_changes() {
    let mut k = keeper();
    k.take_events();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    let ev = k.take_events();
    assert!(matches!(
        ev.as_slice(),
        [Event::Created {
            launcher: Some(Launcher::Desktop),
            untrusted: false,
            ..
        }]
    ));
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    assert!(k.take_events().is_empty(), "no change, nothing said");
    launch(&mut k, 2, &CONSOLE, "project").unwrap();
    assert!(matches!(
        k.take_events().as_slice(),
        [Event::Created {
            launcher: Some(Launcher::Console),
            ..
        }]
    ));
}

#[test]
fn home_cannot_be_stopped_or_set() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "home").unwrap();
    let home = k.job_of("home").unwrap();
    for bytes in [
        msg(&Stop { room: Text("home") }),
        msg(&SetMsg { room: Text("home") }),
    ] {
        assert_eq!(
            simple(&mut k, 1, &DESKTOP, &bytes).unwrap_err().0,
            status::NOT_ALLOWED
        );
    }
    assert!(!k.env().jobs[&home].killed);
    assert_eq!(k.running(), vec!["home".to_string()]);
}

#[test]
fn refusals_are_noticed_once_and_go_to_the_rooms_own_and_the_overview_channels() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    assert!(k.refusals().is_empty());
    let j = k.job_of("agent-1").unwrap();
    k.env().jobs.get_mut(&j).unwrap().refused_mem = 3;
    let n = k.refusals();
    assert_eq!(n.len(), 1);
    assert_eq!((n[0].room.as_str(), n[0].field), ("agent-1", field::MEMORY));
    assert_eq!(n[0].limit, 89 << 20);
    assert!(
        k.refusals().is_empty(),
        "the same count is not noticed twice"
    );
    k.env().jobs.get_mut(&j).unwrap().refused_mem = 4;
    assert_eq!(k.refusals().len(), 1);
    let mut b = [0u8; 128];
    let len = Keeper::<Model>::notice_bytes(&n[0], &mut b).unwrap();
    let (_, r) = decode::<Refused<'_>>(&b[..len], 0).unwrap();
    assert_eq!(r.room.0, "agent-1");
    let room = |n: &str| ChannelKind::Room(RoomName::new(n).unwrap());
    assert!(Keeper::<Model>::wants_notice(&room("agent-1"), "agent-1"));
    assert!(!Keeper::<Model>::wants_notice(&room("agent-2"), "agent-1"));
    assert!(Keeper::<Model>::wants_notice(
        &ChannelKind::Usage,
        "agent-1"
    ));
    assert!(Keeper::<Model>::wants_notice(&DESKTOP, "agent-1"));
}

#[test]
fn a_report_is_kept_and_shown_in_usage() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&Report {
            service: Below(1),
            used: 12,
            limit: 128,
            refused: 2,
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    assert_eq!(
        simple(
            &mut k,
            1,
            &DESKTOP,
            &msg(&Report {
                service: Below(1),
                used: 1,
                limit: 1,
                refused: 0,
                room: Text("nobody")
            }),
        )
        .unwrap_err()
        .0,
        status::NOT_RUNNING
    );
    let (r, out) = ask(&mut k, 1, &DESKTOP, &msg(&Usage { room: Text("") }));
    let Reply::Send(n) = r else { panic!() };
    let (_, list) = decode::<UsageList<'_>>(&out[..n], 0).unwrap();
    let rec = UsageRecord::decode(list.records.0).unwrap();
    assert_eq!(rec.reports[1], [2, 12, 128, 2]);
}

#[test]
fn the_kernel_refusing_a_job_is_said_and_leaves_no_room() {
    let mut k = keeper();
    k.env().refuse_create = true;
    let e = launch(&mut k, 1, &DESKTOP, "project").unwrap_err();
    assert_eq!(e.0, status::KERNEL);
    assert!(k.running().is_empty());
    assert!(
        k.take_events()
            .iter()
            .all(|e| !matches!(e, Event::Created { room, .. } if room == "project"))
    );
}

#[test]
fn four_untrusted_rooms_at_their_defaults_fit_under_the_untrusted_job() {
    // The model of INV-0261-8's arithmetic: whatever the four rooms hold,
    // the untrusted job's ceilings leave the session half its memory, 104
    // processes and half the CPU.
    let m = Machine {
        session_mem: 356 << 20,
        session_procs: 200,
        free_disk: 1 << 40,
    };
    let u = m.untrusted();
    assert!(m.session_mem - u.mem >= m.session_mem / 2);
    assert!(u64::from(m.session_procs - u.procs) >= 104);
    assert!(u.cpu <= 50);
    let mut k = keeper();
    for (i, room) in ["agent-1", "agent-2", "agent-3", "agent-4"]
        .iter()
        .enumerate()
    {
        launch(&mut k, i, &DESKTOP, room).unwrap();
    }
    // Every room job is below the untrusted job, so the kernel's walk over
    // limited ancestors bounds their sum (INV-0261-3, in QEMU: t_rooms).
    for room in ["agent-1", "agent-2", "agent-3", "agent-4"] {
        let j = k.job_of(room).unwrap();
        assert_eq!(k.env().jobs[&j].parent, Some(1));
    }
}

#[test]
fn home_is_a_trusted_room_without_limits_under_the_session() {
    let mut k = keeper();
    let a = launch(&mut k, 1, &DESKTOP, "home").unwrap();
    let b = launch(&mut k, 2, &CONSOLE, "home").unwrap();
    assert_ne!(a, b);
    let room = k.job_of("home").unwrap();
    assert_eq!(k.env().jobs[&room].parent, Some(0));
    assert_eq!(k.env().jobs[&room].limits, JobLimits::default());
    assert_eq!(k.untrusted_running(), 0);
    // `home` never appears in the file, so the table does not matter.
    k.env().table = "BROKEN".into();
    launch(&mut k, 1, &DESKTOP, "home").unwrap();
}

#[test]
fn only_manage_opens_channels_and_names_the_kind() {
    let mut k = keeper();
    let m = msg(&zoraya_proto_rooms::Open {
        kind: Below(0),
        room: Text("agent-1"),
    });
    let (r, _) = ask(&mut k, 1, &DESKTOP, &m);
    let Reply::Open(_, kind) = r else {
        panic!("{r:?}")
    };
    assert_eq!(kind, ChannelKind::Room(RoomName::new("agent-1").unwrap()));
    let m = msg(&zoraya_proto_rooms::Open {
        kind: Below(1),
        room: Text(""),
    });
    let (r, _) = ask(&mut k, 1, &CONSOLE, &m);
    assert!(matches!(r, Reply::Open(_, ChannelKind::Usage)));
    let m = msg(&zoraya_proto_rooms::Open {
        kind: Below(0),
        room: Text(""),
    });
    let (r, _) = ask(&mut k, 1, &DESKTOP, &m);
    assert!(matches!(r, Reply::Send(_)), "an empty room name is refused");
    for kind in [
        ChannelKind::Usage,
        ChannelKind::Room(RoomName::new("agent-1").unwrap()),
    ] {
        let m = msg(&zoraya_proto_rooms::Open {
            kind: Below(1),
            room: Text(""),
        });
        let (r, out) = ask(&mut k, 2, &kind, &m);
        let Reply::Send(n) = r else { panic!() };
        assert_eq!(
            decode::<Denied<'_>>(&out[..n], 0).unwrap().1.status.0,
            status::NOT_ALLOWED
        );
    }
}

/// A small xorshift, so that the test needs no dependency and replays.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn random_bytes_on_every_channel_kind_never_panic_or_leave_a_room_channels_room() {
    // Finding 12: whatever a channel sends, the keeper answers without
    // panicking, and a `Room(R)` channel never gets `Open` and never causes
    // a room other than R to be made or a launch job in another room.
    let seeds: std::vec::Vec<std::vec::Vec<u8>> = std::vec![
        msg(&Launch {
            room: Text("agent-2")
        }),
        msg(&Launch {
            room: Text("agent-1")
        }),
        msg(&Launch { room: Text("home") }),
        msg(&Launch { room: Text("") }),
        msg(&Open {
            kind: Below(0),
            room: Text("agent-3")
        }),
        msg(&Open {
            kind: Below(1),
            room: Text("")
        }),
        msg(&Usage { room: Text("") }),
        msg(&Usage {
            room: Text("agent-2")
        }),
        msg(&LeaveMsg {
            room: Text("agent-2")
        }),
        msg(&SetMsg {
            room: Text("agent-2")
        }),
        msg(&Stop {
            room: Text("agent-1")
        }),
    ];
    let own = RoomName::new("agent-1").unwrap();
    let kinds = [ChannelKind::Room(own), DESKTOP, CONSOLE, ChannelKind::Usage];
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for kind in &kinds {
        let mut k = keeper();
        // The room the Room channel is in exists.
        launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
        for round in 0..4000 {
            let mut bytes = if round % 4 == 0 {
                (0..rng.below(80)).map(|_| rng.next() as u8).collect()
            } else {
                seeds[rng.below(seeds.len())].clone()
            };
            if round % 4 != 0 {
                for _ in 0..rng.below(4) {
                    if !bytes.is_empty() {
                        let i = rng.below(bytes.len());
                        bytes[i] = rng.next() as u8;
                    }
                }
                if rng.below(5) == 0 {
                    bytes.truncate(rng.below(bytes.len() + 1));
                }
            }
            let handles = rng.below(3);
            let mut out = vec![0u8; OUT_SIZE];
            let before: std::vec::Vec<String> = k.running();
            let reply = k.handle(3, kind, &bytes, handles, &mut out);
            if let ChannelKind::Room(r) = kind {
                assert!(!matches!(reply, Reply::Open(..)), "{bytes:?}");
                if let Reply::SendJob(_, job) = reply {
                    let parent = k.env().jobs[&job].parent;
                    assert_eq!(parent, k.job_of(r.as_str()), "{bytes:?}");
                }
                assert_eq!(k.running(), before, "a Room channel changed the rooms");
            }
            if matches!(kind, ChannelKind::Usage) {
                assert_eq!(k.running(), before, "a Usage channel changed the rooms");
                assert!(!matches!(reply, Reply::SendJob(..) | Reply::Open(..)));
            }
            // Keep the model small: stopped rooms may come back by Launch.
            if k.running().len() > 40 {
                break;
            }
        }
    }
}

fn records(k: &mut Keeper<Model>, room: &str) -> std::vec::Vec<UsageRecord> {
    let (r, out) = ask(k, 1, &ChannelKind::Usage, &msg(&Usage { room: Text(room) }));
    let Reply::Send(n) = r else { panic!("{r:?}") };
    let (_, list) = decode::<UsageList<'_>>(&out[..n], 0).unwrap();
    list.records
        .0
        .chunks(RECORD_SIZE)
        .map(|c| UsageRecord::decode(c).unwrap())
        .collect()
}

#[test]
fn a_resumed_room_reads_its_line_and_resuming_twice_keeps_the_others() {
    let mut first = keeper();
    launch(&mut first, 1, &DESKTOP, "build").unwrap();
    launch(&mut first, 1, &DESKTOP, "project").unwrap();
    let (b, p) = (
        first.job_of("build").unwrap(),
        first.job_of("project").unwrap(),
    );
    let model = core::mem::take(first.env());
    let mut k = Keeper::new(model, Some(1));
    k.resume("build", b, false, None);
    k.resume("project", p, false, None);
    k.resume("build", b, false, None);
    assert_eq!(k.running().len(), 2, "one entry per room");
    // The disk word is the line's alone: the keeper reads it again.
    assert_eq!(records(&mut k, "build")[0].disk_limit, 2 << 30);
}

#[test]
fn procs_refusals_are_noticed_once_and_a_named_usage_lists_one_room() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "build").unwrap();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    let j = k.job_of("build").unwrap();
    k.env().jobs.get_mut(&j).unwrap().refused_procs = 2;
    let n = k.refusals();
    assert_eq!(n.len(), 1);
    assert_eq!(
        (n[0].room.as_str(), n[0].field),
        ("build", field::PROCESSES)
    );
    assert!(k.refusals().is_empty());
    assert_eq!(records(&mut k, "build").len(), 1);
    assert_eq!(records(&mut k, "").len(), 2);
}

#[test]
fn a_record_says_running_untrusted_and_leased() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    let a = records(&mut k, "agent-1")[0];
    assert_eq!(a.flags, flags::RUNNING | flags::UNTRUSTED | flags::LEASED);
    let p = records(&mut k, "project")[0];
    assert_eq!(p.flags, flags::RUNNING | flags::LEASED);
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("project"),
        }),
    )
    .unwrap();
    assert_eq!(records(&mut k, "project")[0].flags, flags::RUNNING);
}

#[test]
fn a_collected_room_does_not_hide_the_ones_after_it() {
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    launch(&mut k, 2, &CONSOLE, "build").unwrap();
    simple(
        &mut k,
        2,
        &CONSOLE,
        &msg(&LeaveMsg {
            room: Text("build"),
        }),
    )
    .unwrap();
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running(), vec!["project".to_string()]);
}

#[test]
fn a_resumed_rooms_lease_ends_only_when_its_kind_of_channel_closes() {
    // Round 3, findings 1 and 2: zosh reconnects only at its next call, so
    // the room is its own however long it takes; Stop and Launch must see
    // one job. With no process the room would be collected at once if the
    // lease ended.
    let (mut k, j) = restarted(Some(Launcher::Console), 0);
    k.env().now += 100 * IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "the console's lease holds");
    assert_eq!(k.job_of("agent-1"), Some(j));
    assert!(!k.env().jobs[&j].killed);
    // The desktop is refused, and the console launches into the same job.
    assert_eq!(
        launch(&mut k, 10, &DESKTOP, "agent-1").unwrap_err().0,
        status::BUSY
    );
    // A channel of the other kind closing does not end it.
    k.channel_closed(9, &DESKTOP);
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "a desktop channel is not the holder");
    // A channel of its kind closing does.
    k.channel_closed(9, &CONSOLE);
    k.collect_idle();
    assert!(k.running().is_empty());
}

#[test]
fn stop_frees_a_resumed_room_of_unknown_holder() {
    // Finding 6: the BUSY of "running, holder unknown" has a way out.
    let (mut k, j) = restarted(None, 2);
    assert_eq!(
        launch(&mut k, 1, &CONSOLE, "agent-1").unwrap_err(),
        (status::BUSY, "running, holder unknown".to_string())
    );
    simple(
        &mut k,
        1,
        &CONSOLE,
        &msg(&Stop {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    assert!(k.env().jobs.get(&j).unwrap().killed);
    assert!(k.running().is_empty());
    launch(&mut k, 1, &CONSOLE, "agent-1").unwrap();
}

#[test]
fn a_denial_answers_the_transaction_of_the_request() {
    // Findings 4 and 5: the process answers with a denial instead of
    // closing the channel.
    let request = msg(&Launch {
        room: Text("agent-1"),
    });
    let mut out = vec![0u8; OUT_SIZE];
    let n = denial(status::TOO_MANY, "no place", &request, &mut out).unwrap();
    let (h, d) = decode::<Denied<'_>>(&out[..n], 0).unwrap();
    assert_eq!(h.transaction, 5);
    assert_eq!((d.status.0, d.text.0), (status::TOO_MANY, "no place"));
    assert!(denial(status::KERNEL, "x", &[1, 2], &mut out).is_none());
}

#[test]
fn a_launch_that_init_did_not_take_is_undone() {
    // Round 3, finding 4: the client was told KERNEL and sends no Leave, so
    // the lease, the holder and the new room go back to what they were.
    let mut k = keeper();
    k.env().now = 10 * IDLE_MS;
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    k.take_events();
    k.undo_last_launch();
    k.collect_idle();
    assert!(
        k.running().is_empty(),
        "a room made by the launch is let go"
    );

    // A room that runs, its holder the desktop with no lease now: the
    // console's launch is undone, and the desktop is not turned away.
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    k.channel_closed(1, &DESKTOP);
    let j = k.job_of("agent-1").unwrap();
    k.env().jobs.get_mut(&j).unwrap().procs = 1;
    k.take_events();
    launch(&mut k, 2, &CONSOLE, "agent-1").unwrap_err();
    k.env().jobs.get_mut(&j).unwrap().procs = 0;
    launch(&mut k, 2, &CONSOLE, "agent-1").unwrap();
    k.env().jobs.get_mut(&j).unwrap().procs = 1;
    k.undo_last_launch();
    launch(&mut k, 3, &DESKTOP, "agent-1").unwrap();
    // The undone console channel holds no lease: the desktop's leaves, and
    // with no process the room is collected.
    k.env().jobs.get_mut(&j).unwrap().procs = 0;
    simple(
        &mut k,
        3,
        &DESKTOP,
        &msg(&LeaveMsg {
            room: Text("agent-1"),
        }),
    )
    .unwrap();
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert!(k.running().is_empty());

    // A denied launch leaves nothing to undo.
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    launch(&mut k, 2, &CONSOLE, "agent-1").unwrap_err();
    k.undo_last_launch();
    assert_eq!(k.running().len(), 1);
    let j = k.job_of("agent-1").unwrap();
    k.env().jobs.get_mut(&j).unwrap().procs = 0;
    k.env().now += IDLE_MS;
    k.collect_idle();
    assert_eq!(k.running().len(), 1, "the desktop still holds its lease");
}

#[test]
fn room_set_takes_the_missing_words_from_the_running_rooms_usage_or_refuses() {
    // Round 3, finding 5: the logic of `room set` on an untrusted line with
    // fewer than six words.
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "agent-1").unwrap();
    let running = records(&mut k, "agent-1").pop();
    assert!(running.is_some());
    let words = Limits {
        mem: Some(1 << 20),
        ..Limits::default()
    };
    // A running room: the missing words come from it, the given one stays.
    let mut l = words;
    assert!(complete_from_running(&mut l, running.as_ref()));
    assert!(l.is_complete());
    assert_eq!(l.mem, Some(1 << 20));
    assert_eq!(l.procs, record_limits(running.as_ref().unwrap()).procs);
    // A room that does not run, or no keeper: nothing is made up.
    let mut l = words;
    assert!(!complete_from_running(&mut l, None));
    assert_eq!(l, words);
    // A complete line needs nothing.
    let mut full = record_limits(running.as_ref().unwrap());
    assert!(full.is_complete());
    assert!(complete_from_running(&mut full, None));
}
