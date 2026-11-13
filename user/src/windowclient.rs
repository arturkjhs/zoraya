//! A shell's request to the panel for the window of a POSIX program
//! (ADR-0317): one `OpenWindow` and one reply, `WindowOpened` with the client
//! end of a channel to the compositor, or `WindowRefused`.

use alloc::string::String;
use alloc::vec;

use core::sync::atomic::{AtomicU32, Ordering};
use zoraya_abi::{Handle, Mode, Packet, Status, channel_signals};

use zoraya_proto_chooser::{OpenWindow, WindowReply, window_refusal, window_reply};
use zoraya_protocol::{Data, encode};

use crate::{
    channel_read, channel_write, clock_monotonic, handle_close, port_create, port_subscribe,
    port_wait,
};

/// The transaction of the next request: an answer to an earlier one that came
/// after its wait ended is not this one's (review W1).
static NEXT: AtomicU32 = AtomicU32::new(1);

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

/// Reads every message waiting on `panel` and closes what they carry: outside
/// [`ask`] nothing is asked, so each is an answer that came after its wait
/// ended, whose client end would otherwise keep the compositor's client,
/// and the panel's count of the shell's window, alive until the next ask
/// (9e). The shell calls it when the channel is readable at its prompt;
/// `None` when a read failed for another reason than an empty queue.
pub fn drain_stale(panel: Handle) -> Option<usize> {
    let mut buffer = vec![0u8; zoraya_protocol::MAX_WIRE_MESSAGE];
    let mut handles = [panel; 4];
    let mut drained = 0;
    loop {
        match channel_read(panel, &mut buffer, &mut handles) {
            Ok(sizes) => {
                for h in &handles[..sizes.handles as usize] {
                    let _ = handle_close(*h);
                }
                drained += 1;
            }
            Err((Status::WouldBlock, _)) => return Some(drained),
            // A message this cannot take, or the panel gone: the caller
            // stops listening rather than wake for it again.
            Err(_) => return None,
        }
    }
}

/// Asks `panel` for a window for `app`: the client end, or the problem.
pub fn ask(panel: Handle, app: &str, title: &str, entries: &str) -> Result<Handle, String> {
    let m = OpenWindow {
        app: Data(app.as_bytes()),
        title: Data(title.as_bytes()),
        entries: Data(entries.as_bytes()),
    };
    let t = NEXT.fetch_add(1, Ordering::Relaxed).max(1);
    let mut out = vec![0u8; 2048];
    let n = encode(&m, t, &mut out).map_err(|_| String::from("the window request is too large"))?;
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
                match window_reply(msg, got, t) {
                    WindowReply::Opened if got == 1 => break Ok(handles[0]),
                    WindowReply::Refused(status) => {
                        for h in &handles[..got] {
                            let _ = handle_close(*h);
                        }
                        break Err(String::from(refusal_text(status)));
                    }
                    other => {
                        // What a stale answer carries is closed (the client
                        // end of a window nobody asked for).
                        for h in &handles[..got] {
                            let _ = handle_close(*h);
                        }
                        if other != WindowReply::Stale {
                            break Err(String::from("the panel's answer is not one"));
                        }
                    }
                }
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
