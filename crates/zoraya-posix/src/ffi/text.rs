//! A POSIX program's text over `zoraya-text` (ADR-0326): six C functions over
//! `zoraya-fontset`, for NetSurf's font library (`ports/netsurf/font_zoraya.c`).
//! The fonts are read once, by `zoraya_text_init`, as `/system/fonts/NAME`
//! for the names `zoraya-fontset` gives and nothing else (INV-0326-1),
//! through this library's own `open` and `read`; the program sees them only
//! if its line grants `/system/fonts` (a `window` line does, read only).
//! The calls are made by one thread at a time (a lock holds the others out,
//! as the window's do).

use super::*;

use zoraya_fontset::{Face, Fontset, Source};

/// A style as C gives it: NetSurf's generic family and CSS weight, the size
/// in points (22.10 fixed point) and the dpi.
#[repr(C)]
pub struct TextStyle {
    family: u32,
    weight: i32,
    size: i32,
    dpi: u32,
}

/// The face and the size in pixels of a style; sans at 16 pixels for NULL.
///
/// # Safety
///
/// `style` is NULL or points at a `TextStyle`.
unsafe fn face_px(style: *const TextStyle) -> (Face, u32) {
    if style.is_null() {
        return (Face::Sans, 16);
    }
    // SAFETY: the caller's contract: a readable `TextStyle`.
    let s = unsafe { &*style };
    (
        Face::of(s.family, s.weight),
        zoraya_fontset::px(s.size, s.dpi),
    )
}

/// `len` bytes at `text`, or none for NULL.
///
/// # Safety
///
/// `text` is NULL or points at `len` readable bytes that stay unchanged
/// for the call.
unsafe fn bytes<'a>(text: *const c_char, len: usize) -> &'a [u8] {
    if text.is_null() || len == 0 {
        return &[];
    }
    // SAFETY: the caller's contract.
    unsafe { core::slice::from_raw_parts(text.cast::<u8>(), len) }
}

/// The files, read with this library's own calls: a path the program's
/// namespace does not grant is refused there, as any `open` is.
struct Files;

impl Source for Files {
    fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let mut c = Vec::with_capacity(path.len() + 1);
        c.extend_from_slice(path.as_bytes());
        c.push(0);
        // A signal during start-up must not lose a font for the whole
        // process (T7 of the 9f-text review): `EINTR` is tried again.
        let fd = loop {
            let fd = open(c.as_ptr().cast(), o::RDONLY as i32, 0);
            if fd >= 0 || last_errno() != EINTR {
                break fd;
            }
        };
        if fd < 0 {
            return None;
        }
        let mut data = Vec::new();
        let mut chunk = vec![0u8; 64 * 1024];
        let ok = loop {
            let n = read(fd, chunk.as_mut_ptr(), chunk.len());
            if n < 0 && last_errno() == EINTR {
                continue;
            }
            if n < 0 {
                break false;
            }
            if n == 0 {
                break true;
            }
            data.extend_from_slice(&chunk[..n as usize]);
            if data.len() > zoraya_fontset::MAX_FILE {
                break false;
            }
        };
        close(fd);
        ok.then_some(data)
    }
}

/// This thread's `errno`, as the call before set it.
fn last_errno() -> i32 {
    // SAFETY: musl's `__errno_location` gives this thread's errno, always.
    unsafe { *__errno_location() }
}

struct Slot(UnsafeCell<Option<Fontset>>);

// SAFETY: reached only through `fonts`, which holds `TEXT`.
unsafe impl Sync for Slot {}

static TEXT: Lock = Lock::new();
static SLOT: Slot = Slot(UnsafeCell::new(None));

/// Runs `f` on the fonts, holding `TEXT`; `busy` when another thread (or
/// a signal handler inside a call of this thread) holds it, never a wait.
fn fonts<R>(busy: R, f: impl FnOnce(&mut Option<Fontset>) -> R) -> R {
    if !TEXT.try_lock() {
        return busy;
    }
    // SAFETY: `TEXT` is held, so this is the only reference.
    let r = f(unsafe { &mut *SLOT.0.get() });
    TEXT.unlock(&Futex);
    r
}

/// In a `fork` child: the fonts are the parent's bytes, copied with the
/// address space, so they stay when the lock was free. When a thread that is
/// not in the child held it, that thread may have been in the middle of
/// changing the set (a glyph cache insert, a `Vec` growing), so the set is
/// forgotten and the child's `zoraya_text_init` reads the fonts again (T2 of
/// the 9f-text review). The lock is freed either way.
pub(super) fn after_fork_in_child() {
    // SAFETY: a fork child has one thread, and no reference into `SLOT`
    // outlives a call (`plot` may not `fork`: it is called with `TEXT` held).
    crate::lock::reset_after_fork(&TEXT, unsafe { &mut *SLOT.0.get() });
}

