#![no_std]
#![no_main]

//! Memory objects (ADR-0051) from a process.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    CONSOLE, DIRECTORY, ExitReason, Handle, JOB, MapFlags, MemoryFlags, Rights, Status,
};
use zoraya_user::{
    assert_err, directory_lookup, handle_close, handle_duplicate, memory_create, memory_map,
    memory_unmap, raw_syscall, spawn, wait_exit,
};

const PAGE: u64 = 4096;
const FREE_FRAMES: u64 = 0x7006;
const OBJECT_MAPPINGS: u64 = 0x7008;

fn test_call(number: u64) -> u64 {
    raw_syscall(number, [0; 6]).1
}

fn read(address: u64) -> u64 {
    // SAFETY: the caller mapped `address`.
    unsafe { core::ptr::read_volatile(address as *const u64) }
}

fn write(address: u64, value: u64) {
    // SAFETY: the caller mapped `address` writable.
    unsafe { core::ptr::write_volatile(address as *mut u64, value) }
}

/// Spawns `child` with `object` at slot 3 and one role.
fn child_with(object: Handle, rights: Rights, role: &str) -> Handle {
    let image = directory_lookup(DIRECTORY, "child").expect("child");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.handle(HandleEntry {
        slot: 3,
        handle: object,
        rights,
        transfer: Transfer::Duplicate,
    });
    e.arg(role.as_bytes());
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    child
}

/// plan.md step 1: a process shares a page with a child.
fn shared_with_a_child() {
    let object = memory_create(2 * PAGE, MemoryFlags::NONE).expect("create");
    let at = memory_map(object, 0, 2 * PAGE, MapFlags::WRITE).expect("map");
    assert_eq!(read(at), 0, "zeroed");
    assert_eq!(read(at + PAGE), 0, "zeroed");
    write(at + PAGE, 0x1234);
    let child = child_with(object, Rights::READ.union(Rights::WRITE), "memory-writer");
    let record = wait_exit(child);
    assert_eq!(record.code, 0, "{record:?}");
    assert_eq!(read(at), 0x5678, "the child's write");
    memory_unmap(at, 2 * PAGE).expect("unmap");
    handle_close(object).expect("close");
}

/// A read-only view is a rights mask (INV-0051-2).
fn read_only_view() {
    let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    let view = handle_duplicate(object, Rights::READ).expect("view");
    assert_err!(
        memory_map(view, 0, PAGE, MapFlags::WRITE),
        Status::AccessDenied
    );
    let at = memory_map(view, 0, PAGE, MapFlags::READ).expect("map");
    assert_eq!(read(at), 0);
    memory_unmap(at, PAGE).expect("unmap");
    let none = handle_duplicate(object, Rights::NONE).expect("nothing");
    assert_err!(
        memory_map(none, 0, PAGE, MapFlags::READ),
        Status::AccessDenied
    );
    assert_err!(
        memory_map(CONSOLE, 0, PAGE, MapFlags::READ),
        Status::BadHandle
    );
    for h in [view, none, object] {
        handle_close(h).expect("close");
    }
}

/// A write to a read-only mapping, and a read after unmapping, each end a
/// process (INV-0051-2, INV-0051-3).
fn faults() {
    let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    for role in ["memory-write-read-only", "memory-after-unmap"] {
        let child = child_with(object, Rights::READ.union(Rights::WRITE), role);
        assert_eq!(wait_exit(child).reason, ExitReason::Fault, "{role}");
    }
    handle_close(object).expect("close");
}

fn bad_arguments() {
    let bad = Status::InvalidArgument;
    assert_err!(memory_create(0, MemoryFlags::NONE), bad);
    assert_err!(memory_create(100, MemoryFlags::NONE), bad);
    assert_err!(memory_create(1 << 40, MemoryFlags::NONE), Status::NoMemory);
    assert_err!(
        memory_create(1 << 40, MemoryFlags::CONTIGUOUS),
        Status::NoMemory
    );
    let object = memory_create(PAGE, MemoryFlags::CONTIGUOUS).expect("create");
    assert_err!(memory_map(object, 0, 2 * PAGE, MapFlags::READ), bad);
    assert_err!(memory_map(object, PAGE, PAGE, MapFlags::READ), bad);
    assert_err!(memory_map(object, 1, PAGE, MapFlags::READ), bad);
    assert_err!(memory_map(object, 0, 0, MapFlags::READ), bad);
    assert_err!(
        memory_map(object, u64::MAX - 4095, PAGE, MapFlags::READ),
        bad
    );
    assert_err!(memory_unmap(0x1000, PAGE), bad);
    let at = memory_map(object, 0, PAGE, MapFlags::READ).expect("map");
    assert_err!(memory_unmap(at, 2 * PAGE), bad);
    assert_err!(memory_unmap(at + 1, PAGE), bad);
    memory_unmap(at, PAGE).expect("unmap");
    assert_err!(memory_unmap(at, PAGE), bad);
    handle_close(object).expect("close");
}

