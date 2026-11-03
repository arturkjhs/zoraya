//! Signals and ports (ADR-0006, ADR-0041), as pure data.
//!
//! A *source* has signal bits; a *port* has a ready list; a *subscription*
//! links them. Everything refers to everything else by index, so nothing here
//! can form a reference cycle, and the kernel keeps all of it behind one lock.
//!
//! The ready list holds at most one item per subscription plus the user
//! packets and some items left stale by removals, and room for that is reserved
//! when a subscription or user packet is added, so asserting a signal never
//! allocates. Removal marks items stale instead of searching the list, and
//! removing many subscriptions sweeps each port once, so ending a handle costs
//! what it holds, not the square of it. `take` costs the packets
//! it returns plus the entries that went stale, which earlier assertions paid
//! for, and never looks at an idle subscription.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;
#[cfg(test)]
extern crate std;

use alloc::collections::VecDeque;
use alloc::vec::Vec;

use zoraya_abi::{Mode, PACKET_SIGNAL, PACKET_USER, Packet};

/// User packets one port can hold (ADR-0006 leaves the bound to memory; a
/// fixed bound stops one sender from taking all of it).
pub const MAX_USER_PACKETS: usize = 1024;

/// What can go wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    NoMemory,
    /// The port was closed.
    Closed,
    /// A subscription with this key exists on the port.
    KeyInUse,
    /// No subscription has this key.
    NoSuchKey,
    /// The port holds as many user packets as it may.
    Full,
}

/// Takes a slot. `free` always has room for every slot, so that giving one
/// back never allocates.
fn alloc_slot<T>(slots: &mut Vec<Option<T>>, free: &mut Vec<u32>, value: T) -> Result<u32, Error> {
    if let Some(index) = free.pop() {
        slots[index as usize] = Some(value);
        return Ok(index);
    }
    slots.try_reserve(1).map_err(|_| Error::NoMemory)?;
    free.try_reserve(slots.len() + 1)
        .map_err(|_| Error::NoMemory)?;
    slots.push(Some(value));
    Ok(slots.len() as u32 - 1)
}

/// A thing with signal bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceId(u32);

/// A port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PortId(u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    /// The subscription's slot and its serial: a slot is reused, a serial not.
    Subscription(u32, u64),
    User {
        key: u64,
        data: u64,
    },
}

#[derive(Debug)]
struct Subscription {
    port: u32,
    source: u32,
    /// This subscription's place in the source's list.
    place: u32,
    key: u64,
    mask: u32,
    mode: Mode,
    /// The handle it belongs to.
    owner: u64,
    /// In the port's ready list.
    queued: bool,
    /// Told apart from a later user of the same slot.
    serial: u64,
    /// `Edge`: assertions not yet returned, and their bits.
    pending: u32,
    latched: u32,
}

#[derive(Debug)]
struct Source {
    bits: u32,
    subscriptions: Vec<u32>,
}

#[derive(Debug)]
struct Port<W> {
    ready: VecDeque<Item>,
    /// Sorted by key.
    subscriptions: Vec<(u64, u32)>,
    user: usize,
    /// Items in `ready` whose subscription is gone.
    stale: usize,
    /// The last sweep that visited this port.
    swept: u64,
    closed: bool,
    waiters: Vec<W>,
}

impl<W> Port<W> {
    /// Keeps room for every item this port could then hold.
    fn reserve(&mut self, subscriptions: usize, user: usize) -> Result<(), Error> {
        let needed = (subscriptions + user + self.stale).saturating_sub(self.ready.len());
        self.ready.try_reserve(needed).map_err(|_| Error::NoMemory)
    }
}

/// All sources, ports and subscriptions. `W` is what waits on a port.
#[derive(Debug)]
pub struct Events<W> {
    sources: Vec<Option<Source>>,
    free_sources: Vec<u32>,
    ports: Vec<Option<Port<W>>>,
    free_ports: Vec<u32>,
    subscriptions: Vec<Option<Subscription>>,
    free_subscriptions: Vec<u32>,
    serial: u64,
    sweeps: u64,
}

impl<W> Default for Events<W> {
    fn default() -> Self {
        Events::new()
    }
}

impl<W> Events<W> {
    pub const fn new() -> Events<W> {
        Events {
            sources: Vec::new(),
            free_sources: Vec::new(),
            ports: Vec::new(),
            free_ports: Vec::new(),
            subscriptions: Vec::new(),
            free_subscriptions: Vec::new(),
            serial: 0,
            sweeps: 0,
        }
    }

