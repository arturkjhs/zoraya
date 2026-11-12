#![no_std]
#![no_main]

//! Device, interrupt and DMA objects (ADR-0054, ADR-0070) from a process that
//! holds the devices at slots 5 on, with a virtio-blk device attached and no
//! driver.

use zoraya_abi::spawn::{Encoder, HandleEntry, Transfer};
use zoraya_abi::{
    ChannelEntry, DIRECTORY, DeviceObject, FIRST_DEVICE_SLOT, Handle, JOB, MapFlags, MemoryFlags,
    Mode, ObjectType, Packet, Rights, Status, interrupt_signals,
};
use zoraya_user::{
    assert_err, channel_create, channel_read, channel_write, device_interrupt, device_map,
    device_read, device_transport, device_write, directory_lookup, dma_pin, dma_release_quarantine,
    dma_unpin, handle_close, handle_duplicate, interrupt_ack, memory_create, memory_map,
    memory_unmap, port_create, port_subscribe, port_wait, raw_syscall, spawn, wait_exit,
};

const VIRTIO_MAGIC: u32 = 0x7472_6976;
const REG_DEVICE: u64 = 0x08;
const REG_STATUS: u64 = 0x70;
const DEVICE_BLOCK: u32 = 2;
const PAGE: u64 = 4096;

const TRIGGER_INTERRUPT: u64 = 0x7005;
const FREE_FRAMES: u64 = 0x7006;
const DEVICE_MAPPINGS: u64 = 0x7007;
const FIRST_LINE: u64 = 0x7009;
const LINE_ENABLED: u64 = 0x700a;

fn test_call(number: u64) -> u64 {
    raw_syscall(number, [0; 6]).1
}

fn read(address: u64) -> u32 {
    // SAFETY: the caller mapped `address` as a device.
    unsafe { core::ptr::read_volatile(address as *const u32) }
}

fn write(address: u64, value: u32) {
    // SAFETY: as in `read`.
    unsafe { core::ptr::write_volatile(address as *mut u32, value) }
}

fn device(index: u32) -> Option<Handle> {
    let handle = Handle::new(u64::from(FIRST_DEVICE_SLOT + index))?;
    handle_duplicate(handle, Rights::NONE).ok().map(|copy| {
        handle_close(copy).expect("close");
        handle
    })
}

/// The device and range of the block device.
fn find_block_device() -> (Handle, u32) {
    let mut index = 0;
    while let Some(device) = device(index) {
        index += 1;
        let (window, _) = device_map(device, 0).expect("map");
        let mut range = 0;
        // One device per page: its transports are 0x200 apart.
        while range < 8 {
            if read(window + u64::from(range) * 0x200 + REG_DEVICE) == DEVICE_BLOCK {
                memory_unmap(window, PAGE).expect("unmap");
                return (device, range);
            }
            range += 1;
        }
        memory_unmap(window, PAGE).expect("unmap");
    }
    panic!("no virtio-blk device: is --test-disk on?");
}

/// plan.md step 5: a user program reads the virtio magic value.
fn maps_a_device() {
    let (device, range) = find_block_device();
    let (window, offset) = device_map(device, range).expect("map");
    assert_eq!(offset, u64::from(range) * 0x200);
    assert_eq!(read(window + offset), VIRTIO_MAGIC);
    assert_eq!(read(window + offset + REG_DEVICE), DEVICE_BLOCK);
    assert_eq!(test_call(DEVICE_MAPPINGS), 1, "INV-0054-1");
    assert_err!(device_map(device, range), Status::BadState);
    assert_err!(device_map(device, 99), Status::InvalidArgument);
    memory_unmap(window, PAGE).expect("unmap");
    assert_eq!(test_call(DEVICE_MAPPINGS), 0);
    let (again, _) = device_map(device, range).expect("one holder at a time");
    memory_unmap(again, PAGE).expect("unmap");
    let no_map = handle_duplicate(device, Rights::INTERRUPT).expect("narrow");
    assert_err!(device_map(no_map, 0), Status::AccessDenied);
    handle_close(no_map).expect("close");
}

