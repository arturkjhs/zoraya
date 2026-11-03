# Turns ports/posix-stub/stubs.txt into the Rust source of the stub
# zoraya-posix (ADR-0140). Usage: awk -f stubs.awk stubs.txt > lib.rs
BEGIN {
    print "//! The stub zoraya-posix (ADR-0140), generated from ports/posix-stub/stubs.txt"
    print "//! by ports/posix-stub/stubs.awk. Do not edit: TASK 6c replaces it with the"
    print "//! real crate. Every function fails with ENOSYS or does nothing; see the list."
    print ""
    print "#![no_std]"
    print "// musl's internal variables have lower-case names (__default_stacksize)."
    print "#![allow(non_upper_case_globals)]"
    print ""
    print "use core::ffi::{c_int, c_long, c_void};"
    print ""
    print "const ENOSYS: c_int = 38;"
    print ""
    print "unsafe extern \"C\" {"
    print "    /// musl's, from src/errno: the calling thread's errno."
    print "    fn __errno_location() -> *mut c_int;"
    print "}"
    print ""
    print "fn enosys() {"
    print "    // SAFETY: musl's __errno_location returns a valid pointer to the"
    print "    // thread's errno once its start-up has set up the thread pointer, which"
    print "    // happens before any of these functions can be called."
    print "    unsafe { *__errno_location() = ENOSYS }"
    print "}"
    print ""
    print "fn trap() -> ! {"
    print "    // SAFETY: brk only raises a debug exception; the kernel ends the"
    print "    // process with reason Fault (ADR-0034). Nothing is read or written."
    print "    unsafe { core::arch::asm!(\"brk #0x7a\", options(noreturn, nomem, nostack)) }"
    print "}"
    print ""
    print "#[panic_handler]"
    print "fn panic(_: &core::panic::PanicInfo) -> ! {"
    print "    trap()"
    print "}"
}
/^[[:space:]]*(#|$)/ { next }
{
    kind = $1; name = $2
    if (name == "" || $3 != "") { print "bad line: " $0 > "/dev/stderr"; exit 1 }
    if (seen[name]++) { print "duplicate: " name > "/dev/stderr"; exit 1 }
    print ""
    print "#[unsafe(no_mangle)]"
    if (kind == "int") {
        print "pub extern \"C\" fn " name "() -> c_long {"; print "    enosys();"; print "    -1"; print "}"
    } else if (kind == "zero") {
        print "pub extern \"C\" fn " name "() -> c_long {"; print "    enosys();"; print "    0"; print "}"
    } else if (kind == "ptr") {
        print "pub extern \"C\" fn " name "() -> *mut c_void {"; print "    enosys();"; print "    core::ptr::null_mut()"; print "}"
    } else if (kind == "sigerr") {
        print "pub extern \"C\" fn " name "() -> *mut c_void {"; print "    enosys();"; print "    core::ptr::without_provenance_mut(usize::MAX)"; print "}"
    } else if (kind == "err") {
        print "pub extern \"C\" fn " name "() -> c_int {"; print "    ENOSYS"; print "}"
    } else if (kind == "void") {
        print "pub extern \"C\" fn " name "() {}"
    } else if (kind == "raw") {
        print "pub extern \"C\" fn " name "() -> c_long {"; print "    -(ENOSYS as c_long)"; print "}"
    } else if (kind == "trap") {
        print "pub extern \"C\" fn " name "() -> ! {"; print "    trap()"; print "}"
    } else if (kind ~ /^u32:[0-9]+$/) {
        split(kind, v, ":")
        print "pub static mut " name ": u32 = " v[2] ";"
    } else {
        print "unknown kind " kind " for " name > "/dev/stderr"; exit 1
    }
}
