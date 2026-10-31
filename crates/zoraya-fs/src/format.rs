//! On-disk format, version 1 (ADR-0011, ADR-0012, ADR-0014).
//!
//! Every structure is encoded field by field, little-endian, at offsets
//! fixed here; nothing depends on Rust's struct layout. Every decoder takes
//! the bytes as untrusted (ADR-0012 rule 4): it returns `Err(Corrupt)` for
//! any input it does not accept, never panics, never indexes outside its
//! input, and loops at most once per byte of input.
//!
//! # Layout of a volume
//!
//! | Blocks | Contents |
//! |---|---|
//! | 0–3 | root record copy 0 of pairs 0–3 |
//! | 4–7 | root record copy 1 of pairs 0–3 |
//! | 8– | tree nodes, data blocks, allocator blocks, anywhere |
//!
//! Generation `g` is written to both copies of pair `g mod N_PAIRS`
//! (ADR-0011 rule 3d). There is no other fixed location: the root record
//! holds the volume's geometry too, so that the only in-place writes are
//! root records.
//!
//! # Blocks
//!
//! - **Root record**: one block; CRC32C of bytes 0..4088 in the last eight
//!   bytes; all unused bytes zero.
//! - **Metadata block** (tree node, allocator root, bitmap): a 32-byte header
//!   holding magic, version, kind, level, volume id, its own address and the
//!   generation of the commit that wrote it (ADR-0012 rule 1). Its checksum
//!   is in the pointer to it.
//! - **Data block**: raw file or link-target bytes; its checksum and
//!   generation are in the extent item that points to it.
//!
//! # The tree
//!
//! One copy-on-write B+tree (ADR-0011 rule 8). Keys are
//! `(id, kind, off, name)` compared in that order, `name` byte by byte:
//!
//! | Kind | Key | Value |
//! |---|---|---|
//! | `INODE` | `(object, 1, 0, "")` | attribute record ([`Attr`]) |
//! | `PARENT` | `(directory, 2, 0, "")` | the parent directory's id (internal, never exposed as a name: ADR-0013 rule 6) |
//! | `DIRENT` | `(directory, 3, 0, name)` | child id and kind |
//! | `EXTENT` | `(object, 4, block index, "")` | pointer to one data block |
//! | `ORPHAN` | `(0, 5, object, "")` | empty: the object has no name (ADR-0011 rule 7) |

use alloc::rc::Rc;
use alloc::vec::Vec;

use crate::crc32c;
use crate::device::{BLOCK_SIZE, Block};
use crate::name::{MAX_NAME, validate_name};
use crate::time::Timestamp;

/// Format version written by this code and the only one it reads.
pub const FORMAT_VERSION: u16 = 1;
/// Checksum algorithm identifier for CRC32C (ADR-0012 rule 2).
pub const CSUM_CRC32C: u16 = 1;
/// Root record pairs in the ring (ADR-0011 rule 3, an assumption there).
pub const N_PAIRS: u64 = 4;
/// Blocks holding root records.
pub const ROOT_SLOTS: u64 = 2 * N_PAIRS;
/// Most levels in the tree, leaves included (ADR-0012 rule 4).
pub const MAX_DEPTH: u8 = 8;
/// Longest link target in bytes (ADR-0013 rule 7).
pub const MAX_LINK_TARGET: usize = 4096;
/// Largest file size (ADR-0014 rule 1).
pub const MAX_FILE_SIZE: u64 = i64::MAX as u64;
/// Largest block index an extent may have.
pub const MAX_FILE_BLOCK: u64 = MAX_FILE_SIZE / BLOCK_SIZE as u64;
/// Id of the root directory.
pub const ROOT_ID: u64 = 1;
/// First id handed out for a new object.
pub const FIRST_FREE_ID: u64 = 2;

/// Magic of a root record.
pub const ROOT_MAGIC: [u8; 8] = *b"ZORAYAFS";
/// Magic of a metadata block header.
pub const NODE_MAGIC: u32 = 0x4E53_465A; // "ZFSN" little-endian
/// Size of a metadata block header.
pub const HEADER_LEN: usize = 32;
/// Offset of a root record's checksum.
pub const ROOT_CSUM_OFF: usize = BLOCK_SIZE - 8;
/// Bytes of root record that are meaningful; the rest up to the checksum
/// must be zero.
pub const ROOT_USED: usize = 120;

pub const KIND_LEAF: u8 = 1;
pub const KIND_INTERIOR: u8 = 2;
pub const KIND_ALLOC_ROOT: u8 = 3;
pub const KIND_BITMAP: u8 = 4;

