use std::cell::RefCell;
use std::rc::Rc;

use super::*;

const SIZE_SECTORS: u64 = 2048;
const QUEUE_BASE: u64 = 0x10_0000;
const DATA_BASE: u64 = 0x20_0000;

/// How the fake device misbehaves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Hostile {
    #[default]
    None,
    /// The used index jumps past what was submitted.
    UsedIndexJumps,
    /// A used entry names a descriptor that is no head.
    BadId,
    /// A used entry repeats one already returned.
    Repeats,
    /// A status byte that means nothing.
    BadStatus,
    /// An id past the last slot.
    IdOutOfRange,
    /// Completes the first slot for every request.
    RepeatsFirst,
    /// Requests fail.
    IoError,
    /// Requests are unsupported.
    Unsupported,
    /// The used length claims more than the buffers.
    HugeLength,
    /// A read succeeds with less than its data written.
    ShortRead,
    /// Reset never reads back 0.
    NoReset,
    /// No version 1 feature.
    NoVersion1,
    /// `FEATURES_OK` does not stick.
    RefusesFeatures,
    /// The queue is too short.
    ShortQueue,
    /// Never answers.
    Silent,
}

/// A virtio-blk device over plain memory, enough for the driver.
struct Device {
    registers: [u32; 0x200],
    /// The queue memory.
    shared: Vec<u8>,
    /// The data memory.
    data: Vec<u8>,
    disk: Vec<u8>,
    status: u32,
    /// Every value written to `Status`, in order.
    status_writes: Vec<u32>,
    feature_sel: u32,
    avail_seen: u16,
    used: u16,
    flushes: u32,
    flush_offered: bool,
    hostile: Hostile,
    driver_features: [u32; 2],
    published_notify: bool,
}

impl Device {
    fn new(hostile: Hostile) -> Rc<RefCell<Device>> {
        Rc::new(RefCell::new(Device {
            registers: [0; 0x200],
            shared: vec![0; QUEUE_MEMORY],
            data: vec![0; 1 << 20],
            disk: vec![0; (SIZE_SECTORS * SECTOR_SIZE) as usize],
            status: 0,
            status_writes: Vec::new(),
            feature_sel: 0,
            avail_seen: 0,
            used: 0,
            flushes: 0,
            flush_offered: true,
            hostile,
            driver_features: [0; 2],
            published_notify: false,
        }))
    }

