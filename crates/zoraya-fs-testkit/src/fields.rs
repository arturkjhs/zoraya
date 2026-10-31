//! A map of every field of every on-disk structure of a valid image, and a
//! way to make a mutated block's checksums consistent again up to the root
//! record (an attacker recomputes checksums, ADR-0012 rule 4), for the
//! field-level fuzz test of INV-0012-3 (XFS-style: every field of every
//! structure, eight transformations).

use std::collections::BTreeMap;

use zoraya_fs::check::{BlockKind, reachable_from};
use zoraya_fs::crc32c::checksum64;
use zoraya_fs::format::{
    ATTR_FIELDS, BITMAP_BITS_OFF, HEADER_LEN, K_DIRENT, K_EXTENT, K_INODE, K_ORPHAN, K_PARENT,
    KEY_FIXED_LEN, N_PAIRS, ROOT_CSUM_OFF, RootRecord,
};
use zoraya_fs::{BLOCK_SIZE, BlockDevice, select_root};

use crate::devices::ImageDevice;

/// One field: `len` bytes at `off` in `block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    pub block: u64,
    pub off: usize,
    pub len: usize,
    pub what: &'static str,
}

/// Every field of the selected generation's structures, plus the parent of
/// each reachable block.
pub struct FieldMap {
    pub fields: Vec<Field>,
    pub rec: RootRecord,
    /// Root slots of the selected generation.
    pub slots: [u64; 2],
    pub kinds: BTreeMap<u64, (BlockKind, Option<u64>)>,
}

fn u16_at(b: &[u8], o: usize) -> usize {
    u16::from_le_bytes([b[o], b[o + 1]]) as usize
}

fn header(fields: &mut Vec<Field>, block: u64) {
    for (off, len, what) in [
        (0, 4, "magic"),
        (4, 2, "version"),
        (6, 1, "kind"),
        (7, 1, "level"),
        (8, 8, "volume id"),
        (16, 8, "own address"),
        (24, 8, "generation"),
    ] {
        fields.push(Field {
            block,
            off,
            len,
            what,
        });
    }
}

fn ptr(fields: &mut Vec<Field>, block: u64, at: usize) {
    fields.push(Field {
        block,
        off: at,
        len: 8,
        what: "pointer address",
    });
    fields.push(Field {
        block,
        off: at + 8,
        len: 8,
        what: "pointer checksum",
    });
    fields.push(Field {
        block,
        off: at + 16,
        len: 8,
        what: "pointer generation",
    });
}

/// Builds the map of a valid image.
pub fn field_map(dev: &mut ImageDevice) -> Result<FieldMap, String> {
    let sel = select_root(dev).map_err(|e| e.0.to_string())?;
    let rec = sel.record;
    let slots = RootRecord::slots(rec.generation % N_PAIRS);
    let kinds = reachable_from(dev, &rec)?;
    let mut fields = Vec::new();
    for (off, len, what) in [
        (0, 8, "root magic"),
        (8, 2, "root version"),
        (10, 2, "root checksum algorithm"),
        (12, 4, "root reserved"),
        (16, 8, "root volume id"),
        (24, 8, "root block count"),
        (32, 8, "root generation"),
        (40, 8, "root tree address"),
        (48, 8, "root tree checksum"),
        (56, 8, "root tree generation"),
        (64, 1, "root tree height"),
        (65, 7, "root reserved"),
        (72, 8, "root allocator address"),
        (80, 8, "root allocator checksum"),
        (88, 8, "root allocator generation"),
        (96, 8, "root last assigned seconds"),
        (104, 4, "root last assigned nanoseconds"),
        (108, 4, "root reserved"),
        (112, 8, "root id limit"),
        (200, 1, "root padding"),
        (ROOT_CSUM_OFF, 8, "root checksum"),
    ] {
        fields.push(Field {
            block: slots[0],
            off,
            len,
            what,
        });
    }
    for (&b, &(kind, _)) in &kinds {
        let blk = dev.get_block(b);
        match kind {
            BlockKind::RootSlot => {}
            BlockKind::Data => fields.push(Field {
                block: b,
                off: 0,
                len: 1,
                what: "data byte",
            }),
            BlockKind::Bitmap => {
                header(&mut fields, b);
                fields.push(Field {
                    block: b,
                    off: HEADER_LEN,
                    len: 4,
                    what: "bitmap index",
                });
                fields.push(Field {
                    block: b,
                    off: HEADER_LEN + 4,
                    len: 4,
                    what: "bitmap reserved",
                });
                fields.push(Field {
                    block: b,
                    off: BITMAP_BITS_OFF,
                    len: 1,
                    what: "bitmap first byte",
                });
                fields.push(Field {
                    block: b,
                    off: BLOCK_SIZE - 1,
                    len: 1,
                    what: "bitmap last byte",
                });
            }
            BlockKind::AllocRoot => {
                header(&mut fields, b);
                fields.push(Field {
                    block: b,
                    off: HEADER_LEN,
                    len: 4,
                    what: "bitmap count",
                });
                fields.push(Field {
                    block: b,
                    off: HEADER_LEN + 4,
                    len: 4,
                    what: "allocator reserved",
                });
                let n = u32::from_le_bytes(blk[HEADER_LEN..HEADER_LEN + 4].try_into().unwrap())
                    as usize;
                for i in 0..n {
                    ptr(&mut fields, b, HEADER_LEN + 8 + i * 24);
                }
            }
            BlockKind::Node(level) => {
                header(&mut fields, b);
                fields.push(Field {
                    block: b,
                    off: HEADER_LEN,
                    len: 2,
                    what: "entry count",
                });
                fields.push(Field {
                    block: b,
                    off: HEADER_LEN + 2,
                    len: 2,
                    what: "node reserved",
                });
                let count = u16_at(&blk, HEADER_LEN);
                let mut at = HEADER_LEN + 4;
                for _ in 0..count {
                    let kind = blk[at + 8];
                    let nlen = blk[at + 17] as usize;
                    fields.push(Field {
                        block: b,
                        off: at,
                        len: 8,
                        what: "key id",
                    });
                    fields.push(Field {
                        block: b,
                        off: at + 8,
                        len: 1,
                        what: "key kind",
                    });
                    fields.push(Field {
                        block: b,
                        off: at + 9,
                        len: 8,
                        what: "key offset",
                    });
                    fields.push(Field {
                        block: b,
                        off: at + 17,
                        len: 1,
                        what: "key name length",
                    });
                    if nlen > 0 {
                        fields.push(Field {
                            block: b,
                            off: at + 18,
                            len: nlen.min(8),
                            what: "key name",
                        });
                    }
                    at += KEY_FIXED_LEN + nlen;
                    if level == 0 {
                        let vlen = u16_at(&blk, at);
                        fields.push(Field {
                            block: b,
                            off: at,
                            len: 2,
                            what: "value length",
                        });
                        at += 2;
                        match kind {
                            K_INODE => {
                                for &(_, o, l) in ATTR_FIELDS {
                                    fields.push(Field {
                                        block: b,
                                        off: at + o,
                                        len: l,
                                        what: "attribute field",
                                    });
                                }
                            }
                            K_PARENT => fields.push(Field {
                                block: b,
                                off: at,
                                len: 8,
                                what: "parent id",
                            }),
                            K_DIRENT => {
                                fields.push(Field {
                                    block: b,
                                    off: at,
                                    len: 8,
                                    what: "entry child",
                                });
                                fields.push(Field {
                                    block: b,
                                    off: at + 8,
                                    len: 1,
                                    what: "entry kind",
                                });
                            }
                            K_EXTENT => ptr(&mut fields, b, at),
                            K_ORPHAN => {}
                            _ => return Err(format!("unknown item kind {kind} in block {b}")),
                        }
                        at += vlen;
                    } else {
                        ptr(&mut fields, b, at);
                        at += 24;
                    }
                }
                if at < BLOCK_SIZE {
                    fields.push(Field {
                        block: b,
                        off: BLOCK_SIZE - 1,
                        len: 1,
                        what: "node padding",
                    });
                }
            }
        }
    }
    Ok(FieldMap {
        fields,
        rec,
        slots,
        kinds,
    })
}

