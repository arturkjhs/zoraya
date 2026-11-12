use super::*;
use std::vec;
use std::vec::Vec;

fn query(id: u16, name: &str) -> Vec<u8> {
    let mut out = vec![0u8; MAX_QUERY];
    let n = encode_query(id, name, &mut out).unwrap();
    out.truncate(n);
    out
}

/// A response to `query(id, name)`: the question repeated, then `records`
/// (raw bytes of the answer section), with the given flags and count.
fn response(id: u16, name: &str, flags: u16, count: u16, records: &[u8]) -> Vec<u8> {
    let q = query(id, name);
    let mut out = q.clone();
    out[2..4].copy_from_slice(&flags.to_be_bytes());
    out[6..8].copy_from_slice(&count.to_be_bytes());
    out.extend_from_slice(records);
    out
}

/// A record whose owner is a pointer to the question (offset 12).
fn a_record(ttl: u32, address: [u8; 4]) -> Vec<u8> {
    let mut r = vec![0xc0, 12, 0, 1, 0, 1];
    r.extend_from_slice(&ttl.to_be_bytes());
    r.extend_from_slice(&[0, 4]);
    r.extend_from_slice(&address);
    r
}

#[test]
fn a_query_has_the_shape_of_rfc_1035() {
    let q = query(0x1234, "test.zoraya");
    assert_eq!(&q[..4], &[0x12, 0x34, 0x01, 0x00]);
    assert_eq!(&q[4..12], &[0, 1, 0, 0, 0, 0, 0, 0]);
    assert_eq!(&q[12..], b"\x04test\x06zoraya\x00\x00\x01\x00\x01");
    assert_eq!(query(1, "test.zoraya."), query(1, "test.zoraya"));
}

#[test]
fn bad_names_are_refused_and_short_buffers_too() {
    let mut out = [0u8; MAX_QUERY];
    for bad in [
        "",
        ".",
        "a..b",
        "-a b",
        "a b",
        "é.com",
        &"a".repeat(64),
        &"a.".repeat(128),
    ] {
        assert_eq!(
            encode_query(1, bad, &mut out),
            Err(QueryError::BadName),
            "{bad}"
        );
    }
    let long = [
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61),
    ]
    .join(".");
    assert_eq!(long.len(), MAX_NAME);
    assert!(encode_query(1, &long, &mut out).is_ok());
    assert_eq!(
        encode_query(1, &(long.clone() + "d"), &mut out),
        Err(QueryError::BadName)
    );
    assert_eq!(
        encode_query(1, "test.zoraya", &mut out[..20]),
        Err(QueryError::NoRoom)
    );
    assert!(encode_query(1, "a", &mut out[..12 + 1 + 2 + 4]).is_ok());
}

#[test]
fn an_address_answer_is_decoded() {
    let mut records = a_record(300, [10, 0, 2, 2]);
    records.extend(a_record(60, [10, 0, 2, 3]));
    let m = response(7, "Test.Zoraya", 0x8180, 2, &records);
    let Ok(Outcome::Addresses(a)) = parse_response(&m, 7, "test.zoraya") else {
        panic!("not addresses");
    };
    assert_eq!(a.as_slice(), &[[10, 0, 2, 2], [10, 0, 2, 3]]);
    assert_eq!(a.ttl, 60);
}

#[test]
fn a_long_ttl_is_capped() {
    let m = response(7, "a.b", 0x8180, 1, &a_record(u32::MAX, [1, 2, 3, 4]));
    let Ok(Outcome::Addresses(a)) = parse_response(&m, 7, "a.b") else {
        panic!();
    };
    assert_eq!(a.ttl, MAX_TTL);
}

#[test]
fn a_cname_chain_leads_to_the_addresses() {
    // test.zoraya CNAME real.zoraya; real.zoraya A 10.0.2.9
    let mut r = vec![0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60];
    // "real" then a pointer to "zoraya", which starts at offset 17.
    let rdata = b"\x04real\xc0\x11";
    r.extend_from_slice(&[0, rdata.len() as u8]);
    r.extend_from_slice(rdata);
    // The owner of the second record is "real.zoraya" spelled out.
    r.extend_from_slice(b"\x04real\x06zoraya\x00");
    r.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 10, 0, 2, 9]);
    let m = response(9, "test.zoraya", 0x8180, 2, &r);
    let Ok(Outcome::Addresses(a)) = parse_response(&m, 9, "test.zoraya") else {
        panic!("{:?}", parse_response(&m, 9, "test.zoraya"));
    };
    assert_eq!(a.as_slice(), &[[10, 0, 2, 9]]);
    assert_eq!(a.ttl, 30);
}

