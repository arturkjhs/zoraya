//! Malformed input: the parser must return an error (or, for mutations that
//! happen to stay well formed, a usable tree), never panic and never read
//! outside the slice. Out-of-bounds slice access in safe Rust panics, so
//! "never panics" also covers "never reads out of bounds".

use zoraya_dtb::platform::PlatformInfo;
use zoraya_dtb::{Error, Fdt, MAX_DEPTH};

const DEFAULT: &[u8] = include_bytes!("fixtures/virt-default.dtb");
const GICV3: &[u8] = include_bytes!("fixtures/virt-gicv3.dtb");

// --- a minimal blob builder -------------------------------------------------

#[derive(Default)]
struct Builder {
    structs: Vec<u8>,
    strings: Vec<u8>,
}

impl Builder {
    fn u32(&mut self, v: u32) -> &mut Self {
        self.structs.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn begin(&mut self, name: &str) -> &mut Self {
        self.u32(1);
        self.structs.extend_from_slice(name.as_bytes());
        self.structs.push(0);
        self.pad()
    }
    fn end(&mut self) -> &mut Self {
        self.u32(2)
    }
    fn prop(&mut self, name: &str, value: &[u8]) -> &mut Self {
        let off = self.strings.len() as u32;
        self.strings.extend_from_slice(name.as_bytes());
        self.strings.push(0);
        self.u32(3).u32(value.len() as u32).u32(off);
        self.structs.extend_from_slice(value);
        self.pad()
    }
    fn fdt_end(&mut self) -> &mut Self {
        self.u32(9)
    }
    fn pad(&mut self) -> &mut Self {
        while !self.structs.len().is_multiple_of(4) {
            self.structs.push(0);
        }
        self
    }
    /// Header, empty reservation map, structure block, strings block.
    fn build(&self) -> Vec<u8> {
        let rsv = 40u32;
        let st = rsv + 16;
        let sz_st = self.structs.len() as u32;
        let strs = st + sz_st;
        let total = strs + self.strings.len() as u32;
        let mut b = Vec::new();
        for v in [
            0xd00d_feed,
            total,
            st,
            strs,
            rsv,
            17,
            16,
            0,
            self.strings.len() as u32,
            sz_st,
        ] {
            b.extend_from_slice(&u32::to_be_bytes(v));
        }
        b.extend_from_slice(&[0; 16]);
        b.extend_from_slice(&self.structs);
        b.extend_from_slice(&self.strings);
        b
    }
}

fn minimal() -> Builder {
    let mut b = Builder::default();
    b.begin("")
        .prop("#address-cells", &2u32.to_be_bytes())
        .prop("#size-cells", &2u32.to_be_bytes())
        .prop("compatible", b"test\0")
        .begin("memory@40000000")
        .prop("device_type", b"memory\0")
        .prop(
            "reg",
            &[0, 0, 0, 0, 0x40, 0, 0, 0, 0, 0, 0, 0, 0x08, 0, 0, 0],
        )
        .end()
        .begin("uart@9000000")
        .prop("compatible", b"arm,pl011\0arm,primecell\0")
        .prop(
            "reg",
            &[0, 0, 0, 0, 0x09, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0],
        )
        .end()
        .end()
        .fdt_end();
    b
}

fn set_word(blob: &mut [u8], index: usize, v: u32) {
    blob[index * 4..index * 4 + 4].copy_from_slice(&v.to_be_bytes());
}

/// Calls every accessor on whatever parses, so that mutations reach the
/// lazily evaluated paths too.
fn exercise(blob: &[u8]) {
    let Ok(fdt) = Fdt::new(blob) else { return };
    let _ = PlatformInfo::from_fdt(&fdt);
    for r in fdt.reservations() {
        let _ = r;
    }
    for node in fdt.nodes() {
        let _ = (node.name(), node.base_name(), node.own_cells());
        let _ = (node.is_enabled(), node.device_type());
        for p in node.properties() {
            let _ = (p.as_u32(), p.as_u64(), p.as_str());
            for s in p.strings() {
                let _ = s.len();
            }
        }
        if let Some(regs) = node.reg() {
            for r in regs {
                let _ = r;
            }
        }
        for c in node.children() {
            let _ = c.property("reg");
        }
    }
    let _ = fdt.find_node("/chosen");
    let _ = fdt.alias("serial0");
    let _ = fdt.node_by_phandle(0x8002);
    let _ = fdt.find_compatible("arm,pl011");
}

// --- crafted cases ------------------------------------------------------------

#[test]
fn builder_blob_is_valid() {
    let blob = minimal().build();
    let fdt = Fdt::new(&blob).unwrap();
    let info = PlatformInfo::from_fdt(&fdt).unwrap();
    assert_eq!(info.uart.base, 0x0900_0000);
    assert_eq!(info.memory.as_slice()[0].size, 0x0800_0000);
}

#[test]
fn empty_and_short_buffers() {
    assert_eq!(Fdt::new(&[]).unwrap_err(), Error::Truncated);
    let blob = minimal().build();
    for len in 0..40 {
        assert_eq!(Fdt::new(&blob[..len]).unwrap_err(), Error::Truncated);
    }
}

#[test]
fn every_truncation_is_rejected() {
    let blob = minimal().build();
    for len in 40..blob.len() {
        assert_eq!(
            Fdt::new(&blob[..len]).unwrap_err(),
            Error::Truncated,
            "len {len}"
        );
    }
}

#[test]
fn truncation_with_matching_totalsize_is_rejected() {
    // Shrink totalsize to the buffer so the header itself is consistent and
    // the block checks are what must catch the cut.
    // Both bases end exactly at their last block, so every shorter cut
    // removes data a block needs.
    for base in [minimal().build(), compact(DEFAULT)] {
        assert!(Fdt::new(&base).is_ok());
        for len in 40..base.len() {
            let mut b = base[..len].to_vec();
            set_word(&mut b, 1, len as u32);
            assert!(Fdt::new(&b).is_err(), "len {len} accepted");
            exercise(&b);
        }
    }
}

#[test]
fn wrong_magic() {
    let mut blob = minimal().build();
    blob[0] ^= 1;
    assert_eq!(Fdt::new(&blob).unwrap_err(), Error::BadMagic);
    let mut d = DEFAULT.to_vec();
    set_word(&mut d, 0, 0xedfe_0dd0); // byte-swapped
    assert_eq!(Fdt::new(&d).unwrap_err(), Error::BadMagic);
}

#[test]
fn versions() {
    let mut blob = minimal().build();
    set_word(&mut blob, 5, 16);
    assert_eq!(Fdt::new(&blob).unwrap_err(), Error::UnsupportedVersion);
    let mut blob = minimal().build();
    set_word(&mut blob, 6, 18);
    assert_eq!(Fdt::new(&blob).unwrap_err(), Error::UnsupportedVersion);
}

#[test]
fn bad_header_offsets() {
    let base = minimal().build();
    let len = base.len() as u32;
    // (word index, value)
    let cases = [
        (1, 39),           // totalsize below the header
        (2, len),          // struct block starts at the end
        (2, 0),            // struct block overlaps the header
        (2, 57),           // struct block misaligned
        (2, u32::MAX),     // struct offset overflows
        (9, u32::MAX),     // struct size overflows
        (9, len),          // struct block runs past the end
        (3, len + 1),      // strings block past the end
        (8, u32::MAX),     // strings size overflows
        (4, 44),           // reservation map misaligned
        (4, 8),            // reservation map in the header
        (4, len - 8),      // reservation map runs off the end
        (4, u32::MAX - 7), // reservation map offset overflows
    ];
    for (word, v) in cases {
        let mut b = base.clone();
        set_word(&mut b, word, v);
        let e = Fdt::new(&b).unwrap_err();
        assert!(
            matches!(e, Error::BadLayout | Error::Truncated),
            "word {word} = {v:#x}: {e:?}"
        );
    }
}

#[test]
fn reservation_map_without_terminator() {
    let mut b = minimal().build();
    // Make the single terminator entry non-zero; the next 16 bytes are the
    // structure block, whose first word is non-zero, so the map never ends
    // before running off the blob or into garbage. Point it near the end.
    let len = b.len();
    set_word(&mut b, 4, (len - 16) as u32 & !7);
    let r = Fdt::new(&b);
    assert!(r.is_err() || r.unwrap().reservations().count() < 4);
    exercise(&b);
}

#[test]
fn structure_errors() {
    let bad = |f: &dyn Fn(&mut Builder)| {
        let mut b = Builder::default();
        f(&mut b);
        Fdt::new(&b.build()).unwrap_err()
    };
    // Unknown token.
    assert_eq!(
        bad(&|b| {
            b.begin("").u32(7).end().fdt_end();
        }),
        Error::BadStructure
    );
    // Unbalanced: missing END_NODE.
    assert_eq!(
        bad(&|b| {
            b.begin("").begin("a").end().fdt_end();
        }),
        Error::BadStructure
    );
    // Extra END_NODE.
    assert_eq!(
        bad(&|b| {
            b.begin("").end().end().fdt_end();
        }),
        Error::BadStructure
    );
    // Missing FDT_END.
    assert_eq!(
        bad(&|b| {
            b.begin("").end();
        }),
        Error::BadStructure
    );
    // No root.
    assert_eq!(
        bad(&|b| {
            b.fdt_end();
        }),
        Error::BadStructure
    );
    // Root with a name.
    assert_eq!(
        bad(&|b| {
            b.begin("x").end().fdt_end();
        }),
        Error::BadStructure
    );
    // Two roots.
    assert_eq!(
        bad(&|b| {
            b.begin("").end().begin("").end().fdt_end();
        }),
        Error::BadStructure
    );
    // Property outside any node.
    assert_eq!(
        bad(&|b| {
            b.prop("p", b"x\0").begin("").end().fdt_end();
        }),
        Error::BadStructure
    );
    // Property length past the block.
    assert_eq!(
        bad(&|b| {
            b.begin("").u32(3).u32(0x7fff_ffff).u32(0).end().fdt_end();
        }),
        Error::BadStructure
    );
    // Property length that overflows.
    assert_eq!(
        bad(&|b| {
            b.begin("").u32(3).u32(u32::MAX).u32(0).end().fdt_end();
        }),
        Error::BadStructure
    );
    // Too deep.
    assert_eq!(
        bad(&|b| {
            for _ in 0..=MAX_DEPTH {
                b.begin(if b.structs.is_empty() { "" } else { "n" });
            }
            for _ in 0..=MAX_DEPTH {
                b.end();
            }
            b.fdt_end();
        }),
        Error::BadStructure
    );
}

#[test]
fn max_depth_is_accepted() {
    let mut b = Builder::default();
    b.begin("");
    for _ in 1..MAX_DEPTH {
        b.begin("n");
    }
    for _ in 0..MAX_DEPTH {
        b.end();
    }
    b.fdt_end();
    let blob = b.build();
    let fdt = Fdt::new(&blob).unwrap();
    assert_eq!(fdt.nodes().count(), MAX_DEPTH);
}

#[test]
fn string_errors() {
    // Property name offset outside the strings block.
    let mut b = Builder::default();
    b.begin("").u32(3).u32(0).u32(1000).end().fdt_end();
    b.strings.extend_from_slice(b"x\0");
    assert_eq!(Fdt::new(&b.build()).unwrap_err(), Error::BadString);
    // Property name without a terminating NUL.
    let mut b = Builder::default();
    b.begin("").u32(3).u32(0).u32(0).end().fdt_end();
    b.strings.extend_from_slice(b"abc");
    assert_eq!(Fdt::new(&b.build()).unwrap_err(), Error::BadString);
    // Node name that is not UTF-8.
    let mut b = Builder::default();
    b.begin("");
    b.u32(1);
    b.structs.extend_from_slice(&[0xff, 0xfe, 0, 0]);
    b.end().end().fdt_end();
    assert_eq!(Fdt::new(&b.build()).unwrap_err(), Error::BadString);
    // Node name running to the end of the structure block.
    let mut b = Builder::default();
    b.begin("").u32(1);
    b.structs.extend_from_slice(b"abcd");
    assert_eq!(Fdt::new(&b.build()).unwrap_err(), Error::BadString);
}

#[test]
fn odd_property_values_are_not_errors() {
    let mut b = Builder::default();
    b.begin("")
        .prop("#address-cells", &[0, 0, 0, 3]) // too wide for reg
        .prop("compatible", b"no-nul")
        .prop("model", b"a\0b\0")
        .prop("status", b"disabled\0")
        .begin("child")
        .prop("reg", &[1, 2, 3])
        .end()
        .end()
        .fdt_end();
    let blob = b.build();
    let fdt = Fdt::new(&blob).unwrap();
    let root = fdt.root();
    assert!(!root.is_compatible("no-nul"));
    assert_eq!(root.property("compatible").unwrap().strings().count(), 0);
    assert_eq!(root.property("model").unwrap().as_str(), None);
    assert_eq!(root.property("model").unwrap().strings().count(), 2);
    assert!(!root.is_enabled());
    assert!(root.child("child").unwrap().reg().is_none());
    assert!(PlatformInfo::from_fdt(&fdt).is_err());
}

// --- mutation fuzzing ---------------------------------------------------------

/// xorshift64*: deterministic, no dependencies.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

/// Copies the used part of a QEMU blob (header up to the end of the strings
/// block) and sets totalsize to it, so mutations land on real content.
fn compact(blob: &[u8]) -> Vec<u8> {
    let w = |i: usize| u32::from_be_bytes(blob[i * 4..i * 4 + 4].try_into().unwrap()) as usize;
    let end = (w(3) + w(8)).max(w(2) + w(9));
    let mut b = blob[..end].to_vec();
    set_word(&mut b, 1, end as u32);
    assert!(Fdt::new(&b).is_ok());
    b
}

#[test]
fn random_mutations_never_panic() {
    let seeds = [compact(DEFAULT), compact(GICV3), minimal().build()];
    let mut rng = Rng(0x5a17_0b0e_d00d_feed);
    let mut accepted = 0usize;
    for _ in 0..30_000 {
        let base = &seeds[rng.below(seeds.len())];
        let mut b = base.clone();
        for _ in 0..1 + rng.below(8) {
            let i = rng.below(b.len());
            match rng.below(4) {
                0 => b[i] = rng.next() as u8,
                1 => b[i] ^= 1 << rng.below(8),
                // Plausible small integers in aligned words: lengths,
                // offsets and tokens.
                2 => {
                    let i = i & !3;
                    if i + 4 <= b.len() {
                        let v = rng.below(64) as u32;
                        b[i..i + 4].copy_from_slice(&v.to_be_bytes());
                    }
                }
                _ => {
                    let cut = rng.below(b.len());
                    b.truncate(cut.max(1));
                }
            }
            if b.is_empty() {
                break;
            }
        }
        if Fdt::new(&b).is_ok() {
            accepted += 1;
        }
        exercise(&b);
    }
    // Some mutations must survive validation, or the accessors on
    // well-formed-but-odd trees would never be reached.
    assert!(accepted > 100, "only {accepted} mutated blobs parsed");
}
