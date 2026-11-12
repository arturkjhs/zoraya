//! A launcher's side of the room keeper (ADR-0269, proposed ADR-0293): the
//! shell and the panel ask it for a launch job in a room, give it back with
//! `Leave`, read a room's use and hear its refusals. If the keeper's channel
//! has gone (the keeper ended and `init` started another), the one request in
//! hand is made again over a new channel `init` gives on the reconnect
//! channel (ADR-0221 item 6's shape).

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{Handle, Mode, Packet, Status, channel_signals};
use zoraya_proto_rooms as pr;
use zoraya_protocol::{Header, Text, decode, encode};
use zoraya_rt::{
    channel_read, channel_write, clock_monotonic, handle_close, port_create, port_subscribe,
    port_unsubscribe, port_wait,
};

/// How long a request waits for the keeper, in nanoseconds.
const WAIT_NS: u64 = 5_000_000_000;
const KEY: u64 = 1;

/// A room job reached a limit (the keeper's `Refused`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub room: String,
    pub field: u32,
    pub used: u64,
    pub limit: u64,
}

/// Why a request was not done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fail {
    /// The keeper answered no: the status (`zoraya_proto_rooms::status`) and
    /// the sentence for `room: NAME: ...`.
    Denied(u32, String),
    /// No keeper answers.
    Gone,
}

pub struct Keeper {
    end: Handle,
    reconnect: Option<Handle>,
    port: Handle,
    next: u32,
    refusals: Vec<Refusal>,
}

fn drain_note(bytes: &[u8], refusals: &mut Vec<Refusal>) -> bool {
    if let Ok((_, r)) = decode::<pr::Refused<'_>>(bytes, 0) {
        if refusals.len() < 32 {
            refusals.push(Refusal {
                room: String::from(r.room.0),
                field: r.field.0,
                used: r.used,
                limit: r.limit,
            });
        }
        return true;
    }
    false
}

impl Keeper {
    /// `end` is the keeper channel; `reconnect` the channel to `init` that
    /// gives a new one.
    pub fn new(end: Handle, reconnect: Option<Handle>) -> Option<Keeper> {
        let port = port_create().ok()?;
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        port_subscribe(port, end, wanted, KEY, Mode::Level).ok()?;
        Some(Keeper {
            end,
            reconnect,
            port,
            next: 1,
            refusals: Vec::new(),
        })
    }

    fn rebind(&mut self) -> bool {
        let Some(init) = self.reconnect else {
            return false;
        };
        if channel_write(init, b"m", &[]).is_err() {
            return false;
        }
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        let Ok(port) = port_create() else {
            return false;
        };
        if port_subscribe(port, init, wanted, KEY, Mode::Level).is_err() {
            let _ = handle_close(port);
            return false;
        }
        let deadline = clock_monotonic() + WAIT_NS;
        let mut packets = [Packet::default()];
        let got = loop {
            let mut buf = [0u8; 16];
            let mut handles = [init; 1];
            match channel_read(init, &mut buf, &mut handles) {
                Ok(s) => break (s.handles > 0).then_some(handles[0]),
                Err((Status::WouldBlock, _)) => {
                    if port_wait(port, deadline, &mut packets).is_err() {
                        break None;
                    }
                }
                Err(_) => break None,
            }
        };
        let _ = handle_close(port);
        let Some(new) = got else {
            return false;
        };
        let _ = port_unsubscribe(self.port, KEY);
        let _ = handle_close(self.end);
        self.end = new;
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        port_subscribe(self.port, new, wanted, KEY, Mode::Level).is_ok()
    }

    /// Sends `bytes` and waits for the answer of transaction `tx`.
    fn once(&mut self, bytes: &[u8], tx: u32) -> Result<(Vec<u8>, Option<Handle>), Fail> {
        channel_write(self.end, bytes, &[]).map_err(|_| Fail::Gone)?;
        let deadline = clock_monotonic() + WAIT_NS;
        let mut packets = [Packet::default()];
        loop {
            let mut buf = vec![0u8; 8 * 1024];
            let mut handles = [self.end; 1];
            match channel_read(self.end, &mut buf, &mut handles) {
                Ok(s) => {
                    buf.truncate(s.bytes as usize);
                    let handle = (s.handles > 0).then_some(handles[0]);
                    if drain_note(&buf, &mut self.refusals) {
                        continue;
                    }
                    match Header::decode(&buf) {
                        Ok((h, _)) if h.transaction == tx => return Ok((buf, handle)),
                        _ => {
                            if let Some(h) = handle {
                                let _ = handle_close(h);
                            }
                        }
                    }
                }
                Err((Status::WouldBlock, _)) => {
                    if port_wait(self.port, deadline, &mut packets).is_err() {
                        return Err(Fail::Gone);
                    }
                }
                Err(_) => return Err(Fail::Gone),
            }
        }
    }

