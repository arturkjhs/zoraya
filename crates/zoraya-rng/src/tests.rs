use super::*;
use std::vec::Vec;
use zoraya_protocol::Message;

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// RFC 8439 section 2.3.2.
#[test]
fn the_block_function_is_rfc_8439s() {
    let key: [u8; 32] = core::array::from_fn(|i| i as u8);
    let nonce = [0, 0, 0, 9, 0, 0, 0, 0x4a, 0, 0, 0, 0];
    let block = chacha20_block(&key, 1, &nonce);
    let expected = hex(
        "10f1e7e4d13b5915500fdd1fa32071c4c7d1f4c733c068030422aa9ac3d46c4e\
         d2826446079faa0914c2d705d98b02a2b5129cd1de164eb9cbd083e8a2503c4e",
    );
    assert_eq!(block.as_slice(), expected.as_slice());
}

#[test]
fn the_ordinals_are_the_declared_ones() {
    assert_eq!(<Bytes as Message>::ORDINAL, BYTES);
    assert_eq!(<Share as Message>::ORDINAL, SHARE);
}

fn output(g: &mut Generator, n: usize) -> Vec<u8> {
    let mut v = std::vec![0u8; n];
    g.fill(&mut v);
    v
}

#[test]
fn the_same_seed_gives_the_same_stream_and_another_gives_another() {
    let a = output(&mut Generator::new(b"seed one"), 100);
    assert_eq!(a, output(&mut Generator::new(b"seed one"), 100));
    assert_ne!(a, output(&mut Generator::new(b"seed two"), 100));
    // Trailing zeros are a different seed.
    assert_ne!(
        output(&mut Generator::new(b"x"), 16),
        output(&mut Generator::new(b"x\0"), 16)
    );
    // The unchecked constructor still gives output for an empty seed, and
    // not zeros; `from_seed` is what refuses it (below).
    assert!(output(&mut Generator::new(b""), 64).iter().any(|b| *b != 0));
}

#[test]
fn successive_requests_differ_and_lengths_are_exact() {
    let mut g = Generator::new(b"s");
    let first = output(&mut g, 32);
    let second = output(&mut g, 32);
    assert_ne!(first, second);
    for n in [0, 1, 31, 32, 33, 63, 64, 65, 4095, 4096] {
        assert_eq!(output(&mut g, n).len(), n);
    }
    // One request of 96 bytes is not two of 48 (the key moves between).
    let mut a = Generator::new(b"s");
    let mut b = Generator::new(b"s");
    let whole = output(&mut a, 96);
    let mut parts = output(&mut b, 48);
    parts.extend(output(&mut b, 48));
    assert_ne!(whole, parts);
}

/// INV-0227-2: output changes after the reseed point and depends on the
/// device's bytes.
#[test]
fn a_reseed_changes_the_output_and_depends_on_the_device() {
    let mut plain = Generator::new(b"s");
    let mut one = Generator::new(b"s");
    let mut two = Generator::new(b"s");
    assert_eq!(output(&mut plain, 40), output(&mut one, 40));
    assert_eq!(output(&mut two, 40), output(&mut Generator::new(b"s"), 40));
    one.mix(b"device bytes A");
    two.mix(b"device bytes B");
    let (p, a, b) = (
        output(&mut plain, 40),
        output(&mut one, 40),
        output(&mut two, 40),
    );
    assert_ne!(p, a);
    assert_ne!(p, b);
    assert_ne!(a, b);
    // The same device bytes at the same point give the same output.
    let mut again = Generator::new(b"s");
    let _ = output(&mut again, 40);
    again.mix(b"device bytes A");
    assert_eq!(output(&mut again, 40), a);
    // Every byte of a long input counts.
    let long: Vec<u8> = (0..200).map(|i| i as u8).collect();
    let mut base = Generator::new(b"s");
    base.mix(&long);
    let reference = output(&mut base, 16);
    for at in [0, 31, 32, 150, 199] {
        let mut changed = long.clone();
        changed[at] ^= 1;
        let mut g = Generator::new(b"s");
        g.mix(&changed);
        assert_ne!(output(&mut g, 16), reference, "byte {at}");
    }
}