#[test]
fn an_address_for_another_owner_is_not_ours() {
    let mut r = b"\x05other\x06zoraya\x00".to_vec();
    r.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 6, 6, 6, 6]);
    let m = response(9, "test.zoraya", 0x8180, 1, &r);
    assert_eq!(parse_response(&m, 9, "test.zoraya"), Ok(Outcome::NoData));
}

#[test]
fn records_of_another_class_or_type_are_skipped() {
    let mut r = vec![0xc0, 12, 0, 1, 0, 3, 0, 0, 0, 9, 0, 4, 1, 1, 1, 1]; // class CH
    r.extend_from_slice(&[0xc0, 12, 0, 16, 0, 1, 0, 0, 0, 9, 0, 3, 2, b'h', b'i']); // TXT
    r.extend(a_record(10, [9, 9, 9, 9]));
    let m = response(9, "a.b", 0x8180, 3, &r);
    let Ok(Outcome::Addresses(a)) = parse_response(&m, 9, "a.b") else {
        panic!();
    };
    assert_eq!(a.as_slice(), &[[9, 9, 9, 9]]);
}

#[test]
fn the_server_codes_are_reported() {
    let nx = response(1, "a.b", 0x8183, 0, &[]);
    assert_eq!(parse_response(&nx, 1, "a.b"), Ok(Outcome::NoSuchName));
    for rcode in [1u16, 2, 4, 5, 15] {
        let m = response(1, "a.b", 0x8180 | rcode, 0, &[]);
        assert_eq!(parse_response(&m, 1, "a.b"), Ok(Outcome::ServerFailure));
    }
    let empty = response(1, "a.b", 0x8180, 0, &[]);
    assert_eq!(parse_response(&empty, 1, "a.b"), Ok(Outcome::NoData));
    let tc = response(1, "a.b", 0x8380, 0, &[]);
    assert_eq!(parse_response(&tc, 1, "a.b"), Ok(Outcome::Truncated));
}

#[test]
fn a_message_that_is_not_our_answer_is_refused() {
    let good = response(5, "a.b", 0x8180, 1, &a_record(1, [1, 1, 1, 1]));
    assert!(parse_response(&good, 5, "a.b").is_ok());
    assert_eq!(parse_response(&good, 6, "a.b"), Err(ParseError::WrongId));
    assert_eq!(
        parse_response(&good, 5, "c.d"),
        Err(ParseError::WrongQuestion)
    );
    // A query is not an answer; neither is another opcode.
    let q = response(5, "a.b", 0x0180, 1, &a_record(1, [1, 1, 1, 1]));
    assert_eq!(parse_response(&q, 5, "a.b"), Err(ParseError::NotAnAnswer));
    let op = response(5, "a.b", 0x8980, 1, &a_record(1, [1, 1, 1, 1]));
    assert_eq!(parse_response(&op, 5, "a.b"), Err(ParseError::NotAnAnswer));
    // Two questions, none, or a different type.
    let mut two = good.clone();
    two[4..6].copy_from_slice(&2u16.to_be_bytes());
    assert_eq!(
        parse_response(&two, 5, "a.b"),
        Err(ParseError::WrongQuestion)
    );
    let mut none = good.clone();
    none[4..6].copy_from_slice(&0u16.to_be_bytes());
    assert_eq!(
        parse_response(&none, 5, "a.b"),
        Err(ParseError::WrongQuestion)
    );
    let mut aaaa = good.clone();
    let at = 12 + 1 + 1 + 1 + 1 + 1;
    aaaa[at + 1] = 28;
    assert_eq!(
        parse_response(&aaaa, 5, "a.b"),
        Err(ParseError::WrongQuestion)
    );
}

#[test]
fn pointers_that_loop_or_point_forward_are_refused() {
    // The record's owner points at itself.
    let q_len = query(1, "a.b").len();
    let mut r = vec![0xc0, q_len as u8, 0, 1, 0, 1, 0, 0, 0, 1, 0, 4, 1, 1, 1, 1];
    let m = response(1, "a.b", 0x8180, 1, &r);
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::BadName));
    // Forward.
    r[1] = (q_len + 1) as u8;
    let m = response(1, "a.b", 0x8180, 1, &r);
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::BadName));
    // A label then a pointer back to the label: a loop with forward steps.
    let mut loopy = vec![1, b'x', 0xc0, q_len as u8];
    loopy.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 1, 0, 4, 1, 1, 1, 1]);
    let m = response(1, "a.b", 0x8180, 1, &loopy);
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::BadName));
    // The reserved label types.
    for first in [0x40u8, 0x80] {
        let rr = vec![first, 0, 0, 1, 0, 1, 0, 0, 0, 1, 0, 4, 1, 1, 1, 1];
        let m = response(1, "a.b", 0x8180, 1, &rr);
        assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::BadName));
    }
}

