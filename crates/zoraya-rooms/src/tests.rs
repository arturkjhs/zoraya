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
    fn room_line(&mut self, name: &str) -> Result<Option<Room>, ()> {
        if self.table == "BROKEN" {
            return Err(());
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
build --colour 2 --cpu 50% --mem 64M +w ~/build\n\
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
        Reply::Close => Err((99, "closed".into())),
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
            untrusted: true
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
    k.channel_closed(2);
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
fn which_messages_each_channel_kind_may_send() {
    // INV-0269-2: a Room(R) channel is refused every message but
    // Launch { R } and Usage { R }; a Usage channel only reads.
    let mut k = keeper();
    launch(&mut k, 1, &DESKTOP, "project").unwrap();
    launch(&mut k, 1, &DESKTOP, "build").unwrap();
    let room = |n: &str| ChannelKind::Room(RoomName::new(n).unwrap());
    let project = room("project");
    let name = |n| Text::<24>(n);
    let all: [(&str, std::vec::Vec<u8>); 6] = [
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
            Reply::Close => panic!("{what}"),
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
    for ord in [7u32, 15] {
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
    // A room that does not run: nothing to apply.
    simple(
        &mut k,
        1,
        &DESKTOP,
        &msg(&SetMsg {
            room: Text("project"),
        }),
    )
    .unwrap();
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
    second.resume("agent-1", j, true);
    assert_eq!(second.untrusted_running(), 1);
    // The room is not launched into a second time by another job.
    let again = launch(&mut second, 9, &CONSOLE, "agent-1");
    assert!(again.is_ok(), "no holder is known after a restart");
    assert_eq!(second.job_of("agent-1"), Some(j));
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
