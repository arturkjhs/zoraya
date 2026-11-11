use zoraya_virtio::DEVICE_NET;
use zoraya_virtio::fake::{Device, FakeRegisters, FakeShared};

use super::*;

pub(crate) const BASE: u64 = 0x4000_0000;
pub(crate) const FRAMES: u64 = BASE + SHARED_MEMORY as u64;
const MAC: [u8; 6] = [0x52, 0x54, 0, 0x12, 0x34, 0x56];

pub(crate) type FakeNet = Net<FakeRegisters, FakeShared>;

pub(crate) fn device() -> Device {
    let d = Device::new(DEVICE_NET, BASE, SHARED_MEMORY + OBJECT_SIZE as usize);
    {
        let mut f = d.state.borrow_mut();
        f.device_features = [F_MAC | F_STATUS, 1];
        f.config[..6].copy_from_slice(&MAC);
        f.config[6] = 1;
    }
    d
}

pub(crate) fn up(d: &Device) -> FakeNet {
    Net::new(d.registers(), d.shared(), BASE).unwrap()
}

pub(crate) fn attached(d: &Device) -> FakeNet {
    let mut net = up(d);
    net.attach(FRAMES).unwrap();
    net
}

/// What the device does to receive `bytes`: takes the next posted slot's
/// chain, writes the header and the frame, and completes it.
pub(crate) fn deliver(d: &Device, bytes: &[u8]) -> u16 {
    let mut f = d.state.borrow_mut();
    let head = f.next_available(0).expect("a posted buffer");
    let chain = f.chain(0, head);
    assert_eq!(chain.len(), 2);
    assert!(chain.iter().all(|b| b.writable));
    assert!(f.write(chain[0].address, &[0; HEADER as usize]));
    assert!(f.write(chain[1].address, bytes));
    f.complete(0, u32::from(head), HEADER + bytes.len() as u32);
    head
}

/// The newest head the driver made available on the receive queue.
fn last_available(d: &Device) -> Option<u16> {
    let mut f = d.state.borrow_mut();
    let mut last = None;
    while let Some(head) = f.next_available(0) {
        last = Some(head);
    }
    last
}

type Polled = (Vec<(u32, u32)>, Vec<u32>, Result<(), Error>);

pub(crate) fn poll(net: &mut FakeNet) -> Polled {
    let (mut got, mut sent) = (Vec::new(), Vec::new());
    let r = net.poll(
        &mut |slot, length| {
            got.push((slot, length));
            true
        },
        &mut |slot| sent.push(slot),
    );
    (got, sent, r)
}

#[test]
fn it_negotiates_only_mac_and_status_and_reads_the_address() {
    let d = device();
    let net = up(&d);
    assert_eq!(net.mac(), MAC);
    assert!(net.link());
    let f = d.state.borrow();
    assert_eq!(f.driver_features, [F_MAC | F_STATUS, 1]);
    assert_eq!(f.queues[0].num, u32::from(QUEUE_SIZE));
    assert_eq!(f.queues[1].num, u32::from(QUEUE_SIZE));
    assert_eq!((f.queues[0].ready, f.queues[1].ready), (1, 1));
}

#[test]
fn offloads_the_device_offers_are_not_taken() {
    let d = device();
    d.state.borrow_mut().device_features[0] = F_MAC | F_STATUS | 1 | 1 << 15 | 1 << 11 | 1 << 17;
    let _ = up(&d);
    assert_eq!(d.state.borrow().driver_features[0], F_MAC | F_STATUS);
}

#[test]
fn a_device_without_a_usable_address_is_refused() {
    for mac in [[0u8; 6], [0x53, 0, 0, 0, 0, 1]] {
        let d = device();
        d.state.borrow_mut().config[..6].copy_from_slice(&mac);
        assert!(matches!(
            Net::new(d.registers(), d.shared(), BASE),
            Err(Error::Unusable)
        ));
    }
    let d = device();
    d.state.borrow_mut().device_features[0] = F_STATUS;
    assert!(matches!(
        Net::new(d.registers(), d.shared(), BASE),
        Err(Error::Unusable)
    ));
}

