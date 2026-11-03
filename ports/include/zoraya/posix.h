/*
 * <zoraya/posix.h>: Zoraya's own additions to the POSIX interface of libposix
 * (ADR-0142, ADR-0104 items 4 and 5). Not part of musl; installed into the
 * sysroot by ports/Makefile.
 */
#ifndef _ZORAYA_POSIX_H
#define _ZORAYA_POSIX_H

#include <sys/types.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * posix_spawn attribute flag: with POSIX_SPAWN_SETPGROUP, make the child's
 * process group the terminal's foreground group before the child runs (the
 * broker's start_suspended, tcsetpgrp, resume of ADR-0104 item 5). No flag of
 * musl or glibc uses this bit. It is below the sign bit of the short that
 * posix_spawnattr_setflags takes, so that it passes a flag mask unchanged
 * (ADR-0142, Decision).
 */
#define POSIX_SPAWN_TCSETPGRP 0x4000

/* zoraya_fork_group's argument, as setpgid's second: */
#define ZORAYA_FORK_INHERIT (-1) /* the parent's group */
#define ZORAYA_FORK_NEW 0        /* a new group led by the child */
                                 /* > 0: join that group */

/*
 * Names the process group the child of the next fork() goes into, which the
 * broker must know when it creates the child (ADR-0104 item 4). Applies to the
 * next fork() only. Returns 0, or -1 with errno.
 */
int zoraya_fork_group(pid_t pgid);

#ifdef __cplusplus
}
#endif

#endif