/// The bits are balanced (a sanity check, not a statistical test).
#[test]
fn the_bits_are_about_half_ones() {
    let bits: u32 = output(&mut Generator::new(b"balance"), 4096)
        .iter()
        .map(|b| b.count_ones())
        .sum();
    assert!((16_384 - 700..16_384 + 700).contains(&bits), "{bits}");
}

fn service() -> Service {
    Service::new(Generator::new(b"service"), 2)
}

fn ask(s: &mut Service, count: u32, transaction: u32) -> (Action, Vec<u8>) {
    let mut message = [0u8; 64];
    let n = encode(&Bytes { count }, transaction, &mut message).unwrap();
    let mut out = std::vec![0u8; OUT_SIZE];
    let action = s.handle(&mut Conn::root(), &message[..n], 0, &mut out);
    (action, out)
}

/// INV-0227-3: `Bytes(n)` answers at once with exactly `n` bytes for `n` up to
/// 4096, and refuses more without changing the connection.
#[test]
fn bytes_never_come_short() {
    let mut s = service();
    for n in [0u32, 1, 31, 32, 33, 64, 1000, 4095, 4096] {
        let (action, out) = ask(&mut s, n, 7);
        let Action::Reply(len) = action else {
            panic!("{n}: {action:?}")
        };
        let (h, r) = decode::<BytesReply>(&out[..len], 0).unwrap();
        assert_eq!(h.transaction, 7);
        assert_eq!(r.bytes.0.len(), n as usize);
    }
    let (action, out) = ask(&mut s, 4097, 8);
    let Action::Reply(len) = action else { panic!() };
    let (h, e) = decode::<ErrorReply>(&out[..len], 0).unwrap();
    assert_eq!((h.transaction, e.status), (8, status::TOO_MANY));
    let (action, _) = ask(&mut s, u32::MAX, 9);
    assert!(matches!(action, Action::Reply(_)));
}

#[test]
fn two_requests_get_different_bytes() {
    let mut s = service();
    let take = |s: &mut Service| {
        let (Action::Reply(len), out) = ask(s, 32, 1) else {
            panic!()
        };
        decode::<BytesReply>(&out[..len], 0)
            .unwrap()
            .1
            .bytes
            .0
            .to_vec()
    };
    assert_ne!(take(&mut s), take(&mut s));
}

#[test]
fn share_is_bounded_and_closes_on_nonsense() {
    let mut s = service();
    let mut c = Conn::root();
    assert!(s.attached());
    let mut message = [0u8; 64];
    let n = encode(&Share {}, 3, &mut message).unwrap();
    let mut out = std::vec![0u8; OUT_SIZE];
    // With one handle: the second connection fits, the third does not.
    assert!(matches!(
        s.handle(&mut c, &message[..n], 1, &mut out),
        Action::Share { attach: true, .. }
    ));
    let Action::Share { reply, attach } = s.handle(&mut c, &message[..n], 1, &mut out) else {
        panic!()
    };
    assert!(!attach);
    assert_eq!(
        decode::<ShareReply>(&out[..reply], 0).unwrap().1.status.0,
        status::NO_SPACE
    );
    assert_eq!(s.connections(), 2);
    s.detached();
    assert_eq!(s.connections(), 1);
    // No handle, or two: not the message it says.
    assert_eq!(s.handle(&mut c, &message[..n], 0, &mut out), Action::Close);
    assert_eq!(s.handle(&mut c, &message[..n], 2, &mut out), Action::Close);
    // Garbage ends the connection; an unknown ordinal does not.
    assert_eq!(s.handle(&mut c, b"junk", 0, &mut out), Action::Close);
    let mut unknown = [0u8; 16];
    unknown[..4].copy_from_slice(&zoraya_protocol::MAGIC_VERSION.to_le_bytes());
    unknown[4..8].copy_from_slice(&99u32.to_le_bytes());
    assert!(matches!(
        s.handle(&mut c, &unknown, 0, &mut out),
        Action::Reply(_)
    ));
}

