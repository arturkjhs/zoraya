#![no_std]
#![no_main]

//! The terminal service (ADR-0105) as a process, over `zoraya-tty`'s logic.
//! `init` starts it before the broker with the backend, the server ends of
//! the broker's control connection (slot 6) and of the session's first
//! terminal connection (slot 7), and the read end of the output stream
//! (slot 8), whose write end goes to the broker for the session's processes.
//!
//! Backends (ADR-0105 items 9 and 10): the serial console at slot 1 with
//! `READ | WRITE | WAIT`, or with `--window` the stage 5 terminal's channel
//! at slot 5, speaking `zoraya-proto-term` as the shell side does, with the
//! console at slot 1 for this service's own reports.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec;
use alloc::vec::Vec;

use zoraya_abi::{
    CONSOLE, DEADLINE_INFINITE, Handle, Mode, Packet, Status, channel_signals, console_signals,
    stream_signals,
};
use zoraya_proto_term as term;
use zoraya_proto_tty as tp;
use zoraya_protocol::{Header, MAX_WIRE_MESSAGE, Message, NO_REPLY, decode, encode};
use zoraya_tty::{Action, Id, Terminal};
use zoraya_user::{
    channel_read, channel_write, console_read, console_write, handle_close, job_signal,
    port_create, port_subscribe, port_unsubscribe, port_wait, println, startup, stream_read,
};

const WINDOW: u32 = 5;
const CONTROL: u32 = 6;
const FIRST: u32 = 7;
const OUTPUT: u32 = 8;

const KEY_BACKEND: u64 = 1;
const KEY_OUTPUT: u64 = 2;
const KEY_CONN: u64 = 1 << 32;

fn slot(n: u32) -> Handle {
    Handle::from_parts(n, 0).expect("a slot")
}

/// Where the terminal's bytes come from and go.
enum Backend {
    Serial,
    Window(Handle),
}

impl Backend {
    /// Shows `bytes`, all of them.
    fn show(&self, bytes: &[u8]) {
        match self {
            Backend::Serial => {
                let mut at = 0;
                while at < bytes.len() {
                    match console_write(CONSOLE, &bytes[at..]) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => at += n as usize,
                    }
                }
            }
            Backend::Window(end) => {
                let mut out = vec![0u8; term::MAX_OUTPUT + 64];
                for part in bytes.chunks(term::MAX_OUTPUT) {
                    let m = term::Output {
                        bytes: zoraya_protocol::Data(part),
                    };
                    let Ok(n) = encode(&m, NO_REPLY, &mut out) else {
                        return;
                    };
                    loop {
                        match channel_write(*end, &out[..n], &[]) {
                            Ok(()) => break,
                            Err(Status::WouldBlock) => wait_writable(*end),
                            Err(_) => return,
                        }
                    }
                }
            }
        }
    }
}

fn wait_writable(end: Handle) {
    let Ok(port) = port_create() else { return };
    let wanted = channel_signals::WRITABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, end, wanted, 1, Mode::Level).is_ok() {
        let mut packets = [Packet::default()];
        let _ = port_wait(port, DEADLINE_INFINITE, &mut packets);
    }
    let _ = handle_close(port);
}

struct Service {
    port: Handle,
    term: Terminal<Handle>,
    ends: BTreeMap<Id, Handle>,
    control: Handle,
    control_id: Id,
    backend: Backend,
}

impl Service {
    fn watch(&mut self, id: Id, end: Handle) {
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        if port_subscribe(self.port, end, wanted, KEY_CONN + id as u64, Mode::Level).is_ok() {
            self.ends.insert(id, end);
        } else {
            self.term.closed(id);
            let _ = handle_close(end);
        }
    }

    fn act(&mut self, actions: Vec<Action<Handle>>) {
        for a in actions {
            match a {
                Action::Reply { to, bytes } => {
                    if let Some(&end) = self.ends.get(&to) {
                        let _ = channel_write(end, &bytes, &[]);
                    }
                }
                Action::Show(bytes) => self.backend.show(&bytes),
                Action::Signal { job, kind, detail } => {
                    let _ = job_signal(job, kind, detail);
                }
                Action::Suspend { group, signal } => {
                    let mut out = [0u8; 64];
                    if let Ok(n) = encode(&tp::Suspend { signal, group }, NO_REPLY, &mut out) {
                        let _ = channel_write(self.control, &out[..n], &[]);
                    }
                }
                Action::Watch { id, end } => self.watch(id, end),
                Action::Close(h) => {
                    let _ = handle_close(h);
                }
            }
        }
    }

