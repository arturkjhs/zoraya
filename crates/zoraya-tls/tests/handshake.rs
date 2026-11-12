//! INV-0228-1 against a scripted TLS server (rustls with `std`, on a loopback
//! socket) with the test PKI of `tests/net/tls`: the valid name works; an
//! expired certificate, a certificate for another name and an unknown root
//! fail in `connect`, and the server has read no application byte.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::vec::Vec;

use rustls::pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use zoraya_tls::{Config, Connection, Error, Transport, TransportError, pem_certificates};

zoraya_test_alloc::cap_memory!();

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/net/tls");

fn file(name: &str) -> Vec<u8> {
    std::fs::read(std::format!("{DIR}/{name}")).unwrap()
}

fn now() -> Option<u64> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs())
}

struct Tcp(TcpStream);

impl Transport for Tcp {
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError> {
        self.0.read(buffer).map_err(|_| TransportError("read"))
    }

    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError> {
        self.0.write_all(data).map_err(|_| TransportError("write"))
    }
}

/// What the server saw: application bytes read, and whether it ended without a
/// close_notify.
struct Seen {
    app_bytes: Vec<u8>,
    handshake_failed: bool,
}

/// Serves one connection with the leaf `leaf`: after the handshake it echoes
/// a short read upper-cased, and reads on; once it has `expect` bytes it sends
/// `after` bytes of `z`, a close_notify, and ends.
fn serve(leaf: &'static str, expect: usize, after: usize) -> (u16, JoinHandle<Seen>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let chain: Vec<CertificateDer<'static>> =
        CertificateDer::pem_slice_iter(&file(&std::format!("{leaf}.pem")))
            .map(|c| c.unwrap())
            .collect();
    let key = PrivateKeyDer::from_pem_slice(&file(&std::format!("{leaf}.key"))).unwrap();
    let config = Arc::new(
        rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(chain, key)
            .unwrap(),
    );
    let handle = std::thread::spawn(move || {
        let (mut tcp, _) = listener.accept().unwrap();
        let mut conn = rustls::ServerConnection::new(config).unwrap();
        let mut seen = Seen {
            app_bytes: Vec::new(),
            handshake_failed: false,
        };
        let mut tls = rustls::Stream::new(&mut conn, &mut tcp);
        let mut buffer = [0u8; 4096];
        loop {
            match tls.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    seen.app_bytes.extend_from_slice(&buffer[..n]);
                    if n <= 64 {
                        let up: Vec<u8> = buffer[..n].iter().map(u8::to_ascii_uppercase).collect();
                        if tls.write_all(&up).is_err() {
                            break;
                        }
                    }
                    if seen.app_bytes.len() >= expect {
                        break;
                    }
                }
                Err(_) => {
                    seen.handshake_failed = tls.conn.is_handshaking();
                    break;
                }
            }
        }
        if after > 0 && !seen.handshake_failed {
            let _ = tls.write_all(&std::vec![b'z'; after]);
            tls.conn.send_close_notify();
            let _ = tls.flush();
        }
        seen
    });
    (port, handle)
}

/// The 32 bytes a program draws from the random service per connection.
/// The process-wide generator is for one thread (a native program has one):
/// the tests that connect take turns.
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SEED: [u8; 32] = *b"a seed for the tests, 32 bytes!!";

fn config(root: &str) -> Config {
    Config::new(&pem_certificates(&file(root)), now).unwrap()
}

fn connect(port: u16, name: &str, root: &str) -> Result<Connection<Tcp>, Error> {
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    Connection::connect(&config(root), name, Tcp(tcp), &SEED)
}

#[test]
fn the_right_name_and_a_known_root_work_and_data_goes_both_ways() {
    let (port, server) = serve("valid", 9 + 200_000, 100_000);
    let mut c = connect(port, "test.zoraya", "root.pem").expect("handshake");
    c.write_all(b"hello tls").unwrap();
    let mut echoed = [0u8; 9];
    let mut got = 0;
    while got < 9 {
        got += c.read(&mut echoed[got..]).unwrap();
    }
    assert_eq!(&echoed, b"HELLO TLS");
    // 200,000 bytes out, split in records, then 100,000 back from the server
    // and its close_notify, which reads as the end.
    let big: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8 | 0x20).collect();
    c.write_all(&big).unwrap();
    let mut zs = 0;
    let mut sink = [0u8; 5000];
    loop {
        let n = c.read(&mut sink).unwrap();
        if n == 0 {
            break;
        }
        assert!(sink[..n].iter().all(|b| *b == b'z'));
        zs += n;
    }
    assert_eq!(zs, 100_000);
    drop(c.close());
    let seen = server.join().unwrap();
    assert_eq!(seen.app_bytes.len(), 9 + 200_000);
    assert!(!seen.handshake_failed);
}

