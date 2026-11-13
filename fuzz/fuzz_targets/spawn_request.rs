//! `spawn`'s request and the startup block (ADR-0268 point 3's "fuzzing",
//! ADR-0298): a program in an untrusted room writes the request, so the
//! kernel's decoder must refuse or accept every byte string without a panic,
//! and what it accepts must be consistent. The first byte picks the input: a
//! request as it is, a valid request with bytes changed (so the checks past
//! the header are reached), or a startup block.

#![no_main]

use libfuzzer_sys::fuzz_target;
use zoraya_abi::spawn::{Encoder, HandleEntry, SpawnRequest, StartupBlock, Transfer};
use zoraya_abi::{Handle, Rights};

fn seed() -> Vec<u8> {
    let mut buf = vec![0u8; 512];
    let mut e = Encoder::new(&mut buf, Handle::new(3).unwrap(), Handle::new(4).unwrap());
    for slot in 1..4u32 {
        e.handle(HandleEntry {
            slot,
            handle: Handle::new(u64::from(slot) + 10).unwrap(),
            rights: Rights::READ,
            transfer: if slot == 2 { Transfer::Move } else { Transfer::Duplicate },
        });
    }
    e.arg(b"prog").arg(b"-x");
    e.env(b"HOME=/");
    let n = e.finish().unwrap();
    buf.truncate(n);
    buf
}

fuzz_target!(|data: &[u8]| {
    let Some((&pick, rest)) = data.split_first() else { return };
    match pick % 3 {
        0 => check_request(rest),
        1 => {
            let mut v = seed();
            for pair in rest.chunks(3) {
                if let [a, b, c] = *pair {
                    let at = (usize::from(a) << 8 | usize::from(b)) % v.len();
                    v[at] ^= c;
                }
            }
            check_request(&v);
        }
        _ => {
            if let Some(b) = StartupBlock::decode(rest) {
                let _ = (b.argv.count(), b.envp.count());
                assert_eq!(b.program_headers.len() % zoraya_abi::spawn::PROGRAM_HEADER_SIZE, 0);
            }
        }
    }
});

fn check_request(bytes: &[u8]) {
    if let Ok(r) = SpawnRequest::decode(bytes) {
        // Every entry that decode checked is readable, and counted once.
        assert_eq!(r.handles().count(), r.handle_count());
        assert_eq!(r.argv.count(), r.argv.len());
        assert_eq!(r.envp.count(), r.envp.len());
    }
}