    fn word(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.shared[at..at + 4].try_into().unwrap())
    }

    fn half(&self, at: usize) -> u16 {
        u16::from_le_bytes(self.shared[at..at + 2].try_into().unwrap())
    }

    fn long(&self, at: usize) -> u64 {
        u64::from_le_bytes(self.shared[at..at + 8].try_into().unwrap())
    }

    fn descriptor(&self, index: usize) -> (u64, u32, u16, u16) {
        let at = DESC_AT + 16 * index;
        (
            self.long(at),
            self.word(at + 8),
            self.half(at + 12),
            self.half(at + 14),
        )
    }

    fn set(&mut self, at: usize, bytes: &[u8]) {
        self.shared[at..at + bytes.len()].copy_from_slice(bytes);
    }

    fn complete(&mut self, head: u32, length: u32) {
        let entry = USED_AT + 4 + 8 * (usize::from(self.used) % N);
        self.set(entry, &head.to_le_bytes());
        self.set(entry + 4, &length.to_le_bytes());
        self.used = self.used.wrapping_add(1);
    }

    /// Runs what is available.
    fn run(&mut self) {
        let avail = self.half(AVAIL_AT + 2);
        while self.avail_seen != avail {
            let ring = AVAIL_AT + 4 + 2 * (usize::from(self.avail_seen) % N);
            let head = usize::from(self.half(ring));
            self.avail_seen = self.avail_seen.wrapping_add(1);
            self.process(head);
        }
        if self.hostile == Hostile::Silent {
            return;
        }
        let mut used = self.used;
        if self.hostile == Hostile::UsedIndexJumps {
            used = used.wrapping_add(100);
        }
        self.set(USED_AT + 2, &used.to_le_bytes());
    }

    fn process(&mut self, head: usize) {
        let (header, _, flags, mut next) = self.descriptor(head);
        assert_ne!(
            flags & DESC_WRITE,
            DESC_WRITE,
            "the header is device-readable"
        );
        let header = (header - QUEUE_BASE) as usize;
        let kind = self.word(header);
        let sector = self.long(header + 8);
        let mut at = (sector * SECTOR_SIZE) as usize;
        let mut length = 0;
        loop {
            let (address, size, flags, following) = self.descriptor(usize::from(next));
            if flags & DESC_NEXT == 0 {
                assert_eq!(size, 1, "the status is one byte");
                assert_ne!(flags & DESC_WRITE, 0, "the status is device-writable");
                let status = match self.hostile {
                    Hostile::BadStatus => 7,
                    Hostile::IoError => 1,
                    Hostile::Unsupported => 2,
                    _ => 0,
                };
                self.set((address - QUEUE_BASE) as usize, &[status]);
                break;
            }
            let data = (address - DATA_BASE) as usize;
            let size = size as usize;
            match kind {
                BLK_T_IN => {
                    assert_ne!(flags & DESC_WRITE, 0, "a read's data is device-writable");
                    let copied = self.disk[at..at + size].to_vec();
                    self.data[data..data + size].copy_from_slice(&copied);
                }
                BLK_T_OUT => {
                    assert_eq!(flags & DESC_WRITE, 0, "a write's data is device-readable");
                    let copied = self.data[data..data + size].to_vec();
                    self.disk[at..at + size].copy_from_slice(&copied);
                }
                other => panic!("data with kind {other}"),
            }
            at += size;
            length += size as u32;
            next = following;
        }
        if kind == BLK_T_FLUSH {
            self.flushes += 1;
        }
        match self.hostile {
            Hostile::Silent => {}
            Hostile::BadId => self.complete(1, length + 1),
            Hostile::Repeats => {
                self.complete(head as u32, length + 1);
                self.complete(head as u32, length + 1);
            }
            Hostile::HugeLength => self.complete(head as u32, u32::MAX),
            Hostile::IdOutOfRange => self.complete((SLOT * MAX_IN_FLIGHT) as u32, 1),
            Hostile::RepeatsFirst => self.complete(0, 1),
            Hostile::ShortRead if kind == BLK_T_IN => self.complete(head as u32, length),
            // What the device wrote: a read's data, and the status.
            _ if kind == BLK_T_IN => self.complete(head as u32, length + 1),
            _ => self.complete(head as u32, 1),
        }
    }
}

struct Fake(Rc<RefCell<Device>>);

impl Registers for Fake {
    fn read(&self, offset: usize) -> u32 {
        let d = self.0.borrow();
        match offset {
            reg::MAGIC => MAGIC,
            reg::VERSION => 2,
            reg::DEVICE_ID => DEVICE_BLOCK,
            reg::DEVICE_FEATURES => match d.feature_sel {
                0 if d.flush_offered => F_BLK_FLUSH,
                1 if d.hostile != Hostile::NoVersion1 => F_VERSION_1,
                _ => 0,
            },
            reg::QUEUE_NUM_MAX if d.hostile == Hostile::ShortQueue => 16,
            reg::QUEUE_NUM_MAX => 1024,
            reg::QUEUE_READY => 0,
            reg::STATUS if d.hostile == Hostile::NoReset => 1,
            reg::STATUS => d.status,
            reg::CONFIG => SIZE_SECTORS as u32,
            reg::INTERRUPT_STATUS => 1,
            r if r == reg::CONFIG + 4 => 0,
            _ => d.registers[offset / 4],
        }
    }

    fn write(&mut self, offset: usize, value: u32) {
        let mut d = self.0.borrow_mut();
        match offset {
            reg::STATUS => {
                d.status_writes.push(value);
                let refuse = d.hostile == Hostile::RefusesFeatures;
                d.status = if refuse {
                    value & !STATUS_FEATURES_OK
                } else {
                    value
                };
            }
            reg::DEVICE_FEATURES_SEL => d.feature_sel = value,
            reg::DRIVER_FEATURES => {
                let word = d.registers[reg::DRIVER_FEATURES_SEL / 4] as usize;
                d.driver_features[word] = value;
            }
            reg::QUEUE_NOTIFY => {
                d.published_notify = true;
                d.run();
            }
            _ => d.registers[offset / 4] = value,
        }
    }
}

