//! The window of a POSIX program (proposed ADR-0317): five C functions over
//! `zoraya-surface`, for the C surface of NetSurf's `libnsfb` (`ports/netsurf`).
//! The channel is the one the launcher gave in the `ZPBX` message
//! (`EXT_WINDOW`); a program without one gets -1 from `zoraya_window_open`
//! and nothing else changes. One window per process; the calls are made by
//! one thread at a time (a lock holds the others out).

use super::*;

use zoraya_abi::spawn::Transfer;
use zoraya_abi::{ChannelObject, Rights, channel_signals};
use zoraya_platform::{Platform, Poll};
use zoraya_surface::{Event, Surface};

struct Mapped {
    address: u64,
    size: u64,
    pixels: usize,
}

/// The compositor's channel and pixel buffers, for [`Surface`].
struct Gui {
    end: Handle,
    buffers: Vec<Option<Mapped>>,
}

impl Gui {
    fn wait(&self) -> bool {
        let Ok(port) = zoraya_rt::port_create() else {
            return false;
        };
        let wanted = channel_signals::READABLE | channel_signals::PEER_CLOSED;
        let ok = zoraya_rt::port_subscribe(port, self.end, wanted, 1, Mode::Level)
            .and_then(|()| {
                let mut packets = [Packet::default()];
                zoraya_rt::port_wait(port, DEADLINE_INFINITE, &mut packets)
            })
            .is_ok();
        let _ = zoraya_rt::handle_close(port);
        ok
    }

    fn read(&mut self, bytes: &mut [u8]) -> Result<usize, Status> {
        let mut handles = [self.end; 1];
        let sizes = zoraya_rt::channel_read(self.end, bytes, &mut handles).map_err(|(s, _)| s)?;
        // The compositor sends no handle to this client (the protocol's
        // `Drop` is for system clients); one that comes is closed.
        for h in &handles[..sizes.handles as usize] {
            let _ = zoraya_rt::handle_close(*h);
        }
        Ok(sizes.bytes as usize)
    }
}

impl Drop for Gui {
    /// N1: the channel end is closed (the compositor drops the client) and
    /// any buffer left is unmapped.
    fn drop(&mut self) {
        for m in self.buffers.drain(..).flatten() {
            let _ = zoraya_rt::memory_unmap(m.address, m.size);
        }
        let _ = zoraya_rt::handle_close(self.end);
    }
}

impl Platform for Gui {
    type End = Handle;

    fn send(&mut self, bytes: &[u8], handle: Option<Handle>) -> bool {
        // The buffer's view goes read-only.
        let entry = handle.map(|h| ChannelEntry {
            handle: h,
            rights: if zoraya_rt::handle_type(h) == Ok("channel") {
                ChannelObject::RIGHTS
            } else {
                Rights::READ
            },
            transfer: Transfer::Move,
        });
        let entries: &[ChannelEntry] = match &entry {
            Some(e) => core::slice::from_ref(e),
            None => &[],
        };
        zoraya_rt::channel_write(self.end, bytes, entries).is_ok()
    }

    fn receive(&mut self, bytes: &mut [u8]) -> Option<usize> {
        loop {
            match self.read(bytes) {
                Ok(n) => return Some(n),
                Err(Status::WouldBlock) => {
                    if !self.wait() {
                        return None;
                    }
                }
                Err(_) => return None,
            }
        }
    }

    fn poll(&mut self, bytes: &mut [u8]) -> Poll {
        match self.read(bytes) {
            Ok(n) => Poll::Message(n),
            Err(Status::WouldBlock) => Poll::Empty,
            Err(_) => Poll::Gone,
        }
    }