    fn source(&mut self, id: u32) -> &mut Source {
        self.sources[id as usize].as_mut().expect("source is alive")
    }

    fn port(&mut self, id: u32) -> &mut Port<W> {
        self.ports[id as usize].as_mut().expect("port is alive")
    }

    fn subscription(&mut self, id: u32) -> &mut Subscription {
        self.subscriptions[id as usize]
            .as_mut()
            .expect("subscription is alive")
    }

    pub fn new_source(&mut self, bits: u32) -> Result<SourceId, Error> {
        let source = Source {
            bits,
            subscriptions: Vec::new(),
        };
        alloc_slot(&mut self.sources, &mut self.free_sources, source).map(SourceId)
    }

    /// Ends a source and every subscription to it.
    pub fn free_source(&mut self, id: SourceId) {
        self.remove_matching(id, None);
        self.sources[id.0 as usize] = None;
        self.free_sources.push(id.0);
    }

    pub fn new_port(&mut self) -> Result<PortId, Error> {
        let port = Port {
            ready: VecDeque::new(),
            subscriptions: Vec::new(),
            user: 0,
            stale: 0,
            swept: 0,
            closed: false,
            waiters: Vec::new(),
        };
        alloc_slot(&mut self.ports, &mut self.free_ports, port).map(PortId)
    }

    /// Ends every subscription and wakes every waiter. The port stays until
    /// [`Events::free_port`], and every use of it gives `Closed`.
    pub fn close_port(&mut self, id: PortId, wake: &mut impl FnMut(W)) {
        for (_, s) in core::mem::take(&mut self.port(id.0).subscriptions) {
            let subscription = self.subscription(s);
            subscription.queued = false;
            self.detach(s);
        }
        let port = self.port(id.0);
        port.closed = true;
        port.ready.clear();
        port.user = 0;
        port.stale = 0;
        for waiter in port.waiters.drain(..) {
            wake(waiter);
        }
    }

    pub fn free_port(&mut self, id: PortId, wake: &mut impl FnMut(W)) {
        self.close_port(id, wake);
        self.ports[id.0 as usize] = None;
        self.free_ports.push(id.0);
    }

    pub fn is_closed(&mut self, id: PortId) -> bool {
        self.port(id.0).closed
    }

    /// Removes `s` from its source and frees it. The port's lists are the
    /// caller's to fix.
    fn detach(&mut self, s: u32) {
        let (source, place) = {
            let subscription = self.subscription(s);
            (subscription.source, subscription.place as usize)
        };
        let list = &mut self.source(source).subscriptions;
        list.swap_remove(place);
        if let Some(&moved) = list.get(place) {
            self.subscription(moved).place = place as u32;
        }
        self.subscriptions[s as usize] = None;
        self.free_subscriptions.push(s);
    }

    /// Marks a subscription's ready item stale, for `take` to drop.
    fn mark_stale(&mut self, s: u32) {
        let subscription = self.subscription(s);
        let (queued, port) = (subscription.queued, subscription.port);
        if queued {
            self.port(port).stale += 1;
        }
    }

    /// Removes the subscriptions of `source` that belong to `owner`, or all of
    /// them. Each port's list is swept once, however many leave it.
    fn remove_matching(&mut self, source: SourceId, owner: Option<u64>) {
        self.sweeps += 1;
        let sweep = self.sweeps;
        let matches = |s: &Subscription| s.source == source.0 && owner.is_none_or(|o| s.owner == o);
        for i in 0..self.source(source.0).subscriptions.len() {
            let s = self.source(source.0).subscriptions[i];
            let subscription = self.subscription(s);
            if !matches(subscription) {
                continue;
            }
            let port_id = subscription.port;
            self.mark_stale(s);
            if self.port(port_id).swept != sweep {
                self.port(port_id).swept = sweep;
                let mut list = core::mem::take(&mut self.port(port_id).subscriptions);
                list.retain(|(_, s)| {
                    !self.subscriptions[*s as usize]
                        .as_ref()
                        .is_some_and(matches)
                });
                self.port(port_id).subscriptions = list;
            }
        }
        let mut i = 0;
        while let Some(&s) = self.source(source.0).subscriptions.get(i) {
            if matches(self.subscription(s)) {
                self.detach(s);
            } else {
                i += 1;
            }
        }
    }

