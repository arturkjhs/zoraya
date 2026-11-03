use std::format;
use std::vec::Vec;

use super::spawn::*;
use super::*;

/// INV-0045-1: objects are named by handles, and integers are opaque
/// payloads. Listing the kinds by match means a new kind must be judged here
/// to compile.
#[test]
fn parameters_come_from_the_allow_list() {
    for s in SYSCALLS {
        for p in s.params {
            match p {
                Param::Handle
                | Param::Buffer
                | Param::Packets
                | Param::SpawnArgs
                | Param::RelativePath
                | Param::Deadline
                | Param::Flags
                | Param::ExitCode
                | Param::Detail => {}
                Param::Opaque => assert!(
                    matches!(s.name, "port_subscribe" | "port_unsubscribe" | "port_queue"),
                    "{}",
                    s.name
                ),
                Param::Cookie => assert_eq!(s.name, "directory_entry"),
                // ADR-0108 adds `image_create`'s length.
                Param::Size => assert!(
                    matches!(
                        s.name,
                        "memory_create" | "memory_map" | "memory_unmap" | "image_create"
                    ),
                    "{}",
                    s.name
                ),
                Param::Address => assert_eq!(s.name, "memory_unmap"),
                Param::Out => assert_eq!(s.name, "channel_read"),
                Param::RangeIndex => assert!(
                    matches!(s.name, "device_map" | "device_interrupt"),
                    "{}",
                    s.name
                ),
                Param::EntryPoint => assert_eq!(s.name, "thread_create"),
            }
        }
    }
}

#[test]
fn a_relative_path_is_not_absolute() {
    assert_eq!(RelativePath::new("/x"), None);
    assert_eq!(RelativePath::new(""), None);
    assert_eq!(RelativePath::new("x").map(RelativePath::as_str), Some("x"));
}

/// INV-0004-1: nothing registers code to run asynchronously.
#[test]
fn only_thread_creation_takes_an_entry_point() {
    for s in SYSCALLS
        .iter()
        .filter(|s| s.params.contains(&Param::EntryPoint))
    {
        assert_eq!(s.name, "thread_create");
    }
}

/// INV-0006-2: blocking calls take handles, a `Deadline` and a packet buffer,
/// so no signal mask, no array to scan and no relative timeout.
#[test]
fn blocking_calls_take_a_deadline_and_no_masks_or_timeouts() {
    let mut blocking = 0;
    for s in SYSCALLS.iter().filter(|s| s.blocking) {
        blocking += 1;
        assert!(s.params.contains(&Param::Deadline), "{}", s.name);
        for p in s.params {
            assert!(
                matches!(p, Param::Handle | Param::Deadline | Param::Packets),
                "{}",
                s.name
            );
        }
    }
    assert_eq!(blocking, 2);
}

/// INV-0006-6: the vocabulary has no `Interrupted`. `Status::ALL` and the enum
/// come from one list, so a new status is judged here.
#[test]
fn no_status_means_interrupted() {
    assert_eq!(Status::ALL.len(), 15);
    for (raw, s) in Status::ALL.iter().enumerate() {
        assert_eq!(*s as usize, raw);
        assert_eq!(Status::from_raw(raw as u64), Some(*s));
        assert!(!format!("{s:?}").contains("Interrupt"), "{s:?}");
    }
}

#[test]
fn an_exit_record_keeps_the_kind_apart_from_the_whole_detail() {
    for request_kind in [
        None,
        Some(RequestKind::Interrupt),
        Some(RequestKind::Terminate),
        Some(RequestKind::Hangup),
        Some(RequestKind::Quit),
        Some(RequestKind::Custom(0)),
        Some(RequestKind::Custom(31)),
    ] {
        for detail in [0, 1, 1 << 24, u32::MAX] {
            let record = ExitRecord {
                code: 0,
                reason: ExitReason::Terminated,
                detail,
                request_kind,
            };
            assert_eq!(ExitRecord::from_bytes(&record.to_bytes()), Some(record));
        }
    }
}

#[test]
fn an_exit_record_with_no_such_kind_is_refused() {
    let mut bytes = ExitRecord {
        code: 0,
        reason: ExitReason::Exited,
        detail: 0,
        request_kind: None,
    }
    .to_bytes();
    bytes[12..16].copy_from_slice(&(REQUEST_KINDS as u32 + 1).to_le_bytes());
    assert_eq!(ExitRecord::from_bytes(&bytes), None);
}

