#![no_std]
#![no_main]

//! Images from memory objects (ADR-0108) and the startup block's program
//! headers (ADR-0109 item 5), from a process. The test kernel starts it with
//! the device handles and, at `HELLO_SLOT`, a memory object holding the
//! ramdisk's `hello`: its length in the first 8 bytes, the file from 4096.

extern crate alloc;

use zoraya_abi::quota::JobLimits;
use zoraya_abi::spawn::{Encoder, PROGRAM_HEADER_SIZE};
use zoraya_abi::{
    DIRECTORY, Handle, ImageObject, JOB, JobFlags, MapFlags, MemoryFlags, MemoryObject, ObjectType,
    Rights, Status,
};
use zoraya_proto_file::open_flags;
use zoraya_user::files::ALL;
use zoraya_user::stack::{find_disk, start_stack};
use zoraya_user::{
    assert_err, console_entry, directory_lookup, handle_close, handle_duplicate, image,
    image_create, job_create, job_get_usage, job_kill, job_set_limits, memory_create, memory_map,
    memory_unmap, object_same, println, raw_syscall, spawn, startup, wait_exit,
};

const HELLO_SLOT: u64 = 100;
const PAGE: u64 = 4096;
const FREE_FRAMES: u64 = 0x7006;
const HEAP_LIVE: u64 = 0x700b;

const HEAP_BACKING: u64 = 0x7017;

/// The free frames plus the frames the kernel heap has taken. The heap grows
/// by 64 KiB at a time (more for one larger request) and keeps what it grew
/// by, so a call that makes it grow moves frames from free to the heap
/// without leaking any: this sum is what must return to its start.
fn free_frames() -> u64 {
    loop {
        let before = raw_syscall(HEAP_BACKING, [0; 6]).1;
        let free = raw_syscall(FREE_FRAMES, [0; 6]).1;
        // The heap did not grow between the two reads.
        if raw_syscall(HEAP_BACKING, [0; 6]).1 == before {
            return free + before / PAGE;
        }
    }
}

fn heap_live() -> u64 {
    raw_syscall(HEAP_LIVE, [0; 6]).1
}

/// Spawns `program` with the console and `args`, waits, and gives the record.
fn run(program: Handle, args: &[&[u8]]) -> zoraya_abi::ExitRecord {
    let mut buffer = [0; 8192];
    let mut e = Encoder::new(&mut buffer, JOB, program);
    e.handle(console_entry());
    for a in args {
        e.arg(a);
    }
    let length = e.finish().expect("room");
    let child = spawn(&buffer[..length]).expect("spawn");
    let record = wait_exit(child);
    handle_close(child).expect("close");
    record
}

/// A fresh memory object holding `bytes`, mapped writable; and the mapping.
fn object_with(bytes: &[u8]) -> (Handle, u64, u64) {
    let size = (bytes.len() as u64).next_multiple_of(PAGE).max(PAGE);
    let object = memory_create(size, MemoryFlags::NONE).expect("memory");
    let at = memory_map(object, 0, size, MapFlags::WRITE).expect("map");
    // SAFETY: a fresh writable mapping of `size` bytes, which nothing else
    // uses.
    let target = unsafe { core::slice::from_raw_parts_mut(at as *mut u8, size as usize) };
    target[..bytes.len()].copy_from_slice(bytes);
    (object, at, size)
}

/// `hello`'s bytes, from the object the kernel gave: its length at 0, the
/// object's size at 8, the file from 4096.
fn hello_bytes() -> alloc::vec::Vec<u8> {
    let given = Handle::new(HELLO_SLOT).expect("slot");
    let first = memory_map(given, 0, PAGE, MapFlags::READ).expect("map");
    // SAFETY: the first 16 bytes of a readable mapping.
    let (length, size) = unsafe {
        (
            (first as *const u64).read(),
            (first as *const u64).add(1).read(),
        )
    };
    memory_unmap(first, PAGE).expect("unmap");
    let at = memory_map(given, 0, size, MapFlags::READ).expect("map");
    // SAFETY: the kernel put `length` bytes at 4096, inside the object.
    let file = unsafe { core::slice::from_raw_parts((at + PAGE) as *const u8, length as usize) };
    let copy = file.to_vec();
    memory_unmap(at, size).expect("unmap");
    handle_close(given).expect("close");
    copy
}

