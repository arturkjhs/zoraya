//! The GUI stack as `init` starts it (ADR-0097), and the device scan of
//! ADR-0091. A test starts the same one.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, FIRST_DEVICE_SLOT, Handle, Mode, ObjectType,
    Packet, channel_signals,
};
use zoraya_proto_gui as gui;
use zoraya_protocol::{Message, decode, encode};
use zoraya_rt::{
    channel_create, channel_read, channel_write, clock_monotonic, device_map, handle_close,
    handle_type, memory_unmap, port_create, port_subscribe, port_wait, println,
};
use zoraya_virtio::queue::{Page, Transport, identify};
use zoraya_virtio::{DEVICE_BLOCK, DEVICE_GPU, DEVICE_INPUT, reg};

use crate::dup;
use crate::stack::{DRIVER_RIGHTS, console, end_job, moved, start};

/// Restarts allowed for each driver, and for the session (ADR-0097).
pub const MAX_RESTARTS: u32 = 3;

/// The devices `init` can give a driver: one per page (ADR-0091).
#[derive(Default)]
pub struct Devices {
    pub disk: Option<(Handle, u32)>,
    pub gpu: Option<(Handle, u32)>,
    pub inputs: Vec<(Handle, u32)>,
}

/// Reads every transport of a device page, one mapping at a time.
fn read_page(device: Handle) -> Page {
    let mut transports = [Transport {
        range: 0,
        magic: 0,
        version: 0,
        device_id: 0,
    }; 8];
    let mut count = 0;
    for range in 0..8u32 {
        let Ok((window, offset)) = device_map(device, range) else {
            break;
        };
        let register = |at: usize| {
            // SAFETY: a device mapping of this process, at a register of
            // the range's transport, which the kernel checked holds one.
            unsafe { ((window + offset + at as u64) as *const u32).read_volatile() }
        };
        transports[count] = Transport {
            range,
            magic: register(reg::MAGIC),
            version: register(reg::VERSION),
            device_id: register(reg::DEVICE_ID),
        };
        count += 1;
        let _ = memory_unmap(window, 4096);
    }
    identify(&transports[..count])
}

/// Every device page this process holds, sorted by what it is.
pub fn scan() -> Devices {
    let mut found = Devices::default();
    for slot in FIRST_DEVICE_SLOT..FIRST_DEVICE_SLOT + 16 {
        let Some(device) = Handle::new(u64::from(slot)) else {
            continue;
        };
        if handle_type(device) != Ok("device") {
            continue;
        }
        match read_page(device) {
            Page::One { range, device_id } => match device_id {
                DEVICE_BLOCK if found.disk.is_none() => found.disk = Some((device, range)),
                DEVICE_GPU if found.gpu.is_none() => found.gpu = Some((device, range)),
                DEVICE_INPUT => found.inputs.push((device, range)),
                _ => {}
            },
            Page::Several => {
                println!("init: device slot {slot}: more than one device on a page, not used");
            }
            Page::Empty => {}
        }
    }
    found
}

/// A process in its sub-job, and the channel to it.
#[derive(Clone, Copy)]
pub struct Running {
    pub job: Handle,
    pub process: Handle,
}

impl Running {
    pub fn end(self) {
        end_job((self.job, self.process));
    }
}

/// Starts driver `name` on `device`, with a channel at slot 6; gives the
/// process and the other end.
pub fn start_driver(name: &str, device: (Handle, u32)) -> Option<(Running, Handle)> {
    let (ours, theirs) = channel_create().ok()?;
    let digits = [b'0' + device.1 as u8];
    let entries = [console(), dup(5, device.0, DRIVER_RIGHTS), moved(6, theirs)];
    match start(name, &entries, &[&digits]) {
        Some((job, process)) => Some((Running { job, process }, ours)),
        None => {
            let _ = handle_close(ours);
            None
        }
    }
}

fn channel(handle: Handle) -> ChannelEntry {
    ChannelEntry {
        handle,
        rights: ChannelObject::RIGHTS,
        transfer: Transfer::Move,
    }
}

