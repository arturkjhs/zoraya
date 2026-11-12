//! The POSIX broker's and the terminal server's requests (ADR-0104, ADR-0105,
//! ADR-0268 point 3's "fuzzing", ADR-0298). In an untrusted room both are the
//! room's own, per shell; what a program there sends them must never panic
//! them. A model of the kernel under the broker (handles are numbers, the
//! type of a handle is its number modulo 4), connections from the first
//! process, children and a native client, arbitrary requests on any of them,
//! terminations and closes in between; then the same for the terminal.

#![no_main]

mod wire;

use libfuzzer_sys::fuzz_target;
use zoraya_abi::{ExitReason, ExitRecord, RequestKind, Rights, Status};
use zoraya_broker::{Action, Broker, Kernel};
use zoraya_tty::{Action as TtyAction, Terminal};

#[derive(Default)]
struct Model {
    next: u32,
    live: usize,
}

impl Kernel for Model {
    type H = u32;
    fn job_create(&mut self, _: u32) -> Result<u32, Status> {
        if self.live > 400 {
            return Err(Status::NoMemory);
        }
        self.next += 4;
        self.live += 1;
        Ok(self.next)
    }
    fn duplicate(&mut self, h: u32, _: Rights) -> Result<u32, Status> {
        self.next += 4;
        Ok(self.next + (h & 3))
    }
    fn close(&mut self, _: u32) {}
    fn channel_create(&mut self) -> Result<(u32, u32), Status> {
        self.next += 4;
        Ok((self.next + 2, self.next + 6))
    }
    fn job_signal(&mut self, _: u32, _: RequestKind, _: u32) -> Result<(), Status> {
        Ok(())
    }
    fn job_kill(&mut self, _: u32) -> Result<(), Status> {
        Ok(())
    }
    fn job_suspend(&mut self, _: u32) -> Result<(), Status> {
        Ok(())
    }
    fn job_resume(&mut self, _: u32) -> Result<(), Status> {
        Ok(())
    }
    fn signals(&mut self, _: u32) -> Result<u32, Status> {
        Ok(0)
    }
    fn exit_record(&mut self, _: u32) -> Result<ExitRecord, Status> {
        Ok(ExitRecord { code: 0, reason: ExitReason::Exited, detail: 0, request_kind: None })
    }
    fn type_name(&mut self, h: u32) -> Option<&'static str> {
        Some(["job", "process", "channel", "memory"][(h & 3) as usize])
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else { return };
    if pick & 1 == 0 {
        broker(rest);
    } else {
        terminal(rest);
    }
});

fn broker(data: &[u8]) {
    let mut b = Broker::new(Model::default(), 1, pick_terminal(data));
    let mut conns = Vec::new();
    if let Ok((_, actions)) = b.first() {
        for a in &actions {
            if let Action::Watch { conn, .. } = a {
                conns.push(*conn);
            }
        }
        let _ = b.commit_first(8);
    }
    if let Ok((_, actions)) = b.add_client() {
        for a in &actions {
            if let Action::Watch { conn, .. } = a {
                conns.push(*conn);
            }
        }
    }
    for m in wire::messages(data, 48) {
        let conn = if conns.is_empty() { 0 } else { conns[usize::from(m.sel) % conns.len()] };
        let handles: Vec<u32> = (0..m.handles as u32).map(|i| 1000 + 4 * i + u32::from(m.sel & 3)).collect();
        let actions = b.request(conn, &m.bytes, handles);
        for a in &actions {
            if let Action::Watch { conn, .. } = a {
                if conns.len() < 64 {
                    conns.push(*conn);
                }
            }
        }
        match m.sel >> 5 {
            0 => {
                let _ = b.terminated(u32::from(m.sel & 15) + 2);
            }
            1 => {
                let _ = b.closed(conn);
            }
            _ => {}
        }
        let _ = (b.live(), b.statistics());
    }
}

fn pick_terminal(data: &[u8]) -> bool {
    data.first().is_some_and(|b| b & 2 != 0)
}

fn terminal(data: &[u8]) {
    let mut t: Terminal<u32> = Terminal::new();
    let mut ids: Vec<usize> = t.add_control().into_iter().collect();
    ids.extend(t.add_terminal(2));
    for m in wire::messages(data, 48) {
        let id = if ids.is_empty() { 0 } else { ids[usize::from(m.sel) % ids.len()] };
        let handles: Vec<u32> = (0..m.handles as u32).map(|i| 5 + i).collect();
        for a in t.request(id, &m.bytes, handles) {
            if let TtyAction::Watch { id, .. } = a {
                ids.push(id);
            }
        }
        match m.sel >> 5 {
            0 => {
                let _ = t.keys(&m.bytes);
            }
            1 => {
                let _ = t.resize(u32::from(m.sel), u32::from(m.sel) * 3);
            }
            2 => {
                let mut out = Vec::new();
                t.output(&m.bytes, &mut out);
            }
            3 => t.closed(id),
            _ => {}
        }
    }
}
