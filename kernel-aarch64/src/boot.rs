//! Boot entry point and exception vectors.
//!
//! The image starts with the 64-byte **Linux arm64 image header**, which
//! both QEMU (`-kernel`) and the Raspberry Pi firmware (`kernel8.img`)
//! understand. They load the image at `RAM base + text_offset` and jump to
//! it with the physical address of the device tree in `x0`.
//!
//! `_start` then, in assembly, before any Rust code runs:
//!
//! 1. parks every core except core 0,
//! 2. drops from EL2 to EL1 if the firmware entered at EL2 (Raspberry Pi),
//! 3. enables the FP/NEON unit (Rust uses it for all `f32` maths),
//! 4. sets up the boot stack and zeroes `.bss`,
//! 5. applies the `R_AARCH64_RELATIVE` relocations, because the kernel is
//!    linked at address 0 but may be loaded anywhere,
//! 6. calls [`crate::rust_main`] with the device-tree address.
//!
//! Reference: Linux `Documentation/arch/arm64/booting.rst`.

use core::arch::global_asm;

global_asm!(
    r#"
.section .text.boot, "ax"
.global _start
_start:
    // ---- Linux arm64 image header (64 bytes) ----
    b       1f                      // code0: jump over the header
    .word   0                       // code1
    .quad   0x80000                 // text_offset
    .quad   __image_size            // image_size, including .bss and the stack
    .quad   0xA                     // flags: little endian, 4K pages, load anywhere
    .quad   0, 0, 0                 // reserved
    .ascii  "ARM\x64"               // magic
    .word   0                       // reserved (PE header offset)

1:  mov     x19, x0                 // Keep the device-tree address.

    // Only core 0 boots; the others sleep forever.
    mrs     x1, mpidr_el1
    and     x1, x1, #0xff
    cbnz    x1, 9f

    // Drop from EL2 to EL1 if needed.
    mrs     x1, CurrentEL
    lsr     x1, x1, #2
    cmp     x1, #2
    b.ne    2f
    mov     x1, #(1 << 31)          // HCR_EL2.RW: EL1 runs in AArch64.
    msr     hcr_el2, x1
    mov     x1, #3                  // CNTHCTL_EL2: EL1 may use the physical timer.
    msr     cnthctl_el2, x1
    msr     cntvoff_el2, xzr
    mov     x1, #0x3c5              // SPSR: EL1h with DAIF masked.
    msr     spsr_el2, x1
    adr     x1, 2f
    msr     elr_el2, x1
    eret

2:  mov     x1, #(3 << 20)          // CPACR_EL1.FPEN: no FP/NEON traps.
    msr     cpacr_el1, x1
    isb

    adrp    x1, __stack_top
    add     x1, x1, :lo12:__stack_top
    mov     sp, x1

    // Zero .bss.
    adrp    x1, __bss_start
    add     x1, x1, :lo12:__bss_start
    adrp    x2, __bss_end
    add     x2, x2, :lo12:__bss_end
3:  cmp     x1, x2
    b.hs    4f
    str     xzr, [x1], #8
    b       3b

    // Apply R_AARCH64_RELATIVE (type 1027) relocations: *(base + off) = base + addend.
4:  adrp    x0, __image_start
    add     x0, x0, :lo12:__image_start
    adrp    x1, __rela_start
    add     x1, x1, :lo12:__rela_start
    adrp    x2, __rela_end
    add     x2, x2, :lo12:__rela_end
5:  cmp     x1, x2
    b.hs    6f
    ldp     x3, x4, [x1], #16       // r_offset, r_info
    ldr     x5, [x1], #8            // r_addend
    and     x4, x4, #0xffffffff
    cmp     x4, #1027
    b.ne    5b
    add     x5, x5, x0
    str     x5, [x0, x3]
    b       5b

6:  adrp    x1, exception_vectors
    add     x1, x1, :lo12:exception_vectors
    msr     vbar_el1, x1
    isb
    mov     x0, x19
    bl      rust_main

9:  wfe
    b       9b

// ---- Exception vector table: 16 entries of 128 bytes, 2 KiB aligned ----
.macro VECTOR kind
    .balign 0x80
    mov     x0, #\kind
    mrs     x1, esr_el1
    mrs     x2, elr_el1
    mrs     x3, far_el1
    b       exception_entry
.endm

.balign 0x800
exception_vectors:
    VECTOR 0
    VECTOR 1
    VECTOR 2
    VECTOR 3
    VECTOR 4
    VECTOR 5
    VECTOR 6
    VECTOR 7
    VECTOR 8
    VECTOR 9
    VECTOR 10
    VECTOR 11
    VECTOR 12
    VECTOR 13
    VECTOR 14
    VECTOR 15
"#
);

/// Common exception handler. llmos does not use interrupts or system calls
/// on AArch64 yet, so every exception is a bug: report it and stop.
#[unsafe(no_mangle)]
extern "C" fn exception_entry(kind: u64, esr: u64, elr: u64, far: u64) -> ! {
    const KINDS: [&str; 4] = ["synchronous", "IRQ", "FIQ", "SError"];
    panic!(
        "{} exception (vector {kind}): ESR_EL1={esr:#x} (class {:#x}) ELR_EL1={elr:#x} FAR_EL1={far:#x}",
        KINDS[(kind % 4) as usize],
        esr >> 26
    );
}
