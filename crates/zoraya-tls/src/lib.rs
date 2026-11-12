//! TLS for native programs (ADR-0228, ADR-0226, ADR-0227): a blocking client
//! over rustls 0.23's unbuffered API, with ring as the crypto provider, no
//! `std`, and three things the system supplies:
//!
//! - **the date**: a `TimeProvider` over a function the program gives (the
//!   wall clock of ADR-0226); without a date the handshake fails (rustls
//!   cannot check a validity period), never skips the check;
//! - **randomness**: ring asks `getrandom` for bytes, which this crate answers
//!   from a [`zoraya_rng::Generator`] the program seeds with bytes from the
//!   random service ([`seed_random`]); unseeded, a request fails and so
//!   does the handshake;
//! - **the roots**: DER certificates the program read from the roots file
//!   ([`pem_certificates`] reads a PEM file); the name verified is the one
//!   given to [`Connection::connect`].
//!
//! A connection whose certificate does not match the name, is expired
//! against the clock or chains to no root fails in `connect`, before any
//! application byte is sent (INV-0228-1): the handshake is the only thing
//! `connect` does, and `write` is not reachable without it.

#![no_std]
#![forbid(unsafe_op_in_unsafe_fn)]

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cell::UnsafeCell;
use core::fmt;
use core::sync::atomic::{AtomicBool, Ordering};

use rustls::client::{UnbufferedClientConnection, WebPkiServerVerifier};
use rustls::crypto::ring as ring_provider;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::time_provider::TimeProvider;
use rustls::unbuffered::{ConnectionState, EncodeError, EncryptError, UnbufferedStatus};
use rustls::{CertificateError, ClientConfig, Error as RustlsError, RootCertStore};
use zoraya_rng::Generator;

mod pem;

pub use pem::pem_certificates;

// ---- Randomness ---------------------------------------------------------------

/// The program's generator, behind a flag: native programs have one thread
/// (ADR-0080's heap says the same), and the flag makes a second user see
/// "busy" rather than a data race.
struct Source {
    busy: AtomicBool,
    generator: UnsafeCell<Option<Generator>>,
}

// SAFETY: access to `generator` is only through `with`, which holds `busy`.
unsafe impl Sync for Source {}

static SOURCE: Source = Source {
    busy: AtomicBool::new(false),
    generator: UnsafeCell::new(None),
};

fn with<R>(f: impl FnOnce(&mut Option<Generator>) -> R) -> Option<R> {
    if SOURCE.busy.swap(true, Ordering::Acquire) {
        return None;
    }
    // SAFETY: `busy` was false and is now ours until the store below, so no
    // other reference to the cell's content exists.
    let result = f(unsafe { &mut *SOURCE.generator.get() });
    SOURCE.busy.store(false, Ordering::Release);
    Some(result)
}

/// Seeds (or reseeds) the generator with bytes from the random service
/// (ADR-0227 item 6). Refuses fewer than 32 bytes with [`Error::BadSeed`],
/// and a first seed of all zeros. [`Connection::connect`] takes its own 32
/// bytes and calls this, so two connections never continue one stream.
pub fn seed_random(bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() < zoraya_rng::MIN_SEED {
        return Err(Error::BadSeed);
    }
    let done = with(|g| match g {
        Some(g) => {
            g.mix(bytes);
            Ok(())
        }
        None => {
            *g = Some(Generator::from_seed(bytes).map_err(|_| Error::BadSeed)?);
            Ok(())
        }
    });
    done.unwrap_or(Err(Error::NoRandom))
}

/// True once [`seed_random`] has been called.
pub fn is_seeded() -> bool {
    with(|g| g.is_some()).unwrap_or(false)
}

/// Fills `buf` from the program's generator: what ring's `getrandom` is on
/// this target (on a host with an operating system `getrandom` has its own
/// source and this is only used by tests). Fails if the generator was never
/// seeded, so a program with no random service cannot start a handshake.
pub fn fill_random(buf: &mut [u8]) -> Result<(), Error> {
    match with(|g| g.as_mut().map(|g| g.fill(buf))) {
        Some(Some(())) => Ok(()),
        _ => Err(Error::NoRandom),
    }
}