    fn call<'a, M: zoraya_protocol::Message<'a>>(
        &mut self,
        m: &M,
    ) -> Result<(Vec<u8>, Option<Handle>), Fail> {
        let mut b = [0u8; 128];
        let tx = self.next;
        self.next = self.next.wrapping_add(1).max(1);
        let n = encode(m, tx, &mut b).map_err(|_| Fail::Gone)?;
        match self.once(&b[..n], tx) {
            Err(Fail::Gone) if self.rebind() => self.once(&b[..n], tx),
            other => other,
        }
    }

    fn denied(bytes: &[u8]) -> Fail {
        match decode::<pr::Denied<'_>>(bytes, 0) {
            Ok((_, d)) => Fail::Denied(d.status.0, String::from(d.text.0)),
            Err(_) => Fail::Gone,
        }
    }

    fn done(&mut self, bytes: &[u8], handle: Option<Handle>) -> Result<(), Fail> {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        if decode::<pr::Done>(bytes, 0).is_ok() {
            Ok(())
        } else {
            Err(Self::denied(bytes))
        }
    }

    /// A launch job in `room`: a sub-job of the room's job with every right
    /// but `LIMIT`.
    pub fn launch(&mut self, room: &str) -> Result<Handle, Fail> {
        let (bytes, handle) = self.call(&pr::Launch { room: Text(room) })?;
        match (decode::<pr::Launched>(&bytes, 1), handle) {
            (Ok(_), Some(h)) => Ok(h),
            _ => Err(Self::denied(&bytes)),
        }
    }

    /// This launcher is done with `room` for now.
    pub fn leave(&mut self, room: &str) -> Result<(), Fail> {
        let (b, h) = self.call(&pr::Leave { room: Text(room) })?;
        self.done(&b, h)
    }

    /// Applies the room's line to its running job.
    pub fn set(&mut self, room: &str) -> Result<(), Fail> {
        let (b, h) = self.call(&pr::Set { room: Text(room) })?;
        self.done(&b, h)
    }

    /// `job_kill` on the room's job.
    pub fn stop(&mut self, room: &str) -> Result<(), Fail> {
        let (b, h) = self.call(&pr::Stop { room: Text(room) })?;
        self.done(&b, h)
    }

    /// Forwards a service account's counts (`service` 0 to 3).
    pub fn report(&mut self, room: &str, service: u32, used: u32, limit: u32, refused: u32) {
        let m = pr::Report {
            service: zoraya_protocol::Below(service),
            used,
            limit,
            refused,
            room: Text(room),
        };
        if let Ok((b, h)) = self.call(&m) {
            let _ = self.done(&b, h);
        }
    }

    /// A new channel to the keeper for a program this launcher starts:
    /// `Room(room)` for a shell in a room, or `Usage` when `room` is empty.
    pub fn open(&mut self, room: &str) -> Result<Handle, Fail> {
        let kind = u32::from(room.is_empty());
        let (bytes, handle) = self.call(&pr::Open {
            kind: zoraya_protocol::Below(kind),
            room: Text(room),
        })?;
        match (decode::<pr::Opened>(&bytes, 1), handle) {
            (Ok(_), Some(h)) => Ok(h),
            _ => Err(Self::denied(&bytes)),
        }
    }

    /// Each running room's use (`room` empty: all).
    pub fn usage(&mut self, room: &str) -> Result<Vec<pr::UsageRecord>, Fail> {
        let (bytes, handle) = self.call(&pr::Usage { room: Text(room) })?;
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        match decode::<pr::UsageList<'_>>(&bytes, 0) {
            Ok((_, u)) => Ok(u
                .records
                .0
                .chunks(pr::RECORD_SIZE)
                .filter_map(pr::UsageRecord::decode)
                .collect()),
            Err(_) => Err(Self::denied(&bytes)),
        }
    }

    /// The refusals that arrived since the last call, without waiting.
    pub fn take_refusals(&mut self) -> Vec<Refusal> {
        self.pump();
        core::mem::take(&mut self.refusals)
    }

    /// Reads what the keeper sent, without waiting; when its channel has
    /// closed (a restarted keeper), binds to the new one now. A shell calls
    /// it before each prompt.
    pub fn pump(&mut self) {
        loop {
            let mut buf = [0u8; 256];
            let mut handles = [self.end; 1];
            match channel_read(self.end, &mut buf, &mut handles) {
                Ok(s) => {
                    if s.handles > 0 {
                        let _ = handle_close(handles[0]);
                    }
                    drain_note(&buf[..(s.bytes as usize).min(256)], &mut self.refusals);
                }
                Err((Status::WouldBlock, _)) => break,
                Err(_) => {
                    // The keeper's end is closed: bind to the next keeper
                    // now, before the next command, not at the call that
                    // fails (the keeper's idle collection runs every turn).
                    let _ = self.rebind();
                    break;
                }
            }
        }
    }
}