/// INV-0108-2: `hello` through the volume, then from memory, behaves as it
/// does from the ramdisk.
fn a_program_from_the_volume_runs_as_from_the_ramdisk(hello: &[u8]) {
    let ramdisk = image("hello");
    let expected = run(ramdisk, &[]);
    handle_close(ramdisk).expect("close");
    assert_eq!(expected.code, 7);

    let disk = find_disk().expect("the test disk");
    let stack = start_stack(disk.0, disk.1).expect("storage");
    let file = stack
        .root
        .open("hello.elf", open_flags::CREATE | open_flags::TRUNCATE, ALL)
        .expect("create");
    let (source, source_at, size) = object_with(hello);
    file.attach(source, size).expect("attach");
    file.write_buffer(0, 0, hello.len() as u32).expect("write");
    drop(file);
    memory_unmap(source_at, size).expect("unmap");
    handle_close(source).expect("close");

    let file = stack.root.open("hello.elf", 0, ALL).expect("open");
    let length = file.stat().expect("stat").size;
    assert_eq!(length, hello.len() as u64);
    let target = memory_create(length.next_multiple_of(PAGE), MemoryFlags::NONE).expect("memory");
    file.attach(target, length.next_multiple_of(PAGE))
        .expect("attach");
    assert_eq!(file.read_buffer(0, 0, length as u32), Ok(length));
    drop(file);
    let program = image_create(target, length, JOB).expect("an image of the file");
    handle_close(target).expect("the image holds the object");
    assert_eq!(run(program, &[]), expected, "same behaviour");
    assert_eq!(run(program, &[]), expected, "and again");
    handle_close(program).expect("close");
    stack.end();
}

/// INV-0108-1: what the loader refuses, `image_create` refuses, and a refused
/// call holds nothing.
fn bad_programs_are_refused_and_leave_nothing(hello: &[u8]) {
    let frames = free_frames();
    let heap = heap_live();
    let mutations: &[(usize, &[u8])] = &[
        (0, b"\x7fELG"),
        (4, &[1]),                                   // 32-bit
        (16, &[3, 0]),                               // ET_DYN
        (18, &[62, 0]),                              // x86-64
        (54, &[55, 0]),                              // header size
        (56, &[33, 0]),                              // too many headers
        (32, &[0xf0, 0xff, 0xff, 0xff, 0, 0, 0, 0]), // table past the end
        (24, &[0, 0, 0, 0, 0, 0, 0, 0]),             // entry nowhere
    ];
    for (at, bytes) in mutations {
        let mut file = hello.to_vec();
        file[*at..*at + bytes.len()].copy_from_slice(bytes);
        let (object, at, size) = object_with(&file);
        assert_err!(
            image_create(object, file.len() as u64, JOB),
            Status::InvalidArgument
        );
        memory_unmap(at, size).expect("unmap");
        handle_close(object).expect("close");
    }
    // Every program header made writable and executable.
    let mut file = hello.to_vec();
    let phoff = u64::from_le_bytes(file[32..40].try_into().unwrap()) as usize;
    let phnum = u16::from_le_bytes(file[56..58].try_into().unwrap()) as usize;
    for i in 0..phnum {
        let flags = phoff + i * 56 + 4;
        file[flags..flags + 4].copy_from_slice(&7u32.to_le_bytes());
    }
    let (object, at, size) = object_with(&file);
    assert_err!(
        image_create(object, file.len() as u64, JOB),
        Status::InvalidArgument
    );
    // Lengths: none, short, past the object.
    assert_err!(image_create(object, 0, JOB), Status::InvalidArgument);
    assert_err!(image_create(object, 63, JOB), Status::InvalidArgument);
    assert_err!(image_create(object, size + 1, JOB), Status::InvalidArgument);
    assert_err!(image_create(object, u64::MAX, JOB), Status::InvalidArgument);
    memory_unmap(at, size).expect("unmap");
    handle_close(object).expect("close");
    // Past the object with a good program in it: only the length is wrong.
    let (object, at, size) = object_with(hello);
    assert_err!(image_create(object, size + 1, JOB), Status::InvalidArgument);
    assert_err!(
        image_create(object, size + 4096, JOB),
        Status::InvalidArgument
    );
    let good = image_create(object, hello.len() as u64, JOB).expect("the same object, its length");
    handle_close(good).expect("close");
    memory_unmap(at, size).expect("unmap");
    handle_close(object).expect("close");
    assert_eq!(
        free_frames(),
        frames,
        "a refused object's frames all came back"
    );
    assert_eq!(heap_live(), heap, "and the heap");
}