fn custom_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
    fill_random(buf).map_err(|_| getrandom::Error::UNSUPPORTED)
}

getrandom::register_custom_getrandom!(custom_getrandom);

// ---- Time ---------------------------------------------------------------------

/// The clock rustls checks validity periods against.
struct Clock(fn() -> Option<u64>);

impl fmt::Debug for Clock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Clock")
    }
}

impl TimeProvider for Clock {
    fn current_time(&self) -> Option<UnixTime> {
        (self.0)().map(|secs| UnixTime::since_unix_epoch(core::time::Duration::from_secs(secs)))
    }
}

// ---- Errors -------------------------------------------------------------------

/// What a transport could not do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransportError(pub &'static str);

/// Why a TLS operation failed. [`fmt::Display`] says it for a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// The roots, or no root was usable.
    NoRoots,
    /// The server name is not a DNS name or an address.
    BadName,
    /// The system has no date, so a validity period cannot be checked.
    NoClock,
    /// The random service was not asked for bytes (see [`seed_random`]).
    NoRandom,
    /// The seed for the generator was under 32 bytes, or all zero.
    BadSeed,
    /// The server's certificate has expired.
    Expired,
    /// The server's certificate is not yet valid.
    NotYetValid,
    /// The certificate is not for the name asked for.
    WrongName,
    /// The certificate chains to no root in the roots file.
    UnknownIssuer,
    /// Another certificate fault, with rustls's words.
    Certificate(String),
    /// The peer broke the protocol, or sent an alert.
    Protocol(String),
    /// The transport failed, or the peer closed before the handshake ended.
    Transport(&'static str),
    /// The peer closed the connection without a close_notify.
    Truncated,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoRoots => f.write_str("no trusted roots"),
            Error::BadName => f.write_str("the name is not one a certificate can be for"),
            Error::NoClock => {
                f.write_str("the system has no date to check the certificate against")
            }
            Error::NoRandom => f.write_str("no random bytes for the handshake"),
            Error::BadSeed => f.write_str("the random seed is too short or all zero"),
            Error::Expired => f.write_str("certificate expired"),
            Error::NotYetValid => f.write_str("certificate not yet valid"),
            Error::WrongName => f.write_str("certificate is for another name"),
            Error::UnknownIssuer => f.write_str("certificate is not signed by a trusted root"),
            Error::Certificate(why) => write!(f, "certificate: {why}"),
            Error::Protocol(why) => write!(f, "TLS: {why}"),
            Error::Transport(why) => write!(f, "connection: {why}"),
            Error::Truncated => f.write_str("the connection ended without a close_notify"),
        }
    }
}

fn map(e: RustlsError) -> Error {
    match e {
        RustlsError::InvalidCertificate(c) => match c {
            CertificateError::Expired | CertificateError::ExpiredContext { .. } => Error::Expired,
            CertificateError::NotValidYet | CertificateError::NotValidYetContext { .. } => {
                Error::NotYetValid
            }
            CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. } => {
                Error::WrongName
            }
            CertificateError::UnknownIssuer => Error::UnknownIssuer,
            other => Error::Certificate(alloc::format!("{other:?}")),
        },
        RustlsError::FailedToGetCurrentTime => Error::NoClock,
        RustlsError::FailedToGetRandomBytes => Error::NoRandom,
        other => Error::Protocol(other.to_string()),
    }
}

// ---- Configuration ------------------------------------------------------------

/// What every connection of a program shares: the roots and the clock.
#[derive(Clone)]
pub struct Config(Arc<ClientConfig>);

