# ADR-0346: The names OpenSSH's client adds to libc.a, the patches that make it ADR-0340's, and what is left for part 10g

- **Status:** proposed
- **Date:** 2026-11-15
- **Depends on:** ADR-0005, ADR-0100, ADR-0141, ADR-0287, ADR-0310, ADR-0312, ADR-0331, ADR-0340, ADR-0342, ADR-0390
- **Evidence:** CP-27

## Context

ADR-0342 item 1 asks each port for a link probe and a `posix-surface-added` block, and ADR-0340 item 1 says OpenSSH's
`ssh` and `ssh-keygen` are built against the image's OpenSSL (ADR-0312) and zlib. TASK-31 part `ports-early` builds them
ahead of part 10g with the pins of `ports/sources.lock` (OpenSSH portable 10.6p2) and no cargo and no QEMU (see
ADR-0390 for how `libc.a` was relinked). CP-27 (a name needs a program behind it) is the rule for the block.
The probe is the linker: it names what `libc.a` lacks; the other names that `configure` thought missing were checked by
the link too (item 2).

## Decision drivers

1. A name is exported because a program in the image imports it (ADR-0342 driver 1); a name only a feature the
   description does not offer reaches is not worth exporting (CP-27).
2. ADR-0340's rules (no configuration file, no askpass program, a user name must be given, options end at the host,
   no host-key checking through DNS) are built into the program, not into the shell alone: a second guard.
3. `libposix` is not changed by this slot; what only it can supply is listed for 10g.

## Considered options

### Option A: export the musl-own names, shim and list the rest, patch the five rules in (the recommendation)
- **Description:** the block below; `ports/apps/sshshim.c` and `ports/apps/ttyshim.c` link the other names into
  these early builds only; four patches.
- **Evidence for:** drivers 1 to 3.
- **Evidence against:** the shimmed names are unreachable but present.
- **Cost:** *(estimate)* none beyond the block.
- **Precedent:** ADR-0310, ADR-0343, ADR-0390.

### Option B: export every name `ssh` imports, as a full libc would
- **Evidence against:** `socketpair`, `getifaddrs`, `execlp`, `initgroups` and `madvise` serve ProxyCommand, interface
  binding, security-key helpers and privilege changes, none of which the description offers; ADR-0005 keeps
  `setuid` and its kin out.
- **Precedent:** every Linux distribution.

