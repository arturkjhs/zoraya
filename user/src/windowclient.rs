//! A shell's request to the panel for the window of a POSIX program
//! (ADR-0317): one `OpenWindow` and one reply, `WindowOpened` with the client
//! end of a channel to the compositor, or `WindowRefused`.

use alloc::string::String;
use alloc::vec;

use zoraya_abi::{Handle, Mode, Packet, Status, channel_signals};
use zoraya_proto_chooser::{OpenWindow, WindowOpened, WindowRefused, window_refusal};
use zoraya_protocol::{Data, Header, Message, decode, encode};

use crate::{
    channel_read, channel_write, clock_monotonic, handle_close, port_create, port_subscribe,
    port_wait,
};

/// How long the panel is waited for.
const WAIT_NS: u64 = 5_000_000_000;

/// The sentence for a refusal code.
pub fn refusal_text(status: u32) -> &'static str {
    match status {
        window_refusal::UNTRUSTED => {
            "a window of a POSIX program is not given in an untrusted room"
        }
        window_refusal::BUSY => "this shell has been given its windows",
        window_refusal::INVALID => "the panel does not accept this window request",
        _ => "the panel could not make the window",
    }
}

/// Asks `panel` for a window for `app`: the client end, or the problem.
pub fn ask(panel: Handle, app: &str, title: &str, entries: &str) -> Result<Handle, String> {
    let m = OpenWindow {
        app: Data(app.as_bytes()),
        title: Data(title.as_bytes()),
        entries: Data(entries.as_bytes()),
    };
    let mut out = vec![0u8; 2048];
    let n = encode(&m, 1, &mut out).map_err(|_| String::from("the window request is too large"))?;
    channel_write(panel, &out[..n], &[]).map_err(|_| String::from("the panel is gone"))?;
    let port = port_create().map_err(|_| String::from("no port"))?;
    let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
    let _ = port_subscribe(port, panel, wanted, 1, Mode::Level);
    let until = clock_monotonic() + WAIT_NS;
    let mut buffer = vec![0u8; 256];
    let mut handles = [panel; 1];
    let result = loop {
        match channel_read(panel, &mut buffer, &mut handles) {
            Ok(sizes) => {
                let msg = &buffer[..sizes.bytes as usize];
                let got = sizes.handles as usize;
                let ordinal = Header::decode(msg).map_or(0, |(h, _)| h.ordinal);
                if ordinal == WindowOpened::ORDINAL
                    && got == 1
                    && decode::<WindowOpened>(msg, 1).is_ok()
                {
                    break Ok(handles[0]);
                }
                for h in &handles[..got] {
                    let _ = handle_close(*h);
                }
                break Err(match decode::<WindowRefused>(msg, 0) {
                    Ok((_, r)) => String::from(refusal_text(r.status.0)),
                    Err(_) => String::from("the panel's answer is not one"),
                });
            }
            Err((Status::WouldBlock, _)) => {
                let mut packets = [Packet::default()];
                if port_wait(port, until, &mut packets).is_err() {
                    break Err(String::from("the panel did not answer"));
                }
            }
            Err(_) => break Err(String::from("the panel is gone")),
        }
    };
    let _ = handle_close(port);
    result
}
