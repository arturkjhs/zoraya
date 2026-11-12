//! The GUI stack as `init` starts it (ADR-0097), and the device scan of
//! ADR-0091. A test starts the same one.

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{
    ChannelEntry, ChannelObject, DEADLINE_INFINITE, FIRST_DEVICE_SLOT, Handle, Mode, ObjectType,
    Packet, channel_signals,
};
use zoraya_proto_gui as gui;
use zoraya_protocol::{Message, decode, encode};
use zoraya_rt::{
    channel_create, channel_read, channel_write, clock_monotonic, device_map, device_read,
    device_transport, handle_close, handle_type, memory_unmap, port_create, port_subscribe,
    port_wait, println,
};
use zoraya_virtio::queue::{Page, Transport, identify};
use zoraya_virtio::{DEVICE_BLOCK, DEVICE_GPU, DEVICE_INPUT, DEVICE_NET, reg};

use crate::dup;
use crate::stack::{console, driver_rights, end_job, moved, start};

/// Restarts allowed for each driver, and for the session (ADR-0097).
pub const MAX_RESTARTS: u32 = 3;

/// The devices `init` can give a driver: one per page (ADR-0091).
#[derive(Default)]
pub struct Devices {
    pub disk: Option<(Handle, u32)>,
    pub gpu: Option<(Handle, u32)>,
    pub inputs: Vec<(Handle, u32)>,
    /// The network device (ADR-0221 item 4), on its own page.
    pub net: Option<(Handle, u32)>,
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

/// Reads transport `range` of a shared page through its transport device
/// (ADR-0295).
fn read_transport(device: Handle, range: u32) -> Option<(Handle, Transport)> {
    let transport = device_transport(device, range).ok()?;
    let register = |at: usize| device_read(transport, at as u64).unwrap_or(0);
    let read = Transport {
        range: 0,
        magic: register(reg::MAGIC),
        version: register(reg::VERSION),
        device_id: register(reg::DEVICE_ID),
    };
    Some((transport, read))
}

/// Puts a device `init` found, as `(handle, range)`, with the drivers that
/// want its kind.
fn classify(found: &mut Devices, device: (Handle, u32), device_id: u32) -> bool {
    match device_id {
        DEVICE_BLOCK if found.disk.is_none() => found.disk = Some(device),
        DEVICE_GPU if found.gpu.is_none() => found.gpu = Some(device),
        DEVICE_INPUT => found.inputs.push(device),
        DEVICE_NET if found.net.is_none() => found.net = Some(device),
        _ => return false,
    }
    true
}

/// Every device page this process holds, sorted by what it is. A page with
/// one device goes to its driver whole, mapped; a page with several is
/// never given whole: each of its devices goes to its driver as a transport
/// device that the driver reaches by register calls (ADR-0295, ADR-0221
/// item 8), so the drivers on the page cannot reach one another's registers.
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
            Page::One { range, device_id } => {
                classify(&mut found, (device, range), device_id);
            }
            Page::Several => {
                let mut used = 0;
                for range in 0..8u32 {
                    let Some((transport, read)) = read_transport(device, range) else {
                        break;
                    };
                    let kept = match identify(&[read]) {
                        Page::One { range, device_id } => {
                            classify(&mut found, (transport, range), device_id)
                        }
                        _ => false,
                    };
                    if kept {
                        used += 1;
                    } else {
                        let _ = handle_close(transport);
                    }
                }
                println!(
                    "init: device slot {slot}: shared page, {used} device(s) by register calls"
                );
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
    let entries = [
        console(),
        dup(5, device.0, driver_rights(device.0)),
        moved(6, theirs),
    ];
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

/// Set when the compositor reported a hung display driver (ADR-0093,
/// Decision), however the report was read.
static DISPLAY_HUNG: AtomicBool = AtomicBool::new(false);

/// True once after the compositor reported a hung display driver.
pub fn take_display_hung() -> bool {
    DISPLAY_HUNG.swap(false, Ordering::Relaxed)
}

/// Reads what the compositor sent the manager unasked, without blocking.
pub fn read_manager_events(manager: Handle) {
    let mut bytes = [0u8; 64];
    let mut handles = [manager; 1];
    while let Ok(sizes) = channel_read(manager, &mut bytes, &mut handles) {
        for h in &handles[..sizes.handles as usize] {
            let _ = handle_close(*h);
        }
        note_event(&bytes[..sizes.bytes as usize]);
    }
}

fn note_event(bytes: &[u8]) {
    if decode::<gui::DisplayHung>(bytes, 0).is_ok() {
        DISPLAY_HUNG.store(true, Ordering::Relaxed);
    }
}

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
    let mut out = [0u8; 512];
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
        // An event, or a reply to an earlier request: noted, not lost.
        note_event(&reply[..n]);
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
    /// Starts the drivers, then the compositor with their channels and
    /// `config`, `/config` to read its settings from (ADR-0169), at slot 11.
    pub fn start(
        gpu: (Handle, u32),
        inputs: &[(Handle, u32)],
        config: Option<Handle>,
    ) -> Option<Gui> {
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
        for (i, end) in ends.iter().take(4).enumerate() {
            entries.push(moved(7 + i as u32, *end));
        }
        if let Some(c) = config {
            entries.push(moved(11, c));
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

    /// A terminal's client in `home`, with its launch record: the shell
    /// in it holds the volume's root (ADR-0168, ADR-0172 point 4).
    pub fn new_terminal_client(&self) -> Option<Handle> {
        let (ours, theirs) = channel_create().ok()?;
        // What its shell holds: home's grants (ADR-0242's condition).
        let entries = zoraya_panel::terminal(None).entries.join("\n");
        let m = gui::AddRoomClient {
            untrusted: 0,
            colour: 0,
            tag: 0,
            room: zoraya_protocol::Data(b""),
            app: zoraya_protocol::Data(b"term"),
            entries: zoraya_protocol::Data(entries.as_bytes()),
            line: zoraya_protocol::Data(b"term"),
        };
        manager_send(self.manager, &m, Some(theirs), 0, &mut [])?;
        Some(ours)
    }

    /// The panel (ADR-0168): its client manager, the volume's root and the
    /// console; `None` if the compositor refuses or the panel does not
    /// start.
    /// Starts the chooser (ADR-0167, proposed ADR-0245): the volume's
    /// root, a compositor channel the driver manager marks as a system
    /// client's, and its end of the channel the panel registers
    /// applications on; gives the panel's end.
    pub fn start_chooser(&self, root: &crate::files::File) -> Option<(Running, Handle)> {
        let (ours, theirs) = channel_create().ok()?;
        if !attached(self.manager, &gui::AddSystemClient {}, theirs) {
            let _ = handle_close(ours);
            return None;
        }
        let Ok((panel, chooser)) = channel_create() else {
            let _ = handle_close(ours);
            return None;
        };
        let Ok(root) = root.clone_as(crate::files::ALL) else {
            let _ = handle_close(ours);
            let _ = handle_close(panel);
            let _ = handle_close(chooser);
            return None;
        };
        let entries = [
            console(),
            moved(5, root.end),
            moved(6, ours),
            moved(7, chooser),
        ];
        match start("chooser", &entries, &[]) {
            Some((job, process)) => {
                // Moved into the chooser with the spawn.
                core::mem::forget(root);
                Some((Running { job, process }, panel))
            }
            None => {
                let _ = handle_close(panel);
                None
            }
        }
    }

    pub fn start_panel(
        &self,
        root: &crate::files::File,
        chooser: Option<Handle>,
        keeper: Option<(Handle, Handle)>,
    ) -> Option<Running> {
        let (ours, theirs) = channel_create().ok()?;
        if !attached(self.manager, &gui::AttachClientManager {}, theirs) {
            let _ = handle_close(ours);
            return None;
        }
        let Ok(root) = root.clone_as(crate::files::ALL) else {
            let _ = handle_close(ours);
            return None;
        };
        // Its own job, to make its launches' jobs under it, and the image
        // directory, to find what it launches.
        let started = (|| {
            use zoraya_abi::{DIRECTORY, DirectoryObject, JOB, JobFlags, JobObject};
            let job = zoraya_rt::job_create(JOB, JobFlags::NONE).ok()?;
            let Ok(image) = zoraya_rt::directory_lookup(DIRECTORY, "panel") else {
                let _ = zoraya_rt::job_kill(job);
                let _ = handle_close(job);
                return None;
            };
            let mut buffer = [0; 512];
            let mut e = zoraya_abi::spawn::Encoder::new(&mut buffer, job, image);
            // The console to write and to hand to what it launches (to
            // write only, INV-0097-3), as init's own shell has it.
            let console = zoraya_abi::Rights::WRITE.union(zoraya_abi::Rights::DUPLICATE);
            for entry in [
                dup(zoraya_abi::CONSOLE.slot(), zoraya_abi::CONSOLE, console),
                dup(JOB.slot(), job, JobObject::RIGHTS),
                dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS),
                moved(5, root.end),
                moved(6, ours),
            ] {
                e.handle(entry);
            }
            // The chooser's registration channel (proposed ADR-0245).
            if let Some(c) = chooser {
                e.handle(moved(7, c));
            }
            // The room keeper's `Manage` channel and the way to a new one
            // (ADR-0269 point 3).
            if let Some((manage, reconnect)) = keeper {
                e.handle(moved(8, manage));
                e.handle(moved(9, reconnect));
            }
            let process = e
                .finish()
                .and_then(|length| zoraya_rt::spawn(&buffer[..length]).ok());
            let _ = handle_close(image);
            if process.is_none() {
                let _ = zoraya_rt::job_kill(job);
                let _ = handle_close(job);
            }
            Some((job, process?))
        })();
        match started {
            Some((job, process)) => {
                // Moved into the panel with the spawn.
                core::mem::forget(root);
                Some(Running { job, process })
            }
            None => {
                // Not taken: closed here (the root with its drop).
                let _ = handle_close(ours);
                if let Some((manage, reconnect)) = keeper {
                    let _ = handle_close(manage);
                    let _ = handle_close(reconnect);
                }
                None
            }
        }
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
    let client = gui.new_terminal_client()?;
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