    fn remove_subscription(&mut self, s: u32) {
        let (port_id, key) = {
            let subscription = self.subscription(s);
            (subscription.port, subscription.key)
        };
        self.mark_stale(s);
        let port = self.port(port_id);
        if let Ok(at) = port.subscriptions.binary_search_by_key(&key, |e| e.0) {
            port.subscriptions.remove(at);
        }
        self.detach(s);
    }

    /// Links `source` to `port`. A `Level` subscription whose bits are already
    /// asserted is ready at once (INV-0006-5). `Edge` waits for an assertion.
    pub fn subscribe(
        &mut self,
        port_id: PortId,
        source: SourceId,
        mask: u32,
        key: u64,
        mode: Mode,
        owner: u64,
    ) -> Result<(), Error> {
        let port = self.port(port_id.0);
        if port.closed {
            return Err(Error::Closed);
        }
        let at = match port.subscriptions.binary_search_by_key(&key, |e| e.0) {
            Ok(_) => return Err(Error::KeyInUse),
            Err(at) => at,
        };
        // Stale items are bounded by what is live, so subscribing and removing
        // in a loop cannot grow the list.
        if port.stale > port.subscriptions.len() + port.user {
            let live = port.ready.len() - port.stale;
            let subscriptions = &self.subscriptions;
            let port = self.ports[port_id.0 as usize]
                .as_mut()
                .expect("port is alive");
            port.ready.retain(|item| match item {
                Item::Subscription(s, serial) => subscriptions[*s as usize]
                    .as_ref()
                    .is_some_and(|sub| sub.serial == *serial),
                Item::User { .. } => true,
            });
            debug_assert_eq!(port.ready.len(), live);
            port.stale = 0;
        }
        let port = self.port(port_id.0);
        // Everything that can fail comes first.
        port.subscriptions
            .try_reserve(1)
            .map_err(|_| Error::NoMemory)?;
        let (count, user) = (port.subscriptions.len() + 1, port.user);
        port.reserve(count, user)?;
        self.source(source.0)
            .subscriptions
            .try_reserve(1)
            .map_err(|_| Error::NoMemory)?;
        let place = self.source(source.0).subscriptions.len() as u32;
        self.serial += 1;
        let serial = self.serial;
        let s = alloc_slot(
            &mut self.subscriptions,
            &mut self.free_subscriptions,
            Subscription {
                port: port_id.0,
                source: source.0,
                place,
                key,
                mask,
                mode,
                owner,
                queued: false,
                serial,
                pending: 0,
                latched: 0,
            },
        )?;
        self.source(source.0).subscriptions.push(s);
        self.port(port_id.0).subscriptions.insert(at, (key, s));
        if mode == Mode::Level && self.source(source.0).bits & mask != 0 {
            self.subscription(s).queued = true;
            self.port(port_id.0)
                .ready
                .push_back(Item::Subscription(s, serial));
        }
        Ok(())
    }

    pub fn unsubscribe(&mut self, port: PortId, key: u64) -> Result<(), Error> {
        let state = self.port(port.0);
        if state.closed {
            return Err(Error::Closed);
        }
        let at = state
            .subscriptions
            .binary_search_by_key(&key, |e| e.0)
            .map_err(|_| Error::NoSuchKey)?;
        let s = state.subscriptions[at].1;
        self.remove_subscription(s);
        Ok(())
    }

    /// Ends the subscriptions of one handle (INV-0006-4).
    pub fn remove_owner(&mut self, source: SourceId, owner: u64) {
        self.remove_matching(source, Some(owner));
    }

    pub fn signals(&mut self, source: SourceId) -> u32 {
        self.source(source.0).bits
    }

    /// Asserts `bits`, and wakes the waiters of each port that gained an item.
    /// Allocates nothing.
    pub fn set(&mut self, source: SourceId, bits: u32, wake: &mut impl FnMut(W)) {
        let state = self.source(source.0);
        let rising = bits & !state.bits;
        state.bits |= bits;
        if rising == 0 {
            return;
        }
        for i in 0..self.source(source.0).subscriptions.len() {
            let s = self.source(source.0).subscriptions[i];
            let subscription = self.subscription(s);
            let hit = rising & subscription.mask;
            if hit == 0 {
                continue;
            }
            if subscription.mode == Mode::Edge {
                subscription.pending = subscription.pending.saturating_add(1);
                subscription.latched |= hit;
            }
            if !subscription.queued {
                subscription.queued = true;
                let (port_id, serial) = (subscription.port, subscription.serial);
                let port = self.port(port_id);
                port.ready.push_back(Item::Subscription(s, serial));
                for waiter in port.waiters.drain(..) {
                    wake(waiter);
                }
            }
        }
    }

