//! The receive path of the NIC driver (ADR-0221, INV-0221-2) on arbitrary
//! bytes: the bytes are a frame for the ARP parser, and also a script for a
//! hostile device that completes used-ring entries (any queue, any head,
//! any length) while the service releases and transmits arbitrary slots.
//! Nothing may panic; a broken device is an `Err`.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zoraya_proto_nic::arp;
use zoraya_virtio::DEVICE_NET;
use zoraya_virtio::fake::Device;
use zoraya_virtio_net::{Net, OBJECT_SIZE, SHARED_MEMORY};

const BASE: u64 = 0x4000_0000;

fuzz_target!(|data: &[u8]| {
    let mut out = [0u8; 64];
    let _ = arp::parse(data);
    let _ = arp::answer(data, [0x52, 0x54, 0, 1, 2, 3], [10, 0, 2, 15], &mut out);

    let d = Device::new(DEVICE_NET, BASE, SHARED_MEMORY + OBJECT_SIZE as usize);
    {
        let mut f = d.state.borrow_mut();
        f.device_features = [(1 << 5) | (1 << 16), 1];
        f.config[..6].copy_from_slice(&[0x52, 0x54, 0, 1, 2, 3]);
    }
    let Ok(mut net) = Net::new(d.registers(), d.shared(), BASE) else {
        return;
    };
    if net.attach(BASE + SHARED_MEMORY as u64).is_err() {
        return;
    }
    // Five bytes a step: an action, and two numbers.
    for step in data.chunks_exact(5) {
        let a = u32::from(u16::from_le_bytes([step[1], step[2]]));
        let b = u32::from(u16::from_le_bytes([step[3], step[4]]));
        match step[0] % 6 {
            0 => {
                let _ = net.release(a);
            }
            1 => {
                let _ = net.transmit(a, b);
            }
            2 => {
                let mut f = d.state.borrow_mut();
                if let Some(head) = f.next_available(0) {
                    f.complete(0, u32::from(head), b);
                }
            }
            3 => {
                let mut f = d.state.borrow_mut();
                f.complete(usize::from(step[1] & 1), a, b);
            }
            4 => d.state.borrow_mut().set_used_index(usize::from(step[1] & 1), a as u16),
            _ => {
                if net.poll(&mut |_, _| b & 1 == 0, &mut |_| {}).is_err() {
                    return;
                }
            }
        }
    }
});