impl Config {
    /// `roots` are DER certificates; `now` gives the wall clock in seconds
    /// since the epoch, or `None` if the system has no date. At least one
    /// root must be usable.
    pub fn new(roots: &[Vec<u8>], now: fn() -> Option<u64>) -> Result<Config, Error> {
        let mut store = RootCertStore::empty();
        let (added, _) = store
            .add_parsable_certificates(roots.iter().map(|d| CertificateDer::from(d.as_slice())));
        if added == 0 {
            return Err(Error::NoRoots);
        }
        let provider = Arc::new(ring_provider::default_provider());
        let verifier =
            WebPkiServerVerifier::builder_with_provider(Arc::new(store), provider.clone())
                .build()
                .map_err(|e| Error::Certificate(alloc::format!("{e:?}")))?;
        let config = ClientConfig::builder_with_details(provider, Arc::new(Clock(now)))
            .with_safe_default_protocol_versions()
            .map_err(map)?
            .with_webpki_verifier(verifier)
            .with_no_client_auth();
        Ok(Config(Arc::new(config)))
    }
}

// ---- The connection ------------------------------------------------------------

/// A byte transport under TLS: a stream pair, a socket.
pub trait Transport {
    /// Reads at least one byte, or 0 at the end of the stream.
    fn read(&mut self, buffer: &mut [u8]) -> Result<usize, TransportError>;
    /// Writes all of `data`.
    fn write_all(&mut self, data: &[u8]) -> Result<(), TransportError>;
}

/// The most ciphertext kept waiting: a record is at most 16,645 bytes.
const INCOMING_LIMIT: usize = 3 * 16_384;
/// The most plaintext one `encrypt` call takes.
const WRITE_CHUNK: usize = 16_000;

enum Job<'a> {
    Handshake,
    Read,
    Write(&'a [u8]),
    Close,
}

/// A TLS client connection, after its handshake.
pub struct Connection<T: Transport> {
    conn: UnbufferedClientConnection,
    io: T,
    incoming: Vec<u8>,
    plain: Vec<u8>,
    plain_at: usize,
    peer_closed: bool,
    sent_close: bool,
}

enum Next {
    More,
    Done,
    NeedData,
}

fn transport(e: TransportError) -> Error {
    Error::Transport(e.0)
}

impl<T: Transport> Connection<T> {
    /// Starts a connection to `name` over `io` and runs its handshake.
    /// Nothing the program sends goes out before it has succeeded. `seed` is
    /// 32 fresh bytes from the random service, mixed into the program's
    /// generator before anything is drawn from it, so that no two
    /// connections (or a program and its forked child) continue one stream.
    pub fn connect(
        config: &Config,
        name: &str,
        io: T,
        seed: &[u8; 32],
    ) -> Result<Connection<T>, Error> {
        seed_random(seed)?;
        let server_name = ServerName::try_from(name.to_string()).map_err(|_| Error::BadName)?;
        let conn = UnbufferedClientConnection::new(config.0.clone(), server_name).map_err(map)?;
        let mut c = Connection {
            conn,
            io,
            incoming: Vec::new(),
            plain: Vec::new(),
            plain_at: 0,
            peer_closed: false,
            sent_close: false,
        };
        c.pump(Job::Handshake)?;
        Ok(c)
    }

    /// Reads decrypted bytes: at least one, or 0 when the peer closed with a
    /// close_notify.
    pub fn read(&mut self, out: &mut [u8]) -> Result<usize, Error> {
        if out.is_empty() {
            return Ok(0);
        }
        while self.plain_at == self.plain.len() {
            if self.peer_closed {
                return Ok(0);
            }
            self.pump(Job::Read)?;
        }
        let n = out.len().min(self.plain.len() - self.plain_at);
        out[..n].copy_from_slice(&self.plain[self.plain_at..self.plain_at + n]);
        self.plain_at += n;
        if self.plain_at == self.plain.len() {
            self.plain.clear();
            self.plain_at = 0;
        }
        Ok(n)
    }

    /// Encrypts and sends all of `data`.
    pub fn write_all(&mut self, data: &[u8]) -> Result<(), Error> {
        for chunk in data.chunks(WRITE_CHUNK) {
            self.pump(Job::Write(chunk))?;
        }
        Ok(())
    }