    fn buffer(&mut self, pixels: usize) -> Option<(Handle, usize)> {
        // W3: any size a caller or a `Configure` names is checked, not
        // multiplied (the surface checks too; this is the second line).
        let bytes = pixels.checked_mul(4)?;
        if pixels > zoraya_surface::MAX_PIXELS as usize {
            return None;
        }
        let size = bytes.next_multiple_of(4096) as u64;
        let memory = zoraya_rt::memory_create(size, MemoryFlags::NONE).ok()?;
        let Ok(address) = zoraya_rt::memory_map(memory, 0, size, MapFlags::WRITE) else {
            let _ = zoraya_rt::handle_close(memory);
            return None;
        };
        let view = zoraya_rt::handle_duplicate(memory, Rights::READ.union(Rights::TRANSFER));
        let _ = zoraya_rt::handle_close(memory);
        let Ok(view) = view else {
            let _ = zoraya_rt::memory_unmap(address, size);
            return None;
        };
        let mapped = Mapped {
            address,
            size,
            pixels,
        };
        let i = match self.buffers.iter().position(Option::is_none) {
            Some(i) => {
                self.buffers[i] = Some(mapped);
                i
            }
            None => {
                self.buffers.push(Some(mapped));
                self.buffers.len() - 1
            }
        };
        Some((view, i))
    }

    fn pixels(&mut self, buffer: usize) -> &mut [u32] {
        match self.buffers.get(buffer).and_then(Option::as_ref) {
            // SAFETY: a writable mapping of `size` bytes made by `buffer`,
            // page-aligned and holding `pixels` u32 values, kept until
            // `free` unmaps it; the slice lives no longer than `&mut self`.
            Some(m) => unsafe { core::slice::from_raw_parts_mut(m.address as *mut u32, m.pixels) },
            None => &mut [],
        }
    }

    fn free(&mut self, buffer: usize) {
        if let Some(m) = self.buffers.get_mut(buffer).and_then(Option::take) {
            let _ = zoraya_rt::memory_unmap(m.address, m.size);
        }
    }
}

struct Slot(UnsafeCell<Option<Surface<Gui>>>);

// SAFETY: reached only through `window`, which holds `WINDOW`.
unsafe impl Sync for Slot {}

static WINDOW: Lock = Lock::new();
static SLOT: Slot = Slot(UnsafeCell::new(None));

/// Runs `f` on the window, holding `WINDOW`. A call that finds it held (another
/// thread inside, or a signal handler that interrupted a call of this
/// thread, whose wait is a raw port wait the signal cannot end) does not wait:
/// it gets `busy` (-1 or 0 to C), never a deadlock (N2 of the 9d review).
fn window<R>(busy: R, f: impl FnOnce(&mut Option<Surface<Gui>>) -> R) -> R {
    if !WINDOW.try_lock() {
        return busy;
    }
    // SAFETY: `WINDOW` is held, so this is the only reference.
    let r = f(unsafe { &mut *SLOT.0.get() });
    WINDOW.unlock(&Futex);
    r
}

/// In a `fork` child: the parent's window is not the child's. Its channel
/// end names a slot that is empty here (a pipe may take it) and its buffer is
/// the parent's mapping, so the surface is forgotten, neither closed nor
/// unmapped (W1 of the 9d review), and the lock, held by a thread that is not
/// in the child, is freed.
pub(super) fn after_fork_in_child() {
    WINDOW.reset();
    // SAFETY: a fork child has one thread, and no reference into `SLOT`
    // outlives a call.
    crate::lock::forget_taken(unsafe { &mut *SLOT.0.get() });
}

/// An event for C: `kind` and four words. Kinds: 1 key (`a` the code, `b` the
/// code point or 0, `c` 1 for a press, `d` the modifier bits), 2 pointer moved
/// (`a`, `b` the pixel), 3 button (`a`, `b` the pixel, `c` the button bit,
/// `d` 1 for a press), 4 focus (`a` 1 gained), 5 resize (`a`, `b` the new size:
/// read the buffer again), 6 close requested, 7 the compositor is gone, 8 a
/// wheel turned (`a`, `b` the pixel, `c` and `d` the horizontal and vertical
/// turn as `int32_t` in 120ths of a notch, `d` above 0 toward the person:
/// proposed ADR-0322).
#[repr(C)]
pub struct WindowEvent {
    kind: u32,
    a: u32,
    b: u32,
    c: u32,
    d: u32,
}