/// INV-0040-2, -3: rights and kinds.
fn image_create_checks_its_handle() {
    let (object, at, size) = object_with(&[0; 64]);
    let no_read = handle_duplicate(object, Rights::WRITE).expect("dup");
    assert_err!(image_create(no_read, 64, JOB), Status::AccessDenied);
    assert_err!(image_create(JOB, 64, JOB), Status::AccessDenied);
    let ramdisk = image("hello");
    assert_err!(image_create(ramdisk, 64, JOB), Status::BadHandle);
    let read_dir = handle_duplicate(DIRECTORY, Rights::READ.union(Rights::LOOKUP));
    assert!(read_dir.is_err(), "a directory has no READ");
    memory_unmap(at, size).expect("unmap");
    for h in [object, no_read, ramdisk] {
        handle_close(h).expect("close");
    }
    let _ = MemoryObject::RIGHTS;
}

/// A job under this process's own, with a memory limit if given.
fn job_with(limit: Option<u64>) -> Handle {
    let job = job_create(JOB, JobFlags::NONE).expect("job");
    if let Some(memory) = limit {
        let limits = JobLimits {
            memory: Some(memory),
            processes: None,
            cpu: None,
            threads: None,
        };
        job_set_limits(job, &limits).expect("limit");
    }
    job
}

fn used(job: Handle) -> u64 {
    job_get_usage(job, false).expect("usage").memory.used
}