    /// Deasserts `bits`. A `Level` item that this leaves stale goes at `take`.
    pub fn clear(&mut self, source: SourceId, bits: u32) {
        self.source(source.0).bits &= !bits;
    }

    /// Queues a user packet and wakes the waiters.
    pub fn queue(
        &mut self,
        id: PortId,
        key: u64,
        data: u64,
        wake: &mut impl FnMut(W),
    ) -> Result<(), Error> {
        let port = self.port(id.0);
        if port.closed {
            return Err(Error::Closed);
        }
        if port.user == MAX_USER_PACKETS {
            return Err(Error::Full);
        }
        let (subscriptions, user) = (port.subscriptions.len(), port.user + 1);
        port.reserve(subscriptions, user)?;
        port.ready.push_back(Item::User { key, data });
        port.user = user;
        for waiter in port.waiters.drain(..) {
            wake(waiter);
        }
        Ok(())
    }

    /// Fills `out` from the ready list, and gives how many. A `Level` item
    /// still asserted goes to the back, so it appears once per call.
    pub fn take(&mut self, id: PortId, out: &mut [Packet]) -> Result<usize, Error> {
        if self.port(id.0).closed {
            return Err(Error::Closed);
        }
        let mut count = 0;
        for _ in 0..self.port(id.0).ready.len() {
            if count == out.len() {
                break;
            }
            let Some(item) = self.port(id.0).ready.pop_front() else {
                break;
            };
            match item {
                Item::User { key, data } => {
                    self.port(id.0).user -= 1;
                    out[count] = Packet {
                        key,
                        kind: PACKET_USER,
                        signals: 0,
                        data,
                    };
                    count += 1;
                }
                Item::Subscription(s, serial) => {
                    let alive = self.subscriptions[s as usize]
                        .as_ref()
                        .is_some_and(|sub| sub.serial == serial);
                    if !alive {
                        self.port(id.0).stale -= 1;
                        continue;
                    }
                    let source = self.subscription(s).source;
                    let bits = self.source(source).bits;
                    let subscription = self.subscription(s);
                    let key = subscription.key;
                    let signals = match subscription.mode {
                        Mode::Level => bits & subscription.mask,
                        Mode::Edge => {
                            subscription.pending -= 1;
                            subscription.latched
                        }
                    };
                    let again = match subscription.mode {
                        Mode::Level => signals != 0,
                        Mode::Edge => subscription.pending != 0,
                    };
                    if !again {
                        subscription.queued = false;
                        subscription.latched = 0;
                    }
                    if again {
                        self.port(id.0).ready.push_back(item);
                    }
                    if signals != 0 {
                        out[count] = Packet {
                            key,
                            kind: PACKET_SIGNAL,
                            signals,
                            data: 0,
                        };
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    /// Lists a waiter, to be woken with the next packet or the close.
    pub fn add_waiter(&mut self, id: PortId, waiter: W) -> Result<(), W> {
        let port = self.port(id.0);
        if port.waiters.try_reserve(1).is_err() {
            return Err(waiter);
        }
        port.waiters.push(waiter);
        Ok(())
    }

    /// Takes a waiter off the list again, after a deadline.
    pub fn remove_waiter(&mut self, id: PortId, is_it: impl Fn(&W) -> bool) -> Option<W> {
        let port = self.port(id.0);
        let at = port.waiters.iter().position(is_it)?;
        Some(port.waiters.swap_remove(at))
    }

    /// Whether the ready list can take an item for every subscription that has
    /// none, without allocating.
    pub fn has_room(&mut self, id: PortId) -> bool {
        let port = self.port(id.0);
        port.ready.capacity() >= port.subscriptions.len() + port.user + port.stale
    }

    /// Items on the ready list, stale ones too.
    pub fn ready_len(&mut self, id: PortId) -> usize {
        self.port(id.0).ready.len()
    }

    /// Live subscriptions, for tests of the kernel.
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.iter().flatten().count()
    }

    /// Live sources and ports, for tests of the kernel.
    pub fn live(&self) -> (usize, usize) {
        (
            self.sources.iter().flatten().count(),
            self.ports.iter().flatten().count(),
        )
    }
}

mod requests;
pub use requests::{MAX_REQUESTS, QueueFull, RequestQueue};

#[cfg(test)]
mod tests;

// A runaway test fails at 512 MiB instead of taking the host (docs/backlog.md).
#[cfg(test)]
zoraya_test_alloc::cap_memory!();
