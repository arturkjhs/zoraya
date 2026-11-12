//! Without bytes from the random service the program has no random bytes
//! (ADR-0227 item 6), and ring's `getrandom` on the target is exactly this. Alone in
//! its file, so that no other test seeds the process-wide generator first.

use zoraya_tls::{Error, fill_random, is_seeded, seed_random};

zoraya_test_alloc::cap_memory!();

#[test]
fn an_unseeded_program_has_no_random_bytes_and_a_seeded_one_has_different_ones() {
    assert!(!is_seeded());
    let mut a = [0u8; 32];
    assert_eq!(fill_random(&mut a), Err(Error::NoRandom));
    assert_eq!(a, [0u8; 32], "nothing written on failure");
    // A short seed, or a first seed of zeros, is refused and leaves it unseeded.
    assert_eq!(seed_random(b"short"), Err(Error::BadSeed));
    assert_eq!(seed_random(&[0u8; 32]), Err(Error::BadSeed));
    assert!(!is_seeded());
    seed_random(b"seed bytes from the random service, 32 or more").unwrap();
    let mut b = [0u8; 32];
    let mut c = [0u8; 32];
    fill_random(&mut b).unwrap();
    fill_random(&mut c).unwrap();
    assert!(is_seeded());
    assert_ne!(b, c);
    // A reseed changes the stream.
    assert_eq!(seed_random(b"more bytes"), Err(Error::BadSeed));
    seed_random(b"more bytes, but at least thirty-two of them").unwrap();
    let mut d = [0u8; 32];
    fill_random(&mut d).unwrap();
    assert_ne!(c, d);
}