/// Bytes available for items in a tree node.
pub const NODE_CAPACITY: usize = BLOCK_SIZE - HEADER_LEN - 4;
/// Bitmap bits per bitmap block.
pub const BITS_PER_BITMAP: u64 = ((BLOCK_SIZE - HEADER_LEN - 8) * 8) as u64;
/// Bitmap blocks an allocator root can point to.
pub const MAX_BITMAPS: u64 = ((BLOCK_SIZE - HEADER_LEN - 8) / PTR_LEN) as u64;
/// Largest volume in blocks: about 20.9 GiB (proposed ADR-0016, item 4).
pub const MAX_BLOCKS: u64 = MAX_BITMAPS * BITS_PER_BITMAP;

/// Encoded size of a [`BlockPtr`].
pub const PTR_LEN: usize = 24;

/// A decoding failure, with what was wrong, for `fsck` reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Corrupt(pub &'static str);

type R<T> = Result<T, Corrupt>;

// ---------------------------------------------------------------------------
// Byte reader and writer.

/// Bounds-checked little-endian reader over untrusted bytes.
pub struct Rd<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Rd<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Rd { b, pos: 0 }
    }
    pub fn at(b: &'a [u8], pos: usize) -> Self {
        Rd { b, pos }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn bytes(&mut self, n: usize) -> R<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Corrupt("length overflow"))?;
        let s = self.b.get(self.pos..end).ok_or(Corrupt("truncated"))?;
        self.pos = end;
        Ok(s)
    }
    fn arr<const N: usize>(&mut self) -> R<[u8; N]> {
        let s = self.bytes(N)?;
        let mut a = [0u8; N];
        a.copy_from_slice(s);
        Ok(a)
    }
    pub fn u8(&mut self) -> R<u8> {
        Ok(self.arr::<1>()?[0])
    }
    pub fn u16(&mut self) -> R<u16> {
        Ok(u16::from_le_bytes(self.arr()?))
    }
    pub fn u32(&mut self) -> R<u32> {
        Ok(u32::from_le_bytes(self.arr()?))
    }
    pub fn u64(&mut self) -> R<u64> {
        Ok(u64::from_le_bytes(self.arr()?))
    }
    pub fn i64(&mut self) -> R<i64> {
        Ok(i64::from_le_bytes(self.arr()?))
    }
    pub fn rest(&self) -> &'a [u8] {
        self.b.get(self.pos..).unwrap_or(&[])
    }
}

/// An encoder ran past the end of its block: a bug in the engine, reported
/// as an error rather than a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow;

/// Bounds-checked little-endian writer.
pub struct Wr<'a> {
    b: &'a mut [u8],
    pos: usize,
}

impl<'a> Wr<'a> {
    pub fn new(b: &'a mut [u8]) -> Self {
        Wr { b, pos: 0 }
    }
    pub fn at(b: &'a mut [u8], pos: usize) -> Self {
        Wr { b, pos }
    }
    pub fn pos(&self) -> usize {
        self.pos
    }
    pub fn bytes(&mut self, s: &[u8]) -> Result<(), Overflow> {
        let end = self.pos.checked_add(s.len()).ok_or(Overflow)?;
        self.b
            .get_mut(self.pos..end)
            .ok_or(Overflow)?
            .copy_from_slice(s);
        self.pos = end;
        Ok(())
    }
    pub fn u8(&mut self, v: u8) -> Result<(), Overflow> {
        self.bytes(&[v])
    }
    pub fn u16(&mut self, v: u16) -> Result<(), Overflow> {
        self.bytes(&v.to_le_bytes())
    }
    pub fn u32(&mut self, v: u32) -> Result<(), Overflow> {
        self.bytes(&v.to_le_bytes())
    }
    pub fn u64(&mut self, v: u64) -> Result<(), Overflow> {
        self.bytes(&v.to_le_bytes())
    }
    pub fn i64(&mut self, v: i64) -> Result<(), Overflow> {
        self.bytes(&v.to_le_bytes())
    }
}

// ---------------------------------------------------------------------------
// Block pointers and headers.

/// A pointer to a block: its address, its checksum (ADR-0012 rule 1: CRC32C
/// in the low half of a 64-bit field) and the generation of the commit that
/// wrote it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockPtr {
    pub addr: u64,
    pub csum: u64,
    pub generation: u64,
}

impl BlockPtr {
    pub fn encode(&self, w: &mut Wr<'_>) -> Result<(), Overflow> {
        w.u64(self.addr)?;
        w.u64(self.csum)?;
        w.u64(self.generation)
    }

