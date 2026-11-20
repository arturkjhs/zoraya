# TASK-31: review of the part `work/rust-std`

An independent review by the `opus-review` subagent, which wrote none of the code, of `git diff origin/main...work/rust-std` at `0d4e64e5` (it
began at `1f0e05d4`; the later commit landed while it read). Read only: it ran no build. Findings below with the disposition of each; fixes are in
the commit after this file, and `make check`, `make test-rust-std` and `make test-libposix` were rerun on it (30 of 30, `PASS: 62 steps`,
`PASS: 199 steps` and the three others).

| # | Severity | Finding | Disposition |
|---|---|---|---|
| 1 | blocking | `RandomConnection` is answered to any process, any number of times: one program could drain the broker's 64 and, with depth 2 children making 8, the service's 256 | **Fixed.** The broker keeps a credit per connection: one for the image's own `exec`, one for each `SpawnPrepare` it made; a further ask is `NOT_PERMITTED` (`random_credit`, `zoraya-broker`; test `a_process_gets_a_random_connection_for_its_child_while_the_broker_has_one_to_give` asks 5 times with no child). The depth-2 point is a note for the architect in ADR-0453's open questions |
| 2 | should-fix | the svc check took any input word containing `/libc.a(`; three ways round it (a section named like a member, a build script's own `libc.a`, a target directory named like one) | **Fixed.** `zcargo-link` passes `--libc ARCHIVE` (the sysroot's exact path); `libc_ranges` takes only `ARCHIVE(member):(section)` with a plain member and section; host tests for the forged forms and another archive |
| 3 | should-fix | ways round zcargo's refusals: `--config FILE`, `build.rustc*` keys, `--codegen`, arguments after `--`, `RUSTC` and wrapper variables, globbing in `check_flags` | **Fixed** except two: a project's `.cargo/config.toml` `[build] rustc-wrapper` or `[unstable]` is neither refused nor noted, and `__CARGO_TEST_CHANNEL_OVERRIDE...` with `[unstable] build-std` is refused as a variable but not tried; both are left to the architect (the part's test of each is `tools/test-zcargo.sh refusals`, extended by seven cases) |
| 4 | should-fix | `libzoraya-rust.a` ships in the guest's `/usr/lib`; `zcc` accepts `-lzoraya-rust` anywhere | **Fixed.** The image leaves it out (`CXX_IMAGE_LEFT_OUT`), `zcc` refuses the name unless `ZORAYA_RUST=1` (`compare-predefs.sh` tests it), `check-rust-archive` allows the two files that name it |
| 5 | should-fix | `tools/test-map.txt` did not rerun `test-libposix` and `test-rust-std` for the broker, `zoraya-abi`, `zosh`, `random.rs`, `zoraya-rng`, `init`, `panel`, `ports/Makefile` | **Fixed** (the lines were extended) |
| 6 | should-fix | no host test of `broker_random` and of the broker branch of `grant_connections` | **Not done.** The host rig has no broker fake; the guest tests `t_spawn` (`getrandom=0` without a grant, with one) and `rstest process` cover it, and a mutant that drops the cleanup loop survives the host tests. Left as a gap |
| 7 | should-fix | `child-no-network` runs under a description with no network, so it cannot tell "not passed" from "had none" | **Not done.** The parent's own connection is covered by `t_spawn grants`/`default` (`connect=13` with a parent that has none); a network-holding parent in `rsnet` is a follow-up |
| 8 | should-fix | the broker's loop blocks up to 5 s on the random service, and a late `ShareReply` can be taken for the next | **Not done**, recorded as a limit: `exchange` is the shell's and `init`'s own call as well; an asynchronous share is the follow-up |
| 9 | minor | handle leaks of the broker's connection when `channel_create`, `stack` or `st.root` fail | **Fixed** (`beside.rs` closes it; `init.rs` and `panel.rs` make it inside the closure) |
| 10 | should-fix | ADR-0453's "never Share" against ADR-0435's 8; a default child holds more than a granted one | **Recorded** in ADR-0453's open questions for the architect |
| 11 | minor | INV-0453-1 `checked` rests on proposed ADR-0468 for its fork clause | **Fixed** (the row says so) |
| 12 | should-fix | `design.md` said both paths work, named the wrong starters, left out the fork-path limit | **Fixed** |
| 13 | minor | `addopen` ignores an earlier `addchdir` | **Not done**; recorded as a limit here (the order of actions of different kinds is not followed for `addopen`) |
| 14 | minor | `syscall()` read the futex `op` as 64 bits | **Fixed** (`b as i32`) |
| 15 | minor | `Seen`'s sentinel `i64::MIN` and `number as u32` reporting `(1<<32)\|98` as 98 | **Fixed**: only `0..=u32::MAX` is remembered or reported; test |
| 16 | minor | the report test lets a second note or a note for a fallback number pass | **Not done** |
| 17 | minor | a comment about a retired image; the `inherit_failed_at` debug numbers | **Left**: the numbers are documented in `jobs.rs` as a debug aid for `ZORAYA_POSIX_DEBUG` |

ADR-0468's analysis was checked against the unpacked `std` source and found right. It stays `proposed`.
