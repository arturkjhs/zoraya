//! The room keeper's decisions on arbitrary messages (ADR-0269, ADR-0293;
//! INV-0269-2: what each kind of channel may send). Messages arrive from the
//! three kinds of channel, in any order, with a model of the kernel's jobs
//! under the keeper; between them the keeper's timers run. Nothing may panic
//! and no launch job may carry `LIMIT`.

#![no_main]

mod wire;

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use zoraya_abi::Status;
use zoraya_abi::quota::{JobLimits, JobUsage};
use zoraya_proto_rooms::alloc_free::RoomName;
use zoraya_proto_rooms::{ChannelKind, Launcher};
use zoraya_rooms::{Env, JobId, Keeper, Parent, TableError};
use zoraya_settings::rooms::{self, Machine, Room};

#[derive(Default)]
struct Model {
    jobs: BTreeMap<JobId, (Option<JobId>, bool)>,
    next: JobId,
    now: u64,
}

const TABLE: &str = "project --colour 1 +w ~/project\n\
build --colour 2 --cpu 50% --mem 64M --disk 2G +w ~/build\n\
agent-1 --untrusted +w ~/sandbox/agent-1\n\
agent-2 --untrusted --mem 16M +w ~/sandbox/agent-2\n\
agent-3 --untrusted +w ~/sandbox/agent-3\n\
agent-4 --untrusted +w ~/sandbox/agent-4\n\
agent-5 --untrusted +w ~/sandbox/agent-5\n";

impl Env for Model {
    fn create_job(&mut self, parent: Parent, _: &JobLimits) -> Result<JobId, Status> {
        if self.jobs.len() > 200 {
            return Err(Status::NoMemory);
        }
        self.next += 1;
        let p = match parent {
            Parent::Session => 0,
            Parent::Job(j) => j,
        };
        self.jobs.insert(self.next, (Some(p), true));
        Ok(self.next)
    }
    fn launch_job(&mut self, room: JobId) -> Result<JobId, Status> {
        self.next += 1;
        self.jobs.insert(self.next, (Some(room), false));
        Ok(self.next)
    }
    fn set_limits(&mut self, job: JobId, _: &JobLimits) -> Result<(), Status> {
        match self.jobs.get(&job) {
            Some((_, true)) => Ok(()),
            Some(_) => Err(Status::AccessDenied),
            None => Err(Status::BadHandle),
        }
    }
    fn usage(&mut self, job: JobId, _: bool) -> Option<JobUsage> {
        self.jobs.get(&job).map(|_| JobUsage::default())
    }
    fn kill(&mut self, _: JobId) {}
    fn close(&mut self, job: JobId) {
        self.jobs.remove(&job);
    }
    fn now_ms(&mut self) -> u64 {
        self.now += 7;
        self.now
    }
    fn room_line(&mut self, name: &str) -> Result<Option<Room>, TableError> {
        Ok(rooms::read(TABLE.as_bytes()).get(name).cloned())
    }
    fn machine(&mut self) -> Machine {
        Machine { session_mem: 356 << 20, session_procs: 200, free_disk: 1 << 40 }
    }
}

fuzz_target!(|data: &[u8]| {
    let mut model = Model::default();
    model.jobs.insert(0, (None, true));
    let mut k = Keeper::new(model, None);
    let kinds = [
        ChannelKind::Manage(Launcher::Desktop),
        ChannelKind::Manage(Launcher::Console),
        ChannelKind::Room(RoomName::new("agent-1").unwrap()),
        ChannelKind::Room(RoomName::new("project").unwrap()),
        ChannelKind::Usage,
    ];
    let mut out = vec![0u8; 8 * 1024];
    for m in wire::messages(data, 64) {
        let chan = usize::from(m.sel >> 4) % 6;
        let kind = &kinds[usize::from(m.sel) % kinds.len()];
        let _ = k.handle(chan, kind, &m.bytes, m.handles, &mut out);
        match m.sel % 7 {
            0 => k.collect_idle(),
            1 => {
                let _ = k.refusals();
            }
            2 => k.channel_closed(chan, kind),
            _ => {}
        }
        let _ = k.take_events();
    }
    // INV-0269-2: no job a launcher holds can set limits.
    for (&job, &(_, limit)) in k.env().jobs.clone().iter() {
        if !limit {
            assert!(k.env().set_limits(job, &JobLimits::default()).is_err());
        }
    }
});