    /// Decodes and checks a pointer found in a block of generation
    /// `parent_gen`: the address is inside the volume and not a root slot, the
    /// generation is between 1 and `parent_gen` (ADR-0012 rule 4), and the
    /// checksum's high half is zero (algorithm 1).
    pub fn decode(r: &mut Rd<'_>, block_count: u64, parent_gen: u64) -> R<BlockPtr> {
        let p = BlockPtr {
            addr: r.u64()?,
            csum: r.u64()?,
            generation: r.u64()?,
        };
        p.check(block_count, parent_gen)?;
        Ok(p)
    }

    pub fn check(&self, block_count: u64, parent_gen: u64) -> R<()> {
        if self.addr < ROOT_SLOTS || self.addr >= block_count {
            return Err(Corrupt("block address outside the volume"));
        }
        if self.generation == 0 || self.generation > parent_gen {
            return Err(Corrupt("child generation newer than its parent"));
        }
        if self.csum >> 32 != 0 {
            return Err(Corrupt("checksum high half not zero"));
        }
        Ok(())
    }

    /// Whether `block` has the checksum this pointer records.
    pub fn verifies(&self, block: &Block) -> bool {
        crc32c::checksum64(block) == self.csum
    }
}

/// The header of a metadata block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub kind: u8,
    pub level: u8,
    pub volume_id: u64,
    pub addr: u64,
    pub generation: u64,
}

impl Header {
    pub fn encode(&self, w: &mut Wr<'_>) -> Result<(), Overflow> {
        w.u32(NODE_MAGIC)?;
        w.u16(FORMAT_VERSION)?;
        w.u8(self.kind)?;
        w.u8(self.level)?;
        w.u64(self.volume_id)?;
        w.u64(self.addr)?;
        w.u64(self.generation)
    }

    pub fn decode(r: &mut Rd<'_>) -> R<Header> {
        if r.u32()? != NODE_MAGIC {
            return Err(Corrupt("bad metadata magic"));
        }
        if r.u16()? != FORMAT_VERSION {
            return Err(Corrupt("unknown format version"));
        }
        Ok(Header {
            kind: r.u8()?,
            level: r.u8()?,
            volume_id: r.u64()?,
            addr: r.u64()?,
            generation: r.u64()?,
        })
    }
}

/// What a metadata block must be, from the pointer that leads to it.
#[derive(Debug, Clone, Copy)]
pub struct Expect {
    pub volume_id: u64,
    pub block_count: u64,
    pub kind: u8,
    pub level: u8,
}

/// Checks a metadata block's checksum against `ptr` and its header against
/// `ptr` and `exp` (ADR-0012 rules 1 and 4). Returns a reader positioned
/// after the header.
pub fn check_meta<'a>(block: &'a Block, ptr: &BlockPtr, exp: &Expect) -> R<Rd<'a>> {
    if !ptr.verifies(block) {
        return Err(Corrupt("checksum mismatch"));
    }
    let mut r = Rd::new(block);
    let h = Header::decode(&mut r)?;
    if h.kind != exp.kind {
        return Err(Corrupt("unexpected block kind"));
    }
    if h.level != exp.level {
        return Err(Corrupt("unexpected tree level"));
    }
    if h.volume_id != exp.volume_id {
        return Err(Corrupt("block of another volume"));
    }
    if h.addr != ptr.addr {
        return Err(Corrupt("block written at another address"));
    }
    if h.generation != ptr.generation {
        return Err(Corrupt("block of another generation"));
    }
    Ok(r)
}

// ---------------------------------------------------------------------------
// Root records.

/// A root record (ADR-0011 rule 3d).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RootRecord {
    pub volume_id: u64,
    pub block_count: u64,
    pub generation: u64,
    pub tree_root: BlockPtr,
    /// Levels in the tree, leaves included: 1 to [`MAX_DEPTH`].
    pub tree_height: u8,
    pub alloc_root: BlockPtr,
    /// The highest time assigned so far (ADR-0014 rule 3).
    pub last_assigned: Timestamp,
    /// Every object id below this may have been handed out (proposed
    /// ADR-0016, item 1).
    pub id_limit: u64,
}

impl RootRecord {
    pub fn encode(&self) -> Result<Block, Overflow> {
        let mut b = [0u8; BLOCK_SIZE];
        let mut w = Wr::new(&mut b);
        w.bytes(&ROOT_MAGIC)?;
        w.u16(FORMAT_VERSION)?;
        w.u16(CSUM_CRC32C)?;
        w.u32(0)?;
        w.u64(self.volume_id)?;
        w.u64(self.block_count)?;
        w.u64(self.generation)?;
        self.tree_root.encode(&mut w)?;
        w.u8(self.tree_height)?;
        w.bytes(&[0; 7])?;
        self.alloc_root.encode(&mut w)?;
        w.i64(self.last_assigned.secs)?;
        w.u32(self.last_assigned.nanos)?;
        w.u32(0)?;
        w.u64(self.id_limit)?;
        debug_assert_eq!(w.pos(), ROOT_USED);
        let csum = crc32c::checksum64(&b[..ROOT_CSUM_OFF]);
        Wr::at(&mut b, ROOT_CSUM_OFF).u64(csum)?;
        Ok(b)
    }