/// INV-0003-3: `spawn` takes one buffer of vectors, never a string.
#[test]
fn spawn_takes_vectors() {
    let spawn = SYSCALLS.iter().find(|s| s.name == "spawn").unwrap();
    assert_eq!(spawn.params, &[Param::SpawnArgs]);
}

/// INV-0006-1, over the list; [`ObjectType`] makes it a compile error to
/// define a type without saying.
#[test]
fn every_object_type_says_how_it_waits() {
    assert_eq!(OBJECT_TYPES.len(), 15);
    for t in OBJECT_TYPES {
        if let Waiting::Signals(bits) = t.waiting {
            assert_ne!(bits, 0, "{}", t.name);
        }
        assert_ne!(t.rights.raw(), 0, "{}", t.name);
    }
    let port = OBJECT_TYPES.iter().find(|t| t.name == "port").unwrap();
    assert!(
        !port.rights.contains(Rights::DUPLICATE),
        "ADR-0003 rule 3: a port is not duplicable"
    );
}

/// ADR-0102 item 3: the types `snapshot_process` duplicates, by name, and a
/// type is duplicable exactly when its handles can carry `DUPLICATE`.
#[test]
fn duplicable_types_are_the_ones_adr_0102_lists() {
    let duplicable: Vec<&str> = OBJECT_TYPES
        .iter()
        .filter(|t| t.duplication == Duplication::Duplicable)
        .map(|t| t.name)
        .collect();
    assert_eq!(
        duplicable,
        [
            "process",
            "job",
            "console",
            "power",
            "device",
            "event",
            "memory",
            "directory",
            "image",
            "stream-read",
            "stream-write"
        ]
    );
    for t in OBJECT_TYPES {
        assert_eq!(
            t.duplication == Duplication::Duplicable,
            t.rights.contains(Rights::DUPLICATE),
            "{}",
            t.name
        );
    }
}

/// ADR-0101: the two ends' signals and rights.
#[test]
fn stream_ends_have_the_signals_and_rights_of_adr_0101() {
    use stream_signals::*;
    assert_eq!(
        StreamReadObject::WAITING,
        Waiting::Signals(READABLE | PEER_CLOSED)
    );
    assert_eq!(
        StreamWriteObject::WAITING,
        Waiting::Signals(WRITABLE | PEER_CLOSED)
    );
    let both = Rights::WAIT
        .union(Rights::DUPLICATE)
        .union(Rights::TRANSFER);
    assert_eq!(StreamReadObject::RIGHTS, both.union(Rights::READ));
    assert_eq!(StreamWriteObject::RIGHTS, both.union(Rights::WRITE));
    assert_eq!((STREAM_CAPACITY, STREAM_ATOMIC), (65536, 4096));
}

/// INV-0002-2: the set of `#[personality_only]` calls is ADR-0002's list, one
/// entry.
#[test]
fn snapshot_process_is_the_one_personality_only_call() {
    let tagged: Vec<&str> = SYSCALLS
        .iter()
        .filter(|s| s.personality_only)
        .map(|s| s.name)
        .collect();
    assert_eq!(tagged, ["snapshot_process"]);
    for s in SYSCALLS {
        assert_eq!(
            is_personality_only(s.call as u64),
            s.personality_only,
            "{}",
            s.name
        );
    }
    assert!(!is_personality_only(u64::MAX));
}

/// ADR-0105 item 8: a job's `SUSPENDED` can be waited for.
#[test]
fn a_job_can_be_waited_on_for_suspended() {
    assert_eq!(
        JobObject::WAITING,
        Waiting::Signals(job_signals::TERMINATED | job_signals::SUSPENDED)
    );
}

/// INV-0032-1.
#[test]
fn numbers_round_trip_and_unknown_ones_are_refused() {
    for s in SYSCALLS {
        assert_eq!(
            Syscall::from_number(s.call as u64),
            Some(s.call),
            "{}",
            s.name
        );
    }
    assert_eq!(Syscall::from_number(SYSCALLS.len() as u64), None);
    assert_eq!(Syscall::from_number(u64::MAX), None);
    for (i, s) in SYSCALLS.iter().enumerate() {
        assert_eq!(s.call as usize, i, "table in number order");
    }
}

#[test]
fn registers_are_enough() {
    for s in SYSCALLS {
        let registers: usize = s.params.iter().map(|p| p.registers()).sum();
        assert!(registers <= 6, "{}", s.name);
    }
}

