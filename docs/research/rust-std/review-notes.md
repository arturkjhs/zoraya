# Independent review of ADR-0450 to ADR-0455 (TASK-35)

An `opus-review` subagent reviewed the six proposed ADRs, the survey, `rows-to-add.md` and the probes on 2026-11-15, at
commit `d9f31d72` of `adr/rust-std`. It read the code, the ADRs, Rust 1.99.1's `std` source and the probe logs, ran only
`zoraya-adr-check`, `nm` and `grep`, and changed nothing. Its findings, condensed, with what was done about each.

**What it confirmed.** Every measured number it sampled matches the `rprobe:` lines of the logs (the condvar's 37.47 s
and 10.04 ms, 3,152,322 spurious wake-ups, 1.667 µs and 110 ms, 4294967295, 18446744073709551615, `ENOTTY`, `EACCES`,
exit 101, signal 6, the data abort, the `/proc/self/cgroup` hint, the child's panic at `random/linux.rs:159`). The `int`
return of `syscall` (`ffi.rs:2783`, `2796`), the futex forms `zoraya_posix_futex` answers (`ffi.rs:1417-1490`), the weak
`syscall!` lookup and `getrandom` in its own archive member, and `FIONBIO` as the only `ioctl` are as the survey says.
`adr-check` reported only the six expected index rows.

| # | Severity | Finding | Done |
|---|---|---|---|
| 1 | blocking | ADR-0453 item 3 (a child's random connection made from its parent's) cannot be built: a program's connection is depth 2 and may not `Share` (`crates/zoraya-rng/src/lib.rs:233-238`, ADR-0288 item 6); the Cost line wrongly said libposix already asks for the shell's programs (the shell does) | Fixed: the broker becomes ADR-0227 item 3's launcher for POSIX children, with a launcher connection from the shell that may `Share`; ADR-0288 item 6 is named as superseded in part; exhaustion restated (64 per broker, 256 in the service); INV-0453-1 rewritten; open question on parallel builds |
| 2 | should-fix | ADR-0453 misnamed the accepted items it changes: ADR-0227 item 3 already gives every POSIX process a connection (the broker as launcher) and one for `fork` and `spawn`; the conflict is with INV-0411-4, ADR-0310 item 3 and INV-0310-1, which was not named | Fixed: Context states the disagreement; item 3 implements ADR-0227 item 3 and supersedes in part INV-0411-4, ADR-0310 item 3, INV-0310-1's `fork` clause and ADR-0288 item 6, for randomness only |
| 3 | should-fix | `std`'s `Command` forks and executes for `uid`, `gid`, `groups`, `chroot`, `pre_exec` and `PATH` set with a bare name (`process/unix/unix.rs:467-474`; `setsid` on musl, `757-762`); such a child had no remedy | Fixed: the broker gives a connection at every `exec` too; ADR-0453 item 5 names the fork path; INV-0453-1's check runs a fork-path child |
| 4 | should-fix | Facts: `sendmsg`, `setgroups`, `setsid` are hidden `ENOSYS` stubs, not implementations; `getauxval` is hidden too (7 hidden, 15 absent); `memfd_create` is exported; `/proc/self/maps` is read by backtrace symbolisation, not for the main guard | Fixed in the survey, ADR-0451's Context and items 3 and 7, and `zshim.c`'s comment |
| 5 | should-fix | The `int` defect also hits `tee` and `sendmsg` (`ssize_t`); INV-0451-1's header check would check nothing (the header already says `long`) | Fixed: item 1 makes every `not_yet!` stub return its prototype's width; INV-0451-1's check is now a C test through the prototypes |
| 6 | should-fix | `libzoraya-rust.a` lets deny-listed names resolve in every `zcargo` link, a crate's C code included, which INV-0005-2 does not cover | Fixed: ADR-0451 item 5 names it as a scoped exception to INV-0005-2's intent; libposix still exports none; INV-0451-2 keeps the archive out of `zcc` and `zc++` links |
| 7 | should-fix | `std` needs `posix_spawnp`, `socketpair`, `sendmsg`, `recvmsg` to link; they come with ADR-0411's and ADR-0412's unbuilt parts | Fixed: ADR-0451 item 4 orders the parts (the Rust part builds `posix_spawnp` per ADR-0411 item 1 if needed; the three socket names are temporary `ENOSYS` in the archive); ADR-0455 gate (a) says so |
| 8 | should-fix | ADR-0451 silently reversed ADR-0366's rejected Option B (`sched_yield` in libposix) and duplicated `libpyshim.a`'s `dirfd`, `pause`, `sched_yield` (ADR-0320 item 3) | Fixed: named in item 3; libc++'s shim copy is weak, so INV-0366-2 stands; `libpyshim.a` loses the three in the building part |
| 9 | should-fix | Overclaims: `connect_timeout` was never shown working (the fixed run's `EACCES` came from the grant); "every check passes except `current_dir`" left out the child's `HashMap` | Fixed in the survey's table and text, ADR-0450's Context, ADR-0451's Option B and Consequences |
| 10 | should-fix | The main stack is 64 KiB (`USER_STACK_SIZE`), `std` threads take 2 MiB; no ADR said so | Fixed: the survey's "Stacks"; ADR-0454 item 3, an open question on commitment and a revisit trigger |
| 11 | should-fix | `set_read_timeout` succeeds and never fires (`SO_RCVTIMEO` not honoured, ADR-0287 item 4) | Fixed: the survey and ADR-0453 item 4 |
| 12 | should-fix | INV-0450-2 overclaimed (a portability lint, not confinement) and attributed `svc` by symbol name, which a crate named `zoraya_rt` or stripping defeats | Fixed: the Effect of the vision says what it protects; the check attributes by the link map's `libc.a` sections and runs before stripping |
| 13 | should-fix | `zcargo` could run a project's own toolchain (Ladybird pins 1.98.0) or `RUSTUP_TOOLCHAIN`; `CARGO_ENCODED_RUSTFLAGS` drops a project's `rustflags`; build-script link arguments escape item 2 | Fixed: ADR-0452 items 1 and 4 force the toolchain, state the `rustflags` rule, and name `zoraya-elf-check` as the catch for build-script arguments |
| 14 | should-fix | "INV-0109-2 unchanged for C" overstated: `build.rs` sees `CARGO_CFG_TARGET_OS=linux` and picks C sources; `env::consts::OS` is `linux` | Fixed: ADR-0450 item 3 names the erosion of the rule's intent and what bounds it; no further check proposed (a crate's Linux-only C fails at link or at run time, as an unported C program does) |
| 15 | should-fix | ADR-0455's gate tests the probe, not Ladybird; a link-only build of its 433 crates could be measured now | Fixed: ADR-0455 item 0 and gate (a) |
| 16a | minor | ADR-0451 and ADR-0452 had no non-Unix option of their own | Fixed: ADR-0451 Option D (a Zoraya `std` over typed calls, as Fuchsia's), ADR-0452 Option D (an SDK, as Fuchsia's) |
| 16b | minor | ADR-0454 item 4 rested on an `[unverified]` claim alone and ignored `extern "C-unwind"` | Fixed: item 4 decides only the shared unwinder; Rust's boundary rules are an open question |
| 16c | minor | The survey cited `unix.rs:615` (not musl's path) | Fixed: `642-646`, a strong reference on musl |
| 16d | minor | "these four" listed five names | Fixed |
| 16e | minor | `process_group` works; only `setsid` fails | Fixed in ADR-0453 |
| 16f | minor | "five times the code" is about 3.8 times (5.3 for C++) | Fixed in ADR-0455 |
| 16g | minor | `SIGSEGV`/`SIGBUS` handlers, `SIGPIPE`, fd sanitising, terminal stdin unmentioned | Fixed: the survey's "Start-up and signals" and ADR-0454's Context; terminal stdin recorded as not probed |

**Its verdict**, condensed: Route A is well argued and its core evidence holds; ADR-0450's choice and ADR-0455's
"wait for the gates" follow from the evidence; the set was not ready as written because of finding 1 and the
misnamed changes, and findings 1 to 9 should be fixed before the architect's decision. All were addressed as above;
the fixes were not re-reviewed.