/// Reads the fonts (once: a second call reads nothing) and gives how many
/// of the four were read and parsed, or -1 when another call is inside.
#[unsafe(no_mangle)]
pub extern "C" fn zoraya_text_init() -> i32 {
    fonts(-1, |slot| {
        let set = slot.get_or_insert_with(|| Fontset::load(&mut Files));
        set.loaded() as i32
    })
}

/// The advance of `len` bytes of UTF-8 at `text` in `style`, in pixels; 0
/// before `zoraya_text_init`.
///
/// # Safety
///
/// `style` is NULL or readable; `text` is NULL or `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_text_width(
    style: *const TextStyle,
    text: *const c_char,
    len: usize,
) -> i32 {
    // SAFETY: the caller's contract.
    let ((face, px), s) = unsafe { (face_px(style), bytes(text, len)) };
    fonts(0, |slot| slot.as_mut().map_or(0, |f| f.width(face, px, s)))
}

/// The character boundary nearest to `x`: its byte offset (0 to `len`),
/// and its x in `*actual_x`.
///
/// # Safety
///
/// As `zoraya_text_width`; `actual_x` is writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_text_position(
    style: *const TextStyle,
    text: *const c_char,
    len: usize,
    x: i32,
    actual_x: *mut i32,
) -> usize {
    // SAFETY: the caller's contract.
    let ((face, px), s) = unsafe { (face_px(style), bytes(text, len)) };
    let (o, ax) = fonts((0, 0), |slot| {
        slot.as_mut().map_or((0, 0), |f| f.position(face, px, s, x))
    });
    if !actual_x.is_null() {
        // SAFETY: the caller's contract: writable.
        unsafe { *actual_x = ax };
    }
    o
}

/// Where to break the text to fit `x` (NetSurf's `split`): the byte offset,
/// and its x in `*actual_x`. `len` when it fits or cannot be broken.
///
/// # Safety
///
/// As `zoraya_text_position`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_text_split(
    style: *const TextStyle,
    text: *const c_char,
    len: usize,
    x: i32,
    actual_x: *mut i32,
) -> usize {
    // SAFETY: the caller's contract.
    let ((face, px), s) = unsafe { (face_px(style), bytes(text, len)) };
    let (o, ax) = fonts((len, 0), |slot| {
        slot.as_mut().map_or((len, 0), |f| f.split(face, px, s, x))
    });
    if !actual_x.is_null() {
        // SAFETY: the caller's contract: writable.
        unsafe { *actual_x = ax };
    }
    o
}

/// The ascent and descent (positive, below the baseline) of the style's
/// face in pixels; 0, or -1 before `zoraya_text_init`. A NULL `ascent` or
/// `descent` is not written.
///
/// # Safety
///
/// `style` is NULL or readable; `ascent` and `descent` are NULL or
/// writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_text_metrics(
    style: *const TextStyle,
    ascent: *mut i32,
    descent: *mut i32,
) -> i32 {
    // SAFETY: the caller's contract.
    let (face, px) = unsafe { face_px(style) };
    let m = fonts(None, |slot| slot.as_ref().map(|f| f.metrics(face, px)));
    let Some(m) = m else { return -1 };
    if !ascent.is_null() {
        // SAFETY: the caller's contract: writable when not NULL.
        unsafe { *ascent = m.ascent as i32 };
    }
    if !descent.is_null() {
        // SAFETY: the caller's contract: writable when not NULL.
        unsafe { *descent = m.descent as i32 };
    }
    0
}

/// What `zoraya_text_glyphs` calls for each glyph: `ctx`, the glyph's top
/// left from the pen's origin on the baseline, its width and height, and
/// `width × height` bytes of coverage, valid for the call only.
pub type Plot = unsafe extern "C" fn(*mut core::ffi::c_void, i32, i32, u32, u32, *const u8);

/// Calls `plot` for every glyph of the text and gives its advance. `plot`
/// must not call the `zoraya_text_*` functions (they would get the busy
/// answer), nor `fork` (the child would forget the set under the call).
///
/// # Safety
///
/// As `zoraya_text_width`; `plot` is NULL or a function of that type, safe
/// to call with `ctx`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn zoraya_text_glyphs(
    style: *const TextStyle,
    text: *const c_char,
    len: usize,
    plot: Option<Plot>,
    ctx: *mut core::ffi::c_void,
) -> i32 {
    // SAFETY: the caller's contract.
    let ((face, px), s) = unsafe { (face_px(style), bytes(text, len)) };
    fonts(0, |slot| {
        let Some(f) = slot.as_mut() else { return 0 };
        f.glyphs(face, px, s, |g| {
            if let Some(plot) = plot {
                // SAFETY: the caller's contract for `plot` and `ctx`; the
                // coverage holds `width × height` bytes (`Fontset::glyphs`)
                // and lives for the call.
                unsafe { plot(ctx, g.x, g.y, g.width, g.height, g.coverage.as_ptr()) };
            }
        })
    })
}