#[test]
fn status_round_trips() {
    assert_eq!(Status::from_raw(99), None);
    assert_eq!(
        Status::from_raw(Status::Cancelled as u64),
        Some(Status::Cancelled)
    );
}

#[test]
fn handle_zero_is_not_valid() {
    assert_eq!(Handle::new(0), None);
    assert_eq!(Handle::new(1), Some(CONSOLE));
    assert_eq!(Handle::from_parts(0, 3), None);
    assert_eq!(Handle::from_parts(MAX_HANDLES + 1, 0), None);
    let h = Handle::from_parts(5, 9).unwrap();
    assert_eq!((h.slot(), h.generation()), (5, 9));
    assert_eq!(Handle::from_parts(3, 0), Some(DIRECTORY));
}

#[derive(Default)]
struct Echo {
    last: Option<&'static str>,
    keys: Vec<u64>,
}

macro_rules! echo {
    ($($name:ident($($arg:ident: $type:ty),*);)*) => {
        impl Handlers for Echo {
            $(fn $name(&mut self, $($arg: $type),*) -> Result<u64, Status> {
                self.last = Some(stringify!($name));
                $(let _ = $arg;)*
                Ok(0)
            })*
        }
    };
}

echo! {
    process_exit(code: ExitCode);
    thread_yield();
    console_write(console: Handle, bytes: Bytes);
    clock_monotonic();
    sleep_until(deadline: Deadline);
    handle_close(handle: Handle);
    handle_duplicate(handle: Handle, rights: Rights);
    job_create(parent: Handle, flags: JobFlags);
    spawn(args: SpawnBuffer);
    process_resume(process: Handle);
    process_exit_with_reason(reason: ExitReason, detail: Detail);
    process_exit_record(process: Handle, record: Bytes);
    process_signal(process: Handle, kind: RequestKind, detail: Detail);
    process_set_disposition(kind: RequestKind, disposition: Disposition);
    process_take_request();
    job_signal(job: Handle, kind: RequestKind, detail: Detail);
    job_kill(job: Handle);
    port_create();
    port_subscribe(port: Handle, handle: Handle, signals: SignalSet, key: Opaque, mode: Mode);
    port_unsubscribe(port: Handle, key: Opaque);
    port_queue(port: Handle, key: Opaque, data: Opaque);
    port_wait(port: Handle, deadline: Deadline, packets: PacketBuffer);
    directory_lookup(directory: Handle, name: PathBuffer);
    console_read(console: Handle, bytes: Bytes);
    console_dropped(console: Handle);
    directory_entry(directory: Handle, cookie: Cookie, entry: Bytes);
    power_off(power: Handle);
    memory_create(size: Size, flags: MemoryFlags);
    memory_map(object: Handle, offset: Size, length: Size, flags: MapFlags);
    memory_unmap(address: Address, length: Size);
    channel_create(ends: Bytes);
    channel_write(end: Handle, bytes: Bytes, handles: Bytes);
    channel_read(end: Handle, bytes: Bytes, handles: Bytes, sizes: Out);
    device_map(device: Handle, index: RangeIndex, out: Bytes);
    device_interrupt(device: Handle, index: RangeIndex);
    interrupt_ack(interrupt: Handle);
    dma_pin(memory: Handle, device: Handle, out: Bytes);
    dma_unpin(pin: Handle);
    dma_release_quarantine(device: Handle);
    handle_type(handle: Handle);
    event_create();
    object_same(a: Handle, b: Handle);
    stream_create(ends: Bytes);
    stream_read(end: Handle, bytes: Bytes);
    stream_write(end: Handle, bytes: Bytes);
    snapshot_process(args: SnapshotBuffer);
    image_create(memory: Handle, length: Size);
    job_suspend(job: Handle);
    job_resume(job: Handle);
    object_get_signals(handle: Handle);
}

/// INV-0044-1: each number reaches the handler of the name the table gives.
#[test]
fn every_number_reaches_its_handler() {
    for s in SYSCALLS {
        let mut echo = Echo::default();
        let registers = [1; 6];
        let result = dispatch(&mut echo, s.call as u64, &registers);
        assert_eq!(result, Ok(0), "{}", s.name);
        assert_eq!(echo.last, Some(s.name));
    }
    assert!(Echo::default().keys.is_empty());
    let mut echo = Echo::default();
    assert_eq!(
        dispatch(&mut echo, SYSCALLS.len() as u64, &[0; 6]),
        Err(Status::BadSyscall)
    );
}

