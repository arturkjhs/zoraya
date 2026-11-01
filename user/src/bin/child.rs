#![no_std]
#![no_main]

//! What the tests spawn. Its first argument says what it does.

use zoraya_abi::{
    CONSOLE, DEADLINE_INFINITE, Disposition, Handle, JOB, MAX_HANDLES, MapFlags, Mode, Packet,
    RequestKind, Rights, Status, channel_signals,
};
use zoraya_user::{
    assert_err, channel_read, channel_write, clock_monotonic, dma_pin, handle_duplicate,
    memory_map, memory_unmap, port_create, port_queue, port_subscribe, port_unsubscribe, port_wait,
    println, process_exit, process_exit_terminated, process_set_disposition, process_take_request,
    sleep_until, startup, thread_yield,
};

/// Every slot of this process's table is empty (INV-0003-1).
fn holds_nothing() -> bool {
    (1..=MAX_HANDLES).all(|slot| {
        let handle = Handle::new(u64::from(slot)).expect("slot is not zero");
        handle_duplicate(handle, Rights::NONE) == Err(Status::BadHandle)
    })
}

fn at_slot(slot: u64) -> Handle {
    Handle::new(slot).expect("slot is not zero")
}

/// Blocks in a port wait for good, with nothing that could end it.
fn wait_forever() -> ! {
    let port = port_create().expect("port");
    let mut packets = [Packet::default()];
    let _ = port_wait(port, DEADLINE_INFINITE, &mut packets);
    process_exit(99)
}

fn take_requests() -> ! {
    process_set_disposition(RequestKind::Interrupt, Disposition::Deliver).expect("disposition");
    loop {
        match process_take_request() {
            Ok((RequestKind::Interrupt, detail)) => process_exit(detail as i32),
            Ok(_) => process_exit(98),
            Err(_) => thread_yield(),
        }
    }
}

#[unsafe(no_mangle)]
fn main() -> i32 {
    let block = startup();
    let mut argv = block.argv;
    match argv.next().unwrap_or(b"") {
        b"channel-echo" => {
            let end = at_slot(3);
            let port = port_create().expect("port");
            port_subscribe(port, end, channel_signals::READABLE, 1, Mode::Level)
                .expect("subscribe");
            let mut packets = [Packet::default()];
            port_wait(port, DEADLINE_INFINITE, &mut packets).expect("wait");
            let mut buffer = [0; 16];
            let sizes = channel_read(end, &mut buffer, &mut []).expect("read");
            channel_write(end, &buffer[..sizes.bytes as usize], &[]).expect("write");
            0
        }
        b"pin-and-die" => {
            // Slot 3 a memory object, slot 4 a device. It ends with the pin
            // live, as a dead driver does.
            let mut runs = [[0; 2]; 4];
            dma_pin(at_slot(3), at_slot(4), &mut runs).expect("pin");
            0
        }
        b"memory-writer" => {
            let at = memory_map(at_slot(3), 0, 4096, MapFlags::WRITE).expect("map");
            // SAFETY: mapped just above.
            unsafe { core::ptr::write_volatile(at as *mut u64, 0x5678) };
            0
        }
        b"memory-write-read-only" => {
            let at = memory_map(at_slot(3), 0, 4096, MapFlags::READ).expect("map");
            // SAFETY: none: the fault is the point.
            unsafe { core::ptr::write_volatile(at as *mut u64, 1) };
            0
        }
        b"memory-after-unmap" => {
            let at = memory_map(at_slot(3), 0, 4096, MapFlags::READ).expect("map");
            // SAFETY: mapped just above.
            let value = unsafe { core::ptr::read_volatile(at as *const u64) };
            memory_unmap(at, 4096).expect("unmap");
            // SAFETY: none: the fault is the point.
            let again = unsafe { core::ptr::read_volatile(at as *const u64) };
            (value + again) as i32
        }
        b"hello" => {
            println!("child: hello from the child");
            42
        }
        b"empty" => i32::from(!holds_nothing()),
        b"slots" => {
            // INV-0003-6: exactly what the parent listed, where it said.
            let at = |slot| Handle::new(slot).expect("slot is not zero");
            assert_err!(handle_duplicate(at(5), Rights::NONE), Status::BadHandle);
            assert_err!(handle_duplicate(CONSOLE, Rights::NONE), Status::BadHandle);
            assert_err!(handle_duplicate(JOB, Rights::NONE), Status::BadHandle);
            handle_duplicate(at(3), Rights::NONE).expect("slot 3");
            handle_duplicate(at(4), Rights::NONE).expect("slot 4");
            0
        }
        b"args" => {
            assert_eq!(argv.next(), Some(&b"two words"[..]));
            assert_eq!(argv.next(), Some(&b"\xff\0binary"[..]));
            assert_eq!(argv.next(), None);
            let mut envp = block.envp;
            assert_eq!(envp.next(), Some(&b"KEY=value"[..]));
            assert_eq!(envp.next(), None);
            0
        }
        b"exit130" => 130,
        b"terminated" => process_exit_terminated(2),
        b"fault" => {
            // SAFETY: not safe, and that is the point: this address faults.
            unsafe { core::ptr::null_mut::<u8>().write_volatile(1) };
            97
        }
        b"sleep" => {
            sleep_until(DEADLINE_INFINITE);
            96
        }
        b"wait" => wait_forever(),
        b"take" => take_requests(),
        b"nap" => {
            sleep_until(clock_monotonic() + 3_600_000_000_000);
            94
        }
        b"spin" => loop {
            core::hint::spin_loop();
        },
        b"port-wait-only" => {
            // A port with only `WAIT`, and something to subscribe.
            let port = Handle::new(3).expect("slot");
            let job = Handle::new(4).expect("slot");
            let denied = Status::AccessDenied;
            assert_err!(port_queue(port, 0, 0), denied);
            assert_err!(port_unsubscribe(port, 0), denied);
            assert_err!(port_subscribe(port, job, 1, 0, Mode::Level), denied);
            let mut packets = [Packet::default()];
            assert_err!(port_wait(port, 0, &mut packets), Status::DeadlineExceeded);
            0
        }
        b"port-write-only" => {
            let port = Handle::new(3).expect("slot");
            let job = Handle::new(4).expect("slot");
            let mut packets = [Packet::default()];
            assert_err!(port_wait(port, 0, &mut packets), Status::AccessDenied);
            port_subscribe(port, job, 1, 0, Mode::Edge).expect("subscribe");
            port_unsubscribe(port, 0).expect("unsubscribe");
            port_queue(port, 0, 0).expect("queue");
            0
        }
        b"yield" => loop {
            thread_yield();
        },
        _ => 95,
    }
}