    /// Decodes and validates a root slot's content. `device_blocks` is the
    /// device's size in blocks; `slot_pair` is the pair the slot belongs to.
    pub fn decode(b: &Block, device_blocks: u64, slot_pair: u64) -> R<RootRecord> {
        let mut r = Rd::at(b, ROOT_CSUM_OFF);
        let stored = r.u64()?;
        let body = b.get(..ROOT_CSUM_OFF).ok_or(Corrupt("truncated"))?;
        if crc32c::checksum64(body) != stored {
            return Err(Corrupt("root record checksum mismatch"));
        }
        let mut r = Rd::new(b);
        if r.bytes(8)? != ROOT_MAGIC {
            return Err(Corrupt("bad root record magic"));
        }
        if r.u16()? != FORMAT_VERSION {
            return Err(Corrupt("unknown format version"));
        }
        if r.u16()? != CSUM_CRC32C {
            return Err(Corrupt("unknown checksum algorithm"));
        }
        if r.u32()? != 0 {
            return Err(Corrupt("reserved field not zero"));
        }
        let volume_id = r.u64()?;
        let block_count = r.u64()?;
        let generation = r.u64()?;
        let tree_root = BlockPtr {
            addr: r.u64()?,
            csum: r.u64()?,
            generation: r.u64()?,
        };
        let tree_height = r.u8()?;
        if r.bytes(7)?.iter().any(|&x| x != 0) {
            return Err(Corrupt("reserved field not zero"));
        }
        let alloc_root = BlockPtr {
            addr: r.u64()?,
            csum: r.u64()?,
            generation: r.u64()?,
        };
        let secs = r.i64()?;
        let nanos = r.u32()?;
        if r.u32()? != 0 {
            return Err(Corrupt("reserved field not zero"));
        }
        let id_limit = r.u64()?;
        if r.rest()
            .get(..ROOT_CSUM_OFF - ROOT_USED)
            .is_none_or(|s| s.iter().any(|&x| x != 0))
        {
            return Err(Corrupt("root record padding not zero"));
        }
        let last_assigned =
            Timestamp::new(secs, nanos).ok_or(Corrupt("nanoseconds out of range"))?;
        if block_count <= ROOT_SLOTS + 2 || block_count > device_blocks || block_count > MAX_BLOCKS
        {
            return Err(Corrupt("volume size out of range"));
        }
        if generation == 0 || generation % N_PAIRS != slot_pair {
            return Err(Corrupt("generation does not belong to this slot"));
        }
        if tree_height == 0 || tree_height > MAX_DEPTH {
            return Err(Corrupt("tree height out of range"));
        }
        tree_root.check(block_count, generation)?;
        alloc_root.check(block_count, generation)?;
        if id_limit < FIRST_FREE_ID {
            return Err(Corrupt("id limit below the first id"));
        }
        Ok(RootRecord {
            volume_id,
            block_count,
            generation,
            tree_root,
            tree_height,
            alloc_root,
            last_assigned,
            id_limit,
        })
    }

    /// Block numbers of the two copies of pair `pair`.
    pub fn slots(pair: u64) -> [u64; 2] {
        [pair, N_PAIRS + pair]
    }
}

// ---------------------------------------------------------------------------
// Attribute records (ADR-0014 rule 1).

/// Object kinds (ADR-0014 rule 1, ADR-0013 rule 7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjKind {
    File,
    Directory,
    Link,
}

impl ObjKind {
    pub fn code(self) -> u8 {
        match self {
            ObjKind::File => 1,
            ObjKind::Directory => 2,
            ObjKind::Link => 3,
        }
    }
    pub fn from_code(c: u8) -> R<ObjKind> {
        match c {
            1 => Ok(ObjKind::File),
            2 => Ok(ObjKind::Directory),
            3 => Ok(ObjKind::Link),
            _ => Err(Corrupt("unknown object kind")),
        }
    }
}

/// The attribute record: exactly the fields of ADR-0014 rule 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attr {
    pub id: u64,
    pub kind: ObjKind,
    pub size: u64,
    pub mtime: Timestamp,
    pub ctime: Timestamp,
    pub posix_mode: u16,
}

