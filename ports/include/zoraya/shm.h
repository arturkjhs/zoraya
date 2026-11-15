/*
 * <zoraya/shm.h>: the anonymous name of shm_open (ADR-0413 item 1, ADR-0432),
 * included by <sys/mman.h> (ports/musl/zoraya/0004-shm-anon.patch).
 *
 * shm_open(SHM_ANON, O_RDWR, mode) makes a memory descriptor with no name, as
 * FreeBSD's does; ports that find SHM_ANON in the header (WebKit's
 * SharedMemoryUnix.cpp, Ladybird's anon_create) take that path. Any other name
 * is EACCES: there are no global names (ADR-0413 driver 1). The value is
 * FreeBSD's, a pointer that shm_open compares and never reads.
 */
#ifndef _ZORAYA_SHM_H
#define _ZORAYA_SHM_H

#define SHM_ANON ((char *)1)

#endif
