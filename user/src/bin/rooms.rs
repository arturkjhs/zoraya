#![no_std]
#![no_main]

//! The room keeper (ADR-0269, proposed ADR-0293). Slot 5 is the session job
//! (with `LIMIT`), slot 6 the channel to `init`, slot 7 a read connection to
//! `/config`. It owns one job per room, hands launchers launch jobs (no
//! `LIMIT`), applies the limits of `/config/rooms` and tells the channels
//! when a room's job refuses. Everything it decides is `zoraya-rooms`, under
//! host test; this process moves channel messages and calls the kernel.
//!
//! `init` sends it, on channel 6 and before it serves: `Keep` (the untrusted
//! job and the room jobs of a keeper that ended, each with `LIMIT`) and then
//! `Attach` (a channel to serve, with its kind). It sends `init` `Keep` for
//! every job it makes and `Forget` for every job it lets go.

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::quota::{JobLimits, JobUsage};
use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, Handle, JobFlags, JobObject, Mode, ObjectType, Packet, Rights, Status,
    channel_signals, job_signals,
};
use zoraya_proto_rooms::alloc_free::RoomName;
use zoraya_proto_rooms::{ChannelKind, Forget, Keep, Launcher, attach};
use zoraya_protocol::{Below, Text, decode, encode};
use zoraya_rooms::{Env, Event, JobId, Keeper, OUT_SIZE, Parent, Reply};
use zoraya_settings::rooms::{self, Machine, Room};
use zoraya_user::files::{File, read_all};
use zoraya_user::{
    channel_read, channel_write, clock_monotonic, handle_close, handle_type, job_create,
    job_get_usage, job_kill, job_set_limits, port_create, port_subscribe, port_unsubscribe,
    port_wait, println, process_exit,
};

const SESSION: u64 = 5;
const INIT: u64 = 6;
const CONFIG: u64 = 7;
/// Port keys: the channel to `init`, then one per served channel, then one
/// per job.
const KEY_INIT: u64 = 1;
const KEY_CHANNEL: u64 = 1 << 16;
const KEY_JOB: u64 = 1 << 32;
/// Channels at once; `init` makes three kinds, and a shell asks for one.
const MAX_CHANNELS: usize = 64;
/// How often idle rooms are looked for, in nanoseconds.
const TURN_NS: u64 = 1_000_000_000;

fn slot(n: u64) -> Handle {
    Handle::new(n).expect("slot is not zero")
}

/// The kernel and the files, for `zoraya-rooms`.
struct Sys {
    port: Handle,
    session: Handle,
    config: Option<File>,
    /// Job `i + 1`'s handle; `None` once closed.
    jobs: Vec<Option<Handle>>,
}

impl Sys {
    fn add(&mut self, h: Handle) -> JobId {
        let id = match self.jobs.iter().position(Option::is_none) {
            Some(i) => {
                self.jobs[i] = Some(h);
                i
            }
            None => {
                self.jobs.push(Some(h));
                self.jobs.len() - 1
            }
        };
        let id = id as JobId + 1;
        let _ = port_subscribe(
            self.port,
            h,
            job_signals::OVER_QUOTA,
            KEY_JOB + u64::from(id),
            Mode::Level,
        );
        id
    }

    fn get(&self, id: JobId) -> Option<Handle> {
        self.jobs
            .get((id as usize).checked_sub(1)?)
            .copied()
            .flatten()
    }
}

impl Env for Sys {
    fn create_job(&mut self, parent: Parent, limits: &JobLimits) -> Result<JobId, Status> {
        let parent = match parent {
            Parent::Session => self.session,
            Parent::Job(j) => self.get(j).ok_or(Status::BadHandle)?,
        };
        let job = job_create(parent, JobFlags::NONE)?;
        if let Err(s) = job_set_limits(job, limits) {
            let _ = job_kill(job);
            let _ = handle_close(job);
            return Err(s);
        }
        Ok(self.add(job))
    }

    fn launch_job(&mut self, room: JobId) -> Result<JobId, Status> {
        let parent = self.get(room).ok_or(Status::BadHandle)?;
        let job = job_create(parent, JobFlags::NONE)?;
        // Not subscribed: the keeper watches the room's job, whose counters
        // include this one's.
        self.jobs.push(Some(job));
        Ok(self.jobs.len() as JobId)
    }

    fn set_limits(&mut self, job: JobId, limits: &JobLimits) -> Result<(), Status> {
        job_set_limits(self.get(job).ok_or(Status::BadHandle)?, limits)
    }

    fn usage(&mut self, job: JobId, clear: bool) -> Option<JobUsage> {
        job_get_usage(self.get(job)?, clear).ok()
    }

    fn kill(&mut self, job: JobId) {
        if let Some(h) = self.get(job) {
            let _ = job_kill(h);
        }
    }

    fn close(&mut self, job: JobId) {
        if let Some(slot) = self.jobs.get_mut((job as usize).saturating_sub(1))
            && let Some(h) = slot.take()
        {
            let _ = port_unsubscribe(self.port, KEY_JOB + u64::from(job));
            let _ = handle_close(h);
        }
    }