impl Shared for Fake {
    fn read8(&self, offset: usize) -> u8 {
        self.0.borrow().shared[offset]
    }

    fn read16(&self, offset: usize) -> u16 {
        self.0.borrow().half(offset)
    }

    fn read32(&self, offset: usize) -> u32 {
        self.0.borrow().word(offset)
    }

    fn write8(&mut self, offset: usize, value: u8) {
        self.0.borrow_mut().set(offset, &[value]);
    }

    fn write16(&mut self, offset: usize, value: u16) {
        self.0.borrow_mut().set(offset, &value.to_le_bytes());
    }

    fn write32(&mut self, offset: usize, value: u32) {
        self.0.borrow_mut().set(offset, &value.to_le_bytes());
    }

    fn write64(&mut self, offset: usize, value: u64) {
        self.0.borrow_mut().set(offset, &value.to_le_bytes());
    }

    fn publish(&self) {}

    fn acquire(&self) {}
}

type Started = (Rc<RefCell<Device>>, Result<Driver<Fake, Fake>, Error>);

fn driver(hostile: Hostile) -> Started {
    let device = Device::new(hostile);
    let driver = Driver::new(Fake(device.clone()), Fake(device.clone()), QUEUE_BASE);
    (device, driver)
}

fn piece(offset: u64, length: u32) -> Piece {
    Piece {
        address: DATA_BASE + offset,
        length,
    }
}

fn request(kind: Kind, sector: u64, pieces: &[Piece]) -> Request<'_> {
    Request {
        kind,
        sector,
        pieces,
    }
}

#[test]
fn negotiation_takes_version_1_and_flush() {
    let (device, driver) = driver(Hostile::None);
    let driver = driver.unwrap();
    assert_eq!(
        driver.geometry(),
        Geometry {
            sectors: SIZE_SECTORS,
            flush: true
        }
    );
    let d = device.borrow();
    assert_eq!(d.driver_features, [F_BLK_FLUSH, F_VERSION_1]);
    assert_ne!(d.status & STATUS_DRIVER_OK, 0);
    assert_eq!(d.registers[reg::QUEUE_DESC / 4], QUEUE_BASE as u32);
    assert_eq!(d.registers[reg::QUEUE_READY / 4], 1);
}

#[test]
fn a_device_with_no_flush_is_said_to_have_none() {
    let device = Device::new(Hostile::None);
    device.borrow_mut().flush_offered = false;
    let d = Driver::new(Fake(device.clone()), Fake(device.clone()), QUEUE_BASE).unwrap();
    assert!(!d.geometry().flush);
    assert_eq!(device.borrow().driver_features[0], 0);
}

#[test]
fn a_bad_device_is_refused_at_negotiation() {
    assert_eq!(
        driver(Hostile::NoVersion1).1.err(),
        Some(Error::Negotiation)
    );
    assert_eq!(
        driver(Hostile::RefusesFeatures).1.err(),
        Some(Error::Negotiation)
    );
    assert_eq!(driver(Hostile::ShortQueue).1.err(), Some(Error::Queue));
    assert_eq!(driver(Hostile::NoReset).1.err(), Some(Error::Device));
    let (device, _) = driver(Hostile::NoVersion1);
    assert_ne!(device.borrow().status & STATUS_FAILED, 0);
}

#[test]
fn a_write_then_a_read_comes_back() {
    let (device, d) = driver(Hostile::None);
    let mut d = d.unwrap();
    device.borrow_mut().data[..4096].fill(0x5a);
    d.queue(&request(Kind::Write, 8, &[piece(0, 4096)]))
        .unwrap();
    d.notify();
    assert_eq!(d.collect(), Ok(0));
    assert!(!d.take_failure());
    assert_eq!(device.borrow().disk[8 * 512], 0x5a);
    device.borrow_mut().data[..4096].fill(0);
    d.queue(&request(Kind::Read, 8, &[piece(4096, 4096)]))
        .unwrap();
    d.notify();
    assert_eq!(d.collect(), Ok(0));
    assert_eq!(device.borrow().data[4096 + 100], 0x5a);
}