/// INV-0443-1 to -4 (proposed ADR-0443): the image's pages are charged to
/// the job the call names and not to the caller's own; a handle without
/// `CREATE_PROCESS` is refused and leaves nothing; a job at its memory limit
/// refuses with `OverQuota` and holds nothing, while a roomy job takes the
/// same file. The numbers are read from the jobs and set against each other.
fn an_image_is_charged_to_the_job_it_names(hello: &[u8]) {
    let (object, at, size) = object_with(hello);
    let length = hello.len() as u64;
    let (named, other) = (job_with(None), job_with(None));
    let (named_before, other_before) = (used(named), used(other));
    let own_before = used(JOB);
    let image = image_create(object, length, named).expect("charged to the named job");
    let cost = used(named) - named_before;
    assert!(cost > 0, "the pages are the named job's");
    assert_eq!(used(other), other_before, "a sibling pays nothing");
    // The caller's own job holds them once, as the named job's parent: not
    // twice (a double charge would show as nearly 2 * cost; the slack is a
    // few records and heap pages of this process).
    let own_delta = used(JOB) - own_before;
    assert!(own_delta >= cost, "the parent holds what its child holds");
    assert!(own_delta < cost + cost / 2, "and not a second time");
    // A live process of the image, in a job of its own, does not change who
    // paid for the image: it is read while the child still exists.
    let kid = job_with(None);
    let kid_before = used(kid);
    let mut buffer = [0; 8192];
    let mut e = Encoder::new(&mut buffer, kid, image);
    e.suspended().handle(console_entry());
    let request = e.finish().expect("room");
    let child = spawn(&buffer[..request]).expect("spawn");
    assert!(
        used(kid) > kid_before,
        "the child's own pages are its job's"
    );
    assert_eq!(
        used(named) - named_before,
        cost,
        "and the image stays the named job's"
    );
    job_kill(kid).expect("kill");
    let _ = wait_exit(child);
    handle_close(child).expect("close");
    handle_close(kid).expect("close");
    assert_eq!(run(image, &[]).code, 7);
    assert_eq!(used(named) - named_before, cost);
    handle_close(image).expect("close");
    assert_eq!(used(named), named_before, "the last handle gave them back");

    // Without CREATE_PROCESS: refused, and nothing is held or charged.
    let weak = handle_duplicate(named, Rights::WAIT).expect("dup");
    let frames = free_frames();
    let heap = heap_live();
    assert_err!(image_create(object, length, weak), Status::AccessDenied);
    assert_err!(image_create(object, length, object), Status::AccessDenied);
    assert_eq!(used(named), named_before);
    assert_eq!((free_frames(), heap_live()), (frames, heap), "nothing kept");
    handle_close(weak).expect("close");

    // A job with room for the image and nothing more: made, then refused,
    // while a roomy job takes the same file.
    let tight = job_with(None);
    let record = used(tight);
    job_set_limits(
        tight,
        &JobLimits {
            memory: Some(record + cost),
            processes: None,
            cpu: None,
            threads: None,
        },
    )
    .expect("limit");
    let first = image_create(object, length, tight).expect("exactly enough");
    assert_eq!(used(tight), record + cost);
    let refused = job_get_usage(tight, false).expect("usage").memory.refused;
    let frames = free_frames();
    let heap = heap_live();
    assert_err!(image_create(object, length, tight), Status::OverQuota);
    assert_eq!(used(tight), record + cost, "a refusal charges nothing");
    assert_eq!(
        job_get_usage(tight, false).expect("usage").memory.refused,
        refused + 1
    );
    assert_eq!((free_frames(), heap_live()), (frames, heap), "nothing kept");
    // More refusals than the table has slots: each keeps no room in it (a refusal
    // that kept one would end in `LimitReached` for the next call).
    for _ in 0..zoraya_abi::MAX_HANDLES + 8 {
        assert_err!(image_create(object, length, tight), Status::OverQuota);
    }
    let roomy = image_create(object, length, other).expect("the same file elsewhere");
    assert_eq!(used(other) - other_before, cost, "the same cost");
    for h in [first, roomy] {
        handle_close(h).expect("close");
    }
    assert_eq!(used(tight), record);
    assert_eq!(used(other), other_before);

    // The job ends first: the image lives, its pages stay charged to the
    // job that paid, and the last handle gives them all back.
    let doomed = job_with(None);
    let doomed_before = used(doomed);
    let image = image_create(object, length, doomed).expect("image");
    job_kill(doomed).expect("kill");
    // A job that ended takes no new charge, as `spawn` refuses it.
    let frames_dead = free_frames();
    assert_err!(image_create(object, length, doomed), Status::BadState);
    assert_eq!(free_frames(), frames_dead, "nothing kept");
    assert_eq!(used(doomed) - doomed_before, cost, "still charged");
    assert_eq!(
        run(image, &[]).code,
        7,
        "the image outlives its payer's end"
    );
    let frames = free_frames();
    let own_with = used(JOB);
    handle_close(image).expect("close");
    assert_eq!(used(doomed), doomed_before);
    assert_eq!(own_with - used(JOB), cost, "the parent gets it back too");
    assert!(free_frames() > frames, "the pages went back");
    // The caller ends first: closing the job's handle, the image keeps it.
    let held = job_with(None);
    let image = image_create(object, length, held).expect("image");
    let frames = free_frames();
    handle_close(held).expect("the image holds the job");
    assert_eq!(run(image, &[]).code, 7);
    handle_close(image).expect("close");
    assert!(free_frames() > frames, "the pages went back with the last");

    memory_unmap(at, size).expect("unmap");
    for h in [object, named, other, tight, doomed] {
        handle_close(h).expect("close");
    }
}

/// The image keeps its headers and its bytes (ADR-0415 item 2, proposed
/// ADR-0438): new bytes in the object after `image_create` reach no child.
/// Before ADR-0438 they reached the children spawned after, and harmed them.
fn rewriting_the_object_after_the_image_changes_nothing(hello: &[u8]) {
    let (object, at, size) = object_with(hello);
    let program = image_create(object, hello.len() as u64, JOB).expect("image");
    let same = handle_duplicate(program, ImageObject::RIGHTS).expect("dup");
    assert_eq!(object_same(program, same), Ok(true));
    assert_eq!(run(program, &[]).code, 7);
    // Zero every byte after the headers: the text would become `udf`.
    // SAFETY: the writable mapping of `size` bytes made above.
    let bytes = unsafe { core::slice::from_raw_parts_mut(at as *mut u8, size as usize) };
    bytes[PAGE as usize..].fill(0);
    assert_eq!(run(program, &[]).code, 7, "the image kept its own bytes");
    assert_eq!(run(same, &[]).code, 7, "and so does a copy of its handle");
    memory_unmap(at, size).expect("unmap");
    handle_close(object).expect("close");
    assert_eq!(run(program, &[]).code, 7, "with its object closed too");
    for h in [program, same] {
        handle_close(h).expect("close");
    }
}