    /// Sends a close_notify and gives the transport back.
    pub fn close(mut self) -> T {
        if !self.sent_close {
            let _ = self.pump(Job::Close);
        }
        self.io
    }

    /// Runs the state machine until `job` is done.
    fn pump(&mut self, job: Job<'_>) -> Result<(), Error> {
        let mut job = job;
        let mut outgoing: Vec<u8> = Vec::new();
        loop {
            let UnbufferedStatus { mut discard, state } =
                self.conn.process_tls_records(&mut self.incoming);
            let next = match state.map_err(map)? {
                ConnectionState::EncodeTlsData(mut s) => {
                    let start = outgoing.len();
                    let mut room = 512;
                    loop {
                        outgoing.resize(start + room, 0);
                        match s.encode(&mut outgoing[start..]) {
                            Ok(n) => {
                                outgoing.truncate(start + n);
                                break;
                            }
                            Err(EncodeError::InsufficientSize(e)) => room = e.required_size,
                            Err(EncodeError::AlreadyEncoded) => {
                                outgoing.truncate(start);
                                break;
                            }
                        }
                    }
                    Next::More
                }
                ConnectionState::TransmitTlsData(s) => {
                    self.io.write_all(&outgoing).map_err(transport)?;
                    outgoing.clear();
                    s.done();
                    Next::More
                }
                ConnectionState::BlockedHandshake => Next::NeedData,
                ConnectionState::ReadTraffic(mut s) => {
                    while let Some(record) = s.next_record() {
                        let record = record.map_err(map)?;
                        self.plain.extend_from_slice(record.payload);
                        discard += record.discard;
                    }
                    match job {
                        Job::Read => Next::Done,
                        _ => Next::More,
                    }
                }
                ConnectionState::WriteTraffic(mut s) => match &mut job {
                    Job::Handshake => Next::Done,
                    Job::Read => Next::NeedData,
                    Job::Write(data) => {
                        let mut room = data.len() + 64;
                        loop {
                            outgoing.resize(room, 0);
                            match s.encrypt(data, &mut outgoing) {
                                Ok(n) => {
                                    outgoing.truncate(n);
                                    break;
                                }
                                Err(EncryptError::InsufficientSize(e)) => room = e.required_size,
                                Err(EncryptError::EncryptExhausted) => {
                                    return Err(Error::Protocol(String::from(
                                        "the connection's keys are used up",
                                    )));
                                }
                            }
                        }
                        self.io.write_all(&outgoing).map_err(transport)?;
                        outgoing.clear();
                        *data = &[];
                        Next::Done
                    }
                    Job::Close => {
                        outgoing.resize(256, 0);
                        if let Ok(n) = s.queue_close_notify(&mut outgoing) {
                            outgoing.truncate(n);
                            self.io.write_all(&outgoing).map_err(transport)?;
                        }
                        outgoing.clear();
                        self.sent_close = true;
                        Next::Done
                    }
                },
                ConnectionState::PeerClosed => {
                    self.peer_closed = true;
                    match job {
                        Job::Read | Job::Close => Next::Done,
                        _ => Next::More,
                    }
                }
                ConnectionState::Closed => {
                    self.peer_closed = true;
                    Next::Done
                }
                _ => {
                    return Err(Error::Protocol(String::from(
                        "an unexpected connection state",
                    )));
                }
            };
            self.incoming.drain(..discard);
            match next {
                Next::More => {}
                Next::Done => return Ok(()),
                Next::NeedData => {
                    if self.peer_closed {
                        return Err(Error::Truncated);
                    }
                    if self.incoming.len() >= INCOMING_LIMIT {
                        return Err(Error::Protocol(String::from("a record is too large")));
                    }
                    let mut buffer = [0u8; 4096];
                    let n = self.io.read(&mut buffer).map_err(transport)?;
                    if n == 0 {
                        return Err(Error::Truncated);
                    }
                    self.incoming.extend_from_slice(&buffer[..n]);
                }
            }
        }
    }
}