/// The eight transformations (XFS's fuzzer: zeroes, ones, first/middle/last
/// bit, add, subtract, random).
pub const TRANSFORMS: usize = 8;

pub fn transform(bytes: &mut [u8], t: usize, random: u64) {
    let n = bytes.len();
    match t {
        0 => bytes.fill(0),
        1 => bytes.fill(0xFF),
        2 => bytes[n - 1] ^= 0x80,
        3 => bytes[n / 2] ^= 0x08,
        4 => bytes[0] ^= 0x01,
        5 | 6 => {
            let mut carry = true;
            for b in bytes.iter_mut() {
                if !carry {
                    break;
                }
                let (v, c) = if t == 5 {
                    b.overflowing_add(1)
                } else {
                    b.overflowing_sub(1)
                };
                *b = v;
                carry = c;
            }
        }
        _ => {
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = (random >> ((i % 8) * 8)) as u8;
            }
        }
    }
}

/// After `block` was changed, rewrites the checksum in its parent pointer,
/// and so on up to the root record (both copies), so that only structural
/// validation can catch the change.
pub fn fix_checksums(dev: &mut ImageDevice, map: &FieldMap, block: u64, old: &[u8; BLOCK_SIZE]) {
    let mut child = block;
    let mut old_child = *old;
    loop {
        let new = dev.get_block(child);
        let (old_csum, new_csum) = (checksum64(&old_child), checksum64(&new));
        if map.slots.contains(&child) {
            break;
        }
        match map.kinds.get(&child).and_then(|(_, p)| *p) {
            Some(parent) => {
                let mut pb = dev.get_block(parent);
                let before = pb;
                // The pointer to `child` is (address, checksum) somewhere in
                // the parent.
                let mut pat = [0u8; 16];
                pat[..8].copy_from_slice(&child.to_le_bytes());
                pat[8..].copy_from_slice(&old_csum.to_le_bytes());
                if let Some(pos) = pb.windows(16).position(|w| w == pat) {
                    pb[pos + 8..pos + 16].copy_from_slice(&new_csum.to_le_bytes());
                }
                dev.set_block(parent, pb);
                child = parent;
                old_child = before;
            }
            None => {
                // Pointed to by the root record.
                let mut rb = dev.get_block(map.slots[0]);
                for off in [40usize, 72] {
                    if rb[off..off + 8] == child.to_le_bytes()
                        && rb[off + 8..off + 16] == old_csum.to_le_bytes()
                    {
                        rb[off + 8..off + 16].copy_from_slice(&new_csum.to_le_bytes());
                    }
                }
                dev.set_block(map.slots[0], rb);
                break;
            }
        }
    }
    reseal_root(dev, map);
}

/// Recomputes the selected root record's checksum and copies it to both
/// slots of its pair.
pub fn reseal_root(dev: &mut ImageDevice, map: &FieldMap) {
    let mut rb = dev.get_block(map.slots[0]);
    let c = checksum64(&rb[..ROOT_CSUM_OFF]);
    rb[ROOT_CSUM_OFF..].copy_from_slice(&c.to_le_bytes());
    dev.set_block(map.slots[0], rb);
    dev.set_block(map.slots[1], rb);
}

/// The device's block count, for callers that only have a map.
pub fn blocks(dev: &ImageDevice) -> u64 {
    dev.block_count()
}
