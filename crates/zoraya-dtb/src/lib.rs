//! Flattened device tree (DTB) parser.
//!
//! Reads a device tree blob as described in the Devicetree Specification
//! v0.4, chapter 5 ("Flattened Devicetree (DTB) Format"). The parser borrows
//! the blob, never allocates and contains no `unsafe` code.
//!
//! [`Fdt::new`] validates the header and walks the whole structure block once.
//! Malformed input of any kind (truncated, wrong magic, offsets outside the
//! blob, unbalanced nodes, property names outside the strings block) is
//! rejected there with an [`Error`]. Every later accessor still bounds-checks
//! each read, so no input can make the parser panic or read outside the slice
//! it was given; on a blob that passed validation those checks cannot fail and
//! the iterators simply end.
//!
//! [`platform`] builds on this to extract what the kernel needs at boot.

#![no_std]
#![forbid(unsafe_code)]

pub mod platform;

/// Magic number at offset 0 of every blob.
pub const FDT_MAGIC: u32 = 0xd00d_feed;

/// Size of the version 17 header in bytes.
pub const HEADER_SIZE: usize = 40;

/// Deepest node nesting accepted (the root is depth 1).
pub const MAX_DEPTH: usize = 32;

const FDT_BEGIN_NODE: u32 = 0x1;
const FDT_END_NODE: u32 = 0x2;
const FDT_PROP: u32 = 0x3;
const FDT_NOP: u32 = 0x4;
const FDT_END: u32 = 0x9;

/// Why a blob was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The buffer is shorter than the header or than the header's `totalsize`.
    Truncated,
    /// The first word is not [`FDT_MAGIC`].
    BadMagic,
    /// The blob's version is older than 17, or it is not backward compatible
    /// with version 17.
    UnsupportedVersion,
    /// A block offset or size in the header is misaligned or lies outside
    /// the blob.
    BadLayout,
    /// The structure block is malformed: unknown token, unbalanced nodes,
    /// nesting deeper than [`MAX_DEPTH`], a property outside a node, data
    /// running past the block, or a missing `FDT_END`.
    BadStructure,
    /// A node name or property name is not a NUL-terminated UTF-8 string
    /// inside its block.
    BadString,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let s = match self {
            Error::Truncated => "blob is truncated",
            Error::BadMagic => "bad magic number",
            Error::UnsupportedVersion => "unsupported version",
            Error::BadLayout => "header block offsets out of bounds",
            Error::BadStructure => "malformed structure block",
            Error::BadString => "malformed name string",
        };
        f.write_str(s)
    }
}

fn be32(data: &[u8], off: usize) -> Option<u32> {
    let end = off.checked_add(4)?;
    let b = data.get(off..end)?;
    Some(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

fn be64(data: &[u8], off: usize) -> Option<u64> {
    let hi = be32(data, off)?;
    let lo = be32(data, off.checked_add(4)?)?;
    Some((u64::from(hi) << 32) | u64::from(lo))
}

fn align4(off: usize) -> Option<usize> {
    off.checked_add(3).map(|v| v & !3)
}

/// NUL-terminated UTF-8 string starting at `off`; returns it and the offset
/// just past the NUL.
fn cstr(data: &[u8], off: usize) -> Result<(&str, usize), Error> {
    let rest = data.get(off..).ok_or(Error::BadString)?;
    let len = rest.iter().position(|&b| b == 0).ok_or(Error::BadString)?;
    let s = core::str::from_utf8(&rest[..len]).map_err(|_| Error::BadString)?;
    Ok((s, off + len + 1))
}

/// The fixed header fields this parser uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub total_size: u32,
    pub off_dt_struct: u32,
    pub off_dt_strings: u32,
    pub off_mem_rsvmap: u32,
    pub version: u32,
    pub last_comp_version: u32,
    pub boot_cpuid_phys: u32,
    pub size_dt_strings: u32,
    pub size_dt_struct: u32,
}

impl Header {
    /// Reads the header from the start of `data`, checking only the length
    /// and the magic number.
    pub fn read(data: &[u8]) -> Result<Header, Error> {
        if data.len() < HEADER_SIZE {
            return Err(Error::Truncated);
        }
        let w = |i: usize| be32(data, i * 4).ok_or(Error::Truncated);
        if w(0)? != FDT_MAGIC {
            return Err(Error::BadMagic);
        }
        Ok(Header {
            total_size: w(1)?,
            off_dt_struct: w(2)?,
            off_dt_strings: w(3)?,
            off_mem_rsvmap: w(4)?,
            version: w(5)?,
            last_comp_version: w(6)?,
            boot_cpuid_phys: w(7)?,
            size_dt_strings: w(8)?,
            size_dt_struct: w(9)?,
        })
    }
}

/// A validated device tree blob.
#[derive(Clone, Copy)]
pub struct Fdt<'a> {
    header: Header,
    /// The blob, cut to `total_size`.
    data: &'a [u8],
    structs: &'a [u8],
    strings: &'a [u8],
    /// Offset of the root node's `FDT_BEGIN_NODE` token in `structs`.
    root_off: usize,
}

