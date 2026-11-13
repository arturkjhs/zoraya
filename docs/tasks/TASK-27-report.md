# TASK-27 report: stage 9, applications

Running report, updated after each part. This file is created on branch `work/9a` (the task file alone is on `main`);
branch `work/9b-design` has a file of the same name with the 9b design: when both merge, keep that file's "9b" section
and this file's "9a" section, and the newer Status block.

## Status for the architect

| Part | State | Tag |
|---|---|---|
| 9a TLS for C, zlib, curl, wget | implemented on `work/9a`, not merged; tests below; waits for the independent review and the candidate CI | |
| 9b POSIX threads | see the file on `work/9b-design` | |
| 9c CPython | not started | |
| 9d NetSurf | not started | |
| image addition (`EXTRA_ROOT`) | done inside 9a (`tools/run-disk.sh`), without descriptions for unknown programs | |

### Proposed ADRs of 9a

| ADR | Recommendation in one line |
|---|---|
| [0310](../adr/0310-surface-for-tls-curl-and-wget.md) | OpenSSL 3.5, zlib, curl, wget pinned and built by `zcc` (`make apps` into `build/rootfs-apps`); 22 names added to `libc.a`; `getrandom` over `rngd` through a `ZPBX` bootstrap flag; a shim of 13 names inside the two programs; `EXTRA_ROOT` in `tools/run-disk.sh`. |
| [0311](../adr/0311-url-file-names-and-the-roots-directory-for-posix-programs.md) | The description key `names OPTION OPERAND` for `curl -O` (the shell derives the one file to create, or refuses the line); a POSIX program with a network also gets `/system/tls` read-only. |
| [0312](../adr/0312-openssl-for-the-posix-side-of-tls.md) | OpenSSL 3 replaces BearSSL as the C TLS library: **this changes ADR-0228 item 2**, which TASK-27 (your later text) does, so by the task's own rule this part does not merge until you accept ADR-0312 (or tell me to take it out and build another library). |

### Items for the architect

1. **ADR-0312 changes an accepted decision (ADR-0228 item 2).** TASK-27 names OpenSSL, ADR-0228 says BearSSL then Mbed TLS.
   Accept ADR-0312 or say which. Nothing else of 9a changes an accepted text (ADR-0310 adds names by the ADR-0210/0287
   mechanism; ADR-0311 adds a description key and a grant for the image's own descriptions).
2. **A defect found, not fixed: about 8.5 MiB of memory is not given back at each launch of `curl` or `wget`** (8 launches on
   128 MiB, 23 on 256 MiB, 40 or more on 512 MiB; the 9th launch on 128 MiB says "over quota"). Ruled out by measurement:
   the network and `getrandom` (`curl --version` does the same), `posix_child::load` and the file service's attached
   object (`grants curl` ×30 does not leak), the shell's handles (`slots` is the same after each run), time (a 2 s pause
   between launches changes nothing), the shell's `Vec` of the whole file (replaced; it moved the count from 5 to 8). What is
   left: the launch of a large image itself (the loader's copy, the child's `/proc/self/exe` handle, the broker's process
   handle or the kernel's reaper). Small programs probably leak in proportion and nobody saw it. `make run-net`
   now boots 512 MiB (as `make run-gui` and `test-posix` already did), and the apps' tests use 512 MiB. Whoever does 9b should
   look at this first: threads add stacks to it.
3. Manual checks for 9a: none are needed to accept it. To try by hand (needs `make fetch-ports` once): `make apps`, then
   `make run-net`, and in the guest `curl -O https://example.org/index.html`, `wget -O page.html https://example.org/`,
   `grants curl -O https://example.org/index.html`. The guest has no resolver in the test image; `make run-net` has SLIRP's.
4. Choices made without you, cheap to reverse: wget's configure runs with the host triple `aarch64-unknown-linux-musl` (gnulib's
   guesses only; the compiler is `zcc`); `rand`/`srand` are exported (wget wants them; OpenSSL never calls them); `-k`,
   `--cacert`, `--no-check-certificate` are not options of the descriptions (a line with them is a usage mistake and the program
   gets no network at all); `/etc/wgetrc` is in `rootfs/` (roots, `check_certificate = on`, `inet4_only = on`).

## 9a: TLS for C programs, zlib, curl, wget

