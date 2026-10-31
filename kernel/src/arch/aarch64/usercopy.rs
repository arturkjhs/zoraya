//! Copies to and from user memory (ADR-0031). The only code that dereferences
//! a user address.
//!
//! `ldtrb` checks permissions as EL0 would, so even an address in
//! kernel memory faults here. A fault with the PC in this routine resumes at
//! its error exit, which is the one fault the kernel survives (ADR-0024).

use core::arch::global_asm;

global_asm!(
    r#"
    .text
    // Returns 0, or 1 after a fault. x0 is the destination, x1 the source, x2
    // the length. No call writes to user memory yet; the `sttrb` form comes
    // with the first that does.
__user_copy_start:
__copy_from_user:
    cbz     x2, 2f
1:
    ldtrb   w3, [x1]
    strb    w3, [x0], #1
    add     x1, x1, #1
    subs    x2, x2, #1
    b.ne    1b
2:
    mov     x0, #0
    ret

__user_copy_fault:
    mov     x0, #1
    ret
__user_copy_end:
"#
);

unsafe extern "C" {
    fn __copy_from_user(dst: *mut u8, src: *const u8, len: usize) -> u64;
    fn __user_copy_fault();
    static __user_copy_start: u8;
    static __user_copy_end: u8;
}

/// Where a fault at `pc` must resume, if `pc` is in the routine above.
pub fn fixup(pc: u64) -> Option<u64> {
    let start = &raw const __user_copy_start as u64;
    let end = &raw const __user_copy_end as u64;
    (start..end)
        .contains(&pc)
        .then_some(__user_copy_fault as *const () as u64)
}

/// A faulting access ended the copy. The destination may be partly written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadAddress;

/// Copies from user address `src`.
///
/// # Safety
///
/// `dst` is writable for `dst.len()` bytes; `src` is any value, because the
/// access is checked as EL0's.
pub unsafe fn copy_from_user(dst: &mut [u8], src: u64) -> Result<(), BadAddress> {
    // SAFETY: the caller's contract; a bad `src` faults into the error exit.
    match unsafe { __copy_from_user(dst.as_mut_ptr(), src as *const u8, dst.len()) } {
        0 => Ok(()),
        _ => Err(BadAddress),
    }
}
