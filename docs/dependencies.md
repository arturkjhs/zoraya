# External dependencies

Every external crate used by the kernel or by a `no_std` crate is listed here with its exact version, license,
purpose, and the reason it is not written in-house. Only permissively licensed (MIT, Apache-2.0, BSD, ISC, Zlib)
`no_std` crates are allowed. Adding one is a decision (see `CLAUDE.md`): record it here in the same change.

| Crate | Version | License | Used by | Purpose | Why not in-house |
|---|---|---|---|---|---|
| smoltcp | =0.14.0 | 0BSD | `zoraya-net` (`netd`) | The TCP/IP stack: Ethernet, ARP, IPv4, ICMP echo, UDP, TCP with Reno, the DHCPv4 client (ADR-0221 item 3; features `alloc`, `medium-ethernet`, `proto-ipv4`, `socket-dhcpv4`, `socket-icmp`, `socket-udp`, `socket-tcp`, `socket-tcp-reno`; no `std`, no `log`, no IPv6) | A TCP implementation with congestion control, a DHCP client and ARP is tens of thousands of lines whose bugs are remote-code-execution bugs; smoltcp is `no_std`, fuzzed upstream and the survey's choice (`docs/research/network/survey.md#smoltcp`). Proposed ADR-0281 records the decision |
| managed | 0.8.0 | 0BSD | smoltcp | Owned-or-borrowed slices and maps | smoltcp's own dependency |
| byteorder | 1.5.0 | Unlicense OR MIT | smoltcp | Big-endian integer access in the wire parsers | smoltcp's own dependency |
| heapless | 0.9.3 | MIT OR Apache-2.0 | smoltcp; `zoraya-net` tests | Fixed-capacity vectors in smoltcp's API (the DHCP configuration's DNS list) | smoltcp's own dependency |
| bitflags | 1.3.2 | MIT OR Apache-2.0 | smoltcp | Flag types | smoltcp's own dependency |
| cfg-if | 1.0.5 | MIT OR Apache-2.0 | smoltcp | Conditional compilation | smoltcp's own dependency |
| hash32, stable_deref_trait | 0.3.1, 1.2.1 | MIT OR Apache-2.0 | heapless | Hashing and a marker trait | heapless's own dependencies |

| rustls | =0.23.46 (`default-features = false`; `ring`, `tls12`; `std` only for tests and `tools/test-httpd`) | Apache-2.0 OR ISC OR MIT | `zoraya-tls` (`fetch`) | TLS 1.2 and 1.3 client with the unbuffered API, record layer and handshake (ADR-0228, proposed ADR-0289) | A TLS implementation is the largest hostile-input parser after TCP; rustls is `no_std` capable, memory safe and the survey's choice (`docs/research/network/survey.md#rustls`) |
| rustls-webpki | 0.103.16 | ISC | rustls | X.509 path validation, name and validity checks | rustls's own dependency |
| rustls-pki-types | 1.16.0 | MIT OR Apache-2.0 | rustls | DER and PEM types (`CertificateDer`, `ServerName`, `UnixTime`) | rustls's own dependency |
| ring | =0.17.14 (a copy in `vendor/ring`, `tools/vendor-ring.sh`) | Apache-2.0 AND ISC | rustls (the crypto provider) | AES-GCM, ChaCha20-Poly1305, ECDSA, RSA verify, X25519, SHA-2, HKDF; C and pregenerated aarch64 assembly built by the host's clang (ADR-0228 Decision, proposed ADR-0289) | Hand-written crypto is out of the question; ring is the provider ADR-0228 chose. The copy differs from the registry's by one line in `build.rs` (`"none"` added to the operating systems with the Linux ABI, so that its aarch64 assembly is built for `aarch64-unknown-none`) and a `[lints]` table; its tests are left out |
| untrusted | 0.9.0 | ISC | ring, rustls-webpki | Safe parsing of untrusted byte slices | their own dependency |
| subtle | 2.6.1 | BSD-3-Clause | rustls | Constant-time comparisons | rustls's own dependency |
| zeroize | 1.9.1 | Apache-2.0 OR MIT | rustls | Wiping secrets from memory | rustls's own dependency |
| once_cell | 1.21.4 | MIT OR Apache-2.0 | rustls | One-time initialisation | rustls's own dependency |
| getrandom | =0.2.17 (`custom`) | MIT OR Apache-2.0 | ring | The `custom` source is `zoraya_tls::fill_random`, a generator seeded from the random service (ADR-0227, ADR-0288) | ring's own dependency; the target has no operating system to ask |
| cc, shlex, find-msvc-tools | 1.11.1, 2.0.1, 0.1.15 | MIT OR Apache-2.0 | ring (build script only, host) | Compiles ring's C with `tools/ring-cc` | ring's own build dependencies |