/// plan.md step 5, and INV-0054-2: one packet, then none until the ack.
fn takes_an_interrupt() {
    let (device, range) = find_block_device();
    let interrupt = device_interrupt(device, range).expect("interrupt");
    assert_err!(device_interrupt(device, range), Status::BadState);
    assert_err!(device_interrupt(device, 99), Status::InvalidArgument);
    let port = port_create().expect("port");
    port_subscribe(port, interrupt, interrupt_signals::INTERRUPT, 1, Mode::Edge)
        .expect("subscribe");
    let mut packets = [Packet::default()];
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    test_call(TRIGGER_INTERRUPT);
    assert_eq!(
        port_wait(port, zoraya_abi::DEADLINE_INFINITE, &mut packets),
        Ok(1)
    );
    assert_eq!(packets[0].signals, interrupt_signals::INTERRUPT);
    test_call(TRIGGER_INTERRUPT);
    assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
    interrupt_ack(interrupt).expect("ack");
    assert_eq!(
        port_wait(port, zoraya_abi::DEADLINE_INFINITE, &mut packets),
        Ok(1),
        "the second, once acknowledged"
    );
    interrupt_ack(interrupt).expect("ack");
    let line = test_call(FIRST_LINE);
    assert_ne!(line, 0);
    assert_eq!(raw_syscall(LINE_ENABLED, [line, 0, 0, 0, 0, 0]).1, 1);
    handle_close(interrupt).expect("close");
    assert_eq!(
        raw_syscall(LINE_ENABLED, [line, 0, 0, 0, 0, 0]).1,
        0,
        "INV-0054-2: closing masks the line"
    );
    handle_close(port).expect("close");
    // Masked and given back: another object can take the line.
    let again = device_interrupt(device, range).expect("the line is free");
    handle_close(again).expect("close");
}

fn set_status(device: Handle, range: u32, value: u32) -> u32 {
    let (window, offset) = device_map(device, range).expect("map");
    write(window + offset + REG_STATUS, value);
    let now = read(window + offset + REG_STATUS);
    memory_unmap(window, PAGE).expect("unmap");
    now
}

/// `dma_pin` gives device addresses, `dma_unpin` ends a pin.
fn pins_and_unpins() {
    let (device, _) = find_block_device();
    let free = test_call(FREE_FRAMES);
    let memory = memory_create(2 * PAGE, MemoryFlags::CONTIGUOUS).expect("memory");
    let mut runs = [[0; 2]; 4];
    let pin = dma_pin(memory, device, &mut runs).expect("pin");
    assert_ne!(runs[0][0], 0);
    assert_eq!(runs[0][1], 2 * PAGE, "one run");
    assert_eq!(runs[1], [0, 0]);
    // Unmapping and closing everything else leaves the pin holding it.
    let at = memory_map(memory, 0, PAGE, MapFlags::WRITE).expect("map");
    memory_unmap(at, PAGE).expect("unmap in use by a pin");
    handle_close(memory).expect("close");
    assert_eq!(test_call(FREE_FRAMES), free - 2, "held by the pin");
    dma_unpin(pin).expect("unpin");
    assert_eq!(test_call(FREE_FRAMES), free, "freed with its last holder");
    assert_err!(dma_unpin(pin), Status::BadHandle);
}

fn refusals() {
    let (device, _) = find_block_device();
    let memory = memory_create(2 * PAGE, MemoryFlags::NONE).expect("memory");
    let read_only = handle_duplicate(memory, Rights::READ).expect("narrow");
    let mut runs = [[0; 2]; 4];
    assert_err!(dma_pin(read_only, device, &mut runs), Status::AccessDenied);
    let no_dma = handle_duplicate(device, Rights::MAP).expect("narrow");
    assert_err!(dma_pin(memory, no_dma, &mut runs), Status::AccessDenied);
    assert_err!(dma_release_quarantine(no_dma), Status::AccessDenied);
    // Too small for a run, and nothing is pinned.
    let scattered = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    assert_err!(dma_pin(scattered, device, &mut []), Status::InvalidArgument);
    for h in [read_only, no_dma, memory, scattered] {
        handle_close(h).expect("close");
    }
}