/// Highest valid `posix_mode`: permission, set-id and sticky bits.
pub const MODE_MASK: u16 = 0o7777;

/// The attribute record's fields as `(name, offset, length)`. The encoder and
/// decoder are driven by this table, and INV-0014-5's test compares it with
/// ADR-0014 rule 1.
pub const ATTR_FIELDS: &[(&str, usize, usize)] = &[
    ("id", 0, 8),
    ("kind", 8, 1),
    ("size", 9, 8),
    ("mtime", 17, 12),
    ("ctime", 29, 12),
    ("posix_mode", 41, 2),
];
/// Encoded size of an attribute record.
pub const ATTR_LEN: usize = 43;

fn put_time(w: &mut Wr<'_>, t: Timestamp) -> Result<(), Overflow> {
    w.i64(t.secs)?;
    w.u32(t.nanos)
}

fn get_time(r: &mut Rd<'_>) -> R<Timestamp> {
    let secs = r.i64()?;
    let nanos = r.u32()?;
    Timestamp::new(secs, nanos).ok_or(Corrupt("nanoseconds out of range"))
}

impl Attr {
    pub fn encode(&self, out: &mut [u8]) -> Result<(), Overflow> {
        let rec = out.get_mut(..ATTR_LEN).ok_or(Overflow)?;
        for &(name, off, len) in ATTR_FIELDS {
            let mut w = Wr::at(rec, off);
            match name {
                "id" => w.u64(self.id)?,
                "kind" => w.u8(self.kind.code())?,
                "size" => w.u64(self.size)?,
                "mtime" => put_time(&mut w, self.mtime)?,
                "ctime" => put_time(&mut w, self.ctime)?,
                "posix_mode" => w.u16(self.posix_mode)?,
                _ => return Err(Overflow),
            }
            if w.pos() != off + len {
                return Err(Overflow);
            }
        }
        Ok(())
    }

    pub fn decode(b: &[u8]) -> R<Attr> {
        if b.len() != ATTR_LEN {
            return Err(Corrupt("attribute record length"));
        }
        let mut a = Attr {
            id: 0,
            kind: ObjKind::File,
            size: 0,
            mtime: Timestamp::default(),
            ctime: Timestamp::default(),
            posix_mode: 0,
        };
        for &(name, off, len) in ATTR_FIELDS {
            let mut r = Rd::at(b, off);
            match name {
                "id" => a.id = r.u64()?,
                "kind" => a.kind = ObjKind::from_code(r.u8()?)?,
                "size" => a.size = r.u64()?,
                "mtime" => a.mtime = get_time(&mut r)?,
                "ctime" => a.ctime = get_time(&mut r)?,
                "posix_mode" => a.posix_mode = r.u16()?,
                _ => return Err(Corrupt("unknown attribute field")),
            }
            if r.pos() != off + len {
                return Err(Corrupt("attribute field length"));
            }
        }
        if a.size > MAX_FILE_SIZE {
            return Err(Corrupt("size out of range"));
        }
        if a.posix_mode & !MODE_MASK != 0 {
            return Err(Corrupt("mode out of range"));
        }
        if a.kind == ObjKind::Link && a.size as usize > MAX_LINK_TARGET {
            return Err(Corrupt("link target too long"));
        }
        Ok(a)
    }
}

// ---------------------------------------------------------------------------
// Keys and values.

pub const K_INODE: u8 = 1;
pub const K_PARENT: u8 = 2;
pub const K_DIRENT: u8 = 3;
pub const K_EXTENT: u8 = 4;
pub const K_ORPHAN: u8 = 5;

/// Encoded size of a key without its name.
pub const KEY_FIXED_LEN: usize = 18;

/// A tree key. Ordered by `(id, kind, off, name)`, `name` byte by byte.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Key {
    pub id: u64,
    pub kind: u8,
    pub off: u64,
    pub name: Vec<u8>,
}

impl Key {
    /// The smallest key; the first separator of a leftmost interior node.
    pub const MIN: Key = Key {
        id: 0,
        kind: 0,
        off: 0,
        name: Vec::new(),
    };

    pub fn inode(id: u64) -> Key {
        Key {
            id,
            kind: K_INODE,
            off: 0,
            name: Vec::new(),
        }
    }
    pub fn parent(id: u64) -> Key {
        Key {
            id,
            kind: K_PARENT,
            off: 0,
            name: Vec::new(),
        }
    }
    pub fn dirent(dir: u64, name: &[u8]) -> Key {
        Key {
            id: dir,
            kind: K_DIRENT,
            off: 0,
            name: name.to_vec(),
        }
    }
    pub fn extent(id: u64, index: u64) -> Key {
        Key {
            id,
            kind: K_EXTENT,
            off: index,
            name: Vec::new(),
        }
    }
    pub fn orphan(id: u64) -> Key {
        Key {
            id: 0,
            kind: K_ORPHAN,
            off: id,
            name: Vec::new(),
        }
    }