impl core::fmt::Debug for Fdt<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Fdt").field("header", &self.header).finish()
    }
}

/// One token of the structure block.
enum Token<'a> {
    BeginNode { name: &'a str },
    EndNode,
    Prop { name_off: usize, value: &'a [u8] },
    Nop,
    End,
}

impl<'a> Fdt<'a> {
    /// Validates `data` as a device tree blob. `data` may be longer than the
    /// blob; bytes past the header's `totalsize` are ignored.
    pub fn new(data: &'a [u8]) -> Result<Fdt<'a>, Error> {
        let header = Header::read(data)?;
        if header.version < 17 || header.last_comp_version > 17 {
            return Err(Error::UnsupportedVersion);
        }
        let total = header.total_size as usize;
        if total < HEADER_SIZE {
            return Err(Error::BadLayout);
        }
        let data = data.get(..total).ok_or(Error::Truncated)?;
        let block = |off: u32, size: u32, align: usize| -> Result<&'a [u8], Error> {
            let (off, size) = (off as usize, size as usize);
            if off < HEADER_SIZE || off % align != 0 {
                return Err(Error::BadLayout);
            }
            let end = off.checked_add(size).ok_or(Error::BadLayout)?;
            data.get(off..end).ok_or(Error::BadLayout)
        };
        let structs = block(header.off_dt_struct, header.size_dt_struct, 4)?;
        let strings = block(header.off_dt_strings, header.size_dt_strings, 1)?;
        let mut fdt = Fdt {
            header,
            data,
            structs,
            strings,
            root_off: 0,
        };
        fdt.validate_rsvmap()?;
        fdt.root_off = fdt.validate_structure()?;
        Ok(fdt)
    }

    /// The header fields.
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Size of the blob in bytes (the header's `totalsize`).
    pub fn total_size(&self) -> usize {
        self.data.len()
    }

    fn validate_rsvmap(&self) -> Result<(), Error> {
        let mut off = self.header.off_mem_rsvmap as usize;
        if off < HEADER_SIZE || off % 8 != 0 {
            return Err(Error::BadLayout);
        }
        loop {
            let addr = be64(self.data, off).ok_or(Error::BadLayout)?;
            let size = be64(self.data, off + 8).ok_or(Error::BadLayout)?;
            if addr == 0 && size == 0 {
                return Ok(());
            }
            off += 16;
        }
    }

    /// Walks the whole structure block; returns the root node's offset.
    fn validate_structure(&self) -> Result<usize, Error> {
        let mut off = 0;
        let mut depth = 0usize;
        let mut root_off = None;
        loop {
            let tok_off = off;
            let (tok, next) = self.token(off)?;
            off = next;
            match tok {
                Token::BeginNode { name } => {
                    if depth == 0 {
                        if root_off.is_some() || !name.is_empty() {
                            return Err(Error::BadStructure);
                        }
                        root_off = Some(tok_off);
                    }
                    depth += 1;
                    if depth > MAX_DEPTH {
                        return Err(Error::BadStructure);
                    }
                }
                Token::EndNode => {
                    if depth == 0 {
                        return Err(Error::BadStructure);
                    }
                    depth -= 1;
                }
                Token::Prop { name_off, .. } => {
                    if depth == 0 {
                        return Err(Error::BadStructure);
                    }
                    self.prop_name(name_off)?;
                }
                Token::Nop => {}
                Token::End => {
                    return match root_off {
                        Some(r) if depth == 0 => Ok(r),
                        _ => Err(Error::BadStructure),
                    };
                }
            }
        }
    }

    /// Decodes the token at `off`; returns it and the offset of the next one.
    /// Every successful call advances by at least 4 bytes.
    fn token(&self, off: usize) -> Result<(Token<'a>, usize), Error> {
        let s = self.structs;
        let kind = be32(s, off).ok_or(Error::BadStructure)?;
        let body = off + 4;
        match kind {
            FDT_BEGIN_NODE => {
                let (name, end) = cstr(s, body)?;
                let next = align4(end).ok_or(Error::BadStructure)?;
                Ok((Token::BeginNode { name }, next))
            }
            FDT_END_NODE => Ok((Token::EndNode, body)),
            FDT_PROP => {
                let len = be32(s, body).ok_or(Error::BadStructure)? as usize;
                let name_off = be32(s, body + 4).ok_or(Error::BadStructure)? as usize;
                let start = body + 8;
                let end = start.checked_add(len).ok_or(Error::BadStructure)?;
                let value = s.get(start..end).ok_or(Error::BadStructure)?;
                let next = align4(end).ok_or(Error::BadStructure)?;
                Ok((Token::Prop { name_off, value }, next))
            }
            FDT_NOP => Ok((Token::Nop, body)),
            FDT_END => Ok((Token::End, body)),
            _ => Err(Error::BadStructure),
        }
    }

    fn prop_name(&self, name_off: usize) -> Result<&'a str, Error> {
        cstr(self.strings, name_off).map(|(s, _)| s)
    }

    /// The root node.
    pub fn root(&self) -> Node<'a> {
        // The root's name is empty, so its properties start right after the
        // token and the 4-byte (empty, padded) name.
        Node {
            fdt: *self,
            name: "",
            props_off: self.root_off + 8,
            parent_cells: Cells::DEFAULT,
        }
    }

    /// Entries of the memory reservation block (`/memreserve/`).
    pub fn reservations(&self) -> Reservations<'a> {
        Reservations {
            data: self.data,
            off: self.header.off_mem_rsvmap as usize,
        }
    }

    /// Every node in depth-first order, starting with the root.
    pub fn nodes(&self) -> Nodes<'a> {
        let mut stack: [Option<Children<'a>>; MAX_DEPTH] = [const { None }; MAX_DEPTH];
        stack[0] = Some(self.root().children());
        Nodes {
            root: Some(self.root()),
            stack,
            depth: 1,
        }
    }

    /// Finds a node by absolute path, e.g. `/cpus/cpu@0`. A path component
    /// without a unit address also matches a node that has one (`/memory`
    /// matches `memory@40000000`) if no node matches it exactly.
    pub fn find_node(&self, path: &str) -> Option<Node<'a>> {
        let rest = path.strip_prefix('/')?;
        let mut node = self.root();
        for part in rest.split('/').filter(|p| !p.is_empty()) {
            node = node.child(part)?;
        }
        Some(node)
    }

    /// Finds the node whose `phandle` (or legacy `linux,phandle`) is `phandle`.
    pub fn node_by_phandle(&self, phandle: u32) -> Option<Node<'a>> {
        self.nodes().find(|n| {
            n.property("phandle")
                .or_else(|| n.property("linux,phandle"))
                .and_then(|p| p.as_u32())
                == Some(phandle)
        })
    }

    /// Resolves an alias from `/aliases` to the node it names.
    pub fn alias(&self, name: &str) -> Option<Node<'a>> {
        let path = self.find_node("/aliases")?.property(name)?.as_str()?;
        self.find_node(path)
    }

    /// The first node compatible with `compatible`, in depth-first order.
    pub fn find_compatible(&self, compatible: &str) -> Option<Node<'a>> {
        self.nodes().find(|n| n.is_compatible(compatible))
    }
}

