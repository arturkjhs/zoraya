/*
 * Kernel image layout for QEMU virt. RAM starts at 0x40000000; QEMU puts the
 * device tree blob there for a bare-metal ELF (1 MiB on QEMU 11) and refuses
 * to load if the image overlaps it, so the image starts 2 MiB in. The MMU is
 * off at stage 0: these are physical addresses.
 */

ENTRY(_start)

KERNEL_BASE = 0x40200000;
STACK_SIZE = 64K;

SECTIONS
{
    . = KERNEL_BASE;
    __kernel_start = .;

    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }

    .rodata : ALIGN(16) {
        *(.rodata .rodata.*)
    }

    .data : ALIGN(16) {
        *(.data .data.*)
    }

    .bss (NOLOAD) : ALIGN(16) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
    }

    .stack (NOLOAD) : ALIGN(16) {
        __stack_bottom = .;
        . += STACK_SIZE;
        __stack_top = .;
    }

    __kernel_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame .eh_frame_hdr)
        *(.note .note.*)
    }
}