/// INV-0044-2, for each parameter type that is narrower than a register.
#[test]
fn a_value_too_wide_is_refused_not_truncated() {
    let wide = 1u64 << 32;
    assert_eq!(Handle::decode(&[0]), Err(Status::BadHandle));
    assert_eq!(ExitCode::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(Detail::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(SignalSet::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(JobFlags::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(Rights::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(RequestKind::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(Disposition::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(Mode::decode(&[wide]), Err(Status::InvalidArgument));
    assert_eq!(ExitReason::decode(&[wide]), Err(Status::InvalidArgument));
    // And out-of-range values within 32 bits.
    assert_eq!(Rights::decode(&[1 << 14]), Err(Status::InvalidArgument));
    assert_eq!(RequestKind::decode(&[36]), Err(Status::InvalidArgument));
    assert_eq!(RequestKind::decode(&[35]), Ok(RequestKind::Custom(31)));
    assert_eq!(Disposition::decode(&[3]), Err(Status::InvalidArgument));
    assert_eq!(Mode::decode(&[2]), Err(Status::InvalidArgument));
    assert_eq!(ExitReason::decode(&[5]), Err(Status::InvalidArgument));
    assert_eq!(JobFlags::decode(&[2]), Err(Status::InvalidArgument));
}

#[test]
fn an_argument_round_trips_through_the_stub() {
    struct Loop;
    static SEEN: std::sync::Mutex<[u64; 7]> = std::sync::Mutex::new([0; 7]);
    impl Invoke for Loop {
        fn invoke(number: u64, r: [u64; 6]) -> (u64, u64) {
            let mut seen = SEEN.lock().unwrap();
            seen[0] = number;
            seen[1..].copy_from_slice(&r);
            (0, 5)
        }
    }
    let port = Handle::new(7).unwrap();
    let value = stubs::port_wait::<Loop>(
        port,
        Deadline(9),
        PacketBuffer(Buffer {
            address: 0x1000,
            length: 3,
        }),
    );
    assert_eq!(value, Ok(5));
    assert_eq!(*SEEN.lock().unwrap(), [21, 7, 9, 0x1000, 3, 0, 0]);
}

#[test]
fn exit_records_and_packets_encode() {
    let record = ExitRecord {
        code: -2,
        reason: ExitReason::Fault,
        detail: 0x24,
        request_kind: None,
    };
    assert_eq!(ExitRecord::from_bytes(&record.to_bytes()), Some(record));
    let packet = Packet {
        key: 1,
        kind: PACKET_USER,
        signals: 2,
        data: 3,
    };
    assert_eq!(size_of::<Packet>(), Packet::SIZE);
    assert_eq!(packet.to_bytes()[8], 1);
    let (kind, detail) = request_from_value(request_value(RequestKind::Custom(3), 9)).unwrap();
    assert_eq!((kind, detail), (RequestKind::Custom(3), 9));
}

fn sample(buffer: &mut [u8]) -> usize {
    let (job, image) = (JOB, DIRECTORY);
    let mut e = Encoder::new(buffer, job, image);
    e.suspended()
        .group(Handle::new(9).unwrap())
        .disposition(RequestKind::Hangup, Disposition::Ignore)
        .handle(HandleEntry {
            slot: 3,
            handle: Handle::new(4).unwrap(),
            rights: Rights::WAIT,
            transfer: Transfer::Move,
        })
        .handle(HandleEntry {
            slot: 4,
            handle: Handle::new(5).unwrap(),
            rights: Rights::NONE,
            transfer: Transfer::Duplicate,
        })
        .arg(b"name")
        .arg(b"a\0b")
        .env(b"K=V");
    e.finish().unwrap()
}

#[test]
fn a_request_round_trips() {
    let mut b = [0; 512];
    let n = sample(&mut b);
    let r = SpawnRequest::decode(&b[..n]).unwrap();
    assert!(r.suspended);
    assert_eq!((r.job, r.image), (JOB, DIRECTORY));
    assert_eq!(r.group, Handle::new(9));
    assert_eq!(r.dispositions[2], Disposition::Ignore);
    assert_eq!(r.dispositions[0], Disposition::Terminate);
    let handles: Vec<_> = r.handles().collect();
    assert_eq!(handles.len(), r.handle_count());
    assert_eq!(handles[1].slot, 4);
    assert_eq!(handles[1].transfer, Transfer::Duplicate);
    let argv: Vec<_> = r.argv.collect();
    assert_eq!(argv, [&b"name"[..], &b"a\0b"[..]]);
    assert_eq!(r.envp.collect::<Vec<_>>(), [&b"K=V"[..]]);
}

#[test]
fn the_startup_block_round_trips() {
    let mut b = [0; 512];
    let n = sample(&mut b);
    let r = SpawnRequest::decode(&b[..n]).unwrap();
    let mut block = Vec::new();
    let table = [0x5au8; 2 * PROGRAM_HEADER_SIZE];
    block.extend_from_slice(&startup_header(r.argv.len(), r.envp.len(), 2));
    block.extend_from_slice(r.argv.as_bytes());
    block.extend_from_slice(r.envp.as_bytes());
    let strings = block.len() - STARTUP_HEADER_SIZE;
    block.resize(startup_table_offset(strings), 0);
    assert!(block.len().is_multiple_of(8));
    block.extend_from_slice(&table);
    let s = StartupBlock::decode(&block).unwrap();
    assert_eq!(s.argv.len(), 2);
    assert_eq!(s.envp.collect::<Vec<_>>(), [&b"K=V"[..]]);
    assert_eq!(s.program_headers, &table[..]);
    // A table cut short, or a header of another size, is refused.
    assert_eq!(StartupBlock::decode(&block[..block.len() - 1]), None);
    let mut other = block.clone();
    other[8] = 8;
    assert_eq!(StartupBlock::decode(&other), None);
    let empty = startup_header(0, 0, 0);
    let s = StartupBlock::decode(&empty).unwrap();
    assert!(s.argv.is_empty() && s.envp.is_empty() && s.program_headers.is_empty());
    assert_eq!(Strings::empty().count(), 0);
}

/// INV-0042-2: no prefix or mutation panics, and what is accepted iterates to
/// exactly its declared counts.
#[test]
fn the_decoder_is_total() {
    let mut b = [0; 512];
    let n = sample(&mut b);
    for length in 0..n {
        assert!(SpawnRequest::decode(&b[..length]).is_err(), "{length}");
    }
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        x
    };
    let mut accepted = 0;
    for _ in 0..200_000 {
        let mut m = b;
        for _ in 0..=next() % 3 {
            m[next() as usize % n] = next() as u8;
        }
        if let Ok(r) = SpawnRequest::decode(&m[..n]) {
            accepted += 1;
            assert_eq!(r.handles().count(), r.handle_count());
            assert_eq!(r.argv.count(), r.argv.len());
            assert_eq!(r.envp.count(), r.envp.len());
        }
        let _ = StartupBlock::decode(&m[..n]);
    }
    assert!(accepted > 0, "the mutations must reach the end of decoding");
}

#[test]
fn trailing_bytes_flags_and_reserved_fields_are_refused() {
    let mut b = [0; 512];
    let n = sample(&mut b);
    assert!(SpawnRequest::decode(&b[..n + 1]).is_err());
    for (at, value) in [
        (0, 0),
        (4, 0),
        (8, 2),
        (12, 1),
        (52, 1),
        (HEADER_SIZE - 1, 1),
    ] {
        let mut m = b;
        m[at] = value;
        assert_eq!(
            SpawnRequest::decode(&m[..n]).map(|_| ()),
            Err(Status::InvalidArgument),
            "byte {at}"
        );
    }
}

/// One entry alone must be checked too: `handles` would otherwise hide it.
#[test]
fn a_single_bad_entry_is_refused() {
    let mut buffer = [0; 256];
    let mut e = Encoder::new(&mut buffer, JOB, DIRECTORY);
    e.handle(HandleEntry {
        slot: 3,
        handle: Handle::new(4).unwrap(),
        rights: Rights::NONE,
        transfer: Transfer::Move,
    });
    let n = e.finish().unwrap();
    assert!(SpawnRequest::decode(&buffer[..n]).is_ok());
    for (at, value, status) in [
        (HEADER_SIZE, 0, Status::InvalidArgument),
        (HEADER_SIZE + 4, 1, Status::InvalidArgument),
        (HEADER_SIZE + 8, 0, Status::BadHandle),
        (HEADER_SIZE + 17, 0xff, Status::InvalidArgument),
        (HEADER_SIZE + 20, 7, Status::InvalidArgument),
    ] {
        let mut m = buffer;
        m[at] = value;
        assert_eq!(
            SpawnRequest::decode(&m[..n]).map(|_| ()),
            Err(status),
            "{at}"
        );
    }
}

#[test]
fn a_slot_or_a_moved_handle_twice_is_refused() {
    let entry = |slot, handle, transfer| HandleEntry {
        slot,
        handle: Handle::new(handle).unwrap(),
        rights: Rights::NONE,
        transfer,
    };
    let refused = |a, b| {
        let mut buffer = [0; 256];
        let mut e = Encoder::new(&mut buffer, JOB, DIRECTORY);
        e.handle(a).handle(b);
        let n = e.finish().unwrap();
        SpawnRequest::decode(&buffer[..n]).is_err()
    };
    use Transfer::{Duplicate, Move};
    assert!(refused(entry(3, 4, Duplicate), entry(3, 5, Duplicate)));
    assert!(refused(entry(3, 4, Move), entry(5, 4, Duplicate)));
    assert!(refused(entry(3, 4, Duplicate), entry(5, 4, Move)));
    assert!(!refused(entry(3, 4, Duplicate), entry(5, 4, Duplicate)));
}

#[test]
fn an_encoder_that_runs_out_of_room_says_so() {
    let mut small = [0; 10];
    assert_eq!(Encoder::new(&mut small, JOB, DIRECTORY).finish(), None);
    let mut b = [0; HEADER_SIZE + 3];
    let mut e = Encoder::new(&mut b, JOB, DIRECTORY);
    e.arg(b"long");
    assert_eq!(e.finish(), None);
}

const _: () = assert!(STARTUP_BLOCK.is_multiple_of(4096));
const _: () = assert!(STARTUP_BLOCK + STARTUP_BLOCK_MAX < USER_STACK_TOP - USER_STACK_SIZE);

/// INV-0061-4.
#[test]
fn a_directory_entry_round_trips_and_its_decoder_is_total() {
    let entry = DirectoryEntry {
        next: 7,
        name: b"hello",
    };
    let mut buffer = [0; 32];
    let size = entry.encode(&mut buffer).unwrap();
    assert_eq!(size, 21);
    assert_eq!(DirectoryEntry::decode(&buffer[..size]), Some(entry));
    assert_eq!(entry.encode(&mut buffer[..20]), None);
    for cut in 0..size {
        assert_eq!(DirectoryEntry::decode(&buffer[..cut]), None, "{cut}");
    }
    for at in 0..size {
        for bit in 0..8 {
            let mut bytes = buffer;
            bytes[at] ^= 1 << bit;
            let _ = DirectoryEntry::decode(&bytes[..size]);
        }
    }
    buffer[12] = 1;
    assert_eq!(DirectoryEntry::decode(&buffer[..size]), None, "reserved");
}

/// INV-0062-1: only `PowerObject` carries `POWER`, and it carries nothing that
/// could do anything else.
#[test]
fn power_is_one_right_on_one_type() {
    assert_eq!(
        PowerObject::RIGHTS,
        Rights::POWER
            .union(Rights::DUPLICATE)
            .union(Rights::TRANSFER)
    );
    let holders = OBJECT_TYPES
        .iter()
        .filter(|t| t.rights.contains(Rights::POWER));
    assert_eq!(holders.count(), 1);
}

/// INV-0057-3: a generation above 32 bits survives every crossing.
#[test]
fn a_handle_is_64_bits_wherever_it_goes() {
    let high = Handle::from_parts(9, 0xabcd_1234_5678).unwrap();
    assert_eq!(high.slot(), 9);
    assert_eq!(high.generation(), 0xabcd_1234_5678);
    assert_eq!(Handle::new(high.raw()), Some(high));
    let mut register = [0];
    high.encode(&mut register);
    assert_eq!(Handle::decode(&register), Ok(high));

    let mut buffer = [0; 1024];
    let mut e = Encoder::new(&mut buffer, high, high);
    e.group(high).handle(HandleEntry {
        slot: 3,
        handle: high,
        rights: Rights::NONE,
        transfer: Transfer::Duplicate,
    });
    let length = e.finish().unwrap();
    let r = SpawnRequest::decode(&buffer[..length]).unwrap();
    assert_eq!((r.job, r.image, r.group), (high, high, Some(high)));
    assert_eq!(r.handles().next().unwrap().handle, high);
}

#[test]
fn a_generation_past_48_bits_is_refused() {
    assert_eq!(Handle::from_parts(1, 1 << 48), None);
    assert!(Handle::from_parts(1, MAX_GENERATION).is_some());
}

#[test]
fn the_old_format_is_refused() {
    let mut buffer = [0; HEADER_SIZE];
    let length = Encoder::new(&mut buffer, CONSOLE, CONSOLE)
        .finish()
        .unwrap();
    buffer[3] = b'1';
    assert_eq!(
        SpawnRequest::decode(&buffer[..length]).map(|_| ()),
        Err(Status::InvalidArgument)
    );
}

/// INV-0050-5.
#[test]
fn a_channel_has_no_duplicate_right() {
    assert!(!ChannelObject::RIGHTS.contains(Rights::DUPLICATE));
    assert!(ChannelObject::RIGHTS.contains(Rights::TRANSFER));
}

/// ADR-0032's six registers hold every call (ADR-0050's `Out`).
#[test]
fn every_call_fits_six_registers() {
    for s in SYSCALLS {
        let used: usize = s.params.iter().map(|p| p.registers()).sum();
        assert!(used <= 6, "{}", s.name);
    }
}

#[test]
fn a_channel_entry_round_trips_and_refuses_bad_values() {
    let entry = ChannelEntry {
        handle: Handle::from_parts(5, 1 << 40).unwrap(),
        rights: Rights::READ,
        transfer: Transfer::Duplicate,
    };
    assert_eq!(ChannelEntry::from_bytes(&entry.to_bytes()), Ok(entry));
    let mut b = entry.to_bytes();
    b[0..8].fill(0);
    assert_eq!(ChannelEntry::from_bytes(&b), Err(Status::BadHandle));
    let mut b = entry.to_bytes();
    b[11] = 0xff;
    assert_eq!(ChannelEntry::from_bytes(&b), Err(Status::InvalidArgument));
    let mut b = entry.to_bytes();
    b[12] = 2;
    assert_eq!(ChannelEntry::from_bytes(&b), Err(Status::InvalidArgument));
    let sizes = ChannelSizes {
        bytes: 7,
        handles: 2,
    };
    assert_eq!(ChannelSizes::from_bytes(&sizes.to_bytes()), sizes);
}

mod snapshots {
    use std::vec::Vec;

    use crate::snapshot::SnapshotRequest;
    use crate::snapshot::{Encoder, HEADER_SIZE, MAX_MOVES, MAX_SNAPSHOT_SIZE, Move};
    use crate::{Handle, MAX_HANDLES, Status};

    fn h(slot: u32, generation: u64) -> Handle {
        Handle::from_parts(slot, generation).unwrap()
    }

    fn sample(buffer: &mut [u8]) -> usize {
        Encoder::new(buffer, h(2, 0))
            .group(h(7, 3))
            .self_slot(9)
            .suspended()
            .moved(Move {
                slot: 4,
                handle: h(30, 1),
            })
            .moved(Move {
                slot: 5,
                handle: h(31, 0xabcd_1234_5678),
            })
            .finish()
            .unwrap()
    }

    /// INV-0057-3 through the snapshot buffer, and the fields round trip.
    #[test]
    fn a_request_round_trips_with_64_bit_handles() {
        let mut b = [0; 256];
        let n = sample(&mut b);
        assert_eq!(n, HEADER_SIZE + 2 * 16);
        let r = SnapshotRequest::decode(&b[..n]).unwrap();
        assert!(r.suspended);
        assert_eq!(
            (r.job, r.group, r.self_slot),
            (h(2, 0), Some(h(7, 3)), Some(9))
        );
        let moves: Vec<Move> = r.moves().collect();
        assert_eq!(moves.len(), r.move_count());
        assert_eq!(moves[1].handle.generation(), 0xabcd_1234_5678);
        assert_eq!(moves[0].slot, 4);
        let mut b = [0; HEADER_SIZE];
        let n = Encoder::new(&mut b, h(2, 0)).finish().unwrap();
        let r = SnapshotRequest::decode(&b[..n]).unwrap();
        assert!(!r.suspended && r.group.is_none() && r.self_slot.is_none());
        assert_eq!(r.moves().count(), 0);
    }

    /// The decoder is total: every prefix and many mutations, no panic, and
    /// what is accepted iterates to its count.
    #[test]
    fn the_snapshot_decoder_is_total() {
        let mut b = [0; 256];
        let n = sample(&mut b);
        for length in 0..n {
            assert!(SnapshotRequest::decode(&b[..length]).is_err(), "{length}");
        }
        let mut x = 0x9e37_79b9_7f4a_7c15u64;
        let mut next = move || {
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            x.wrapping_mul(0x2545_f491_4f6c_dd1d)
        };
        for _ in 0..20_000 {
            let mut m = b;
            for _ in 0..1 + next() % 4 {
                let at = (next() % n as u64) as usize;
                m[at] = next() as u8;
            }
            let length = if next() % 4 == 0 {
                (next() % 256) as usize
            } else {
                n
            };
            if let Ok(r) = SnapshotRequest::decode(&m[..length]) {
                assert_eq!(r.moves().count(), r.move_count());
            }
        }
    }

    #[test]
    fn bad_buffers_are_refused() {
        let mut b = [0; 256];
        let n = sample(&mut b);
        let refused = |m: &[u8]| SnapshotRequest::decode(m).map(|_| ()).err();
        let bad = Some(Status::InvalidArgument);
        let mut m = b;
        m[3] = b'2';
        assert_eq!(refused(&m[..n]), bad, "magic");
        let mut m = b;
        m[4] = 40;
        assert_eq!(refused(&m[..n]), bad, "header size");
        let mut m = b;
        m[8] = 3;
        assert_eq!(refused(&m[..n]), bad, "unknown flag");
        for at in [12, 40, 47, HEADER_SIZE + 4] {
            let mut m = b;
            m[at] = 1;
            assert_eq!(refused(&m[..n]), bad, "reserved at {at}");
        }
        assert_eq!(refused(&b[..n + 1]), bad, "trailing byte");
        let mut m = b;
        m[16..24].fill(0);
        assert_eq!(refused(&m[..n]), Some(Status::BadHandle), "no job");
        let mut m = b;
        m[HEADER_SIZE + 8..HEADER_SIZE + 16].fill(0);
        assert_eq!(refused(&m[..n]), Some(Status::BadHandle), "no handle");
        let mut m = b;
        m[HEADER_SIZE..HEADER_SIZE + 4].fill(0);
        assert_eq!(refused(&m[..n]), bad, "slot 0");
        let mut m = b;
        m[HEADER_SIZE..HEADER_SIZE + 4].copy_from_slice(&(MAX_HANDLES + 1).to_le_bytes());
        assert_eq!(refused(&m[..n]), bad, "slot past the table");
        let mut m = b;
        m[32..36].copy_from_slice(&(MAX_HANDLES + 1).to_le_bytes());
        assert_eq!(refused(&m[..n]), bad, "self slot past the table");
        let mut m = b;
        m[32..36].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(refused(&m[..n]), bad, "self slot also moved into");
        let mut m = b;
        m[HEADER_SIZE + 16..HEADER_SIZE + 20].copy_from_slice(&4u32.to_le_bytes());
        assert_eq!(refused(&m[..n]), bad, "one slot twice");
        let mut m = b;
        let first = m[HEADER_SIZE + 8..HEADER_SIZE + 16].to_vec();
        m[HEADER_SIZE + 24..HEADER_SIZE + 32].copy_from_slice(&first);
        assert_eq!(refused(&m[..n]), bad, "one handle moved twice");
        let mut m = b;
        m[36] = 3;
        assert_eq!(refused(&m[..n]), bad, "count beyond the bytes");
    }

    #[test]
    fn the_move_list_is_bounded() {
        let mut b = [0; MAX_SNAPSHOT_SIZE + 64];
        let mut e = Encoder::new(&mut b, h(2, 0));
        for i in 0..=MAX_MOVES as u32 {
            e.moved(Move {
                slot: 10 + i,
                handle: h(100 + i, 0),
            });
        }
        let n = e.finish().unwrap();
        assert_eq!(n, MAX_SNAPSHOT_SIZE + 16);
        assert_eq!(
            SnapshotRequest::decode(&b[..n]).map(|_| ()),
            Err(Status::InvalidArgument)
        );
        let mut small = [0; HEADER_SIZE + 8];
        let mut e = Encoder::new(&mut small, h(2, 0));
        e.moved(Move {
            slot: 1,
            handle: h(1, 0),
        });
        assert_eq!(e.finish(), None, "no room is reported");
        let mut tiny = [0; 4];
        assert_eq!(Encoder::new(&mut tiny, h(2, 0)).finish(), None);
    }
}