#[test]
fn a_device_that_is_not_a_net_device_is_refused() {
    let d = Device::new(2, BASE, SHARED_MEMORY);
    assert!(Net::new(d.registers(), d.shared(), BASE).is_err());
}

#[test]
fn attaching_offers_every_receive_slot_as_a_header_and_a_frame_buffer() {
    let d = device();
    let _ = attached(&d);
    let mut f = d.state.borrow_mut();
    let mut seen = Vec::new();
    while let Some(head) = f.next_available(0) {
        let chain = f.chain(0, head);
        assert_eq!(head % 2, 0);
        assert_eq!(chain[0].length, HEADER);
        assert_eq!(chain[1].length, MAX_FRAME);
        assert_eq!(chain[1].address, FRAMES + SLOT_SIZE * u64::from(head / 2));
        assert!(chain[0].address >= BASE && chain[0].address < BASE + SHARED_MEMORY as u64);
        seen.push(head / 2);
    }
    assert_eq!(seen.len(), SLOTS);
    seen.sort_unstable();
    assert!(seen.iter().enumerate().all(|(i, s)| usize::from(*s) == i));
    assert_eq!(f.notifies.last(), Some(&0));
}

#[test]
fn attaching_twice_or_overflowing_the_address_is_refused() {
    let d = device();
    let mut net = up(&d);
    assert_eq!(net.attach(u64::MAX - 10), Err(Refused::BadSlot));
    net.attach(FRAMES).unwrap();
    assert_eq!(net.attach(FRAMES), Err(Refused::Attached));
}

#[test]
fn nothing_works_before_the_attach() {
    let d = device();
    let mut net = up(&d);
    assert_eq!(net.release(0), Err(Refused::NotAttached));
    assert_eq!(net.transmit(64, 60), Err(Refused::NotAttached));
    assert_eq!(poll(&mut net).2, Ok(()));
}

#[test]
fn a_received_frame_is_reported_with_its_slot_and_length() {
    let d = device();
    let mut net = attached(&d);
    let head = deliver(&d, &[7; 60]);
    let (got, sent, r) = poll(&mut net);
    assert_eq!(r, Ok(()));
    assert_eq!(got, [(u32::from(head / 2), 60)]);
    assert!(sent.is_empty());
    let f = d.state.borrow();
    assert_eq!(
        f.read(FRAMES + SLOT_SIZE * u64::from(head / 2), 60)
            .unwrap(),
        [7; 60]
    );
}

#[test]
fn a_slot_comes_back_to_the_device_only_when_released() {
    let d = device();
    let mut net = attached(&d);
    let head = deliver(&d, &[1; 60]);
    let slot = u32::from(head / 2);
    let _ = poll(&mut net);
    // All the other slots are still posted; this one is not.
    {
        let mut f = d.state.borrow_mut();
        let mut count = 0;
        while let Some(h) = f.next_available(0) {
            assert_ne!(h / 2, head / 2);
            count += 1;
        }
        assert_eq!(count, SLOTS - 1);
    }
    assert_eq!(net.release(slot), Ok(()));
    assert_eq!(last_available(&d), Some(head));
    assert_eq!(net.release(slot), Err(Refused::WrongState));
    assert_eq!(net.refused(), 1);
}

#[test]
fn release_refuses_what_is_not_the_services() {
    let d = device();
    let mut net = attached(&d);
    assert_eq!(net.release(3), Err(Refused::WrongState));
    for slot in [64, 65, 127, 128, u32::MAX] {
        assert_eq!(net.release(slot), Err(Refused::BadSlot));
    }
}

#[test]
fn a_frame_the_service_cannot_be_told_of_is_dropped_and_reposted() {
    let d = device();
    let mut net = attached(&d);
    let head = deliver(&d, &[1; 60]);
    let r = net.poll(&mut |_, _| false, &mut |_| {});
    assert_eq!(r, Ok(()));
    assert_eq!(net.dropped(), 1);
    assert_eq!(last_available(&d), Some(head));
}