fn encode(e: Event) -> WindowEvent {
    let (kind, a, b, c, d) = match e {
        Event::Key {
            code,
            text,
            pressed,
            modifiers,
        } => (1, code, text, pressed as u32, modifiers),
        Event::Move { x, y } => (2, x, y, 0, 0),
        Event::Button {
            x,
            y,
            button,
            pressed,
        } => (3, x, y, button, pressed as u32),
        Event::Focus(g) => (4, g as u32, 0, 0, 0),
        Event::Wheel { x, y, dx, dy } => (8, x, y, dx as u32, dy as u32),
        Event::Resize { width, height } => (5, width, height, 0, 0),
        Event::Close => (6, 0, 0, 0, 0),
        Event::Gone => (7, 0, 0, 0, 0),
    };
    WindowEvent { kind, a, b, c, d }
}

/// Opens the program's window, `width` by `height` logical pixels, titled
/// `title` (a C string); 0, or -1 when the program has no window channel, has
/// one open, or the compositor refuses.
///
/// # Safety
///
/// `title` is NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_window_open(title: *const c_char, width: u32, height: u32) -> i32 {
    let title = if title.is_null() {
        String::new()
    } else {
        // SAFETY: the caller's contract: a NUL-terminated string.
        let bytes = unsafe { core::ffi::CStr::from_ptr(title) }.to_bytes();
        String::from_utf8_lossy(bytes).into_owned()
    };
    // The channel is taken only once the window lock is held: a call that
    // gets `busy` leaves it where it was, for the next call (review N2).
    window(-1, |slot| {
        if slot.is_some() {
            return -1;
        }
        let Some(Some(end)) = with(|p| p.window.take()) else {
            return -1;
        };
        let gui = Gui {
            end,
            buffers: Vec::new(),
        };
        match Surface::open(gui, &title, width, height) {
            Some(s) => {
                *slot = Some(s);
                0
            }
            None => -1,
        }
    })
}

/// The buffer: its address, its width and height in pixels (the stride is
/// the width, in 32-bit words, `0x00RRGGBB`). Valid until the next
/// `zoraya_window_poll` that reports a resize, or `zoraya_window_close`.
/// 0, or -1 with no window.
///
/// # Safety
///
/// The three pointers are writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_window_buffer(
    pixels: *mut *mut u32,
    width: *mut u32,
    height: *mut u32,
) -> i32 {
    window(-1, |slot| {
        let Some(s) = slot.as_mut() else { return -1 };
        let (w, h) = s.size();
        let p = s.pixels().as_mut_ptr();
        // SAFETY: the caller's contract: three writable pointers.
        unsafe {
            *pixels = p;
            *width = w;
            *height = h;
        }
        0
    })
}

/// Says that the rectangle changed and shows it (clipped to the buffer: any
/// integers are safe).
#[unsafe(no_mangle)]
pub extern "C" fn zoraya_window_damage(x: i32, y: i32, width: i32, height: i32) {
    window((), |slot| {
        if let Some(s) = slot.as_mut() {
            s.damage(x.into(), y.into(), width.into(), height.into());
            s.commit();
        }
    });
}

/// The next event into `*event`: 1, or 0 when none is there (with `block`
/// nonzero, waits for one); -1 with no window.
///
/// # Safety
///
/// `event` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_window_poll(event: *mut WindowEvent, block: i32) -> i32 {
    window(0, |slot| {
        let Some(s) = slot.as_mut() else { return -1 };
        match s.event(block != 0) {
            Some(e) => {
                // SAFETY: the caller's contract: `event` is writable.
                unsafe { *event = encode(e) };
                1
            }
            None => 0,
        }
    })
}

/// Closes the window; the buffer is gone.
#[unsafe(no_mangle)]
pub extern "C" fn zoraya_window_close() {
    window((), |slot| {
        if let Some(s) = slot.take() {
            s.close();
        }
    });
}

/// The window's scale (physical pixels per logical pixel, ADR-0164): 1 with
/// no window, or while another call is inside. For the size of text
/// (ADR-0326 item 6).
#[unsafe(no_mangle)]
pub extern "C" fn zoraya_window_scale() -> u32 {
    window(1, |slot| slot.as_ref().map_or(1, |s| s.scale()))
}