/// How long the manager waits for the compositor: its attach of a new
/// display driver makes two driver calls of up to 2 s each (ADR-0093).
const MANAGER_WAIT_NS: u64 = 6_000_000_000;

/// Transactions of the manager channel, each used once, so that a late
/// reply is never taken for the reply to a later request.
static NEXT_TRANSACTION: AtomicU32 = AtomicU32::new(1);

/// Sends a message on the manager channel. With a `transaction` that is not
/// 0, the message gets a fresh one, and the call waits up to six seconds for
/// the reply that carries it, skipping replies to earlier requests.
pub fn manager_send<'a, M: Message<'a>>(
    manager: Handle,
    m: &M,
    handle: Option<Handle>,
    transaction: u32,
    reply: &mut [u8],
) -> Option<usize> {
    let transaction = if transaction == 0 {
        0
    } else {
        NEXT_TRANSACTION.fetch_add(1, Ordering::Relaxed).max(1)
    };
    let mut out = [0u8; 64];
    let length = encode(m, transaction, &mut out).ok()?;
    let entries: Vec<ChannelEntry> = handle.into_iter().map(channel).collect();
    if channel_write(manager, &out[..length], &entries).is_err() {
        if let Some(h) = handle {
            let _ = handle_close(h);
        }
        return None;
    }
    if transaction == 0 {
        return Some(0);
    }
    let until = clock_monotonic() + MANAGER_WAIT_NS;
    loop {
        let left = until.checked_sub(clock_monotonic())?;
        let n = wait_reply(manager, reply, left)?;
        let header = zoraya_protocol::Header::decode(&reply[..n]).ok()?.0;
        if header.transaction == transaction {
            return Some(n);
        }
    }
}

/// Waits for one message on `end`, until `nanos` from now.
pub fn wait_reply(end: Handle, reply: &mut [u8], nanos: u64) -> Option<usize> {
    let port = port_create().ok()?;
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, end, wanted, 1, Mode::Level).ok()?;
    let mut packets = [Packet::default()];
    let waited = port_wait(port, clock_monotonic() + nanos, &mut packets);
    let _ = handle_close(port);
    waited.ok()?;
    let mut handles = [end; 1];
    let sizes = channel_read(end, reply, &mut handles).ok()?;
    for h in &handles[..sizes.handles as usize] {
        let _ = handle_close(*h);
    }
    Some(sizes.bytes as usize)
}

/// Gives the compositor a driver's channel and sees it accepted.
fn attached<'a, M: Message<'a>>(manager: Handle, m: &M, end: Handle) -> bool {
    let mut reply = [0u8; 64];
    manager_send(manager, m, Some(end), 1, &mut reply)
        .and_then(|n| manager_reply::<gui::ManagerDone>(&reply[..n]))
        .is_some_and(|d| d.status.0 == gui::status::OK)
}

/// The GUI's processes and `init`'s end of the compositor's manager
/// channel.
pub struct Gui {
    pub gpu: (Handle, u32),
    pub input_devices: Vec<(Handle, u32)>,
    pub manager: Handle,
    pub gpud: Option<Running>,
    pub inputd: Vec<Option<Running>>,
    pub compd: Running,
    pub gpu_restarts: u32,
    pub input_restarts: Vec<u32>,
}