/// R-9 of the review: a failed `dma_pin` makes no pin, so nothing is
/// quarantined: the frames go when the object does.
fn a_refused_pin_leaves_nothing() {
    let (device, _) = find_block_device();
    let memory = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    let mut fill = [memory; 1024];
    let mut filled = 0;
    while let Ok(port) = port_create() {
        fill[filled] = port;
        filled += 1;
    }
    let mut runs = [[0; 2]; 2];
    assert_err!(dma_pin(memory, device, &mut runs), Status::LimitReached);
    for port in &fill[..filled] {
        handle_close(*port).expect("close");
    }
    let free = test_call(FREE_FRAMES);
    handle_close(memory).expect("close");
    assert_eq!(test_call(FREE_FRAMES), free + 1);
}

/// INV-0054-7's second half: a pin handle without `DMA` cannot unpin.
fn unpinning_needs_its_right() {
    let (device, _) = find_block_device();
    let memory = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    let mut runs = [[0; 2]; 2];
    let pin = dma_pin(memory, device, &mut runs).expect("pin");
    let (a, b) = channel_create().expect("create");
    let entry = ChannelEntry {
        handle: pin,
        rights: Rights::TRANSFER,
        transfer: Transfer::Move,
    };
    channel_write(a, b"", &[entry]).expect("write");
    let mut body = [0; 1];
    let mut got = [a; 1];
    channel_read(b, &mut body, &mut got).expect("read");
    assert_err!(dma_unpin(got[0]), Status::AccessDenied);
    for h in [got[0], a, b, memory] {
        handle_close(h).expect("close");
    }
    dma_release_quarantine(device).expect("release");
}

/// INV-0054-7 and ADR-0070: a pin closed with the device active stays
/// quarantined until the kernel has stopped the device.
fn a_closed_pin_stays_quarantined() {
    let (device, range) = find_block_device();
    let free = test_call(FREE_FRAMES);
    let memory = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    let mut runs = [[0; 2]; 4];
    let pin = dma_pin(memory, device, &mut runs).expect("pin");
    // A driver at work: ACKNOWLEDGE and DRIVER.
    assert_eq!(set_status(device, range, 3), 3);
    // INV-0070-1: a live pin refuses.
    assert_err!(dma_release_quarantine(device), Status::BadState);
    handle_close(memory).expect("close");
    handle_close(pin).expect("close with no unpin");
    assert_eq!(test_call(FREE_FRAMES), free - 1, "INV-0054-7");
    // INV-0070-1: a mapping refuses, since it could restart the device.
    let (window, offset) = device_map(device, range).expect("map");
    assert_err!(dma_release_quarantine(device), Status::BadState);
    assert_eq!(test_call(FREE_FRAMES), free - 1);
    memory_unmap(window, PAGE).expect("unmap");
    // INV-0070-2: only the kernel's reset ends it.
    dma_release_quarantine(device).expect("release");
    assert_eq!(test_call(FREE_FRAMES), free);
    let (window, _) = device_map(device, range).expect("map");
    assert_eq!(
        read(window + offset + REG_STATUS),
        0,
        "the device was reset"
    );
    memory_unmap(window, PAGE).expect("unmap");
}

/// Runs `child write-register` on `transport` and gives its exit code.
fn second_writer(transport: Handle) -> i32 {
    let image = directory_lookup(DIRECTORY, "child").expect("child");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, JOB, image);
    e.handle(HandleEntry {
        slot: 4,
        handle: transport,
        rights: Rights::WRITE,
        transfer: Transfer::Duplicate,
    });
    e.arg(b"write-register");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    let code = wait_exit(child).code;
    handle_close(child).expect("close");
    code
}