/// `#address-cells` and `#size-cells` as they apply to a node's `reg`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cells {
    pub address: u32,
    pub size: u32,
}

impl Cells {
    /// Defaults from the Devicetree Specification, section 2.3.5.
    pub const DEFAULT: Cells = Cells {
        address: 2,
        size: 1,
    };
}

/// One `(address, size)` pair from a `reg` property.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reg {
    pub address: u64,
    pub size: u64,
}

/// A node of the tree.
#[derive(Clone, Copy)]
pub struct Node<'a> {
    fdt: Fdt<'a>,
    name: &'a str,
    /// Offset in the structure block of the first token after the name.
    props_off: usize,
    /// Cells declared by the parent, which govern this node's `reg`.
    parent_cells: Cells,
}

impl core::fmt::Debug for Node<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Node").field("name", &self.name).finish()
    }
}

impl<'a> Node<'a> {
    /// The full node name including any unit address (`pl011@9000000`).
    /// The root's name is empty.
    pub fn name(&self) -> &'a str {
        self.name
    }

    /// The name without the unit address (`pl011`).
    pub fn base_name(&self) -> &'a str {
        self.name.split('@').next().unwrap_or(self.name)
    }

    /// The node's properties, in blob order.
    pub fn properties(&self) -> Properties<'a> {
        Properties {
            fdt: self.fdt,
            off: self.props_off,
            done: false,
        }
    }

    /// The property called `name`, if present.
    pub fn property(&self, name: &str) -> Option<Property<'a>> {
        self.properties().find(|p| p.name == name)
    }

    /// The node's direct children, in blob order.
    pub fn children(&self) -> Children<'a> {
        Children {
            fdt: self.fdt,
            off: self.props_off,
            cells: self.own_cells(),
            done: false,
        }
    }

    /// The child called `name`. A name without a unit address also matches
    /// a child that has one, if no child matches exactly.
    pub fn child(&self, name: &str) -> Option<Node<'a>> {
        self.children().find(|c| c.name == name).or_else(|| {
            if name.contains('@') {
                None
            } else {
                self.children().find(|c| c.base_name() == name)
            }
        })
    }

    /// The cells this node declares for its children's `reg`.
    pub fn own_cells(&self) -> Cells {
        let get = |name, default| {
            self.property(name)
                .and_then(|p| p.as_u32())
                .unwrap_or(default)
        };
        Cells {
            address: get("#address-cells", Cells::DEFAULT.address),
            size: get("#size-cells", Cells::DEFAULT.size),
        }
    }

    /// True if the `compatible` list contains `compatible`.
    pub fn is_compatible(&self, compatible: &str) -> bool {
        self.property("compatible")
            .is_some_and(|p| p.strings().any(|s| s == compatible))
    }

    /// True unless `status` is present and is neither `okay` nor `ok`.
    pub fn is_enabled(&self) -> bool {
        match self.property("status") {
            None => true,
            Some(p) => matches!(p.as_str(), Some("okay") | Some("ok")),
        }
    }

    /// The `device_type` property as a string.
    pub fn device_type(&self) -> Option<&'a str> {
        self.property("device_type").and_then(|p| p.as_str())
    }

    /// The entries of `reg`, decoded with the parent's cells. `None` if the
    /// node has no `reg`, if a cell count exceeds 2 (values wider than 64
    /// bits), or if the length is not a whole number of entries.
    pub fn reg(&self) -> Option<RegIter<'a>> {
        let value = self.property("reg")?.value;
        let (ac, sc) = (self.parent_cells.address, self.parent_cells.size);
        if ac == 0 || ac > 2 || sc > 2 {
            return None;
        }
        let entry = (ac + sc) as usize * 4;
        if value.len() % entry != 0 {
            return None;
        }
        Some(RegIter {
            value,
            address_cells: ac,
            size_cells: sc,
            off: 0,
        })
    }
}

