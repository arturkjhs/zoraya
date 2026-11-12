# ADR-0289: What N5 decided where ADR-0228 is silent: ring for `aarch64-unknown-none`, the TLS crate, the roots and the test servers

- **Status:** proposed
- **Date:** 2026-11-12
- **Depends on:** ADR-0100, ADR-0109, ADR-0153, ADR-0166, ADR-0220, ADR-0222, ADR-0223, ADR-0224, ADR-0226, ADR-0227, ADR-0228, ADR-0230, ADR-0286, ADR-0288
- **Evidence:** CP-15, CP-27, DM-048, network/survey.md#rustls, network/survey.md#bearssl, network/survey.md#the-root-store-and-its-licence, network/survey.md#what-a-fetch-over-https-needs

## Context

ADR-0228's Decision made ring the first choice of crypto provider, built with the toolchain's clang, with BearSSL as
the fallback, and left the check to plan task N1. N1 reported that ring "builds for `aarch64-unknown-none`": it built
the Rust crates and ring's C objects. **Measured in N5, and it corrects that report:** the *link* of a program fails
on `ring_core_*_bn_mul4x_mont` and the other assembly routines, because ring's `build.rs` assembles its aarch64
assembly only for operating systems it lists (`linux`, the BSDs and so on) and `none` is not one; and the host's
`ar` makes an empty archive of ELF objects. Both are fixed below, and a program that handshakes with a TLS 1.3 server
of rustls on loopback, and with the test servers of ADR-0230 in QEMU, links and runs. **Assumed:** `fetch` is the only
HTTPS program of this stage; curl and BearSSL (the C half of ADR-0228) wait for a curl port.

