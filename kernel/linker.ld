/*
 * Kernel image layout for QEMU virt (ADR-0020). The image is linked at the
 * top of the address space and loaded at KERNEL_PHYS: segment physical
 * addresses are what QEMU's ELF loader uses, and the ELF entry is the
 * physical address of _start, where the CPU starts with the MMU off.
 *
 * RAM starts at 0x40000000; QEMU puts the device tree blob there for a
 * bare-metal ELF (1 MiB on QEMU 11), but only if it fits below the image:
 * otherwise QEMU silently omits it and the kernel can only report the missing
 * blob through the fallback UART. The image therefore starts 2 MiB in.
 *
 * Both link and load addresses are 2 MiB aligned, because the boot tables map
 * the image with 2 MiB blocks. Sections with different permissions start on
 * page boundaries. The boot tables and the stack guard are never mapped by
 * the final tables.
 */

ENTRY(__entry_phys)

KERNEL_PHYS = 0x40200000;
KERNEL_VIRT = 0xffffffff80000000;
KERNEL_OFFSET = KERNEL_VIRT - KERNEL_PHYS;
ASSERT(KERNEL_PHYS - 0x40000000 >= 2M, "leave 2 MiB below the image for the DTB")
ASSERT(KERNEL_PHYS % 2M == 0, "the boot tables map the image with 2 MiB blocks")
ASSERT(KERNEL_VIRT % 2M == 0, "the boot tables map the image with 2 MiB blocks")
STACK_SIZE = 128K;
EMERGENCY_STACK_SIZE = 32K;

PHDRS
{
    text PT_LOAD FLAGS(5);
    rodata PT_LOAD FLAGS(4);
    data PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = KERNEL_VIRT;
    __kernel_start = .;

    .text : AT(ADDR(.text) - KERNEL_OFFSET) {
        __text_start = .;
        KEEP(*(.text.boot))
        *(.text.vectors)
        *(.text .text.*)
        . = ALIGN(4K);
        __text_end = .;
    } :text

    .rodata : AT(ADDR(.rodata) - KERNEL_OFFSET) {
        __rodata_start = .;
        *(.rodata .rodata.*)
        . = ALIGN(4K);
        __rodata_end = .;
    } :rodata

    .data : AT(ADDR(.data) - KERNEL_OFFSET) {
        __data_start = .;
        *(.data .data.*)
        . = ALIGN(16);
    } :data

    .bss (NOLOAD) : AT(ADDR(.bss) - KERNEL_OFFSET) {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        . = ALIGN(16);
        __bss_end = .;
        . = ALIGN(4K);
        __data_end = .;
    } :data

    /* Built by boot.rs; used until the final tables replace them. */
    .boot_tables (NOLOAD) : AT(ADDR(.boot_tables) - KERNEL_OFFSET) {
        __boot_tables_start = .;
        __boot_l0_low = .;
        . += 4K;
        __boot_l1_low = .;
        . += 4K;
        __boot_l0_high = .;
        . += 4K;
        __boot_l1_high = .;
        . += 4K;
        __boot_l2_high = .;
        . += 4K;
        __boot_tables_end = .;
    } :data

    /* The emergency stack is where a stack overflow gets reported from. */
    .stack (NOLOAD) : AT(ADDR(.stack) - KERNEL_OFFSET) {
        __emergency_stack_bottom = .;
        . += EMERGENCY_STACK_SIZE;
        __emergency_stack_top = .;
        __stack_guard = .;
        . += 4K;
        __stack_bottom = .;
        . += STACK_SIZE;
        __stack_top = .;
    } :data

    /* Page-aligned: boot.rs takes it with adrp. */
    ASSERT(. % 4K == 0, "the image ends on a page boundary")
    __kernel_end = .;

    /DISCARD/ : {
        *(.comment)
        *(.eh_frame .eh_frame_hdr)
        *(.note .note.*)
    }
}

__entry_phys = _start - KERNEL_OFFSET;