/// Iterator over a node's properties.
pub struct Properties<'a> {
    fdt: Fdt<'a>,
    off: usize,
    done: bool,
}

impl<'a> Iterator for Properties<'a> {
    type Item = Property<'a>;

    fn next(&mut self) -> Option<Property<'a>> {
        while !self.done {
            let Ok((tok, next)) = self.fdt.token(self.off) else {
                break;
            };
            self.off = next;
            match tok {
                Token::Nop => {}
                Token::Prop { name_off, value } => {
                    let Ok(name) = self.fdt.prop_name(name_off) else {
                        break;
                    };
                    return Some(Property { name, value });
                }
                // Properties precede subnodes (Devicetree Specification
                // 5.4.2), so the first other token ends the list.
                _ => break,
            }
        }
        self.done = true;
        None
    }
}

/// Iterator over a node's direct children.
pub struct Children<'a> {
    fdt: Fdt<'a>,
    off: usize,
    cells: Cells,
    done: bool,
}

impl<'a> Children<'a> {
    /// Skips the subtree whose `FDT_BEGIN_NODE` token has already been read
    /// (depth 1); returns the offset after its `FDT_END_NODE`.
    fn skip_subtree(&self, mut off: usize) -> Option<usize> {
        let mut depth = 1usize;
        loop {
            let (tok, next) = self.fdt.token(off).ok()?;
            off = next;
            match tok {
                Token::BeginNode { .. } => depth += 1,
                Token::EndNode => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(off);
                    }
                }
                Token::Prop { .. } | Token::Nop => {}
                Token::End => return None,
            }
        }
    }
}