### Option C: patch each unreachable use out of OpenSSH
- **Evidence against:** more patches at every release (ADR-0310 option B's argument), for code that cannot run.
- **Precedent:** toybox's patches for absent names (ADR-0343).

## Recommendation

**Recommendation (not a decision): Option A.**

1. **What was built** (`make openssh`): `ssh` (6823648 bytes: `.text` 3.1 MB, `.rodata` 0.56 MB, debug sections 1.4 MB
   from `libc.a`'s `-g`) and `ssh-keygen` (6382280 bytes) from OpenSSH portable 10.6p2, `-O2`, static, with the image's
   `libssl.a`, `libcrypto.a` and `libz.a` (`build/ports/deps`). Configured `--without-pam --without-selinux
   --without-kerberos5 --without-libedit --without-ldns --with-sandbox=no --disable-pkcs11 --disable-security-key
   --disable-utmp --disable-wtmp --disable-lastlog --without-shadow` (no server, agent, `ssh-add`, `scp` or `sftp`:
   `make ssh ssh-keygen`). The text is large because every cipher and key type of the static `libcrypto.a` is linked.
2. **`configure` answers.** Cross-compiling, `configure` thought `statvfs`, `fstatvfs`, `setlinebuf`, `mblen`, `dirfd`,
   `endgrent` and `setgrent` missing, and OpenSSH's compat layer then defines macros or structures that clash with
   musl's own declarations. They are answered `yes` (`ac_cv_func_*`); the link then shows none is referenced: only
   `sftp-server` and the `sftp` client use `statvfs`, and the others are used by code that does not link. The
   `getnameinfo` the image lacks is OpenSSH's own `fake-rfc2553.c` (IPv4 numeric), which needed one `#include <stdio.h>`
   (patch 0001).
3. **Patches** (`ports/patches/openssh/`, the BSD licence of OpenSSH):
   - `0001-fake-rfc2553-stdio.patch`: the missing `#include`.
   - `0002-no-configuration-files-no-options-after-host.patch` (ADR-0340 items 3 to 5): `process_config_files` reads
     nothing; `-o` and `-F` are fatal; options are not read again after the host (the `goto again` is gone), so
     `ssh -l ada host ls -l` has one reading with or without the shell's `--`; with no `-l` and no `USER@`, `ssh` stops
     with `no user name: give one with -l USER or as USER@HOST`.
   - `0003-no-askpass.patch` (item 3): `read_passphrase` reads neither `DISPLAY`, `WAYLAND_DISPLAY`, `SSH_ASKPASS` nor
     `SSH_ASKPASS_REQUIRE`; `ssh_askpass` and `notify_start` run nothing. The binaries hold no `SSH_ASKPASS` string.
   - `0004-no-host-key-dns.patch`: `verify_host_key_dns` returns at once; the resolver's `res_query` and `dn_expand`
     are not names of `libc.a`, and `VerifyHostKeyDNS` could not be set once the reader is gone.
4. **Names.** The block holds three names that are musl's own code (hidden by the tier list): `openlog`, `closelog` and
   `syslog` of OpenSSH's `log.c` (its client logs to standard error, so they are linked and never reached; musl's
   `syslog` writes to a socket it cannot open, silently). The names `libc.a` still lacks, each linked from
   `ports/apps/sshshim.c` or `ttyshim.c` in these builds and each **left for part 10g (or 10c)** with what the
   program needs it for:

   | name | program | what it needs |
   |---|---|---|
   | `getservbyname` | `ssh` (`readconf.c`) | `-p ssh` as a port: ADR-0342 item 11, from a table (10g) |
   | `socketpair` | `ssh` (`sshconnect.c`) | ProxyCommand and ProxyJump; unreachable (10g: export or patch out) |
   | `getifaddrs`, `freeifaddrs` | `ssh` (`sshconnect.c`) | BindInterface; unreachable (10g) |
   | `execlp` | `ssh` (`ssh-sk-client.c`, `ssh-pkcs11-client.c`) | the security-key and PKCS#11 helpers; unreachable (10g) |
   | `madvise` | `ssh`, `ssh-keygen` (`sshkey.c`) | `MADV_DONTDUMP` on a key's memory; a no-op would do (10g) |
   | `gethostbyaddr` | `ssh` (`fake-rfc2553.c`) | the reverse lookup of the fake `getnameinfo`, which `ssh` does not ask for (10g) |
   | `initgroups` | `ssh`, `ssh-keygen` (`misc.c`) | the privilege change of a subprocess; `setuid` and kin stay unexported (ADR-0005): 10g patches it out |
   | `cfsetospeed`, `cfsetispeed`, `tcsetattr`, `tcflush`, `select` | `ssh` (`ttymodes.c`, `readpassphrase.c`, `clientloop.c`) | the terminal names of ADR-0342 items 4 and 5 (10c) |

```posix-surface-added
closelog ssh
openlog ssh
syslog ssh
```

## Consequences

- **Positive.** `ssh` and `ssh-keygen` build and link on the image's OpenSSL; the rules of ADR-0340 items 3 to 5 are in
  the binaries, which a test can check by string (`ports/check-early.sh`).
- **Negative.** Untested: no QEMU ran. The shimmed names fail with `ENOSYS`; nothing proves they are unreachable beyond
  reading the code (`ProxyCommand` and the others need an option that is gone). The binaries are 6.4 and 6.8 MB.
- **Follow-up.** Part 10g: the names above, the description, the `ssh` test server, a decision on `getnameinfo`.

## Invariants

- **INV-0346-1.** Every line of the block names a program, and that program's objects import the name. *Check:*
  `posix-surface-check imports docs/adr/0346-surface-for-ssh.md ssh=DIR` in `make check-ports` (`ports/check-early.sh`).
- **INV-0346-2.** The `ssh` and `ssh-keygen` binaries contain no `SSH_ASKPASS` string and no name of a configuration file
  (`ssh_config`, `.ssh/config`). *Check:* `ports/check-early.sh` in `make check-ports`.

## Open questions

- Export `socketpair`, `getifaddrs`, `execlp`, `madvise`, `gethostbyaddr` or patch each use out? Each is unreachable
  from the description; 10g decides with the description in hand.
- Is `getnameinfo`'s fake enough for the failure messages (it names IPv4 addresses only)?

## Revisit triggers

- A description offers `-J`, `-o` or `-F`; part 10g builds the shimmed names.

## Architect's decision

(Left empty for the human.)