#[test]
fn an_expired_certificate_fails_before_any_byte_is_sent() {
    let (port, server) = serve("expired", usize::MAX, 0);
    let result = connect(port, "expired.zoraya", "root.pem");
    assert_eq!(result.err(), Some(Error::Expired));
    let seen = server.join().unwrap();
    assert!(seen.app_bytes.is_empty());
}

#[test]
fn a_certificate_for_another_name_fails_before_any_byte_is_sent() {
    let (port, server) = serve("wrong", usize::MAX, 0);
    let result = connect(port, "wrong.zoraya", "root.pem");
    assert_eq!(result.err(), Some(Error::WrongName));
    assert!(server.join().unwrap().app_bytes.is_empty());
}

#[test]
fn a_root_that_did_not_sign_it_fails_before_any_byte_is_sent() {
    let (port, server) = serve("valid", usize::MAX, 0);
    let result = connect(port, "test.zoraya", "other-root.pem");
    assert_eq!(result.err(), Some(Error::UnknownIssuer));
    assert!(server.join().unwrap().app_bytes.is_empty());
}

/// The test root may sign only names under `.zoraya` (its nameConstraints):
/// `outside` is a validly dated leaf for `www.example.org`, signed by that
/// root with its own key, and the right name is asked for. The verifier
/// refuses it for the constraint alone, and the peer reads no application
/// byte. The date, the signature and the name are right, and `valid` shows
/// the same root vouching for a `.zoraya` name. (`tests/net/tls/make-outside.sh`;
/// ADR-0289 item 5.)
#[test]
fn a_name_outside_the_roots_constraints_fails_before_any_byte_is_sent() {
    let (port, server) = serve("outside", usize::MAX, 0);
    let result = connect(port, "www.example.org", "root.pem");
    let err = result.err().expect("the handshake must fail");
    assert!(
        matches!(&err, Error::Certificate(why) if why.contains("NameConstraintViolation")),
        "{err:?}"
    );
    assert!(server.join().unwrap().app_bytes.is_empty());
}

#[test]
fn no_date_means_no_handshake() {
    let (port, server) = serve("valid", usize::MAX, 0);
    let config = Config::new(&pem_certificates(&file("root.pem")), || None).unwrap();
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let result = Connection::connect(&config, "test.zoraya", Tcp(tcp), &SEED);
    assert_eq!(result.err(), Some(Error::NoClock));
    assert!(server.join().unwrap().app_bytes.is_empty());
}

#[test]
fn a_bad_name_and_no_roots_are_refused_up_front() {
    let (port, _server) = serve("valid", usize::MAX, 0);
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let _turn = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(
        Connection::connect(&config("root.pem"), "not a name!", Tcp(tcp), &SEED).err(),
        Some(Error::BadName)
    );
    assert_eq!(Config::new(&[], now).err(), Some(Error::NoRoots));
    assert_eq!(
        Config::new(&[std::vec![1, 2, 3]], now).err(),
        Some(Error::NoRoots)
    );
}

#[test]
fn pem_files_are_read_leniently_and_exactly() {
    let mut both = file("root.pem");
    both.extend_from_slice(b"\nnoise between\n");
    both.extend_from_slice(&file("other-root.pem"));
    both.extend_from_slice(b"-----BEGIN CERTIFICATE-----\n@@@@\n-----END CERTIFICATE-----\n");
    both.extend_from_slice(b"-----BEGIN CERTIFICATE-----\nAAAA");
    let found = pem_certificates(&both);
    assert_eq!(found.len(), 2);
    let direct: Vec<_> = CertificateDer::pem_slice_iter(&both)
        .filter_map(Result::ok)
        .collect();
    assert_eq!(found[0], direct[0].as_ref());
    assert_eq!(found[1], direct[1].as_ref());
    assert!(pem_certificates(b"").is_empty());
}