The evidence this rests on, as ADR-0228 cites it: CP-15 (the program, not a shared component, holds the plaintext),
CP-27 (one TLS library per language), DM-048 (a small trusted base), `network/survey.md#bearssl` (the fallback's
shape), `network/survey.md#the-root-store-and-its-licence` (Mozilla's store as data) and
`network/survey.md#what-a-fetch-over-https-needs` (name, date, entropy and roots).

## Decision drivers

1. ADR-0228 is binding: TLS in each program, roots, date and entropy from the system, `netd` holds no TLS state
   (INV-0228-2).
2. A build needs no network after the first fetch of crates (ADR-0281 item 1), no libc sysroot beyond what the repository
   carries, and the host's own tools (`clang`, `ar` of the toolchain, `make`).
3. Anything the crypto does not check is a hole: no "dangerous" verifier, no skipped date (rustls refuses to
   verify without a time).
4. Names, not addresses, are what a certificate is for; the name verified is the name granted (ADR-0228 driver 2).

## Considered options

### Option A: ring, with a vendored copy that differs by one line (the recommendation)
`vendor/ring` is ring 0.17.14 without its tests, made by `tools/vendor-ring.sh` from the cargo cache: one line adds
`"none"` to the list of operating systems with the Linux ABI so that the pregenerated `*-linux64.S` files build for
the bare-metal target, and a `[lints]` table silences upstream's warnings. `[patch.crates-io]` in the workspace
`Cargo.toml` points at it. Cost: 8 MB in the tree and a diff of two lines to keep on a ring update.
### Option B: BearSSL, the recorded fallback, for native programs too
A C library (TLS 1.2 only, ADR-0228), a port with a shim, a Rust binding and our own certificate-path glue (BearSSL has
a minimal X.509 engine). Larger than A and a second verifier; not needed while A works.
### Option C: a crate that compiles ring's assembly itself from the cargo cache
No vendored copy, but a `build.rs` that finds another crate's directory through `cargo metadata`: fragile, and it
must repeat ring's flags. Rejected.
### Option D: another provider (graviola, rustls-rustcrypto)
New or alpha (`network/survey.md#rustls`); not tried.

## Recommendation

### Decision

**Decided by the architect (his instruction of the follow-up task `work/system-ro`, transcribed by an agent):**
`/system` is read-only at run time for everyone, the console shell included. `fsd` serves it read-only, and no grant
can give write to it (`+w /system` is refused, with a hint). It changes only when the image is built (later: an
explicit system-update path, its own ADR). Item 8 below is how this is built; it replaces the limitation that the
review of N5 recorded.

**Recommendation (not a decision): Option A,** with:

1. **The C compiler and archiver for ring** are set in `.cargo/config.toml` for `aarch64-unknown-none` only:
   `tools/ring-cc` (the host's `clang`, `-ffreestanding`, with `tools/ring-include`: `string.h`, `stdlib.h`,
   `assert.h`, declarations only; the functions come from `compiler_builtins`) and `target/debug/zoraya-ar` (the
   toolchain's archiver, ADR-0109 item 3), which `make` builds before the user programs and before clippy. The musl
   sysroot is not needed.
2. **`zoraya-tls`** (`crates/zoraya-tls`, `no_std` + `alloc`): `Config::new(roots, now)`, `Connection::connect(config,
   name, transport)` (the handshake and nothing else; the program cannot write before it succeeds, INV-0228-1),
   `read`, `write_all`, `close`, a PEM reader, `seed_random`/`fill_random` (ring's `getrandom`, `custom` feature, is
   this generator, seeded by the program from the random service) and a `TimeProvider` over a function the program
   gives (the wall clock; without a date the handshake fails with `NoClock`). It uses rustls 0.23.46's unbuffered
   API, TLS 1.2 and 1.3, `WebPkiServerVerifier`, no client certificates, no resumption.
3. **`fetch https://HOST[:PORT]/PATH`** (`zoraya-http` knows `tls` and port 443): the shell designates `HOST:443`
   as for `http://` and adds `/system/tls/roots` (read, "with the network") to the table of any native program that
   gets `reach`; `fetch` reads it, seeds `zoraya-tls` with 32 bytes of the program's random connection, takes the
   date from `ZNS1`, and verifies `HOST`. Messages: `certificate expired`, `certificate is for another name`,
   `certificate is not signed by a trusted root`, `TLS: ...`.
4. **The roots file** `rootfs/system/tls/roots`: the 108 certificates of Mozilla's included CA list for TLS server
   authentication from the CCADB report `IncludedCACertificateReportPEMCSV` (CCADB data, CDLA-Permissive-2.0, fetched
   2026-11-12), without roots that lack the Websites trust bit, have a "distrust for TLS after" date, or carry
   Mozilla applied constraints (the verifier does not apply constraints). Refreshing it is a manual step; its header
   says where it came from. `tls.roots` (ADR-0229) is not a setting yet: the path is a constant. The file is in the
   root volume and, like all of `/system`, read-only at run time for everyone (item 8).
5. **Test servers** (ADR-0230 item 4): `tools/test-httpd --tls-dir tests/net/tls` runs three TLS terminators (valid,
   expired, wrong name) in front of its plain server, on ports it reports, and logs whether any request byte came
   through; the guest reaches them as `HOST:PORT` through the `net.hosts` names. This replaces the plan's
   `guestfwd` on 443: the port is in the address, which `fetch` handles, and nothing else of SLIRP changes. The test PKI
   is committed (`tests/net/tls`, made by `make.sh` with OpenSSL 3; the expired certificate ended in 2020, the others
   end in 2126) and the disk of the test holds its root as `/system/tls/roots`.
6. **Not built:** the BearSSL port and curl (W12's curl half), `https` for the POSIX `zfetch`, the `tls.roots` setting,
   the `grants` hint for the roots line in the panel's shells, a certificate revocation check, session resumption,
   a TCG run of the HTTPS lines on this machine.
7. **The test roots' name constraints are tested.** `tests/net/tls/outside.pem` (`make-outside.sh`) is a leaf for
   `www.example.org`, dated and signed correctly by the test root, which may sign only names under `.zoraya`;
   `cargo test -p zoraya-tls` shows `connect` refusing it with `NameConstraintViolation` (webpki's) and the peer
   reading no application byte. A program that trusted a leaked copy of the test root would still accept no real site.
8. **`/system` is read-only at run time** (the Decision above). The service of a disk volume (`fsd`, not the
   temporary service) is told `Service::seal("system")`: the root's entry of that name and everything below it
   are sealed. On every connection to a sealed record the mutating rights (`WRITE`, `CREATE`, `REMOVE`, `RENAME`,
   `PUBLISH`) are never held, whatever the connection asked, and a clone, a narrowing, an `open` or a token cannot add
   them; `open` with a create, a truncate or a write right under the seal is refused before the engine runs. On the
   root, the entry itself cannot be removed, renamed (either side, exchange included), replaced by a publish, made
   again or given a limit. Reading, listing and `sync` are unchanged. The records know their place (outside, root,
   sealed) from the path they were opened by, so no path spelling reaches a writable record; a symbolic link is
   resolved by the client from the root again, through the same check. The shell refuses `+w` at or under `/system`
   before the command runs: `zosh: +w /system/tls: /system is read-only`, with the hint "the system image changes
   only when it is built; use +r to let a command read it". Writers that remain: the image build (`zoraya-mkfs
   --from rootfs`, on the host, before the volume is served) and nothing at run time; `init` and `fsd` write nothing
   there (first boot included), checked by reading both. The explicit system-update path that will change `/system`
   on a running system is a later ADR; until it exists a change to `/system` is a new image.

## Consequences
- **Positive.** HTTPS works for native programs with the real date and real entropy; the verifier is the audited
  rustls-webpki; `netd` sees ciphertext.
- **Negative.** 8 MB of vendored C and assembly; a second copy of ring's upgrade path; `fetch` is 1 MB; a handshake
  costs the time shown in `docs/design.md`; a certificate valid for a name behind `net.hosts` is checked against
  that name only.
- **Limitation (review), now closed.** N5's review found that a console shell, which holds the volume root, and a
  granted `fetch URL -o /system/tls/roots` could replace the file. The architect decided that `/system` is read-only
  for everyone (Decision, item 8); the limitation no longer holds. What remains: the file changes only with the
  image, so a root store update is a new image until the system-update path exists.
- **Follow-up.** BearSSL and curl; `getrandom` in `libposix`; the revocation question; the `tls.roots` setting.

## Invariants
- **INV-0289-1.** An expired certificate, a certificate for another name, a name outside the root's name constraints
  (`outside.pem`, item 7), a root that did not sign it and a missing date each fail in `connect`, and the peer has
  read no application byte. *Check:* `cargo test -p zoraya-tls`
  (`tests/handshake.rs`); `make test-net-fetch` (the server's log has, for the expired and the wrong-name server,
  exactly `ended after 0 request bytes`). Which path runs where: the host handshake tests get ring's randomness from
  the operating system, so the generator-backed `getrandom` path (`zoraya-tls`'s `fill_random`, seeded by
  `Connection::connect`'s 32 bytes) runs only in `make test-net-fetch`; `cargo test -p zoraya-tls --test unseeded`
  covers the generator on the host. A date counts only if the real-time clock gave it (the lowest bit of the realtime
  base, `zoraya-abi`'s `checked_wall_seconds`): without one `fetch` fails with "no date".
- **INV-0289-2.** `vendor/ring` equals the registry copy of 0.17.14 apart from `build.rs` (one line) and `Cargo.toml`
  (a lints table) and the missing tests. *Check:* `tools/vendor-ring.sh` regenerates it; `make check` (through `check-host-paths`) compares
  every tracked file of `vendor/ring` with the SHA-256 manifest `vendor/ring.sha256` (`tools/check-vendor-ring.sh`, no
  network). A later change to ring's C or assembly fails until the manifest is rewritten on purpose.
- **INV-0289-3.** `netd` has no TLS dependency (INV-0228-2). *Check:* `cargo tree -p zoraya-user --bin netd` shows
  neither `rustls` nor `ring` (`netd` is a bin of `zoraya-user`; cargo links by use, so the check is on the binary's
  symbols; not automated yet).
- **INV-0289-4.** `/system` is read-only on every connection of the root volume's service, the console shell's
  included: no write, create, remove, rename, publish or limit below it, none of its entry in the root, and no
  clone, narrowing, token or `+w` grant adds one. *Check:* `cargo test -p zoraya-fs-service --test service`
  (`a_sealed_entry_is_read_only_for_every_connection`); `cargo test -p zoraya-shell`
  (`a_write_grant_never_reaches_the_system_directory`); `make test-files` (`tests/shell/system-ro.script`: the
  console shell, a refused `+w /system/tls`, and a `+w /` grant fail to change `/system/tls/roots`, whose size and
  first line are as they were).

## Open questions
- Whether a vendored ring or an upstream change (a `none` OS in ring's table) is the long-term answer.
- Whether the roots file belongs in the volume or in the image (ADR-0228 says the image). `/system` is in the volume
  but only the image build writes it (item 8), which is what "in the image" needs for now.
- The system-update path that changes `/system` on a running system (its own ADR, the architect's instruction).

## Revisit triggers
- ring adds bare-metal aarch64 to its assembly table: drop the vendored copy.
- A curl port: BearSSL (item 6).

## Architect's decision

(Left empty for the architect.)
