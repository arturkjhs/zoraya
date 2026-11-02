//! The shared transport and queue (`queue.rs`) against the fake device.

use crate::fake::{Device, Event};
use crate::queue::{Buffer, Layout, Page, Queue, Transport, begin, identify, ready, reset};
use crate::{DEVICE_GPU, Error, MAGIC, Registers, reg};

const BASE: u64 = 0x4000_0000;

fn device() -> Device {
    Device::new(DEVICE_GPU, BASE, 0x10000)
}

#[test]
fn begin_offers_only_version_1_and_the_wanted_bits_the_device_has() {
    let d = device();
    d.state.borrow_mut().device_features = [0xffff_ffff, 0xffff_ffff];
    let mut r = d.registers();
    assert_eq!(begin(&mut r, DEVICE_GPU, 0b100), Ok(0b100));
    assert_eq!(d.state.borrow().driver_features, [0b100, 1]);
    let d = device();
    d.state.borrow_mut().device_features = [0, 1];
    assert_eq!(begin(&mut d.registers(), DEVICE_GPU, 0b100), Ok(0));
    assert_eq!(d.state.borrow().driver_features, [0, 1]);
}

#[test]
fn begin_refuses_the_wrong_device_and_a_failed_negotiation() {
    let d = device();
    assert_eq!(begin(&mut d.registers(), 18, 0), Err(Error::WrongDevice));
    d.state.borrow_mut().version = 1;
    assert_eq!(
        begin(&mut d.registers(), DEVICE_GPU, 0),
        Err(Error::WrongDevice)
    );
    let d = device();
    d.state.borrow_mut().magic = MAGIC + 1;
    assert_eq!(
        begin(&mut d.registers(), DEVICE_GPU, 0),
        Err(Error::WrongDevice)
    );
    let d = device();
    d.state.borrow_mut().device_features = [0, 0];
    assert_eq!(
        begin(&mut d.registers(), DEVICE_GPU, 0),
        Err(Error::Negotiation)
    );
    assert_ne!(d.state.borrow().status & 128, 0, "FAILED is set");
    let d = device();
    d.state.borrow_mut().refuse_features = true;
    assert_eq!(
        begin(&mut d.registers(), DEVICE_GPU, 0),
        Err(Error::Negotiation)
    );
    let d = device();
    d.state.borrow_mut().no_reset = true;
    assert_eq!(begin(&mut d.registers(), DEVICE_GPU, 0), Err(Error::Device));
}

#[test]
fn begin_resets_first_and_ready_sets_driver_ok() {
    let d = device();
    let mut r = d.registers();
    begin(&mut r, DEVICE_GPU, 0).unwrap();
    ready(&mut r);
    let writes = d.state.borrow().status_writes.clone();
    assert_eq!(writes, [0, 1, 3, 11, 15]);
    assert!(reset(&mut r));
    assert_eq!(r.read(reg::STATUS), 0);
}

fn queue_on(d: &Device, size: u16) -> Queue {
    let q = Queue::new(0, Layout::new(size, 0), BASE);
    q.set_up(&mut d.registers()).unwrap();
    q
}

#[test]
fn layout_is_aligned_and_does_not_overlap() {
    for size in [1u16, 2, 16, 64, 256] {
        let l = Layout::new(size, 100);
        assert_eq!(l.desc % 4096, 0);
        assert!(l.desc >= 100);
        assert!(l.avail >= l.desc + 16 * usize::from(size));
        assert!(l.used >= l.avail + 6 + 2 * usize::from(size));
        assert!(l.end >= l.used + 6 + 8 * usize::from(size));
        assert_eq!(l.used % 4, 0);
    }
}

#[test]
fn set_up_writes_the_addresses_and_refuses_a_short_or_live_queue() {
    let d = device();
    let q = queue_on(&d, 16);
    let s = d.state.borrow().queues[0];
    let l = q.layout();
    assert_eq!((s.num, s.ready), (16, 1));
    assert_eq!(s.desc, BASE + l.desc as u64);
    assert_eq!(s.avail, BASE + l.avail as u64);
    assert_eq!(s.used, BASE + l.used as u64);
    // Live already.
    assert_eq!(q.set_up(&mut d.registers()), Err(Error::Queue));
    let d = device();
    d.state.borrow_mut().queues[0].num_max = 8;
    let q = Queue::new(0, Layout::new(16, 0), BASE);
    assert_eq!(q.set_up(&mut d.registers()), Err(Error::Queue));
}

#[test]
fn a_chain_goes_out_and_comes_back() {
    let d = device();
    let mut q = queue_on(&d, 16);
    let mut shared = d.shared();
    let chain = [
        Buffer {
            address: BASE + 0x8000,
            length: 24,
            writable: false,
        },
        Buffer {
            address: BASE + 0x9000,
            length: 408,
            writable: true,
        },
    ];
    q.offer(&mut shared, 4, &chain).unwrap();
    assert_eq!(q.outstanding(), 1);
    q.notify(&mut d.registers(), &mut shared);
    assert_eq!(d.state.borrow().notifies, [0]);
    let head = d.state.borrow_mut().next_available(0).unwrap();
    assert_eq!(head, 4);
    assert_eq!(d.state.borrow().chain(0, head), chain);
    assert_eq!(q.take(&shared), Ok(None));
    d.state.borrow_mut().complete(0, 4, 408);
    assert_eq!(
        q.take(&shared),
        Ok(Some(crate::queue::Used {
            head: 4,
            length: 408
        }))
    );
    assert_eq!(q.outstanding(), 0);
    assert_eq!(q.take(&shared), Ok(None));
}

