//! A device-side fake of a virtio-mmio transport and its split queues, over
//! plain memory, for host tests of drivers. A test plays the device: a hook
//! runs on every notify and every configuration write, and helpers read the
//! available ring, follow chains and write used entries, truthfully or not.

extern crate alloc;

use alloc::boxed::Box;
use alloc::rc::Rc;
use alloc::vec;
use alloc::vec::Vec;
use core::cell::RefCell;

use crate::queue::{Buffer, DESC_NEXT, DESC_WRITE};
use crate::{MAGIC, Registers, Shared, reg};

/// One queue's registers as the driver set them, and the device's place.
#[derive(Debug, Clone, Copy, Default)]
pub struct FakeQueue {
    pub num_max: u32,
    pub num: u32,
    pub ready: u32,
    pub desc: u64,
    pub avail: u64,
    pub used: u64,
    /// Available entries the device has taken.
    pub avail_seen: u16,
    /// The used index the device has published.
    pub used_index: u16,
}

/// What the device sees happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Notify(u32),
    /// A write to configuration space at this offset.
    Config(usize),
}

pub struct Fake {
    pub magic: u32,
    pub version: u32,
    pub device_id: u32,
    pub device_features: [u32; 2],
    pub driver_features: [u32; 2],
    pub status: u32,
    pub status_writes: Vec<u32>,
    /// Status never reads back 0 after a reset.
    pub no_reset: bool,
    /// `FEATURES_OK` does not stick.
    pub refuse_features: bool,
    pub queues: [FakeQueue; 2],
    pub config: [u8; 256],
    pub interrupt_status: u32,
    /// The shared memory, at device address `base`.
    pub memory: Vec<u8>,
    pub base: u64,
    pub notifies: Vec<u32>,
    feature_sel: u32,
    driver_feature_sel: u32,
    queue_sel: u32,
}

type Hook = Box<dyn FnMut(&mut Fake, Event)>;

/// The device, shared between the driver's two views of it.
#[derive(Clone)]
pub struct Device {
    pub state: Rc<RefCell<Fake>>,
    hook: Rc<RefCell<Hook>>,
}

impl Device {
    /// A device of `device_id` with `size` bytes of shared memory at `base`.
    pub fn new(device_id: u32, base: u64, size: usize) -> Device {
        let fake = Fake {
            magic: MAGIC,
            version: 2,
            device_id,
            device_features: [0, 1],
            driver_features: [0; 2],
            status: 0,
            status_writes: Vec::new(),
            no_reset: false,
            refuse_features: false,
            queues: [FakeQueue {
                num_max: 256,
                ..FakeQueue::default()
            }; 2],
            config: [0; 256],
            interrupt_status: 0,
            memory: vec![0; size],
            base,
            notifies: Vec::new(),
            feature_sel: 0,
            driver_feature_sel: 0,
            queue_sel: 0,
        };
        Device {
            state: Rc::new(RefCell::new(fake)),
            hook: Rc::new(RefCell::new(Box::new(|_, _| {}))),
        }
    }

    /// What the device does on a notify or a configuration write.
    pub fn on(&self, hook: impl FnMut(&mut Fake, Event) + 'static) {
        *self.hook.borrow_mut() = Box::new(hook);
    }

    pub fn registers(&self) -> FakeRegisters {
        FakeRegisters(self.clone())
    }

    pub fn shared(&self) -> FakeShared {
        FakeShared(self.state.clone())
    }

    fn fire(&self, event: Event) {
        let mut hook = self.hook.borrow_mut();
        let mut state = self.state.borrow_mut();
        hook(&mut state, event);
    }
}

impl Fake {
    fn offset(&self, address: u64, length: usize) -> Option<usize> {
        let at = usize::try_from(address.checked_sub(self.base)?).ok()?;
        (at.checked_add(length)? <= self.memory.len()).then_some(at)
    }

    /// Bytes at a device address inside the shared memory.
    pub fn read(&self, address: u64, length: usize) -> Option<Vec<u8>> {
        let at = self.offset(address, length)?;
        Some(self.memory[at..at + length].to_vec())
    }

