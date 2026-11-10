#![no_std]
#![no_main]

//! Helpers of `t_gui`.
//!
//! - `guibench scribble` maps the memory object at slot 5 writable and
//!   rewrites it without pause, as a client that changes its buffer while
//!   the compositor copies it (ADR-0093).
//! - `guibench hang-display` is a display driver on the channel at slot 5
//!   that answers `Info` and `Attach` and never a `Flush`: a hung driver
//!   (ADR-0093, Decision).

use zoraya_abi::{DEADLINE_INFINITE, Handle, MapFlags, Mode, Packet, channel_signals};
use zoraya_proto_display::{Done, FORMAT_XRGB, InfoReply, status};
use zoraya_protocol::{Below, Header, encode};
use zoraya_user::{
    channel_read, channel_write, handle_close, memory_map, port_create, port_subscribe, port_wait,
    startup,
};

fn scribble() -> ! {
    let memory = Handle::new(5).expect("slot");
    let size = 256 * 256 * 4;
    let at = memory_map(memory, 0, size, MapFlags::WRITE).expect("map");
    let mut value = 0u32;
    loop {
        for i in 0..(size / 4) as usize {
            // SAFETY: inside a writable mapping of `size` bytes of this
            // process; another process reads it, which is the point.
            unsafe { ((at as *mut u32).add(i)).write_volatile(value) };
            value = value.wrapping_add(0x0001_0203);
        }
    }
}

fn hang_display() -> i32 {
    let end = Handle::new(5).expect("slot");
    let port = port_create().expect("port");
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    port_subscribe(port, end, wanted, 1, Mode::Level).expect("subscribe");
    let mut message = [0u8; 256];
    let mut out = [0u8; 64];
    loop {
        let mut packets = [Packet::default()];
        if port_wait(port, DEADLINE_INFINITE, &mut packets).is_err() {
            return 1;
        }
        let mut handles = [end; 1];
        let Ok(sizes) = channel_read(end, &mut message, &mut handles) else {
            // The compositor dropped it.
            return 0;
        };
        for h in &handles[..sizes.handles as usize] {
            let _ = handle_close(*h);
        }
        let Ok((header, _)) = Header::decode(&message[..sizes.bytes as usize]) else {
            continue;
        };
        let t = header.transaction;
        let n = match header.ordinal {
            1 => encode(
                &InfoReply {
                    status: Below(status::OK),
                    width: 1024,
                    height: 768,
                    format: FORMAT_XRGB,
                    width_mm: 0,
                    height_mm: 0,
                },
                t,
                &mut out,
            ),
            2 => encode(
                &Done {
                    status: Below(status::OK),
                },
                t,
                &mut out,
            ),
            // A flush is never answered.
            _ => continue,
        };
        if let Ok(n) = n {
            let _ = channel_write(end, &out[..n], &[]);
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    match startup().argv.next() {
        Some(b"scribble") => scribble(),
        Some(b"hang-display") => hang_display(),
        _ => 2,
    }
}