/// INV-0295-3 (review N6 finding 1): a transport and its page are one
/// device to ADR-0070. A mapping of the page, or a process that wrote a
/// transport's registers (its holder), could restart it: either refuses
/// the other, a second holder, and the release of the quarantine.
fn a_transport_and_its_page_keep_holders_apart() {
    let (page, range) = find_block_device();
    let t = device_transport(page, range).expect("transport");
    // The review's scenario: the page mapped, a pin through the transport
    // ended without `dma_unpin`, then the transport's release.
    let (window, _) = device_map(page, range).expect("map");
    let free = test_call(FREE_FRAMES);
    let memory = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    let mut runs = [[0u64; 2]; 1];
    let pin = dma_pin(memory, t, &mut runs).expect("pin through the transport");
    handle_close(memory).expect("close");
    handle_close(pin).expect("close with no unpin");
    assert_eq!(test_call(FREE_FRAMES), free - 1, "quarantined");
    assert_err!(dma_release_quarantine(t), Status::BadState);
    assert_eq!(
        test_call(FREE_FRAMES),
        free - 1,
        "the mapping could restart it"
    );
    // No holder by call while the page is mapped.
    assert_err!(device_write(t, REG_STATUS, 0), Status::BadState);
    memory_unmap(window, PAGE).expect("unmap");
    // A writer is the transport's holder: the page can be neither mapped
    // nor released, and the transport not released, while it holds it.
    device_write(t, REG_STATUS, 0).expect("the first write takes the transport");
    assert_err!(device_map(page, range), Status::BadState);
    assert_err!(dma_release_quarantine(t), Status::BadState);
    assert_err!(dma_release_quarantine(page), Status::BadState);
    assert_eq!(test_call(FREE_FRAMES), free - 1);
    // The hold ends with the holder's last handle to the transport.
    let copy = handle_duplicate(t, Rights::DMA.union(Rights::WRITE)).expect("copy");
    handle_close(t).expect("close");
    assert_err!(dma_release_quarantine(copy), Status::BadState);
    handle_close(copy).expect("close");
    let t = device_transport(page, range).expect("transport");
    dma_release_quarantine(t).expect("release");
    assert_eq!(test_call(FREE_FRAMES), free, "the frame came back");
    // A live pin through the transport refuses the page's mapping and
    // release.
    let memory = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    let pin = dma_pin(memory, t, &mut runs).expect("pin");
    assert_err!(device_map(page, range), Status::BadState);
    assert_err!(dma_release_quarantine(page), Status::BadState);
    dma_unpin(pin).expect("unpin");
    handle_close(memory).expect("close");
    // Two holders are kept apart: a second process's write is refused
    // while this one holds the transport, and allowed once it does not.
    device_write(t, REG_STATUS, 0).expect("hold");
    assert_eq!(second_writer(t), 1, "a second holder is refused");
    handle_close(t).expect("close");
    let t = device_transport(page, range).expect("transport");
    assert_eq!(second_writer(t), 0, "with no other holder it writes");
    // The child's hold ended with it.
    dma_release_quarantine(t).expect("release");
    let (window, _) = device_map(page, range).expect("map");
    memory_unmap(window, PAGE).expect("unmap");
    // A transport device's handle has no `MAP` at all.
    assert_err!(handle_duplicate(t, Rights::MAP), Status::AccessDenied);
    handle_close(t).expect("close");
}

/// INV-0054-3: a driver that dies with a pin leaves its frames unreallocated.
fn a_dead_holder_leaves_its_frames_quarantined() {
    let (device, _) = find_block_device();
    let free = test_call(FREE_FRAMES);
    let memory = memory_create(PAGE, MemoryFlags::NONE).expect("memory");
    let image = directory_lookup(DIRECTORY, "child").expect("child");
    let mut buffer = [0; 512];
    let mut e = Encoder::new(&mut buffer, JOB, image);
    let rights = Rights::WRITE;
    e.handle(HandleEntry {
        slot: 3,
        handle: memory,
        rights,
        transfer: Transfer::Duplicate,
    });
    e.handle(HandleEntry {
        slot: 4,
        handle: device,
        rights: DeviceObject::RIGHTS,
        transfer: Transfer::Duplicate,
    });
    e.arg(b"pin-and-die");
    let length = e.finish().expect("fits");
    let child = spawn(&buffer[..length]).expect("spawn");
    handle_close(image).expect("close");
    assert_eq!(wait_exit(child).code, 0);
    handle_close(child).expect("close");
    handle_close(memory).expect("close");
    // A spawn can grow the heap, so this counts from here.
    let held = test_call(FREE_FRAMES);
    assert!(held < free, "INV-0054-3");
    dma_release_quarantine(device).expect("release");
    assert_eq!(test_call(FREE_FRAMES), held + 1, "the one frame came back");
}

const DEVICE_WRITE: u64 = 55;