#[test]
fn a_name_over_the_limit_is_refused() {
    // 130 labels of one byte: 259 bytes of text.
    let mut r = Vec::new();
    for _ in 0..130 {
        r.extend_from_slice(&[1, b'a']);
    }
    r.push(0);
    r.extend_from_slice(&[0, 1, 0, 1, 0, 0, 0, 1, 0, 4, 1, 1, 1, 1]);
    let m = response(1, "a.b", 0x8180, 1, &r);
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::BadName));
}

#[test]
fn lengths_that_lie_are_refused() {
    let mut r = a_record(1, [1, 1, 1, 1]);
    let n = r.len();
    r[n - 5] = 200; // rdlength beyond the message
    let m = response(1, "a.b", 0x8180, 1, &r);
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::Malformed));
    let mut r = a_record(1, [1, 1, 1, 1]);
    let n = r.len();
    r[n - 5] = 3; // an A record of 3 bytes
    let m = response(1, "a.b", 0x8180, 1, &r);
    assert!(parse_response(&m, 1, "a.b").is_err());
    // More records claimed than present.
    let m = response(1, "a.b", 0x8180, 60000, &a_record(1, [1, 1, 1, 1]));
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::Malformed));
}

#[test]
fn no_more_addresses_are_kept_than_the_bound() {
    let mut r = Vec::new();
    for i in 0..20u8 {
        r.extend(a_record(10, [10, 0, 0, i]));
    }
    let m = response(1, "a.b", 0x8180, 20, &r);
    let Ok(Outcome::Addresses(a)) = parse_response(&m, 1, "a.b") else {
        panic!();
    };
    assert_eq!(a.count, MAX_ADDRESSES);
    assert_eq!(a.as_slice().len(), MAX_ADDRESSES);
}

#[test]
fn a_message_over_the_bound_is_refused() {
    let mut m = response(1, "a.b", 0x8180, 1, &a_record(1, [1, 1, 1, 1]));
    m.resize(MAX_MESSAGE + 1, 0);
    assert_eq!(parse_response(&m, 1, "a.b"), Err(ParseError::Malformed));
}

#[test]
fn every_prefix_and_every_single_byte_change_is_total() {
    let mut r = a_record(300, [10, 0, 2, 2]);
    r.extend_from_slice(&[
        0xc0, 12, 0, 5, 0, 1, 0, 0, 0, 60, 0, 6, 3, b'f', b'o', b'o', 0xc0, 12,
    ]);
    let m = response(3, "test.zoraya", 0x8180, 2, &r);
    for n in 0..m.len() {
        let _ = parse_response(&m[..n], 3, "test.zoraya");
    }
    for i in 0..m.len() {
        for v in [0u8, 1, 0x3f, 0x40, 0x7f, 0x80, 0xbf, 0xc0, 0xff, m[i] ^ 1] {
            let mut x = m.clone();
            x[i] = v;
            let _ = parse_response(&x, 3, "test.zoraya");
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn random_messages_never_panic_and_a_good_header_does_not_help_them() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let base = response(3, "test.zoraya", 0x8180, 1, &a_record(300, [10, 0, 2, 2]));
    for round in 0..100_000 {
        let len = (rng.next() % 300) as usize;
        let mut m: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        match round % 3 {
            // Our header and question, random records.
            1 if m.len() >= 4 => {
                let mut x = base[..base.len() - 16].to_vec();
                x[6..8].copy_from_slice(&((rng.next() % 6) as u16).to_be_bytes());
                x.extend_from_slice(&m);
                m = x;
            }
            // The good message with a few bytes replaced.
            2 => {
                m = base.clone();
                for _ in 0..1 + rng.next() % 4 {
                    let at = (rng.next() as usize) % m.len();
                    m[at] = rng.next() as u8;
                }
            }
            _ => {}
        }
        if let Ok(Outcome::Addresses(a)) = parse_response(&m, 3, "test.zoraya") {
            assert!(a.count >= 1 && a.count <= MAX_ADDRESSES);
            assert!(a.ttl <= MAX_TTL);
        }
    }
}