impl<'a> Iterator for Children<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Node<'a>> {
        while !self.done {
            let Ok((tok, next)) = self.fdt.token(self.off) else {
                break;
            };
            match tok {
                Token::Prop { .. } | Token::Nop => self.off = next,
                Token::BeginNode { name } => {
                    let node = Node {
                        fdt: self.fdt,
                        name,
                        props_off: next,
                        parent_cells: self.cells,
                    };
                    match self.skip_subtree(next) {
                        Some(after) => self.off = after,
                        None => self.done = true,
                    }
                    return Some(node);
                }
                Token::EndNode | Token::End => break,
            }
        }
        self.done = true;
        None
    }
}

/// Depth-first iterator over every node.
pub struct Nodes<'a> {
    root: Option<Node<'a>>,
    stack: [Option<Children<'a>>; MAX_DEPTH],
    depth: usize,
}

impl<'a> Iterator for Nodes<'a> {
    type Item = Node<'a>;

    fn next(&mut self) -> Option<Node<'a>> {
        if let Some(root) = self.root.take() {
            return Some(root);
        }
        while self.depth > 0 {
            let top = self.stack.get_mut(self.depth - 1)?.as_mut()?;
            match top.next() {
                Some(node) => {
                    // Validation bounds the depth by MAX_DEPTH, so a slot is
                    // always free; if not, the subtree is not descended.
                    if let Some(slot) = self.stack.get_mut(self.depth) {
                        *slot = Some(node.children());
                        self.depth += 1;
                    }
                    return Some(node);
                }
                None => {
                    self.stack[self.depth - 1] = None;
                    self.depth -= 1;
                }
            }
        }
        None
    }
}

/// One property: a name and its raw big-endian value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Property<'a> {
    pub name: &'a str,
    pub value: &'a [u8],
}

impl<'a> Property<'a> {
    /// The value as one big-endian `u32` cell.
    pub fn as_u32(&self) -> Option<u32> {
        if self.value.len() == 4 {
            be32(self.value, 0)
        } else {
            None
        }
    }

    /// The value as a `u64`: one or two cells.
    pub fn as_u64(&self) -> Option<u64> {
        match self.value.len() {
            4 => be32(self.value, 0).map(u64::from),
            8 => be64(self.value, 0),
            _ => None,
        }
    }

    /// The value as a single NUL-terminated string.
    pub fn as_str(&self) -> Option<&'a str> {
        let (last, body) = self.value.split_last()?;
        if *last != 0 || body.contains(&0) {
            return None;
        }
        core::str::from_utf8(body).ok()
    }

    /// The value as a string list (`"a\0b\0"`). Yields nothing if the value
    /// is not NUL-terminated; a non-UTF-8 entry ends the list.
    pub fn strings(&self) -> StrList<'a> {
        let rest = match self.value.split_last() {
            Some((0, body)) => Some(body),
            _ => None,
        };
        StrList { rest }
    }
}

/// Iterator over the entries of a string-list property.
pub struct StrList<'a> {
    rest: Option<&'a [u8]>,
}

impl<'a> Iterator for StrList<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<&'a str> {
        let rest = self.rest?;
        let (item, tail) = match rest.iter().position(|&b| b == 0) {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        self.rest = tail;
        match core::str::from_utf8(item) {
            Ok(s) => Some(s),
            Err(_) => {
                self.rest = None;
                None
            }
        }
    }
}

/// Iterator over `reg` entries.
pub struct RegIter<'a> {
    value: &'a [u8],
    address_cells: u32,
    size_cells: u32,
    off: usize,
}

impl RegIter<'_> {
    fn cells(&mut self, n: u32) -> Option<u64> {
        let v = match n {
            0 => Some(0),
            1 => be32(self.value, self.off).map(u64::from),
            2 => be64(self.value, self.off),
            _ => None,
        }?;
        self.off += n as usize * 4;
        Some(v)
    }
}

impl Iterator for RegIter<'_> {
    type Item = Reg;

    fn next(&mut self) -> Option<Reg> {
        if self.off >= self.value.len() {
            return None;
        }
        let address = self.cells(self.address_cells)?;
        let size = self.cells(self.size_cells)?;
        Some(Reg { address, size })
    }
}

/// Iterator over the memory reservation block.
pub struct Reservations<'a> {
    data: &'a [u8],
    off: usize,
}

impl Iterator for Reservations<'_> {
    type Item = Reg;

    fn next(&mut self) -> Option<Reg> {
        let address = be64(self.data, self.off)?;
        let size = be64(self.data, self.off.checked_add(8)?)?;
        if address == 0 && size == 0 {
            return None;
        }
        self.off += 16;
        Some(Reg { address, size })
    }
}