#[test]
fn offers_out_of_range_or_twice_are_refused() {
    let d = device();
    let mut q = queue_on(&d, 4);
    let mut shared = d.shared();
    let one = [Buffer {
        address: BASE,
        length: 8,
        writable: true,
    }];
    assert_eq!(q.offer(&mut shared, 4, &one), Err(Error::TooLarge));
    assert_eq!(q.offer(&mut shared, 3, &[one[0]; 2]), Err(Error::TooLarge));
    assert_eq!(q.offer(&mut shared, 0, &[]), Err(Error::TooLarge));
    q.offer(&mut shared, 1, &one).unwrap();
    assert_eq!(q.offer(&mut shared, 1, &one), Err(Error::TooLarge));
    assert_eq!(q.outstanding(), 1);
}

#[test]
fn a_device_that_lies_about_the_used_ring_is_caught() {
    let one = [Buffer {
        address: BASE,
        length: 8,
        writable: true,
    }];
    // The index jumps past what is outstanding.
    let d = device();
    let mut q = queue_on(&d, 8);
    let mut shared = d.shared();
    q.offer(&mut shared, 0, &one).unwrap();
    d.state.borrow_mut().set_used_index(0, 2);
    assert_eq!(q.take(&shared), Err(Error::Device));
    // An entry names a head never offered.
    let d = device();
    let mut q = queue_on(&d, 8);
    let mut shared = d.shared();
    q.offer(&mut shared, 0, &one).unwrap();
    d.state.borrow_mut().complete(0, 3, 8);
    assert_eq!(q.take(&shared), Err(Error::Device));
    // An entry past the queue.
    let d = device();
    let mut q = queue_on(&d, 8);
    let mut shared = d.shared();
    q.offer(&mut shared, 0, &one).unwrap();
    d.state.borrow_mut().complete(0, 8, 8);
    assert_eq!(q.take(&shared), Err(Error::Device));
    let d = device();
    let mut q = queue_on(&d, 8);
    let mut shared = d.shared();
    q.offer(&mut shared, 0, &one).unwrap();
    d.state.borrow_mut().complete(0, u32::MAX, 8);
    assert_eq!(q.take(&shared), Err(Error::Device));
    // The same head returned twice.
    let d = device();
    let mut q = queue_on(&d, 8);
    let mut shared = d.shared();
    q.offer(&mut shared, 0, &one).unwrap();
    q.offer(&mut shared, 1, &one).unwrap();
    d.state.borrow_mut().complete(0, 0, 8);
    d.state.borrow_mut().complete(0, 0, 8);
    assert!(q.take(&shared).unwrap().is_some());
    assert_eq!(q.take(&shared), Err(Error::Device));
}

#[test]
fn the_ring_wraps() {
    let d = device();
    let mut q = queue_on(&d, 4);
    let mut shared = d.shared();
    let one = [Buffer {
        address: BASE,
        length: 8,
        writable: true,
    }];
    for round in 0..70_000u32 {
        let head = (round % 4) as u16;
        q.offer(&mut shared, head, &one).unwrap();
        q.notify(&mut d.registers(), &mut shared);
        let got = d.state.borrow_mut().next_available(0);
        assert_eq!(got, Some(head));
        d.state.borrow_mut().complete(0, u32::from(head), 8);
        assert_eq!(q.take(&shared).unwrap().unwrap().head, head);
    }
}

#[test]
fn config_writes_reach_the_device_hook() {
    let d = device();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = seen.clone();
    d.on(move |_, e| log.borrow_mut().push(e));
    let mut r = d.registers();
    r.write8(reg::CONFIG + 1, 7);
    r.write(reg::QUEUE_NOTIFY, 0);
    assert_eq!(*seen.borrow(), [Event::Config(1), Event::Notify(0)]);
    assert_eq!(r.read8(reg::CONFIG + 1), 7);
}

fn t(range: u32, magic: u32, version: u32, device_id: u32) -> Transport {
    Transport {
        range,
        magic,
        version,
        device_id,
    }
}

#[test]
fn identify_gives_a_page_only_with_exactly_one_device() {
    assert_eq!(identify(&[]), Page::Empty);
    let empty: Vec<Transport> = (0..8).map(|r| t(r, MAGIC, 2, 0)).collect();
    assert_eq!(identify(&empty), Page::Empty);
    let mut one = empty.clone();
    one[7] = t(7, MAGIC, 2, 16);
    assert_eq!(
        identify(&one),
        Page::One {
            range: 7,
            device_id: 16
        }
    );
    let mut two = one.clone();
    two[3] = t(3, MAGIC, 2, 18);
    assert_eq!(identify(&two), Page::Several);
    // A legacy device beside a modern one still shares the page.
    let mut legacy = one.clone();
    legacy[2] = t(2, MAGIC, 1, 2);
    assert_eq!(identify(&legacy), Page::Several);
    // A lone legacy device is nothing a driver can use.
    let mut lone = empty.clone();
    lone[2] = t(2, MAGIC, 1, 2);
    assert_eq!(identify(&lone), Page::Empty);
    // Not virtio at all.
    let mut other = empty;
    other[0] = t(0, 0x1234, 2, 16);
    assert_eq!(identify(&other), Page::Empty);
}