    /// Writes bytes at a device address inside the shared memory.
    pub fn write(&mut self, address: u64, bytes: &[u8]) -> bool {
        match self.offset(address, bytes.len()) {
            Some(at) => {
                self.memory[at..at + bytes.len()].copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }

    fn half(&self, address: u64) -> u16 {
        let b = self.read(address, 2).expect("ring inside shared memory");
        u16::from_le_bytes([b[0], b[1]])
    }

    /// The next head the driver made available on `queue`.
    pub fn next_available(&mut self, queue: usize) -> Option<u16> {
        let q = self.queues[queue];
        let index = self.half(q.avail + 2);
        if index == q.avail_seen {
            return None;
        }
        let slot = u64::from(q.avail_seen % q.num as u16);
        let head = self.half(q.avail + 4 + 2 * slot);
        self.queues[queue].avail_seen = q.avail_seen.wrapping_add(1);
        Some(head)
    }

    /// The chain at `head`, followed by its next flags, at most `num` long.
    pub fn chain(&self, queue: usize, head: u16) -> Vec<Buffer> {
        let q = self.queues[queue];
        let mut out = Vec::new();
        let mut index = head;
        for _ in 0..q.num {
            let at = q.desc + 16 * u64::from(index);
            let raw = self.read(at, 16).expect("descriptor inside shared memory");
            let address = u64::from_le_bytes(raw[0..8].try_into().unwrap());
            let length = u32::from_le_bytes(raw[8..12].try_into().unwrap());
            let flags = u16::from_le_bytes([raw[12], raw[13]]);
            let next = u16::from_le_bytes([raw[14], raw[15]]);
            out.push(Buffer {
                address,
                length,
                writable: flags & DESC_WRITE != 0,
            });
            if flags & DESC_NEXT == 0 {
                break;
            }
            index = next;
        }
        out
    }

    /// Writes a used entry for `head` and publishes the new used index.
    pub fn complete(&mut self, queue: usize, head: u32, length: u32) {
        let q = self.queues[queue];
        let slot = u64::from(q.used_index % q.num as u16);
        let entry = q.used + 4 + 8 * slot;
        let mut bytes = [0u8; 8];
        bytes[..4].copy_from_slice(&head.to_le_bytes());
        bytes[4..].copy_from_slice(&length.to_le_bytes());
        self.write(entry, &bytes);
        let index = q.used_index.wrapping_add(1);
        self.queues[queue].used_index = index;
        self.write(q.used + 2, &index.to_le_bytes());
    }

    /// Publishes a used index of the device's choosing: a lie, for tests.
    pub fn set_used_index(&mut self, queue: usize, index: u16) {
        let q = self.queues[queue];
        self.write(q.used + 2, &index.to_le_bytes());
    }
}

pub struct FakeRegisters(Device);

impl Registers for FakeRegisters {
    fn read(&self, offset: usize) -> u32 {
        let s = self.0.state.borrow();
        let q = &s.queues[(s.queue_sel as usize).min(1)];
        match offset {
            reg::MAGIC => s.magic,
            reg::VERSION => s.version,
            reg::DEVICE_ID => s.device_id,
            reg::DEVICE_FEATURES => s.device_features[(s.feature_sel as usize).min(1)],
            reg::QUEUE_NUM_MAX => q.num_max,
            reg::QUEUE_READY => q.ready,
            reg::INTERRUPT_STATUS => s.interrupt_status,
            reg::STATUS => s.status,
            at if at >= reg::CONFIG && at + 4 <= reg::CONFIG + 256 => {
                let c = at - reg::CONFIG;
                u32::from_le_bytes(s.config[c..c + 4].try_into().unwrap())
            }
            _ => 0,
        }
    }

    fn write(&mut self, offset: usize, value: u32) {
        let notify = {
            let mut s = self.0.state.borrow_mut();
            let sel = (s.queue_sel as usize).min(1);
            let high = |old: u64| old & 0xffff_ffff | u64::from(value) << 32;
            let low = |old: u64| old & !0xffff_ffff | u64::from(value);
            match offset {
                reg::DEVICE_FEATURES_SEL => s.feature_sel = value,
                reg::DRIVER_FEATURES_SEL => s.driver_feature_sel = value,
                reg::DRIVER_FEATURES => {
                    let i = (s.driver_feature_sel as usize).min(1);
                    s.driver_features[i] = value;
                }
                reg::QUEUE_SEL => s.queue_sel = value,
                reg::QUEUE_NUM => s.queues[sel].num = value,
                reg::QUEUE_READY => s.queues[sel].ready = value,
                reg::QUEUE_DESC => s.queues[sel].desc = low(s.queues[sel].desc),
                0x084 => s.queues[sel].desc = high(s.queues[sel].desc),
                reg::QUEUE_AVAIL => s.queues[sel].avail = low(s.queues[sel].avail),
                0x094 => s.queues[sel].avail = high(s.queues[sel].avail),
                reg::QUEUE_USED => s.queues[sel].used = low(s.queues[sel].used),
                0x0a4 => s.queues[sel].used = high(s.queues[sel].used),
                reg::INTERRUPT_ACK => s.interrupt_status &= !value,
                reg::STATUS => {
                    s.status_writes.push(value);
                    if value == 0 {
                        s.status = if s.no_reset { 1 } else { 0 };
                        let num_max = s.queues[0].num_max;
                        s.queues = [FakeQueue {
                            num_max,
                            ..FakeQueue::default()
                        }; 2];
                    } else if value & 8 != 0 && s.refuse_features {
                        s.status = value & !8;
                    } else {
                        s.status = value;
                    }
                }
                reg::QUEUE_NOTIFY => {
                    s.notifies.push(value);
                }
                at if at >= reg::CONFIG && at + 4 <= reg::CONFIG + 256 => {
                    let c = at - reg::CONFIG;
                    s.config[c..c + 4].copy_from_slice(&value.to_le_bytes());
                }
                _ => {}
            }
            match offset {
                reg::QUEUE_NOTIFY => Some(Event::Notify(value)),
                at if (reg::CONFIG..reg::CONFIG + 256).contains(&at) => {
                    Some(Event::Config(at - reg::CONFIG))
                }
                _ => None,
            }
        };
        if let Some(event) = notify {
            self.0.fire(event);
        }
    }

    fn read8(&self, offset: usize) -> u8 {
        let s = self.0.state.borrow();
        match offset.checked_sub(reg::CONFIG) {
            Some(c) if c < 256 => s.config[c],
            _ => 0,
        }
    }

    fn write8(&mut self, offset: usize, value: u8) {
        let event = {
            let mut s = self.0.state.borrow_mut();
            match offset.checked_sub(reg::CONFIG) {
                Some(c) if c < 256 => {
                    s.config[c] = value;
                    Some(Event::Config(c))
                }
                _ => None,
            }
        };
        if let Some(event) = event {
            self.0.fire(event);
        }
    }
}

pub struct FakeShared(Rc<RefCell<Fake>>);

impl FakeShared {
    fn at(&self, offset: usize, length: usize) -> usize {
        let s = self.0.borrow();
        assert!(
            offset + length <= s.memory.len(),
            "the driver reached past its shared memory: {offset:#x}"
        );
        offset
    }

    fn get<const N: usize>(&self, offset: usize) -> [u8; N] {
        let at = self.at(offset, N);
        self.0.borrow().memory[at..at + N].try_into().unwrap()
    }

    fn put(&mut self, offset: usize, bytes: &[u8]) {
        let at = self.at(offset, bytes.len());
        self.0.borrow_mut().memory[at..at + bytes.len()].copy_from_slice(bytes);
    }
}

impl Shared for FakeShared {
    fn read8(&self, offset: usize) -> u8 {
        self.get::<1>(offset)[0]
    }
    fn read16(&self, offset: usize) -> u16 {
        u16::from_le_bytes(self.get(offset))
    }
    fn read32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.get(offset))
    }
    fn write8(&mut self, offset: usize, value: u8) {
        self.put(offset, &[value]);
    }
    fn write16(&mut self, offset: usize, value: u16) {
        self.put(offset, &value.to_le_bytes());
    }
    fn write32(&mut self, offset: usize, value: u32) {
        self.put(offset, &value.to_le_bytes());
    }
    fn write64(&mut self, offset: usize, value: u64) {
        self.put(offset, &value.to_le_bytes());
    }
    fn publish(&self) {}
    fn acquire(&self) {}
}