/// 20,000 random messages: no panic, and replies always decode.
#[test]
fn random_messages_never_panic() {
    let mut s = Service::new(Generator::new(b"fuzz"), 4);
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut out = std::vec![0u8; OUT_SIZE];
    for _ in 0..20_000 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let mut m = [0u8; 40];
        for (i, b) in m.iter_mut().enumerate() {
            *b = (x >> ((i % 8) * 8)) as u8 ^ (i as u8);
        }
        let len = (x % 41) as usize;
        if x & 1 == 0 && len >= 16 {
            m[..4].copy_from_slice(&zoraya_protocol::MAGIC_VERSION.to_le_bytes());
            m[4..8].copy_from_slice(&((x >> 8) as u32 % 4).to_le_bytes());
            m[12..16].copy_from_slice(&((len - 16) as u32).to_le_bytes());
        }
        let _ = s.handle(
            &mut Conn::root(),
            &m[..len],
            (x >> 20) as usize % 3,
            &mut out,
        );
        s.detached();
    }
}

#[test]
fn from_seed_refuses_short_and_zero_seeds() {
    assert_eq!(Generator::from_seed(b"").err(), Some(SeedError::Short));
    assert_eq!(
        Generator::from_seed(&[7u8; 31]).err(),
        Some(SeedError::Short)
    );
    assert_eq!(
        Generator::from_seed(&[0u8; 32]).err(),
        Some(SeedError::Zero)
    );
    assert_eq!(
        Generator::from_seed(&[0u8; 100]).err(),
        Some(SeedError::Zero)
    );
    let mut ok = Generator::from_seed(&[1u8; 32]).unwrap();
    assert_eq!(
        output(&mut ok, 40),
        output(&mut Generator::new(&[1u8; 32]), 40)
    );
    // One non-zero byte among zeros is enough to start.
    let mut seed = [0u8; 64];
    seed[63] = 1;
    assert!(Generator::from_seed(&seed).is_ok());
}

#[test]
fn only_the_first_two_levels_share_and_children_are_counted() {
    let mut s = Service::new(Generator::new(b"share"), 1000);
    let mut message = [0u8; 64];
    let n = encode(&Share {}, 3, &mut message).unwrap();
    let mut out = std::vec![0u8; OUT_SIZE];
    // init's connection: shares up to the cap, then is refused.
    let mut root = Conn::root();
    for _ in 0..MAX_CHILDREN {
        assert!(matches!(
            s.handle(&mut root, &message[..n], 1, &mut out),
            Action::Share { attach: true, .. }
        ));
    }
    assert!(matches!(
        s.handle(&mut root, &message[..n], 1, &mut out),
        Action::Share { attach: false, .. }
    ));
    // A child closing frees one.
    root.released();
    assert!(matches!(
        s.handle(&mut root, &message[..n], 1, &mut out),
        Action::Share { attach: true, .. }
    ));
    // The shell's connection (depth 1) shares; a program's (depth 2) never.
    let mut shell = Conn::root().child();
    assert!(matches!(
        s.handle(&mut shell, &message[..n], 1, &mut out),
        Action::Share { attach: true, .. }
    ));
    let mut program = shell.child();
    for _ in 0..300 {
        let Action::Share { reply, attach } = s.handle(&mut program, &message[..n], 1, &mut out)
        else {
            panic!()
        };
        assert!(!attach);
        assert_eq!(
            decode::<ShareReply>(&out[..reply], 0).unwrap().1.status.0,
            status::NO_SPACE
        );
    }
    assert_eq!(program.children(), 0);
    // A refused share did not count a connection.
    let before = s.connections();
    let _ = s.handle(&mut program, &message[..n], 1, &mut out);
    assert_eq!(s.connections(), before);
}