impl Gui {
    /// Starts the drivers, then the compositor with their channels.
    pub fn start(gpu: (Handle, u32), inputs: &[(Handle, u32)]) -> Option<Gui> {
        let (gpud, display) = start_driver("gpud", gpu)?;
        let mut inputd = Vec::new();
        let mut ends = Vec::new();
        for device in inputs {
            match start_driver("inputd", *device) {
                Some((running, end)) => {
                    inputd.push(Some(running));
                    ends.push(end);
                }
                None => inputd.push(None),
            }
        }
        // On failure the drivers started so far are ended: nothing else
        // would end them (review R7).
        let give_up = |gpud: Running, inputd: Vec<Option<Running>>| {
            gpud.end();
            inputd.into_iter().flatten().for_each(Running::end);
        };
        let Ok((manager, theirs)) = channel_create() else {
            let _ = handle_close(display);
            ends.iter().for_each(|e| {
                let _ = handle_close(*e);
            });
            give_up(gpud, inputd);
            return None;
        };
        let mut entries = alloc::vec![console(), moved(5, theirs), moved(6, display)];
        for (i, end) in ends.iter().enumerate() {
            entries.push(moved(7 + i as u32, *end));
        }
        let Some((job, process)) = start("compd", &entries, &[]) else {
            let _ = handle_close(manager);
            give_up(gpud, inputd);
            return None;
        };
        Some(Gui {
            gpu,
            input_devices: inputs.to_vec(),
            manager,
            gpud: Some(gpud),
            inputd,
            compd: Running { job, process },
            gpu_restarts: 0,
            input_restarts: alloc::vec![0; inputs.len()],
        })
    }

    /// A new client connection: the client's end.
    pub fn new_client(&self) -> Option<Handle> {
        let (ours, theirs) = channel_create().ok()?;
        manager_send(self.manager, &gui::AddClient {}, Some(theirs), 0, &mut [])?;
        Some(ours)
    }

    /// Starts the GPU driver again and gives it to the compositor.
    pub fn restart_gpu(&mut self) -> bool {
        if let Some(old) = self.gpud.take() {
            old.end();
        }
        if self.gpu_restarts >= MAX_RESTARTS {
            println!("init: the GPU driver ended: no more restarts");
            return false;
        }
        self.gpu_restarts += 1;
        let Some((running, end)) = start_driver("gpud", self.gpu) else {
            return false;
        };
        self.gpud = Some(running);
        attached(self.manager, &gui::AttachDisplay {}, end)
    }

    /// Starts input driver `i` again and gives it to the compositor.
    pub fn restart_input(&mut self, i: usize) -> bool {
        if let Some(old) = self.inputd[i].take() {
            old.end();
        }
        if self.input_restarts[i] >= MAX_RESTARTS {
            println!("init: an input driver ended: no more restarts");
            return false;
        }
        self.input_restarts[i] += 1;
        let Some((running, end)) = start_driver("inputd", self.input_devices[i]) else {
            return false;
        };
        self.inputd[i] = Some(running);
        attached(self.manager, &gui::AttachInput {}, end)
    }

    /// Ends every GUI process.
    pub fn end(self) {
        self.compd.end();
        if let Some(g) = self.gpud {
            g.end();
        }
        for r in self.inputd.into_iter().flatten() {
            r.end();
        }
        let _ = handle_close(self.manager);
    }
}

/// Starts a terminal as a client of `gui`: the terminal process, and the
/// shell's end of the terminal channel, for slot 6 of the shell.
pub fn start_terminal(gui: &Gui) -> Option<(Running, Handle)> {
    let client = gui.new_client()?;
    let (shell, theirs) = channel_create().ok()?;
    let entries = [console(), moved(5, client), moved(6, theirs)];
    match start("term", &entries, &[]) {
        Some((job, process)) => Some((Running { job, process }, shell)),
        None => {
            let _ = handle_close(shell);
            None
        }
    }
}

/// Reads one reply of type `M` from a manager call, for tests.
pub fn manager_reply<M: for<'b> Message<'b>>(bytes: &[u8]) -> Option<M> {
    decode::<M>(bytes, 0).ok().map(|(_, m)| m)
}

/// Blocks until `end` has a message or closes.
pub fn wait_readable(end: Handle) {
    let Ok(port) = port_create() else { return };
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    if port_subscribe(port, end, wanted, 1, Mode::Level).is_ok() {
        let mut packets = [Packet::default()];
        let _ = port_wait(port, DEADLINE_INFINITE, &mut packets);
    }
    let _ = handle_close(port);
}
