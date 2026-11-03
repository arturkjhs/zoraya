/*
 * crt1.S: the entry of a POSIX program on Zoraya (ADR-0109 item 4).
 *
 * A process starts with every register zero and its argument and environment
 * vectors in the startup block (ADR-0042). _start checks the first, as
 * zoraya-rt's _start does, so that a leak from the kernel or another process
 * faults (brk #2) rather than passing unseen; clears the frame pointer and the
 * link register as musl's _start does; and calls zoraya_posix_start with the
 * stack pointer and the address of _start_c. zoraya_posix_start (zoraya-posix)
 * lays out argc, argv, envp and the auxiliary vector in the form musl's
 * _start_c expects, reads the bootstrap messages and calls _start_c, which
 * this object also holds (musl's crt/crt1.c, linked in by ports/Makefile). It
 * never returns. _start_c travels as an argument so that libc.a refers to
 * nothing outside itself (posix-surface-check, ADR-0141 item 7).
 */
	.text
	.global _start
	.type _start, %function
_start:
	.irp n, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30
	orr x0, x0, x\n
	orr v0.16b, v0.16b, v\n\().16b
	.endr
	orr v0.16b, v0.16b, v31.16b
	mrs x1, tpidr_el0
	orr x0, x0, x1
	mrs x1, fpcr
	orr x0, x0, x1
	umov x1, v0.d[0]
	orr x0, x0, x1
	umov x1, v0.d[1]
	orr x0, x0, x1
	cbz x0, 1f
	brk #2
1:
	mov x29, #0
	mov x30, #0
	mov x0, sp
	and sp, x0, #-16
	adrp x1, _start_c
	add x1, x1, :lo12:_start_c
	bl zoraya_posix_start
	brk #3
	.size _start, . - _start

	.section .note.GNU-stack, "", %progbits
