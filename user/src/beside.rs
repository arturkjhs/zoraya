//! The broker beside a native shell (ADR-0153 rule 10): started by `init`
//! for its first shell, and by the panel for each terminal it launches, so
//! that a shell in a room runs POSIX programs (ADR-0172's decision of
//! 2026-11-09). It holds the volume's root for its temporary service; the
//! shell gets only a client connection and the temporary root, and the
//! broker ends when that client closes and nothing it started runs.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, ChannelObject, DEADLINE_INFINITE, DIRECTORY, DirectoryObject, Handle, JOB, JobFlags,
    JobObject, Mode, ObjectType, Packet, Rights, channel_signals, process_signals,
};
use zoraya_rt::{
    channel_create, channel_read, directory_lookup, handle_close, job_create, job_kill,
    port_create, port_subscribe, port_wait, println, spawn,
};

use crate::dup;
use crate::files::{ALL, File};
use crate::stack::{end_job, moved};

fn give(slot: u32, handle: Handle, rights: Rights) -> HandleEntry {
    HandleEntry {
        slot,
        handle,
        rights,
        transfer: Transfer::Move,
    }
}

/// The broker beside `zosh` (ADR-0153 rule 10): no first shell, the root
/// for its temporary service, and at slot 9 a channel on which it sends
/// `zosh`'s client connection and the temporary service's root. Only on a
/// volume with `/bin`, where the POSIX programs are; it ends with its shell.
pub struct Beside {
    pub job: (Handle, Handle),
    pub client: Handle,
    pub tmp: Handle,
}

pub fn start_broker(root: &File) -> Option<Beside> {
    start_broker_in(root, JOB)
}

/// As [`start_broker`], with its job under `parent`: a room's launch job
/// (ADR-0269, INV-0268-3), so that the broker counts against the room's
/// limits.
pub fn start_broker_in(root: &File, parent: Handle) -> Option<Beside> {
    let wanted = zoraya_proto_file::rights::READ;
    root.open("bin", zoraya_proto_file::open_flags::DIRECTORY, wanted)
        .ok()?;
    let base = root.time_base().unwrap_or(0);
    let (handoff, handoff_broker) = channel_create().ok()?;
    // Every way out below closes what was made before it (review K6).
    let Ok(job) = job_create(parent, JobFlags::POSIX) else {
        let _ = handle_close(handoff);
        let _ = handle_close(handoff_broker);
        return None;
    };
    let Some(broker) = spawn_broker(root, base, job, handoff_broker) else {
        let _ = handle_close(handoff);
        let _ = handle_close(handoff_broker);
        let _ = job_kill(job);
        let _ = handle_close(job);
        return None;
    };
    // The hand-off: `ZPBC`, the client connection and the temporary root.
    // No deadline (review K7): a broker that ends without one is seen by its
    // `TERMINATED`, and one that is merely slow under TCG is waited for.
    let got = match port_create() {
        Ok(port) => {
            let readable = channel_signals::READABLE | channel_signals::PEER_CLOSED;
            let terminated = process_signals::TERMINATED;
            let subscribed = port_subscribe(port, handoff, readable, 1, Mode::Level)
                .and_then(|()| port_subscribe(port, broker, terminated, 2, Mode::Level));
            let mut packets = [Packet::default()];
            let waited = subscribed
                .and_then(|()| port_wait(port, DEADLINE_INFINITE, &mut packets).map(|_| ()));
            let _ = handle_close(port);
            let mut bytes = [0u8; 16];
            let mut handles = [handoff; 2];
            waited.ok().and_then(|()| {
                let s = channel_read(handoff, &mut bytes, &mut handles).ok()?;
                if &bytes[..s.bytes as usize] == b"ZPBC" && s.handles == 2 {
                    return Some((handles[0], handles[1]));
                }
                for h in &handles[..s.handles as usize] {
                    let _ = handle_close(*h);
                }
                None
            })
        }
        Err(_) => None,
    };
    let _ = handle_close(handoff);
    match got {
        Some((client, tmp)) => Some(Beside {
            job: (job, broker),
            client,
            tmp,
        }),
        None => {
            println!("the broker gave no client connection");
            end_job((job, broker));
            None
        }
    }
}

/// Spawns the broker in `job`, with the hand-off end at slot 9; the image and
/// every handle not handed over are closed whatever happens.
fn spawn_broker(root: &File, base: u64, job: Handle, handoff: Handle) -> Option<Handle> {
    let image = directory_lookup(DIRECTORY, "broker").ok()?;
    let spawned = root.clone_as(ALL).ok().and_then(|root| {
        let mut buffer = [0; 512];
        let mut e = Encoder::new(&mut buffer, job, image);
        e.handle(dup(
            CONSOLE.slot(),
            CONSOLE,
            Rights::WRITE.union(Rights::DUPLICATE),
        ));
        e.handle(dup(JOB.slot(), job, JobObject::RIGHTS));
        e.handle(dup(DIRECTORY.slot(), DIRECTORY, DirectoryObject::RIGHTS));
        e.handle(give(5, root.end, ChannelObject::RIGHTS));
        e.handle(moved(9, handoff));
        let base = alloc::format!("{base}");
        for a in [
            &b"broker"[..],
            b"--time-base",
            base.as_bytes(),
            b"--no-shell",
        ] {
            e.arg(a);
        }
        let length = e.finish()?;
        let broker = spawn(&buffer[..length]).ok()?;
        // Moved into the broker with the spawn.
        core::mem::forget(root);
        Some(broker)
    });
    let _ = handle_close(image);
    spawned
}