/// INV-0051-1 at exhaustion: more than is free fails, and frees what it took.
fn exhaustion_changes_nothing() {
    for flags in [MemoryFlags::NONE, MemoryFlags::CONTIGUOUS] {
        let free = test_call(FREE_FRAMES);
        assert_err!(memory_create((free + 16) * PAGE, flags), Status::NoMemory);
        assert_eq!(test_call(FREE_FRAMES), free, "nothing kept");
    }
}

/// ADR-0051: zeroed, contiguous or not, even over another object's frames.
fn new_objects_are_zeroed() {
    for flags in [MemoryFlags::NONE, MemoryFlags::CONTIGUOUS] {
        let dirty = memory_create(4 * PAGE, flags).expect("create");
        let at = memory_map(dirty, 0, 4 * PAGE, MapFlags::WRITE).expect("map");
        for page in 0..4 {
            write(at + page * PAGE, u64::MAX);
        }
        memory_unmap(at, 4 * PAGE).expect("unmap");
        handle_close(dirty).expect("close");
        let clean = memory_create(4 * PAGE, flags).expect("create");
        let at = memory_map(clean, 0, 4 * PAGE, MapFlags::READ).expect("map");
        for page in 0..4 {
            assert_eq!(read(at + page * PAGE), 0, "{page}");
        }
        memory_unmap(at, 4 * PAGE).expect("unmap");
        handle_close(clean).expect("close");
    }
}

/// INV-0051-2: the kernel's own walk of this process's tables.
fn no_object_mapping_is_executable() {
    let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    let a = memory_map(object, 0, PAGE, MapFlags::WRITE).expect("map");
    let b = memory_map(object, 0, PAGE, MapFlags::READ).expect("map");
    assert_eq!(test_call(OBJECT_MAPPINGS), 2);
    memory_unmap(a, PAGE).expect("unmap");
    memory_unmap(b, PAGE).expect("unmap");
    assert_eq!(test_call(OBJECT_MAPPINGS), 0);
    handle_close(object).expect("close");
}

/// 256 mappings, then `LimitReached`, and each ends a place to reuse.
fn mapping_limit() {
    let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    let mut first = 0;
    let mut last = 0;
    for i in 0..256 {
        last = memory_map(object, 0, PAGE, MapFlags::READ).expect("map");
        if i == 0 {
            first = last;
        }
    }
    assert_err!(
        memory_map(object, 0, PAGE, MapFlags::READ),
        Status::LimitReached
    );
    memory_unmap(last, PAGE).expect("unmap");
    let again = memory_map(object, 0, PAGE, MapFlags::READ).expect("room again");
    assert_eq!(again, last);
    // Mappings go at exit, so a process need not end them all. `first` keeps
    // the object alive after its last handle.
    handle_close(object).expect("close");
    assert_eq!(read(first), 0, "a mapping holds the object");
}

/// A handle closed while mapped leaves the frames, and a mapping is held by
/// the frames, in either order (INV-0051-4).
fn lifetime_either_order() {
    let object = memory_create(PAGE, MemoryFlags::NONE).expect("create");
    let at = memory_map(object, 0, PAGE, MapFlags::WRITE).expect("map");
    handle_close(object).expect("close");
    write(at, 7);
    assert_eq!(read(at), 7);
    memory_unmap(at, PAGE).expect("unmap");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    shared_with_a_child();
    read_only_view();
    faults();
    bad_arguments();
    exhaustion_changes_nothing();
    new_objects_are_zeroed();
    no_object_mapping_is_executable();
    lifetime_either_order();
    mapping_limit();
    0
}