#[test]
fn a_flush_reaches_the_device() {
    let (device, d) = driver(Hostile::None);
    let mut d = d.unwrap();
    d.queue(&request(Kind::Flush, 0, &[])).unwrap();
    d.notify();
    assert_eq!(d.collect(), Ok(0));
    assert_eq!(device.borrow().flushes, 1);
}

#[test]
fn a_batch_completes_together_and_the_rings_wrap() {
    let (device, d) = driver(Hostile::None);
    let mut d = d.unwrap();
    // Past 65,536: the 16-bit indexes wrap.
    for round in 0..17_000u64 {
        for i in 0..4 {
            let sector = (round + i * 8) % 1024;
            d.queue(&request(Kind::Write, sector, &[piece(i * 512, 512)]))
                .unwrap();
        }
        d.notify();
        assert_eq!(d.collect(), Ok(0), "round {round}");
    }
    assert_eq!(device.borrow().avail_seen, (68_000u32 % 65_536) as u16);
}

#[test]
fn the_queue_refuses_what_does_not_fit() {
    let (_, d) = driver(Hostile::None);
    let mut d = d.unwrap();
    let many = [piece(0, 512); MAX_PIECES + 1];
    assert_eq!(
        d.queue(&request(Kind::Read, 0, &many)),
        Err(Error::TooLarge)
    );
    assert_eq!(d.capacity(), MAX_IN_FLIGHT);
    for _ in 0..MAX_IN_FLIGHT {
        d.queue(&request(Kind::Read, 0, &[piece(0, 512)])).unwrap();
    }
    assert_eq!(d.capacity(), 0);
    assert_eq!(
        d.queue(&request(Kind::Read, 0, &[piece(0, 512)])),
        Err(Error::TooLarge)
    );
}

#[test]
fn a_failed_request_is_a_failure_and_the_queue_goes_on() {
    for hostile in [Hostile::IoError, Hostile::Unsupported] {
        let (device, d) = driver(hostile);
        let mut d = d.unwrap();
        d.queue(&request(Kind::Read, 0, &[piece(0, 512)])).unwrap();
        d.notify();
        assert_eq!(d.collect(), Ok(0));
        assert!(d.take_failure());
        assert!(!d.take_failure(), "once");
        device.borrow_mut().hostile = Hostile::None;
        d.queue(&request(Kind::Read, 0, &[piece(0, 512)])).unwrap();
        d.notify();
        assert_eq!(d.collect(), Ok(0));
        assert!(!d.take_failure());
    }
}

/// INV-0054-4: whatever the device writes, the driver reports it and holds no
/// wrong index.
#[test]
fn a_lying_device_is_a_device_error() {
    for hostile in [
        Hostile::UsedIndexJumps,
        Hostile::BadId,
        Hostile::Repeats,
        Hostile::BadStatus,
        Hostile::IdOutOfRange,
        Hostile::RepeatsFirst,
    ] {
        let (_, d) = driver(hostile);
        let mut d = d.unwrap();
        // Two, so that a repeat of the first is not more than is outstanding.
        d.queue(&request(Kind::Write, 0, &[piece(0, 512)])).unwrap();
        d.queue(&request(Kind::Write, 8, &[piece(512, 512)]))
            .unwrap();
        d.notify();
        assert_eq!(d.collect(), Err(Error::Device));
    }
}

#[test]
fn a_used_length_is_not_trusted() {
    for (hostile, kind) in [
        (Hostile::HugeLength, Kind::Read),
        (Hostile::HugeLength, Kind::Write),
        (Hostile::ShortRead, Kind::Read),
    ] {
        let (_, d) = driver(hostile);
        let mut d = d.unwrap();
        d.queue(&request(kind, 0, &[piece(0, 512)])).unwrap();
        d.notify();
        assert_eq!(d.collect(), Err(Error::Device), "{hostile:?} {kind:?}");
    }
    // The same device: a write's length is its status byte, and is right.
    let (device, d) = driver(Hostile::ShortRead);
    let mut d = d.unwrap();
    d.queue(&request(Kind::Write, 0, &[piece(0, 512)])).unwrap();
    d.notify();
    assert_eq!(d.collect(), Ok(0), "a write's length is the status");
    drop(device);
}

