//! `BlockDevice` over the block protocol (ADR-0054). Calls are synchronous:
//! the service's one thread waits here. A driver that goes away or misses its
//! deadline is lost for good: from then on every read fails at once.

use alloc::vec;
use alloc::vec::Vec;

use zoraya_fs::{BLOCK_SIZE, Block, BlockDevice, DevError};
use zoraya_proto_block::{self as proto, Flush, Info, InfoReply, Range, Read, Write, status};
use zoraya_protocol::{Header, MAX_WIRE_MESSAGE, Seq, decode, encode};

const SECTORS_PER_BLOCK: u32 = (BLOCK_SIZE / 512) as u32;

/// What a call can end in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// The channel closed, or the deadline passed.
    Lost,
}

/// The way to the driver: a channel, and the data memory attached to it.
pub trait Link {
    /// Sends a request and waits for its reply, into `reply`.
    fn call(&mut self, request: &[u8], reply: &mut [u8]) -> Result<usize, LinkError>;
    /// The data memory the driver reads and writes.
    fn data(&mut self) -> &mut [u8];
}

pub struct ChannelDevice<L> {
    link: L,
    sectors: u64,
    flush: bool,
    lost: bool,
    transaction: u32,
    /// On the heap: a user stack is 64 KiB.
    message: Vec<u8>,
    request: Vec<u8>,
    ranges: Vec<Range>,
}

impl<L: Link> ChannelDevice<L> {
    /// Asks the driver what the device is.
    pub fn new(link: L) -> Result<ChannelDevice<L>, DevError> {
        let mut device = ChannelDevice {
            link,
            sectors: 0,
            flush: false,
            lost: false,
            transaction: 0,
            message: vec![0; MAX_WIRE_MESSAGE],
            request: vec![0; 16 * 1024],
            ranges: Vec::with_capacity(proto::MAX_BATCH),
        };
        let length = device.send(&Info {})?;
        let (_, info) =
            decode::<InfoReply>(&device.message[..length], 0).map_err(|_| DevError::IoError)?;
        if info.status.0 != status::OK {
            return Err(DevError::IoError);
        }
        device.sectors = info.sectors;
        device.flush = info.flush;
        Ok(device)
    }

    /// True once the driver is gone.
    pub fn is_lost(&self) -> bool {
        self.lost
    }

    /// The channel to the driver closed.
    pub fn mark_lost(&mut self) {
        self.lost = true;
    }

    /// Sends a request. The reply is at the front of `message`.
    fn send<'a, M: zoraya_protocol::Message<'a>>(&mut self, m: &M) -> Result<usize, DevError> {
        if self.lost {
            return Err(DevError::IoError);
        }
        self.transaction = self.transaction.wrapping_add(1).max(1);
        let size = encode(m, self.transaction, &mut self.request).map_err(|_| DevError::IoError)?;
        match self.link.call(&self.request[..size], &mut self.message) {
            Ok(length) => {
                let bytes = &self.message[..length.min(MAX_WIRE_MESSAGE)];
                let (header, _) = Header::decode(bytes).map_err(|_| DevError::IoError)?;
                if header.transaction != self.transaction {
                    return Err(self.lose());
                }
                Ok(length)
            }
            Err(LinkError::Lost) => Err(self.lose()),
        }
    }

    fn lose(&mut self) -> DevError {
        self.lost = true;
        DevError::IoError
    }

    fn done(&self, length: usize) -> Result<(), DevError> {
        let (_, reply) =
            decode::<proto::Done>(&self.message[..length], 0).map_err(|_| DevError::IoError)?;
        if reply.status.0 == status::OK {
            Ok(())
        } else {
            Err(DevError::IoError)
        }
    }
}

impl<L: Link> BlockDevice for ChannelDevice<L> {
    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn flush_supported(&self) -> bool {
        self.flush
    }

    fn read(&mut self, block: u64, buf: &mut Block) -> Result<(), DevError> {
        let range = Range {
            sector: block * u64::from(SECTORS_PER_BLOCK),
            offset: 0,
            count: SECTORS_PER_BLOCK,
        };
        let length = self.send(&Read { range })?;
        self.done(length)?;
        buf.copy_from_slice(&self.link.data()[..BLOCK_SIZE]);
        Ok(())
    }

    fn write(&mut self, writes: &[(u64, &Block)]) -> Result<(), DevError> {
        let room = self.link.data().len() / BLOCK_SIZE;
        for batch in writes.chunks(room.clamp(1, proto::MAX_BATCH)) {
            let mut ranges = core::mem::take(&mut self.ranges);
            ranges.clear();
            for (i, (block, data)) in batch.iter().enumerate() {
                let offset = i * BLOCK_SIZE;
                self.link.data()[offset..offset + BLOCK_SIZE].copy_from_slice(&data[..]);
                ranges.push(Range {
                    sector: block * u64::from(SECTORS_PER_BLOCK),
                    offset: offset as u64,
                    count: SECTORS_PER_BLOCK,
                });
            }
            let result = self.send(&Write {
                ranges: Seq::Of(&ranges),
            });
            self.ranges = ranges;
            let length = result?;
            self.done(length)?;
        }
        Ok(())
    }

    fn flush(&mut self) -> Result<(), DevError> {
        let length = self.send(&Flush {})?;
        self.done(length)
    }
}