    fn now_ms(&mut self) -> u64 {
        clock_monotonic() / 1_000_000
    }

    fn room_line(&mut self, name: &str) -> Result<Option<Room>, ()> {
        let Some(config) = &self.config else {
            return Err(());
        };
        // No file is no rooms.
        let Ok(file) = config.open("rooms", 0, zoraya_proto_file::rights::READ) else {
            return Ok(None);
        };
        let bytes = read_all(&file).map_err(|_| ())?;
        Ok(rooms::read(&bytes).get(name).cloned())
    }

    fn machine(&mut self) -> Machine {
        let u = job_get_usage(self.session, false).ok();
        Machine {
            session_mem: u.and_then(|u| u.memory.limit).unwrap_or(1 << 40),
            session_procs: u
                .and_then(|u| u.processes.limit)
                .map_or(200, |p| p.min(u64::from(u32::MAX)) as u32),
            // The volume's free space is not known here: the default of
            // `--disk` is the person's to see in `room new`, and a line
            // that lacks it gets the ceiling of 1 GiB.
            free_disk: 8 << 30,
        }
    }
}

/// The kind of channel `init` asked for.
fn kind_of(kind: u32, room: &str) -> Option<ChannelKind> {
    match kind {
        attach::MANAGE_DESKTOP => Some(ChannelKind::Manage(Launcher::Desktop)),
        attach::MANAGE_CONSOLE => Some(ChannelKind::Manage(Launcher::Console)),
        attach::USAGE => Some(ChannelKind::Usage),
        _ => RoomName::new(room).map(ChannelKind::Room),
    }
}

struct Chan {
    end: Handle,
    kind: ChannelKind,
}

struct Rooms {
    keeper: Keeper<Sys>,
    init: Handle,
    chans: Vec<Option<Chan>>,
    request: Vec<u8>,
    out: Vec<u8>,
}

impl Rooms {
    fn attach(&mut self, end: Handle, kind: ChannelKind) -> bool {
        let id = match self.chans.iter().position(Option::is_none) {
            Some(i) => i,
            None if self.chans.len() < MAX_CHANNELS => {
                self.chans.push(None);
                self.chans.len() - 1
            }
            None => return false,
        };
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        let port = self.keeper.env().port;
        if port_subscribe(port, end, wanted, KEY_CHANNEL + id as u64, Mode::Level).is_err() {
            return false;
        }
        self.chans[id] = Some(Chan { end, kind });
        true
    }

    fn drop_channel(&mut self, id: usize) {
        if let Some(c) = self.chans.get_mut(id).and_then(Option::take) {
            let port = self.keeper.env().port;
            let _ = port_unsubscribe(port, KEY_CHANNEL + id as u64);
            let _ = handle_close(c.end);
            self.keeper.channel_closed(id);
        }
    }

    /// Tells `init` what the keeper made and let go (ADR-0269 point 5).
    fn tell_init(&mut self) {
        for event in self.keeper.take_events() {
            let mut b = [0u8; 128];
            match event {
                Event::Created {
                    room,
                    job,
                    untrusted,
                } => {
                    let Some(handle) = self.keeper.env().get(job) else {
                        continue;
                    };
                    let Ok(n) = encode(
                        &Keep {
                            untrusted: Below(u32::from(untrusted && room.is_empty())),
                            room: Text(&room),
                        },
                        0,
                        &mut b,
                    ) else {
                        continue;
                    };
                    // A duplicate with every right: init may hand it back.
                    let entry = ChannelEntry {
                        handle,
                        rights: JobObject::RIGHTS,
                        transfer: Transfer::Duplicate,
                    };
                    let _ = channel_write(self.init, &b[..n], &[entry]);
                }
                Event::Gone { room } => {
                    if let Ok(n) = encode(&Forget { room: Text(&room) }, 0, &mut b) {
                        let _ = channel_write(self.init, &b[..n], &[]);
                    }
                }
            }
        }
    }

    fn serve(&mut self, id: usize) {
        for _ in 0..8 {
            let Some((end, kind)) = self
                .chans
                .get(id)
                .and_then(|c| c.as_ref())
                .map(|c| (c.end, c.kind.clone()))
            else {
                return;
            };
            let mut handles = [end; 2];
            let sizes = match channel_read(end, &mut self.request, &mut handles) {
                Ok(s) => s,
                Err((Status::WouldBlock, _)) => return,
                Err(_) => {
                    self.drop_channel(id);
                    return;
                }
            };
            let count = (sizes.handles as usize).min(handles.len());
            // A launcher sends no handle with a request.
            for h in &handles[..count] {
                let _ = handle_close(*h);
            }
            let bytes = &self.request[..sizes.bytes as usize];
            let reply = if count > 0 {
                Reply::Close
            } else {
                self.keeper.handle(id, &kind, bytes, 0, &mut self.out)
            };
            let sent = match reply {
                Reply::Send(n) => channel_write(end, &self.out[..n], &[]).is_ok(),
                Reply::SendJob(n, job) => {
                    let ok = self.keeper.env().get(job).is_some_and(|h| {
                        let entry = ChannelEntry {
                            handle: h,
                            // Every right but `LIMIT` (INV-0269-2).
                            rights: JobObject::RIGHTS.without(Rights::LIMIT),
                            transfer: Transfer::Duplicate,
                        };
                        channel_write(end, &self.out[..n], &[entry]).is_ok()
                    });
                    self.keeper.env().close(job);
                    ok
                }
                Reply::Close => false,
            };
            self.tell_init();
            if !sent {
                self.drop_channel(id);
                return;
            }
        }
    }

