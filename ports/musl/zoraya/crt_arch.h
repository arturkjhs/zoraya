/* Zoraya: empty in place of musl's arch/aarch64/crt_arch.h when crt/crt1.c is
 * compiled, so that crt1.o takes `_start_c` from musl and `_start` from
 * ports/crt/crt1.S (ADR-0109 item 4). */