    pub fn encoded_len(&self) -> usize {
        KEY_FIXED_LEN + self.name.len()
    }

    pub fn encode(&self, w: &mut Wr<'_>) -> Result<(), Overflow> {
        w.u64(self.id)?;
        w.u8(self.kind)?;
        w.u64(self.off)?;
        w.u8(u8::try_from(self.name.len()).map_err(|_| Overflow)?)?;
        w.bytes(&self.name)
    }

    /// Decodes a key and checks it is well formed for its kind.
    pub fn decode(r: &mut Rd<'_>) -> R<Key> {
        let id = r.u64()?;
        let kind = r.u8()?;
        let off = r.u64()?;
        let n = usize::from(r.u8()?);
        let name = r.bytes(n)?;
        let k = Key {
            id,
            kind,
            off,
            name: name.to_vec(),
        };
        k.validate()?;
        Ok(k)
    }

    fn validate(&self) -> R<()> {
        let plain = self.name.is_empty();
        let ok = match self.kind {
            0 => self.id == 0 && self.off == 0 && plain, // Key::MIN, separators only
            K_INODE | K_PARENT => self.id >= ROOT_ID && self.off == 0 && plain,
            K_DIRENT => {
                if self.name.len() > MAX_NAME || validate_name(&self.name).is_err() {
                    return Err(Corrupt("invalid name in directory entry"));
                }
                self.id >= ROOT_ID && self.off == 0
            }
            K_EXTENT => self.id >= ROOT_ID && self.off <= MAX_FILE_BLOCK && plain,
            K_ORPHAN => self.id == 0 && self.off >= FIRST_FREE_ID && plain,
            _ => false,
        };
        if ok {
            Ok(())
        } else {
            Err(Corrupt("malformed key"))
        }
    }
}

/// A data block reference held in an extent item: on disk, or a new version
/// in memory not yet written by a commit.
#[derive(Debug, Clone)]
pub enum DataRef {
    Clean(BlockPtr),
    Dirty(Rc<Block>),
}

/// A tree item's value.
#[derive(Debug, Clone)]
pub enum Value {
    Inode(Attr),
    Parent(u64),
    Dirent { child: u64, kind: ObjKind },
    Extent(DataRef),
    Orphan,
}

impl Value {
    pub fn encoded_len(&self) -> usize {
        match self {
            Value::Inode(_) => ATTR_LEN,
            Value::Parent(_) => 8,
            Value::Dirent { .. } => 9,
            Value::Extent(_) => PTR_LEN,
            Value::Orphan => 0,
        }
    }

    fn kind_code(&self) -> u8 {
        match self {
            Value::Inode(_) => K_INODE,
            Value::Parent(_) => K_PARENT,
            Value::Dirent { .. } => K_DIRENT,
            Value::Extent(_) => K_EXTENT,
            Value::Orphan => K_ORPHAN,
        }
    }

    /// Encodes a value; an extent must have been written already.
    fn encode(&self, w: &mut Wr<'_>) -> Result<(), Overflow> {
        match self {
            Value::Inode(a) => {
                let mut rec = [0u8; ATTR_LEN];
                a.encode(&mut rec)?;
                w.bytes(&rec)
            }
            Value::Parent(p) => w.u64(*p),
            Value::Dirent { child, kind } => {
                w.u64(*child)?;
                w.u8(kind.code())
            }
            Value::Extent(DataRef::Clean(p)) => p.encode(w),
            Value::Extent(DataRef::Dirty(_)) => Err(Overflow),
            Value::Orphan => Ok(()),
        }
    }