    /// Sends the `Refused` of every room that reached a limit.
    fn notices(&mut self) {
        let mut b = [0u8; 128];
        for notice in self.keeper.refusals() {
            let Some(n) = Keeper::<Sys>::notice_bytes(&notice, &mut b) else {
                continue;
            };
            for c in self.chans.iter().flatten() {
                if Keeper::<Sys>::wants_notice(&c.kind, &notice.room) {
                    // A full channel loses the notice; the next refusal
                    // sends another.
                    let _ = channel_write(c.end, &b[..n], &[]);
                }
            }
        }
    }

    /// One message from `init`: a channel to serve.
    fn from_init(&mut self) -> bool {
        let mut handles = [self.init; 2];
        let mut buf = [0u8; 128];
        let sizes = match channel_read(self.init, &mut buf, &mut handles) {
            Ok(s) => s,
            Err((Status::WouldBlock, _)) => return false,
            Err(_) => process_exit(0),
        };
        let bytes = &buf[..sizes.bytes as usize];
        let count = sizes.handles as usize;
        if let Ok((_, m)) = decode::<zoraya_proto_rooms::Attach<'_>>(bytes, count) {
            let ok = kind_of(m.kind.0, m.room.0).is_some_and(|k| self.attach(handles[0], k));
            if !ok {
                let _ = handle_close(handles[0]);
            }
        } else {
            for h in &handles[..count.min(2)] {
                let _ = handle_close(*h);
            }
        }
        true
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let Ok(port) = port_create() else {
        return 1;
    };
    let init = slot(INIT);
    let mut sys = Sys {
        port,
        session: slot(SESSION),
        config: (handle_type(slot(CONFIG)) == Ok("channel")).then(|| File { end: slot(CONFIG) }),
        jobs: Vec::new(),
    };
    // What `init` queued before the keeper started: the jobs of a keeper that
    // ended (`Keep`) and the channels to serve (`Attach`, held until the
    // keeper exists).
    let mut untrusted = None;
    let mut resumed: Vec<(String, JobId)> = Vec::new();
    let mut pending: Vec<(ChannelKind, Handle)> = Vec::new();
    loop {
        let mut handles = [init; 2];
        let mut buf = [0u8; 128];
        let Ok(sizes) = channel_read(init, &mut buf, &mut handles) else {
            break;
        };
        let bytes = &buf[..sizes.bytes as usize];
        let count = sizes.handles as usize;
        if let Ok((_, m)) = decode::<Keep<'_>>(bytes, count) {
            let id = sys.add(handles[0]);
            if m.untrusted.0 == 1 {
                untrusted = Some(id);
            } else {
                resumed.push((String::from(m.room.0), id));
            }
        } else if let Ok((_, m)) = decode::<zoraya_proto_rooms::Attach<'_>>(bytes, count) {
            match kind_of(m.kind.0, m.room.0) {
                Some(k) => pending.push((k, handles[0])),
                None => {
                    let _ = handle_close(handles[0]);
                }
            }
        } else {
            for h in &handles[..count.min(2)] {
                let _ = handle_close(*h);
            }
        }
    }
    let mut rooms = Rooms {
        keeper: Keeper::new(sys, untrusted),
        init,
        chans: Vec::new(),
        request: vec![0u8; 512],
        out: vec![0u8; OUT_SIZE],
    };
    for (name, job) in resumed {
        // An untrusted room's job is under the untrusted job; the line says.
        let line = rooms.keeper.env().room_line(&name).ok().flatten();
        rooms
            .keeper
            .resume(&name, job, line.is_some_and(|l| l.untrusted));
    }
    for (kind, end) in pending {
        if !rooms.attach(end, kind) {
            let _ = handle_close(end);
        }
    }
    rooms.tell_init();
    if port_subscribe(
        port,
        init,
        channel_signals::READABLE | channel_signals::PEER_CLOSED,
        KEY_INIT,
        Mode::Level,
    )
    .is_err()
    {
        return 1;
    }
    println!("rooms: serving");
    let mut packets = [Packet::default(); 8];
    loop {
        let deadline = clock_monotonic() + TURN_NS;
        let count = match port_wait(port, deadline, &mut packets) {
            Ok(n) => n,
            Err(Status::DeadlineExceeded) => 0,
            Err(_) => process_exit(2),
        };
        for p in &packets[..count] {
            match p.key {
                KEY_INIT => while rooms.from_init() {},
                k if k >= KEY_JOB => rooms.notices(),
                k if k >= KEY_CHANNEL => rooms.serve((k - KEY_CHANNEL) as usize),
                _ => {}
            }
        }
        rooms.keeper.collect_idle();
        rooms.tell_init();
    }
}
