# Patches to the ported programs

Applied by `ports/Makefile` in name order after unpacking. Each patch modifies, and
quotes lines of, files under the licence of the original program, given in
[`../README.md`](../README.md); **they are not covered by the repository's
BSD-2-Clause `LICENSE`**.

| Patch | Program and version | Files | Why |
|---|---|---|---|
| `dash/0001-vforkexec-to-posix_spawn.patch` | dash 0.5.13.5 (BSD-3-Clause) | `src/exec.c`, `src/exec.h`, `src/jobs.c` | TASK-02's M4 patch, unchanged: a non-interactive simple command starts with `posix_spawn` (ADR-0109 item 9). |
| `dash/0002-fork-group-hint.patch` | dash 0.5.13.5 (BSD-3-Clause) | `src/jobs.c` | `forkshell` calls `zoraya_fork_group` with the group `forkparent` and `forkchild` will `setpgid` to (ADR-0104 item 4, ADR-0142). |
| `dash/0003-vforkexec-setpgroup-tcsetpgrp.patch` | dash 0.5.13.5 (BSD-3-Clause) | `src/exec.c`, `src/exec.h`, `src/jobs.c` | `vforkexec` always uses `posix_spawn`, with `SETPGROUP` and `POSIX_SPAWN_TCSETPGRP` under job control and `SETSIGDEF` for the signals `forkchild` resets (ADR-0104 item 5, ADR-0142); the `vfork` path stays as the fallback. |
| `make/0001-config-sub-zoraya.patch` | GNU make 4.4.1, `build-aux/config.sub` (GPL-3.0-or-later with the Autoconf exception) | `build-aux/config.sub` | `zoraya*` as an operating system, so that `--host=aarch64-unknown-zoraya` is accepted (ADR-0109 item 1). |
| `toybox/0001-zoraya-portability.patch` | toybox 0.8.15 (0BSD) | `lib/portability.h`, `lib/portability.c` | a Zoraya branch where toybox stops an unknown OS with `#error`: `struct statfs` from musl, `dev_t` in musl's (Linux's) encoding, extended attributes through the tier's `f*xattr` (the path forms refuse with `ENOTSUP`), and no mount table, file notification or file system type names (ADR-0109 item 6). |
| `tcc/0001-zoraya-target.patch` | TinyCC `e23dabed` (LGPL, version 2 or later) | `configure`, `conftest.c`, `include/tccdefs.h`, `lib/Makefile`, `libtcc.c`, `tcc.c`, `tcc.h`, `tccpp.c` | the `Zoraya` target OS (ADR-0109 item 7, ADR-0143). |
| `tcc/0002-skip-shf-exclude-sections.patch` | TinyCC `e23dabed` (LGPL, version 2 or later) | `tccelf.c` | skip `SHF_EXCLUDE` input sections, which the Rust members of `libc.a` carry (ADR-0143). |
| `less/0001-zoraya-compiled-in-defaults.patch` | less 710 (the Less License) | `defines.h.in`, `filename.c` | the compiled-in defaults of ADR-0335 item 8: no key files (`USERFILE` 0), no tags (`TAGS` 0), no history file (`LESSHISTFILE` is `-`), `LESSOPEN` and `LESSCLOSE` ignored. |
| `nvi2/0001-no-startup-files.patch` | nvi2 2.2.2 (BSD-3-Clause) | `ex/ex_init.c` | the system (`/etc/vi.exrc`) and the home (`~/.nexrc`, `~/.exrc`) start-up files are not read; `NEXINIT` and `EXINIT` and the current directory's `.exrc` (option `exrc`, off by default) stay (ADR-0335 item 6). |
| `nvi2/0002-no-script.patch` | nvi2 2.2.2 (BSD-3-Clause) | `ex/ex_script.c` | `:script` says it is not available: it needs a pseudo-terminal (ADR-0338); the code that opened one is removed. |
| `nvi2/0003-no-flock.patch` | nvi2 2.2.2 (BSD-3-Clause) | `common/exf.c` | the `flock` call is removed; the `lock` option is accepted and locks nothing (ADR-0342 item 8: no `flock`). |
| `db1/0001-hash-errno-field.patch` | Berkeley DB 1.85 (BSD-3-Clause) | `hash/hash.c`, `hash/hash.h`, `hash/ndbm.c` | the field `errno` of the hash table's structure is renamed `dberrno`: musl's `<errno.h>` makes `errno` a macro. |
| `openssh/0001-fake-rfc2553-stdio.patch` | OpenSSH portable 10.6p2 (BSD) | `openbsd-compat/fake-rfc2553.c` | the `#include <stdio.h>` its `getnameinfo` needs (`snprintf` is an error without it in clang 17 and later). |
| `openssh/0002-no-configuration-files-no-options-after-host.patch` | OpenSSH portable 10.6p2 (BSD) | `ssh.c` | no configuration file is read, `-o` and `-F` are fatal, options are not read again after the host, and a user name is required (ADR-0340 items 3 to 5). |
| `openssh/0003-no-askpass.patch` | OpenSSH portable 10.6p2 (BSD) | `readpass.c` | `SSH_ASKPASS`, `SSH_ASKPASS_REQUIRE`, `DISPLAY` and `WAYLAND_DISPLAY` are not read and no askpass program runs (ADR-0340 item 3). |
| `openssh/0004-no-host-key-dns.patch` | OpenSSH portable 10.6p2 (BSD) | `dns.c` | `verify_host_key_dns` returns at once: the resolver's names are not in `libc.a`. |
| `git/0001-configuration-paths-and-variables.patch` | git 2.56.1 (GPL-2.0-only) | `config.c` | the global configuration is `~/.config/git/config` always; `GIT_CONFIG_GLOBAL`, `GIT_CONFIG_SYSTEM`, `GIT_CONFIG_NOSYSTEM` decide nothing (ADR-0339 item 4). |
| `git/0002-no-askpass-no-credential-helper.patch` | git 2.56.1 (GPL-2.0-only) | `prompt.c`, `credential.c` | no askpass program, no credential helper in any scope (ADR-0339 items 8 and 9). |
| `git/0003-no-tls-or-proxy-configuration.patch` | git 2.56.1 (GPL-2.0-only) | `http.c` | the `http.ssl*`, `http.proxy*` and kindred keys are ignored (ADR-0339 item 8). |
| `git/0004-variables-not-read.patch` | git 2.56.1 (GPL-2.0-only) | `git-compat-util.h`, `wrapper.c` | `getenv` is `zoraya_getenv`, which answers "not set" for the variables that would decide trust, a program to run or the configuration (ADR-0339 item 8). |