Branch `work/9a` from `origin/main` at `972bbf0`; one Sonnet 5.5 session, 2026-11-13.

### What exists

- `ports/sources.lock`: OpenSSL 3.5.9, zlib 1.3.1, curl 8.23.0, wget 1.25.0 (version, SHA-256, URLs; `make fetch-ports`
  downloaded and verified them; `docs/dependencies.md` and `ports/README.md` list them with licences and patches).
- `make apps` (`ports/Makefile`): zlib, OpenSSL (static, `no-threads`), curl and wget with `zcc`, 6.7 MiB each without debug
  information; the end of the target checks the logs for `-D__linux` and that every OpenSSL patch is under `__zoraya__`.
  Patches: `ports/patches/openssl/` (3), `curl/` (config.sub), `wget/` (plain-fd BIO, no `MSG_PEEK`).
- `getrandom` in libposix (`crates/zoraya-posix/src/random.rs`), the `ZPBX` bootstrap form (`zoraya-abi`), the share in `zosh`
  (`user/src/bin/zosh.rs`, `user/src/posix_child.rs`).
- Descriptions `user/describe/posix/curl.cmd` and `wget.cmd`; the key `names` (`zoraya-describe`); `grants` and the run give a
  POSIX program the roots directory and show the `net:` line; `posix_child::load` reads the program into its image directly.
- `tools/run-disk.sh` `EXTRA_ROOT`; `rootfs/etc/wgetrc`; `ports/apps/zshim.c`.
- 22 names in `libc.a` (now 337 exports); ADR-0310 to ADR-0312 with 6 INV rows (the Total is 538).

### Tests (what ran in this session, on the M4, HVF)

| Command | Result |
|---|---|
| `cargo test -p zoraya-posix --test random` | 7 passed |
| `cargo test -p zoraya-describe -p zoraya-shell -p zoraya-command` | passed (the `names` key, `url_file_name`, 4 `curl_o_*` plans) |
| `cargo test -p zoraya-abi` | 69 passed (one new: the `ZPBX` flags) |
| `make check` | 17 of 17 passed (adr-check 0 errors; `check-run-disk` is new) |
| `make test-apps` (new) | passed: both programs over HTTP and TLS, expired and wrong-name certificates refused by both with no request byte at the server, `grants`, refused `-O` names, `-k` as a usage mistake |
| `make test-hand-run` | passed: the production `run-net` image with `curl` and `wget`: HTTP works, HTTPS reaches the certificate and is refused (curl exit 60, wget "cannot verify") |

Not run here: `make test-changed`, `test-net`, `test-net-posix`, `test-libposix`, `test-files`, `test-boot` (the files `zosh.rs`,
`posix_child.rs`, `zoraya-posix` and `grants` changed, so CI's candidate run covers them), anything under TCG, `FULL=1`, mutation
testing. The CI workflow changed: the build job also builds `make apps` (cache paths and `ports-build-v3`; timeout 30 minutes).

### Decisions

See ADR-0310 to ADR-0312 and Items 2 and 4 above. Not done in 9a: `getrandom` for programs a POSIX parent starts, `tls.roots`
for C programs, threads (9b), a hand-written mutation list for the new code (`tools/mutants/`).

### For the independent review (security and trust boundaries first)

1. `crates/zoraya-posix/src/random.rs`, `ffi/sockets.rs` (`getrandom`), `ffi.rs` (`bootstrap`'s `ZPBX` parse: handle counts,
   closing what is not a channel), `user/src/posix_child.rs` (the message and every close path on failure), the share in `zosh`.
2. The description files (`curl.cmd`, `wget.cmd`): the options list is the policy (no `-k`, no `--cacert`, no config file); the `names` key
   (`crates/zoraya-describe`, `crates/zoraya-shell/src/grants.rs`): can the name the shell approves differ from the file curl
   makes? (`url_file_name` is deliberately stricter than curl.)
3. TLS root handling in C programs: `zosh.rs` `ready_from_volume` (the roots directory added with the network),
   `ports/patches/openssl/0003`, `rootfs/etc/wgetrc`, `ports/patches/wget/0001` (a plain-fd BIO).
4. `tools/run-disk.sh` `EXTRA_ROOT`, and `grants` for POSIX programs (`grants_report`, `Host::grants`).
5. The wget patches and the shim (`ports/apps/zshim.c`) are C that runs with the program's authority only; skim.