/// A status the device never wrote does not read as success.
#[test]
fn a_silent_device_stays_outstanding() {
    let (_, d) = driver(Hostile::Silent);
    let mut d = d.unwrap();
    d.queue(&request(Kind::Read, 0, &[piece(0, 512)])).unwrap();
    d.notify();
    assert_eq!(d.collect(), Ok(1));
}

#[test]
fn reset_stops_the_device() {
    let (device, d) = driver(Hostile::None);
    assert!(d.unwrap().reset());
    assert_eq!(device.borrow().status, 0);
}

#[test]
fn acknowledge_clears_what_is_pending() {
    let (device, d) = driver(Hostile::None);
    d.unwrap().acknowledge();
    assert_eq!(device.borrow().registers[reg::INTERRUPT_ACK / 4], 1);
}

#[test]
fn pieces_split_at_runs() {
    let runs = [(0x1000, 0x1000), (0x8000, 0x2000)];
    let mut out = [Piece {
        address: 0,
        length: 0,
    }; 4];
    assert_eq!(pieces(&runs, 0x800, 0x1000, &mut out), Some(2));
    assert_eq!(
        out[..2],
        [
            Piece {
                address: 0x1800,
                length: 0x800
            },
            Piece {
                address: 0x8000,
                length: 0x800
            }
        ]
    );
    assert_eq!(pieces(&runs, 0x2800, 0x800, &mut out), Some(1));
    assert_eq!(out[0].address, 0x9800);
    assert_eq!(pieces(&runs, 0, 0x3001, &mut out), None, "off the end");
    assert_eq!(pieces(&runs, 0x4000, 1, &mut out), None);
    assert_eq!(pieces(&runs, u64::MAX, 1, &mut out), None);
    assert_eq!(pieces(&runs, 0, 0x3000, &mut out[..1]), None, "too many");
}

/// `blkd`'s check of a service's range (INV-0054-4).
#[test]
fn a_range_is_placed_only_inside_the_device_and_the_memory() {
    let runs = [(0x1000, 0x1000), (0x8000, 0x2000)];
    let mut out = [Piece {
        address: 0,
        length: 0,
    }; 4];
    assert_eq!(place(&runs, 100, 0, 8, 0, &mut out), Some(1));
    assert_eq!(
        place(&runs, 100, 92, 8, 0, &mut out),
        Some(1),
        "the last sectors"
    );
    assert_eq!(
        place(&runs, 100, 93, 8, 0, &mut out),
        None,
        "past the device"
    );
    assert_eq!(place(&runs, 100, u64::MAX, 1, 0, &mut out), None, "wraps");
    assert_eq!(place(&runs, 100, 0, 0, 0, &mut out), None, "empty");
    assert_eq!(
        place(&runs, 100, 0, 8, 0x2000, &mut out),
        Some(1),
        "the memory's end"
    );
    assert_eq!(
        place(&runs, 100, 0, 8, 0x2001, &mut out),
        None,
        "past the memory"
    );
    assert_eq!(
        place(&runs, 100, 0, 1, u64::MAX, &mut out),
        None,
        "offset wraps"
    );
    assert_eq!(
        place(&runs, u64::MAX, 0, u32::MAX, 0, &mut out),
        None,
        "too large"
    );
}

/// INV-0080-1, the driver's half: a new driver resets the device before it
/// touches anything, and `reset` writes 0 and waits for it.
#[test]
fn a_new_driver_resets_the_device_first() {
    let (device, d) = driver(Hostile::None);
    let mut d = d.unwrap();
    let writes = device.borrow().status_writes.clone();
    assert_eq!(writes.first(), Some(&0), "{writes:?}");
    assert!(writes[1..].iter().all(|&w| w != 0), "once, first");
    assert!(d.reset());
    assert_eq!(device.borrow().status_writes.last(), Some(&0));
    assert_eq!(device.borrow().status, 0);
}