#[test]
fn a_runt_is_dropped_and_reposted() {
    let d = device();
    let mut net = attached(&d);
    let head = deliver(&d, &[1; 5]);
    let (got, _, r) = poll(&mut net);
    assert_eq!((got.len(), r), (0, Ok(())));
    assert_eq!(net.dropped(), 1);
    assert_eq!(last_available(&d), Some(head));
}

#[test]
fn a_length_beyond_the_buffer_or_below_the_header_is_a_broken_device() {
    for length in [0, 5, HEADER - 1, HEADER + MAX_FRAME + 1, u32::MAX] {
        let d = device();
        let mut net = attached(&d);
        let head = d.state.borrow_mut().next_available(0).unwrap();
        d.state.borrow_mut().complete(0, u32::from(head), length);
        assert_eq!(poll(&mut net).2, Err(Error::Device), "length {length}");
    }
}

#[test]
fn a_used_entry_naming_no_offered_chain_is_a_broken_device() {
    for head in [1u32, 2 * SLOTS as u32, 255, 256, 1000, u32::MAX] {
        let d = device();
        let mut net = attached(&d);
        // Head 1 is a descriptor inside a chain, never a head.
        d.state.borrow_mut().complete(0, head, HEADER + 60);
        assert_eq!(poll(&mut net).2, Err(Error::Device), "head {head}");
    }
}

#[test]
fn a_used_index_that_runs_ahead_is_a_broken_device() {
    let d = device();
    let mut net = attached(&d);
    d.state.borrow_mut().set_used_index(0, SLOTS as u16 + 1);
    assert_eq!(poll(&mut net).2, Err(Error::Device));
    let d = device();
    let mut net = attached(&d);
    d.state.borrow_mut().set_used_index(1, 1);
    assert_eq!(poll(&mut net).2, Err(Error::Device));
}

#[test]
fn the_same_slot_completed_twice_is_a_broken_device() {
    let d = device();
    let mut net = attached(&d);
    let head = deliver(&d, &[1; 60]);
    d.state
        .borrow_mut()
        .complete(0, u32::from(head), HEADER + 60);
    assert_eq!(poll(&mut net).2, Err(Error::Device));
}

#[test]
fn a_transmit_is_a_zero_header_and_the_frame_and_is_reported_when_done() {
    let d = device();
    let mut net = attached(&d);
    assert_eq!(net.transmit(66, 98), Ok(()));
    let head = {
        let mut f = d.state.borrow_mut();
        let head = f.next_available(1).unwrap();
        let chain = f.chain(1, head);
        assert_eq!(chain.len(), 2);
        assert!(chain.iter().all(|b| !b.writable));
        assert_eq!((chain[0].length, chain[1].length), (HEADER, 98));
        assert_eq!(chain[1].address, FRAMES + SLOT_SIZE * 66);
        assert_eq!(f.read(chain[0].address, 12).unwrap(), [0; 12]);
        assert_eq!(f.notifies.last(), Some(&1));
        head
    };
    // Not free until the device is done.
    assert_eq!(net.transmit(66, 98), Err(Refused::WrongState));
    d.state.borrow_mut().complete(1, u32::from(head), 0);
    let (got, sent, r) = poll(&mut net);
    assert_eq!((got.len(), sent, r), (0, vec![66], Ok(())));
    // Finished is not free: only after the service has been told.
    assert_eq!(net.transmit(66, 98), Err(Refused::WrongState));
    assert!(!net.reported(67));
    assert!(net.reported(66));
    assert!(!net.reported(66));
    assert!(!net.reported(3));
    assert_eq!(net.transmit(66, 98), Ok(()));
}

#[test]
fn an_undone_receive_is_reposted_and_counted() {
    let d = device();
    let mut net = attached(&d);
    deliver(&d, &[0; 60]);
    let (got, _, _) = poll(&mut net);
    assert_eq!(got.len(), 1);
    let before = net.dropped();
    assert_eq!(net.undo(got[0].0), Ok(()));
    assert_eq!(net.dropped(), before + 1);
    assert_eq!(net.undo(got[0].0), Err(Refused::WrongState));
}