    /// A message on connection `id`, or its end.
    fn connection(&mut self, id: Id, bytes: &mut [u8]) {
        let Some(&end) = self.ends.get(&id) else {
            return;
        };
        let mut got = [end; 4];
        match channel_read(end, bytes, &mut got) {
            Ok(sizes) => {
                let handles = got[..sizes.handles as usize].to_vec();
                let actions = self
                    .term
                    .request(id, &bytes[..sizes.bytes as usize], handles);
                self.act(actions);
            }
            Err((Status::WouldBlock, _)) => {}
            Err((Status::BufferTooSmall, _)) => {
                // Bigger than any request: the peer is not speaking the
                // protocol.
                self.drop_connection(id, end);
            }
            Err(_) => self.drop_connection(id, end),
        }
    }

    fn drop_connection(&mut self, id: Id, end: Handle) {
        let _ = port_unsubscribe(self.port, KEY_CONN + id as u64);
        let _ = handle_close(end);
        self.ends.remove(&id);
        self.term.closed(id);
        if id == self.control_id {
            // The broker is gone: the session is over, and with it the
            // foreground.
            println!("ttyd: the broker's connection closed");
        }
    }

    /// Input from the backend; `false` when it is gone.
    fn backend(&mut self, bytes: &mut [u8]) -> bool {
        match self.backend {
            Backend::Serial => {
                let n = console_read(CONSOLE, bytes).unwrap_or(0);
                if n > 0 {
                    let actions = self.term.keys(&bytes[..n]);
                    self.act(actions);
                }
                true
            }
            Backend::Window(end) => {
                let mut got = [end; 1];
                match channel_read(end, bytes, &mut got) {
                    Ok(sizes) => {
                        let n = sizes.bytes as usize;
                        let ordinal = Header::decode(&bytes[..n]).map_or(0, |(h, _)| h.ordinal);
                        if ordinal == term::Input::ORDINAL {
                            if let Ok((_, m)) = decode::<term::Input<'_>>(&bytes[..n], 0) {
                                let keys = m.bytes.0.to_vec();
                                let actions = self.term.keys(&keys);
                                self.act(actions);
                            }
                        } else if ordinal == term::Size::ORDINAL
                            && let Ok((_, m)) = decode::<term::Size>(&bytes[..n], 0)
                        {
                            let actions = self.term.resize(m.rows, m.columns);
                            self.act(actions);
                        }
                        true
                    }
                    Err((Status::WouldBlock, _)) => true,
                    Err(_) => false,
                }
            }
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let window = startup().argv.any(|a| a == b"--window");
    let backend = if window {
        Backend::Window(slot(WINDOW))
    } else {
        Backend::Serial
    };
    let port = port_create().expect("port");
    let mut term: Terminal<Handle> = Terminal::new();
    let control_id = term.add_control().expect("room");
    let first_id = term.add_terminal(0).expect("room");
    let control = slot(CONTROL);
    let output = slot(OUTPUT);
    let mut s = Service {
        port,
        term,
        ends: BTreeMap::new(),
        control,
        control_id,
        backend,
    };
    s.watch(control_id, control);
    s.watch(first_id, slot(FIRST));
    let backend_signals = match s.backend {
        Backend::Serial => (CONSOLE, console_signals::READABLE),
        Backend::Window(end) => (
            end,
            channel_signals::READABLE | channel_signals::PEER_CLOSED,
        ),
    };
    port_subscribe(
        port,
        backend_signals.0,
        backend_signals.1,
        KEY_BACKEND,
        Mode::Level,
    )
    .expect("subscribe the backend");
    let out_signals = stream_signals::READABLE | stream_signals::PEER_CLOSED;
    port_subscribe(port, output, out_signals, KEY_OUTPUT, Mode::Level).expect("subscribe");
    let mut bytes = vec![0u8; MAX_WIRE_MESSAGE];
    let mut shown = Vec::with_capacity(8192);
    let mut hung_up = false;
    loop {
        let mut packets = [Packet::default()];
        if port_wait(port, DEADLINE_INFINITE, &mut packets).is_err() {
            return 1;
        }
        match packets[0].key {
            KEY_BACKEND => {
                if !s.backend(&mut bytes) && !hung_up {
                    hung_up = true;
                    let _ = port_unsubscribe(port, KEY_BACKEND);
                    let actions = s.term.hangup();
                    s.act(actions);
                }
            }
            KEY_OUTPUT => match stream_read(output, &mut bytes[..4096]) {
                Ok(n) if n > 0 => {
                    shown.clear();
                    s.term.output(&bytes[..n], &mut shown);
                    s.backend.show(&shown);
                }
                Ok(_) | Err(Status::WouldBlock) => {}
                // Every writer gone: nothing more to show.
                Err(_) => {
                    let _ = port_unsubscribe(port, KEY_OUTPUT);
                }
            },
            k if k >= KEY_CONN => s.connection((k - KEY_CONN) as Id, &mut bytes),
            _ => {}
        }
    }
}
