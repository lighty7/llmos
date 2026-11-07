/*
 * Linker script for the llmos AArch64 kernel.
 *
 * The kernel is linked at address 0 as a static position-independent
 * executable and relocates itself at boot (see `src/boot.rs`), so the same
 * image runs wherever the firmware loads it: 0x80000 on a Raspberry Pi,
 * 0x40080000 on QEMU's `virt` board.
 */
ENTRY(_start)

SECTIONS
{
    . = 0;
    __image_start = .;

    .text : {
        KEEP(*(.text.boot))
        *(.text .text.*)
    }
    .rodata : ALIGN(16) { *(.rodata .rodata.*) }
    .data.rel.ro : ALIGN(16) { *(.data.rel.ro .data.rel.ro.*) }
    .got : ALIGN(8) { *(.got .got.*) }
    .data : ALIGN(16) { *(.data .data.*) }
    .rela.dyn : ALIGN(8) {
        __rela_start = .;
        *(.rela .rela.*)
        __rela_end = .;
    }
    .dynamic : { *(.dynamic) }
    .dynsym : { *(.dynsym) }
    .dynstr : { *(.dynstr) }
    .hash : { *(.hash) }
    .gnu.hash : { *(.gnu.hash) }

    . = ALIGN(4096);
    __bss_start = .;
    .bss (NOLOAD) : { *(.bss .bss.*) *(COMMON) }
    . = ALIGN(16);
    __bss_end = .;

    /* Boot stack: 1 MiB, not part of the file. */
    . = ALIGN(4096);
    __stack_bottom = .;
    . += 0x100000;
    __stack_top = .;

    __image_end = .;
    __image_size = __image_end - __image_start;

    /DISCARD/ : { *(.comment) *(.eh_frame*) *(.note*) *(.interp) }
}