/// INV-0040-3: a full table refuses an image and keeps nothing.
fn a_full_table_refuses_an_image(hello: &[u8]) {
    let (object, at, size) = object_with(hello);
    let mut held = alloc::vec::Vec::new();
    while let Ok(h) = handle_duplicate(object, MemoryObject::RIGHTS) {
        held.push(h);
    }
    assert_err!(
        image_create(object, hello.len() as u64, JOB),
        Status::LimitReached
    );
    for h in held {
        handle_close(h).expect("close");
    }
    let program = image_create(object, hello.len() as u64, JOB).expect("room again");
    handle_close(program).expect("close");
    memory_unmap(at, size).expect("unmap");
    handle_close(object).expect("close");
}

/// The leak loop: images made and dropped, some used.
fn images_come_and_go(hello: &[u8]) {
    let frames = free_frames();
    for round in 0..16 {
        let (object, at, size) = object_with(hello);
        let program = image_create(object, hello.len() as u64, JOB).expect("image");
        if round % 4 == 0 {
            assert_eq!(run(program, &[]).code, 7);
        }
        memory_unmap(at, size).expect("unmap");
        if round % 2 == 0 {
            handle_close(object).expect("close");
            handle_close(program).expect("close");
        } else {
            handle_close(program).expect("close");
            handle_close(object).expect("close");
        }
    }
    assert_eq!(free_frames(), frames, "every image's object went");
}

/// ADR-0109 item 5: a child finds its own program headers in its block.
fn the_startup_block_holds_the_program_headers(hello: &[u8]) {
    let me = directory_lookup(DIRECTORY, "t_image").expect("myself");
    assert_eq!(run(me, &[b"headers"]).code, 0, "from the ramdisk");
    handle_close(me).expect("close");
    let (object, at, size) = object_with(hello);
    let program = image_create(object, hello.len() as u64, JOB).expect("image");
    // `hello` reads its arguments through its description and refuses
    // these as a usage mistake (exit 2, ADR-0155): it could read the block
    // the kernel wrote.
    let long = [b'a'; 4000];
    assert_eq!(run(program, &[&long, b"b", b"c"]).code, 2);
    memory_unmap(at, size).expect("unmap");
    for h in [object, program] {
        handle_close(h).expect("close");
    }
}

/// The child's half: its block's table is its own.
fn check_own_headers() -> i32 {
    let block = startup();
    let table = block.program_headers;
    let address = table.as_ptr() as u64;
    if table.is_empty()
        || !table.len().is_multiple_of(PROGRAM_HEADER_SIZE)
        || !address.is_multiple_of(8)
    {
        return 1;
    }
    let me = check_own_headers as *const () as u64;
    let mut found = false;
    for ph in table.chunks(PROGRAM_HEADER_SIZE) {
        let word = |at: usize| u64::from_le_bytes(ph[at..at + 8].try_into().unwrap());
        let kind = u32::from_le_bytes(ph[0..4].try_into().unwrap());
        let flags = u32::from_le_bytes(ph[4..8].try_into().unwrap());
        let (vaddr, memsz) = (word(16), word(40));
        if kind == 1 && flags & 1 != 0 && vaddr <= me && me < vaddr + memsz {
            found = true;
        }
    }
    if found { 0 } else { 2 }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    if let Some(b"headers") = startup().argv.next() {
        return check_own_headers();
    }
    let hello = hello_bytes();
    assert!(hello.len() > 64 && hello.starts_with(b"\x7fELF"));
    a_program_from_the_volume_runs_as_from_the_ramdisk(&hello);
    bad_programs_are_refused_and_leave_nothing(&hello);
    image_create_checks_its_handle();
    an_image_is_charged_to_the_job_it_names(&hello);
    rewriting_the_object_after_the_image_changes_nothing(&hello);
    a_full_table_refuses_an_image(&hello);
    images_come_and_go(&hello);
    the_startup_block_holds_the_program_headers(&hello);
    println!("t_image: all passed");
    0
}