#[test]
fn a_transmit_outside_its_rules_is_refused_and_counted() {
    let d = device();
    let mut net = attached(&d);
    for slot in [0, 63, 128, 129, u32::MAX] {
        assert_eq!(net.transmit(slot, 60), Err(Refused::BadSlot));
    }
    for length in [0, 13, 1515, u32::MAX] {
        assert_eq!(net.transmit(64, length), Err(Refused::BadLength));
    }
    assert_eq!(net.refused(), 9);
    assert!(d.state.borrow_mut().next_available(1).is_none());
    assert_eq!(net.transmit(64, 14), Ok(()));
    assert_eq!(net.transmit(65, 1514), Ok(()));
}

#[test]
fn a_transmit_completion_for_a_free_slot_is_a_broken_device() {
    let d = device();
    let mut net = attached(&d);
    d.state.borrow_mut().complete(1, 0, 0);
    assert_eq!(poll(&mut net).2, Err(Error::Device));
}

#[test]
fn the_link_follows_the_status_field() {
    let d = device();
    let mut net = up(&d);
    assert_eq!(net.link_changed(), None);
    d.state.borrow_mut().config[6] = 0;
    assert_eq!(net.link_changed(), Some(false));
    assert!(!net.link());
    assert_eq!(net.link_changed(), None);
    d.state.borrow_mut().config[6] = 1;
    assert_eq!(net.link_changed(), Some(true));
}

#[test]
fn without_status_the_link_is_always_up() {
    let d = device();
    d.state.borrow_mut().device_features[0] = F_MAC;
    d.state.borrow_mut().config[6] = 0;
    let mut net = up(&d);
    assert!(net.link());
    assert_eq!(net.link_changed(), None);
}

#[test]
fn acknowledge_reports_a_configuration_change() {
    let d = device();
    let mut net = up(&d);
    d.state.borrow_mut().interrupt_status = 2;
    assert!(net.acknowledge());
    d.state.borrow_mut().interrupt_status = 1;
    assert!(!net.acknowledge());
}

#[test]
fn the_layout_fits_its_memory() {
    const { assert!(RX_HEADERS_AT >= TX.end) };
    const { assert!(TX_HEADER_AT + 16 <= SHARED_MEMORY) };
    assert_eq!(SHARED_MEMORY % 4096, 0);
    assert_eq!(OBJECT_SIZE, 196_608);
}

/// A deterministic generator (xorshift), so a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// INV-0221-2: no value from the device and no request of the service
/// panics the driver. A broken device ends the run, as it ends the driver.
#[test]
fn random_requests_and_completions_never_panic() {
    for seed in 1..=200u64 {
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let d = device();
        let mut net = attached(&d);
        for _ in 0..300 {
            match rng.next() % 6 {
                0 => {
                    let _ = net.release(rng.next() as u32 % 140);
                }
                1 => {
                    let length = match rng.next() % 4 {
                        0 => rng.next() as u32,
                        _ => rng.next() as u32 % 1600,
                    };
                    let _ = net.transmit(rng.next() as u32 % 140, length);
                }
                2 => {
                    // A truthful receive, if a buffer is posted.
                    let mut f = d.state.borrow_mut();
                    if let Some(head) = f.next_available(0) {
                        let length = HEADER + rng.next() as u32 % (MAX_FRAME + 1);
                        f.complete(0, u32::from(head), length);
                    }
                }
                3 => {
                    let mut f = d.state.borrow_mut();
                    if let Some(head) = f.next_available(1) {
                        f.complete(1, u32::from(head), rng.next() as u32);
                    }
                }
                4 => {
                    // A lie: any head, any length.
                    let queue = (rng.next() % 2) as usize;
                    let head = rng.next() as u32 % 300;
                    d.state
                        .borrow_mut()
                        .complete(queue, head, rng.next() as u32);
                }
                _ => {
                    let keep = !rng.next().is_multiple_of(3);
                    if net.poll(&mut |_, _| keep, &mut |_| {}).is_err() {
                        break;
                    }
                }
            }
        }
    }
}