Build tools for ring on the target: the host's `clang` (already needed by ADR-0109) and the toolchain's own `zoraya-ar`
(`.cargo/config.toml`); no Perl, no NASM and no libc sysroot (the pregenerated assembly is used, and `tools/ring-include`
holds three declaration-only headers).

## Host tools

| Tool | Version | Licence | Used by | Purpose |
|---|---|---|---|---|
| ncurses (`tic`, `infocmp`, and its test programs) | 6.6, `ports/sources.lock` (SHA-256 `355b4cbb...`) | X11 (MIT-style) | `tools/term-terminfo/terminfo.sh`, `tools/term-conformance/check.sh`, `tools/term-record/build-curses-tests.sh` | `tic` compiles the four terminfo entries of ADR-0333 (INV-0333-1), `infocmp` dumps the entry the terminal is tested against (INV-0332-1), the test programs are recorded to test the terminal (ADR-0332 item 10). Built for the host into `build/ports/host/`; nothing enters the image in part 10a. The port for the target is in the table of ports above (TASK-31 ports-early, built into `build/ports/curses`; in the image from part 10d, ADR-0334) |
| vttest | 20251205, `ports/sources.lock` (SHA-256 `cd6886f9...`) | MIT-style (Thomas E. Dickey's notice in `COPYING`) | `tools/term-record/build-vttest.sh` | The terminal test program whose cursor and screen menus are recorded and replayed (ADR-0332 item 10, ADR-0391 item 10). Built for the host into `build/ports/host/vttest`; nothing of it is in the image or a crate |
| musl's `wcwidth.c` | 1.2.5, `ports/sources.lock` | MIT | `tools/term-width/gen.sh` | Compiled for the host and asked for every code point, to write `crates/zoraya-width/src/table.rs` (ADR-0332 item 9) |
| Python 3 (standard library only) | the host's | PSF | `tools/term-record/record.py`, `compare-tmux.py` | Records a program on a pseudo-terminal; compares replayed grids with tmux. Run with `-I`; not part of `make test` |
| tmux | the host's | ISC | `tools/term-record/compare-tmux.py` | A second emulator for the text of the replayed grids (not part of `make test`) |

## Data

| File | Source | Licence | Used by | Purpose |
|---|---|---|---|---|
| `rootfs/system/tls/roots` | The CCADB report `IncludedCACertificateReportPEMCSV` (Mozilla's included CA certificates), fetched 2026-11-12; 108 certificates with the Websites trust bit, no distrust date and no applied constraints | CDLA-Permissive-2.0 (data only, ADR-0228 Decision; `CLAUDE.md`) | `fetch` (through the shell's roots entry) | The trust anchors for HTTPS |
| `tests/net/tls/*` | Made by `tests/net/tls/make.sh` with OpenSSL 3 | none (test keys, protect nothing) | `zoraya-tls` tests, `tools/test-httpd`, `make test-net-fetch` | A test root, a valid, an expired and a wrong-name leaf |

The kernel uses `core` and `alloc`, and user programs `core`. The crates under `crates/` use only `core` (and `alloc`:
`zoraya-fs`), and their tests use `std`, with seeded generators instead of a property-testing crate. `zoraya-fs-testkit`,
`zoraya-mkfs` and `zoraya-fsck` (TASK-07) use only `std` and the workspace's own crates. The GUI's crates (TASK-14: `zoraya-virtio-gpu`, `zoraya-virtio-input`, the four `zoraya-proto-*` of
the GUI, `zoraya-keymap`, `zoraya-tokens`, `zoraya-text`, `zoraya-fonts`, `zoraya-term`, `zoraya-compositor`) use only `core` and `alloc` and each
other. The GUI's text is `zoraya-text`, written for Zoraya (ADR-0162), with no rasteriser crate; its fonts are data,
in the table below. The host-side tools
(`zoraya-test-runner`, `zoraya-adr-check`, `zoraya-mkramdisk`, `zoraya-mutants`) use
only `std`, and the test runner also the workspace's `zoraya-text`, `zoraya-term` and `zoraya-compositor`, to read the
screen as the terminal draws it. `Cargo.lock` has registry entries since N2 (smoltcp and the crates above, with their checksums; `Cargo.lock` also lists
smoltcp's optional `defmt` crates, which no build compiles). The first build of a checkout downloads them from crates.io
(`cargo fetch`); after that no build needs network access. This is a deviation from the earlier rule that a build needs
only the toolchain: proposed ADR-0281 item 1 records it and the alternative (vendoring).

## Fonts

Fonts are a kind of dependency of their own (ADR-0162): data, not code, embedded in the programs that draw text. Each
file in `fonts/` is unmodified from its release (the OFL restricts modified versions, `desktop/survey.md#the-ofl-next-to-bsd-code`), ships with its licence text, and is listed here with its SHA-256; `zoraya-fonts`'s
host test hashes every file against this table (INV-0162-5). The releases are the archives `tools/mockups/fetch-fonts.sh`
downloads and checks (Inter 4.1, SHA-256 `9883fdd4a49d4fb66bd8177ba6625ef9a64aa45899767dde3d36aa425756b11e`, and
JetBrains Mono 2.304, `6f6376c6ed2960ea8a963cd7387ec9d76e3f629125bc33d1fdcd7eb7012f7bbf`), from which the files below
were extracted on 2026-11-08 (TASK-24, 7a).

Since TASK-27 9f-text (ADR-0326) the same files, unmodified and with their licence texts, are also copied into the
volume's `/system/fonts` by `tools/run-disk.sh` and by `make test-netsurf`, and NetSurf (GPL-2.0) reads them there at run
time; nothing of a font is compiled or linked into `netsurf` or `libc.a`. No font was added. Whether the licence rule of
`CLAUDE.md` names OFL-1.1 for such unmodified data files is proposed ADR-0327.

| File | Release | Licence | Used by | Purpose | SHA-256 |
|---|---|---|---|---|---|
| `fonts/Inter-Regular.ttf` | Inter 4.1, `extras/ttf/` | OFL-1.1 | `zoraya-fonts` | the UI font (`font.ui`) | `40d692fce188e4471e2b3cba937be967878f631ad3ebbbdcd587687c7ebe0c82` |
| `fonts/Inter-Medium.ttf` | Inter 4.1, `extras/ttf/` | OFL-1.1 | `zoraya-fonts` | titles and headings (`font.ui.medium`) | `97ad806f526e41546d46365bb3a393145f75b7b1568913db74549ad8b8dba872` |
| `fonts/JetBrainsMono-Regular.ttf` | JetBrains Mono 2.304, `fonts/ttf/` | OFL-1.1 | `zoraya-fonts` | the terminal (`font.mono`) | `a0bf60ef0f83c5ed4d7a75d45838548b1f6873372dfac88f71804491898d138f` |
| `fonts/JetBrainsMono-Bold.ttf` | JetBrains Mono 2.304, `fonts/ttf/` | OFL-1.1 | `zoraya-fonts` | bold in the terminal (`font.mono.bold`) | `5590990c82e097397517f275f430af4546e1c45cff408bde4255dad142479dcb` |
| `fonts/Inter-LICENSE.txt` | Inter 4.1, `LICENSE.txt` | OFL-1.1 | | Inter's licence | `262481e844521b326f5ecd053e59b98c8b2da78c8ee1bdbb6e8174305e54935a` |
| `fonts/JetBrainsMono-OFL.txt` | JetBrains Mono 2.304, `OFL.txt` | OFL-1.1 | | JetBrains Mono's licence | `30f0c136e3c88e422d0791acd97238870f9054a9729bc34cf2ff0d4ed8cac4ad` |

## Fuzzing (outside the workspace)

`fuzz/` is a separate cargo project (its own `[workspace]`, excluded from the main one) for coverage-guided fuzzing
of `zoraya-fs` with cargo-fuzz (TASK-07). It needs a nightly toolchain and network access to fetch its crates, so
`make test` does not build it; the workspace runs a deterministic fuzz test on the pinned toolchain instead. Its
versions are pinned in `fuzz/Cargo.lock`.

| Crate | Version | License | Used by | Purpose | Why not in-house |
|---|---|---|---|---|---|
| `libfuzzer-sys` | 0.4.10 | (MIT OR Apache-2.0) AND NCSA (the bundled LLVM libFuzzer sources) | `fuzz/` only (host) | links LLVM's libFuzzer, the coverage-guided fuzzing engine cargo-fuzz drives | a coverage-guided fuzzer is compiler instrumentation plus an engine; not a small thing |

`libfuzzer-sys` brings in `arbitrary` 1.5.0, and to build libFuzzer `cc` 1.10.0, `jobserver` 0.1.35, `libc`
0.2.191, `shlex` 2.0.1, `cfg-if` 1.0.5, `getrandom` 0.4.3 and `find-msvc-tools` 0.1.15, each MIT OR Apache-2.0 per
its `Cargo.toml`; `r-efi` 6.0.0 is in the lock file for UEFI targets only and is not built on Linux or macOS.

## Toolchain and host tools

These are not crates, but the build depends on them:

| Tool | Version used | Where it comes from |
|---|---|---|
| Rust | 1.99.1 (stable), pinned in `rust-toolchain.toml` with `rustfmt`, `clippy` and the `aarch64-unknown-none` target | rustup |
| QEMU | 11.1.2 (`qemu-system-aarch64`) | Homebrew on macOS; distribution package on Linux |
| GNU make, POSIX `sh` | the host's | macOS Command Line Tools / distribution |
| `cargo-mutants` (only for `make mutants-host`) | 27.1.0, MIT, <https://github.com/sourcefrog/cargo-mutants> | `cargo install cargo-mutants --version 27.1.0 --locked` |
| Rust nightly and `cargo-fuzz` (only for `fuzz/`) | nightly 2026-10-29 (`rustc 1.101.0-nightly`), cargo-fuzz 0.13.2 | rustup, `cargo install cargo-fuzz` |
| Rust nightly for `tools/run-fuzz.sh` (`make test-fuzz`, ADR-0298 item 3; accepted 2026-11-13) | `nightly-2026-11-12` (`rustc 1.102.0-nightly`, 3ef0c22cd); cargo-fuzz is not needed | rustup |
| rustup | 1.29.3 (accepted 2026-11-13; it was 1.29.2 before installing the nightly) | the host's installer |

`dtc` (Device Tree Compiler) is useful for inspecting the fixtures but is not needed by any build or test.

### The cross toolchain (stage 6b, ADR-0109 item 10)

Needed by `make fetch-ports` and `make ports` only; `make test` needs none of them except the POSIX tools it already
uses (its `check-fetch-ports` step runs `curl` on a local `file://` URL and `shasum` or `sha256sum`). `make ports`
prints the clang it uses.

| Tool | Version used | Where it comes from |
|---|---|---|
| clang, any version that targets `aarch64-unknown-none-elf` and has `--ld-path` (`tools/zcc`) | Apple clang 21.0.0 (clang-2100.0.123.102) on the M4; on Linux CI the runner's `clang` (unverified here) | Xcode Command Line Tools / distribution |
| `rust-lld` (through the toolchain's `gcc-ld/ld.lld`) | LLD 23.1.1, from Rust 1.99.1 | rustup, with the pinned toolchain; nothing to add |
| `core`, `alloc` and `compiler_builtins` for `aarch64-unknown-none` (in `libc.a`, with `zoraya-posix`, ADR-0100) | from Rust 1.99.1 | the pinned toolchain's target; not a crate from a registry |
| the host's C compiler (`cc`): dash's helpers, tcc's `c2str` and the host tcc of the link check | Apple clang 21.0.0 on the M4 | Xcode Command Line Tools / distribution |
| `curl`, `shasum` or `sha256sum` (`make fetch-ports`) | curl 8.7.1 | macOS / distribution |
| `patch`, `tar` with gzip and bzip2, `unzip` (SQLite's archive), `awk`, `sed` | the host's | macOS / distribution |
| bison (host only) | the pinned GNU Bison 3.8.2 of `ports/sources.lock`, built into `build/ports/host` (the GNU bison row below) | `make awk` (ADR-0336 item 3): generates one-true-awk's parser from `awkgram.y`, the same on every host (its version is the first line of `awk-grammar.log`); not part of the image or any crate |
| GNU sed, as `gsed` or `sed` (toybox's build scripts) | GNU sed 4.10 (Homebrew `gnu-sed`) on the M4 | `brew install gnu-sed` on macOS; the default `sed` on Linux |

`rust-objcopy`, which ADR-0109 item 10 lists, is not used: it needs the `llvm-tools` component, which the minimal
profile of `rust-toolchain.toml` does not install, so `posix-surface-check` renames symbols itself (ADR-0141).

## Ported programs (third-party C, stage 6b)

Not crates: C sources built by `ports/Makefile` into `build/sysroot` and `build/rootfs-posix` (and, since TASK-27 9a, `build/ports/deps` and `build/rootfs-apps`), fetched by
`make fetch-ports` and never committed. Versions and hashes are in `ports/sources.lock`, licences and patches in
`ports/README.md`. Why they are not written in-house: ADR-0100 (musl's pure C) and ADR-0005 (the programs are the
measured T1 workloads).

| Program | Version | Licence | Used for |
|---|---|---|---|
| musl | 1.2.5 | MIT (a few files public domain or under other permissive terms, its `COPYRIGHT`) | the retained C of `libc.a` and the sysroot's headers (ADR-0100) |
| dash | 0.5.13.5 | BSD-3-Clause | `/bin/sh` |
| GNU make | 4.4.1 | GPL-3.0-or-later | `/bin/make` |
| toybox | 0.8.15 | 0BSD | the commands of `ports/toybox-applets.txt` (ADR-0336), in `/usr/libexec/toybox` with links in `/bin` |
| one-true-awk | `72e9b56caddb4a6f8d1405315a5183c26b8650a4` (second edition) | Lucent Technologies permission notice (permissive; ADR-0336 item 3, the architect is asked to confirm) | `/bin/awk`; no other awk is permissive and complete (stage-10/survey.md#awk) |
| TinyCC | `e23dabed154a8495f3ac1ff3cc9d04fa1c207893` | LGPL (`COPYING` 2.1, the LGPL files version 2 or later; some files permissive) | `/bin/tcc`, `libtcc1.a` |
| pdpmake | `0faaf258c9b3eaa9f5e4f6842e2484cbea6f23f8` (2.0.4) | public domain (the Unlicense) | `/bin/pdpmake`, TASK-02's comparison make for W2 and W4 (ADR-0110) |
| Lua | 5.5.1 | MIT | W2's source, in the image only, built inside Zoraya |
| SQLite | 3540100 | public domain | W3's source, in the image only, built inside Zoraya |
| libc-test | `63e36e934e8f2bedf645303a90b64098e2af6c8f` | MIT | the thread tests of musl's conformance suite, built by `make test-posix-suites` (ADR-0309); a test input, never in the image or `libc.a`; pinned by commit, fetched with git from repo.or.cz (its snapshot URLs sit behind a bot check and their gzip bytes differ by machine), and its lock hash is of the gunzipped tar (`ports/sources.lock`, `tools/fetch-ports.sh`) |
| SQLite source tree | 3540100 | public domain | `test/threadtest3.c` (ADR-0309 item 2); a test input |
| GNU hello | 2.12.3 | GPL-3.0-or-later | W4's source, in the image only, built inside Zoraya |
| OpenSSL | 3.5.9 (LTS) | Apache-2.0 | `libssl.a` and `libcrypto.a` for curl and wget, static, with threads (TASK-27 9a, ADR-0310, ADR-0312). Why not in-house: a TLS 1.3 client and a certificate verifier are not small things to write; the native side's `zoraya-tls` is rustls |
| zlib | 1.3.1 | zlib | `libz.a` for curl and wget (TASK-27 9a) |
| curl | 8.23.0 | curl (MIT-like, its `COPYING`) | `/bin/curl` on the volume of `make run` and the apps' tests (TASK-27 9a) |
| GNU wget | 1.25.0 | GPL-3.0-or-later | `/bin/wget`, same (TASK-27 9a) |
| ncurses | 6.6 | X11 (MIT-style) | `libncursesw.a` and `libtinfow.a` for the target, static, wide-character, no database compiled in, no lookup in the home directory (ADR-0334; TASK-31 ports-early; `make ncurses`); the host's `tic` is a host tool (below) |
| less | 710 | the Less License (BSD-2-clause-style; also offered under GPL-3.0-or-later, not used) | `/bin/less` (ADR-0335 item 8; built by TASK-31 ports-early, in the image since part 10d) |
| nvi2 | 2.2.2 | BSD-3-Clause | `/bin/vi`, `ex`, `view` (ADR-0335 item 1; built by TASK-31 ports-early, in the image since part 10d); its CMake build is replaced by `ports/nvi/build.sh` |
| Berkeley DB (DB 1) | 1.85 | the 4-clause BSD licence of the University of California (the advertising clause is in the files; UC rescinded it in 1999, so the licence reads as 3-clause BSD, accepted by the architect on 2026-11-16 (ADR-0390); the program does not advertise) | `libdb1.a`, nvi's buffer (its `recno` access method; ADR-0335 item 2). The reason it is not written in-house: nvi's buffer is `dbopen`'s record file. A maintained copy (the BSDs' libc) is an open question of ADR-0390 |
| OpenSSH portable | 10.6p2 | BSD and ISC (its `LICENCE`) | `ssh` and `ssh-keygen` only, on the image's OpenSSL and zlib (ADR-0340, ADR-0346; built by TASK-31 ports-early, in the image from part 10g) |
| git | 2.56.1 | GPL-2.0-only; a separate program in the image (`git` and `git-remote-http`). The licence question (`git-remote-http` linking libcurl and Apache-2.0 OpenSSL, which GPL-2.0-only code cannot combine with) is decided by the architect on 2026-11-16 (ADR-0461, proposed): `git-remote-http` links a libcurl built with mbedTLS (below). Until ADR-0461 is accepted and 10h builds that transport, git is built and not in any image | `/bin/git` and `git-remote-http` (ADR-0339, ADR-0347; built by TASK-31 ports-early with the library of the pinned curl) |
| mbedTLS | 3.6 LTS (the exact version and SHA-256 are pinned by the part that builds it, 10h's part; not yet in `ports/sources.lock`) | Apache-2.0 OR GPL-2.0-or-later; the GPL-2.0-or-later option is used, so the combination with git is GPL-2.0-only | the TLS backend of the libcurl that `git-remote-http` links, for git only; certificate verification on, roots from `/system/tls/roots`, no insecure options (ADR-0461, proposed). Why not in-house, and why not OpenSSL: OpenSSL 3 is Apache-2.0, which GPL-2.0-only code cannot link. The rest of the system keeps OpenSSL (ADR-0310, ADR-0312) and rustls (ADR-0228) |
| libcurl (second build, with mbedTLS) | the curl port's version (8.23.0) | curl (MIT-like, its `COPYING`) | `libcurl.a` linked by `git-remote-http` only (ADR-0461, proposed); the curl port's own `/bin/curl` stays on OpenSSL |
| NetSurf (netsurf-all) | 3.11 | GPL-2.0-only with the additional permission to link with OpenSSL (COPYING lines 8-15) (the browser), MIT (its libraries `libnsfb`, `libcss`, `libdom`, `libhubbub`, `libparserutils`, `libwapcaplet`, `libnsbmp`, `libnsgif`, `libnslog`, `libnspsl`, `libnsutils`, `libutf8proc`), as ADR-0109 shipped GPL ports | `/bin/netsurf` (TASK-27 9d, proposed ADR-0317, ADR-0318); built by `make netsurf` (no JavaScript, no SVG) |
| libpng | 1.6.50 | libpng licence (permissive) | PNG images for NetSurf (9d) |
| IJG libjpeg | 9f | IJG licence (permissive) | JPEG images for NetSurf (9d) |
| LLVM (libc++, libc++abi, libunwind; LLVM libc's `shared/` headers) | 20.1.8 | Apache-2.0 WITH LLVM-exception | the C++ runtime every C++ program links, static, built by `zc++` into `build/sysroot` (TASK-30, proposed ADR-0360, ADR-0361); libc++'s test suite is a test input (`make test-cxx-libcxx`). Why not in-house: a C++ standard library and unwinder are not small; libc++ is clang's own and runs on musl |
| GoogleTest | 1.18.0 | BSD-3-Clause | a test input only: built by CMake with `ports/cmake/zoraya.cmake`, its own tests run by `make test-cxx-gtest` (TASK-30, proposed ADR-0367); never in the image |
| pkg-config | 0.29.2 | GPL-2.0 | host tool only, built into `build/ports/host` when the host has none; never linked (9d) |
| GNU bison | 3.8.2 | GPL-3.0-or-later (with the Bison exception) | host tool only, built into `build/ports/host`: for NetSurf when the host's is older than 3 (macOS ships 2.3; libnslog's grammar; 9d), and always for one-true-awk's grammar (10e); never linked. The host also needs perl, gperf and flex (CI image: `.github/ci/Dockerfile`) |

### Mutation testing

`make mutants` runs `tools/mutants` (`zoraya-mutants`, `std` only): hand-written mutations, each with the tests that
should catch it, including the in-QEMU ones. `make mutants-host CRATE=name` runs `cargo-mutants` on one host crate.
It is a separate program on the host, not a dependency: nothing in the workspace links it, `Cargo.lock` does not list
it, and `make test` does not run it. It makes its mutants from the code's syntax (operators, return values, deleted
statements), which a hand-written list cannot do at that scale: 168 mutants of `zoraya-virtio` in 53 s on an M4,
25 missed, when evaluated on 2026-11-02. It only runs `cargo test`, so it cannot reach the kernel, the user programs or
QEMU; those stay with `tools/mutants`. Many of its misses are equivalent mutants, for example a layout constant the
fake device reads from the same expression, so its `missed.txt` goes to the `mutant-triage` subagent, not straight
into new tests. Installing it needs network access once.