/// ADR-0295: a transport device is one transport of a page, reached by
/// register calls that the kernel bounds, and nothing else of the page.
fn transport_devices() {
    let (page, range) = find_block_device();
    let t = device_transport(page, range).expect("transport");
    assert_eq!(device_read(t, 0), Ok(VIRTIO_MAGIC));
    assert_eq!(device_read(t, REG_DEVICE), Ok(DEVICE_BLOCK));
    // Bounds: the block is 0x200 bytes of aligned words, whatever the caller says.
    assert!(device_read(t, 0x1fc).is_ok());
    for bad in [
        0x200,
        0x1fe,
        1,
        2,
        3,
        0x1000,
        0x2000,
        u64::MAX,
        u64::MAX - 3,
        1 << 32,
    ] {
        assert_err!(device_read(t, bad), Status::InvalidArgument);
        assert_err!(device_write(t, bad, 0), Status::InvalidArgument);
    }
    // The cost of one register call (reported, never asserted): 2,000 reads
    // of the magic value, timed.
    let from = zoraya_user::clock_monotonic();
    for _ in 0..2000 {
        assert_eq!(device_read(t, 0), Ok(VIRTIO_MAGIC));
    }
    let nanos = (zoraya_user::clock_monotonic() - from) / 2000;
    zoraya_user::println!("t_devices: a register read by call takes {nanos} ns");
    // A value wider than a register is refused, not cut.
    let (status, _) = raw_syscall(DEVICE_WRITE, [t.raw(), REG_STATUS, 1 << 32, 0, 0, 0]);
    assert_eq!(status, Status::InvalidArgument as u64);
    // A write reaches the register: reset, and read back.
    device_write(t, REG_STATUS, 0).expect("write");
    assert_eq!(device_read(t, REG_STATUS), Ok(0));
    // No other transport of the page is reachable through it: another
    // range's device is another handle, with its own registers.
    let other = (range + 1) % 8;
    let o = device_transport(page, other).expect("another transport");
    assert_ne!(device_read(o, REG_DEVICE), Ok(DEVICE_BLOCK));
    assert_err!(device_transport(page, 99), Status::InvalidArgument);
    // A transport device is not a page: no mapping, no further transports,
    // and a page device has no register calls.
    assert_err!(device_map(t, 0), Status::AccessDenied);
    assert_err!(device_transport(t, 0), Status::AccessDenied);
    assert_err!(device_read(page, 0), Status::AccessDenied);
    assert_err!(device_write(page, REG_STATUS, 0), Status::AccessDenied);
    assert_eq!(test_call(DEVICE_MAPPINGS), 0);
    // Rights: reading needs READ, writing WRITE, `device_transport` MAP.
    let write_only = handle_duplicate(t, Rights::WRITE).expect("narrow");
    assert_err!(device_read(write_only, 0), Status::AccessDenied);
    handle_close(write_only).expect("close");
    let read_only = handle_duplicate(t, Rights::READ).expect("narrow");
    assert_err!(device_write(read_only, REG_STATUS, 0), Status::AccessDenied);
    handle_close(read_only).expect("close");
    let no_map = handle_duplicate(page, Rights::READ).expect("narrow");
    assert_err!(device_transport(no_map, range), Status::AccessDenied);
    handle_close(no_map).expect("close");
    // Not a device: the right is checked first, then the type.
    let memory = memory_create(PAGE, MemoryFlags::CONTIGUOUS).expect("memory");
    assert_err!(device_read(memory, 0), Status::BadHandle);
    assert_err!(device_write(memory, 0, 0), Status::BadHandle);
    assert_err!(device_transport(JOB, 0), Status::AccessDenied);
    // The interrupt and the pin are the transport's own, as the page's were.
    assert_err!(device_interrupt(t, 1), Status::InvalidArgument);
    let interrupt = device_interrupt(t, 0).expect("interrupt");
    assert_err!(device_interrupt(page, range), Status::BadState);
    handle_close(interrupt).expect("close");
    let mut runs = [[0u64; 2]; 1];
    let pin = dma_pin(memory, t, &mut runs).expect("pin through the transport");
    dma_unpin(pin).expect("unpin");
    handle_close(memory).expect("close");
    handle_close(o).expect("close");
    handle_close(t).expect("close");
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    maps_a_device();
    transport_devices();
    a_transport_and_its_page_keep_holders_apart();
    takes_an_interrupt();
    pins_and_unpins();
    refusals();
    a_refused_pin_leaves_nothing();
    unpinning_needs_its_right();
    a_closed_pin_stays_quarantined();
    a_dead_holder_leaves_its_frames_quarantined();
    0
}