    fn decode(key: &Key, b: &[u8], block_count: u64, node_gen: u64) -> R<Value> {
        let mut r = Rd::new(b);
        let v = match key.kind {
            K_INODE => {
                let a = Attr::decode(b)?;
                if a.id != key.id {
                    return Err(Corrupt("attribute record under another id"));
                }
                return Ok(Value::Inode(a));
            }
            K_PARENT => Value::Parent(r.u64()?),
            K_DIRENT => {
                let child = r.u64()?;
                let kind = ObjKind::from_code(r.u8()?)?;
                if child < FIRST_FREE_ID {
                    return Err(Corrupt("directory entry for the root or id 0"));
                }
                Value::Dirent { child, kind }
            }
            K_EXTENT => Value::Extent(DataRef::Clean(BlockPtr::decode(
                &mut r,
                block_count,
                node_gen,
            )?)),
            K_ORPHAN => Value::Orphan,
            _ => return Err(Corrupt("malformed key")),
        };
        if r.pos() != b.len() {
            return Err(Corrupt("value length"));
        }
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// Tree nodes.

/// A pointer from an interior node: on disk, or a new node in memory.
#[derive(Debug, Clone)]
pub enum Child {
    Clean(BlockPtr),
    Dirty(Rc<Node>),
}

/// A decoded tree node.
#[derive(Debug, Clone)]
pub struct Node {
    /// 0 for a leaf.
    pub level: u8,
    pub body: Body,
}

#[derive(Debug, Clone)]
pub enum Body {
    Leaf(Vec<(Key, Value)>),
    /// Entry `i`'s key is the smallest key that can be in child `i`, for
    /// `i > 0`; entry 0's key is not used for routing.
    Interior(Vec<(Key, Child)>),
}

/// Size of a leaf item when encoded.
pub fn item_len(k: &Key, v: &Value) -> usize {
    k.encoded_len() + 2 + v.encoded_len()
}

/// Size of an interior entry when encoded.
pub fn entry_len(k: &Key) -> usize {
    k.encoded_len() + PTR_LEN
}

impl Node {
    /// Bytes the node's entries take when encoded (at most
    /// [`NODE_CAPACITY`] for a node that can be written).
    pub fn used(&self) -> usize {
        match &self.body {
            Body::Leaf(items) => items.iter().map(|(k, v)| item_len(k, v)).sum(),
            Body::Interior(es) => es.iter().map(|(k, _)| entry_len(k)).sum(),
        }
    }

    pub fn len(&self) -> usize {
        match &self.body {
            Body::Leaf(items) => items.len(),
            Body::Interior(es) => es.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Encodes the node into a block. Every child and extent must be
    /// [`Child::Clean`] / [`DataRef::Clean`].
    pub fn encode(&self, volume_id: u64, addr: u64, generation: u64) -> Result<Block, Overflow> {
        let mut b = [0u8; BLOCK_SIZE];
        let mut w = Wr::new(&mut b);
        let kind = match self.body {
            Body::Leaf(_) => KIND_LEAF,
            Body::Interior(_) => KIND_INTERIOR,
        };
        Header {
            kind,
            level: self.level,
            volume_id,
            addr,
            generation,
        }
        .encode(&mut w)?;
        w.u16(u16::try_from(self.len()).map_err(|_| Overflow)?)?;
        w.u16(0)?;
        match &self.body {
            Body::Leaf(items) => {
                for (k, v) in items {
                    if k.kind != v.kind_code() {
                        return Err(Overflow);
                    }
                    k.encode(&mut w)?;
                    w.u16(u16::try_from(v.encoded_len()).map_err(|_| Overflow)?)?;
                    v.encode(&mut w)?;
                }
            }
            Body::Interior(es) => {
                for (k, c) in es {
                    k.encode(&mut w)?;
                    match c {
                        Child::Clean(p) => p.encode(&mut w)?,
                        Child::Dirty(_) => return Err(Overflow),
                    }
                }
            }
        }
        Ok(b)
    }

    /// Decodes and validates a node reached through `ptr` (ADR-0012 rule 4):
    /// checksum, header, level, counts within the block, keys well formed,
    /// strictly increasing and within `[lower, upper)`, values well formed,
    /// child pointers inside the volume and not newer than this node, and
    /// unused space zero.
    pub fn decode(
        block: &Block,
        ptr: &BlockPtr,
        volume_id: u64,
        block_count: u64,
        level: u8,
        lower: Option<&Key>,
        upper: Option<&Key>,
    ) -> R<Node> {
        let kind = if level == 0 { KIND_LEAF } else { KIND_INTERIOR };
        let exp = Expect {
            volume_id,
            block_count,
            kind,
            level,
        };
        let mut r = check_meta(block, ptr, &exp)?;
        let count = usize::from(r.u16()?);
        if r.u16()? != 0 {
            return Err(Corrupt("reserved field not zero"));
        }
        // Each entry takes at least KEY_FIXED_LEN bytes, which bounds count.
        if count > NODE_CAPACITY / KEY_FIXED_LEN {
            return Err(Corrupt("entry count too large"));
        }
        let in_range = |k: &Key| lower.is_none_or(|l| k >= l) && upper.is_none_or(|u| k < u);
        let body = if level == 0 {
            let mut items: Vec<(Key, Value)> = Vec::with_capacity(count);
            for _ in 0..count {
                let k = Key::decode(&mut r)?;
                if k.kind == 0 {
                    return Err(Corrupt("separator key in a leaf"));
                }
                if items.last().is_some_and(|(p, _)| *p >= k) || !in_range(&k) {
                    return Err(Corrupt("keys out of order"));
                }
                let vlen = usize::from(r.u16()?);
                let vb = r.bytes(vlen)?;
                let v = Value::decode(&k, vb, block_count, ptr.generation)?;
                items.push((k, v));
            }
            Body::Leaf(items)
        } else {
            if count == 0 {
                return Err(Corrupt("empty interior node"));
            }
            let mut es: Vec<(Key, Child)> = Vec::with_capacity(count);
            for i in 0..count {
                let k = Key::decode(&mut r)?;
                if es.last().is_some_and(|(p, _)| *p >= k) || (i > 0 && !in_range(&k)) {
                    return Err(Corrupt("keys out of order"));
                }
                let p = BlockPtr::decode(&mut r, block_count, ptr.generation)?;
                es.push((k, Child::Clean(p)));
            }
            Body::Interior(es)
        };
        if r.rest().iter().any(|&x| x != 0) {
            return Err(Corrupt("node padding not zero"));
        }
        Ok(Node { level, body })
    }
}

// ---------------------------------------------------------------------------
// Allocator blocks.

/// Number of bitmap blocks a volume of `block_count` blocks has.
pub fn bitmap_count(block_count: u64) -> u64 {
    block_count.div_ceil(BITS_PER_BITMAP)
}

/// Encodes the allocator root: pointers to every bitmap block in order.
pub fn encode_alloc_root(
    volume_id: u64,
    addr: u64,
    generation: u64,
    bitmaps: &[BlockPtr],
) -> Result<Block, Overflow> {
    let mut b = [0u8; BLOCK_SIZE];
    let mut w = Wr::new(&mut b);
    Header {
        kind: KIND_ALLOC_ROOT,
        level: 0,
        volume_id,
        addr,
        generation,
    }
    .encode(&mut w)?;
    w.u32(u32::try_from(bitmaps.len()).map_err(|_| Overflow)?)?;
    w.u32(0)?;
    for p in bitmaps {
        p.encode(&mut w)?;
    }
    Ok(b)
}

/// Decodes the allocator root reached through `ptr`.
pub fn decode_alloc_root(
    block: &Block,
    ptr: &BlockPtr,
    volume_id: u64,
    block_count: u64,
) -> R<Vec<BlockPtr>> {
    let exp = Expect {
        volume_id,
        block_count,
        kind: KIND_ALLOC_ROOT,
        level: 0,
    };
    let mut r = check_meta(block, ptr, &exp)?;
    let n = u64::from(r.u32()?);
    if r.u32()? != 0 {
        return Err(Corrupt("reserved field not zero"));
    }
    if n != bitmap_count(block_count) || n > MAX_BITMAPS {
        return Err(Corrupt("bitmap count does not match the volume size"));
    }
    let mut v = Vec::with_capacity(n as usize);
    for _ in 0..n {
        v.push(BlockPtr::decode(&mut r, block_count, ptr.generation)?);
    }
    if r.rest().iter().any(|&x| x != 0) {
        return Err(Corrupt("allocator root padding not zero"));
    }
    Ok(v)
}

/// Offset of the bits in a bitmap block.
pub const BITMAP_BITS_OFF: usize = HEADER_LEN + 8;

/// Encodes bitmap block `index`; `bits` is its bit array (bit set = used).
pub fn encode_bitmap(
    volume_id: u64,
    addr: u64,
    generation: u64,
    index: u32,
    bits: &[u8],
) -> Result<Block, Overflow> {
    let mut b = [0u8; BLOCK_SIZE];
    let mut w = Wr::new(&mut b);
    Header {
        kind: KIND_BITMAP,
        level: 0,
        volume_id,
        addr,
        generation,
    }
    .encode(&mut w)?;
    w.u32(index)?;
    w.u32(0)?;
    w.bytes(bits)?;
    Ok(b)
}

/// Decodes bitmap block `index` reached through `ptr`; returns its bits.
pub fn decode_bitmap<'a>(
    block: &'a Block,
    ptr: &BlockPtr,
    volume_id: u64,
    block_count: u64,
    index: u32,
) -> R<&'a [u8]> {
    let exp = Expect {
        volume_id,
        block_count,
        kind: KIND_BITMAP,
        level: 0,
    };
    let mut r = check_meta(block, ptr, &exp)?;
    if r.u32()? != index {
        return Err(Corrupt("bitmap block out of order"));
    }
    if r.u32()? != 0 {
        return Err(Corrupt("reserved field not zero"));
    }
    Ok(r.rest())
}
