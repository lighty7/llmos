//! MMU setup: an identity map of the low 4 GiB.
//!
//! With the MMU off, AArch64 treats every data access as *Device* memory:
//! uncached and alignment-checked. That is correct but extremely slow on real
//! hardware, so the kernel turns the MMU on as soon as it knows where RAM is.
//!
//! Translation uses a 4 KiB granule with a 32-bit input range (`T0SZ = 32`),
//! so the walk starts at level 1 with four 1 GiB entries. Each points to a
//! level-2 table of 2 MiB blocks:
//!
//! * blocks inside a device-tree `memory` range: Normal, write-back cacheable,
//! * everything else: Device-nGnRnE, never executable.
//!
//! Virtual addresses equal physical addresses, so nothing moves.
//!
//! Reference: Arm Architecture Reference Manual for A-profile, D8 "The AArch64
//! Virtual Memory System Architecture".

use core::arch::asm;

const BLOCK_2M: u64 = 2 * 1024 * 1024;
const ENTRIES: usize = 512;

/// MAIR attribute indices.
const ATTR_DEVICE: u64 = 0;
const ATTR_NORMAL: u64 = 1;

const DESC_VALID: u64 = 1 << 0;
const DESC_TABLE: u64 = 1 << 1; // Level 1: next-level table. Level 2: clear = block.
const DESC_AF: u64 = 1 << 10; // Access flag.
const DESC_SH_INNER: u64 = 3 << 8;
const DESC_PXN: u64 = 1 << 53;
const DESC_UXN: u64 = 1 << 54;

#[repr(C, align(4096))]
struct Table([u64; ENTRIES]);

static mut LEVEL1: Table = Table([0; ENTRIES]);
static mut LEVEL2: [Table; 4] = [const { Table([0; ENTRIES]) }; 4];

/// Builds the identity map and enables the MMU and caches.
///
/// `ram` lists the physical RAM ranges `(start, size)`. Only the first
/// 4 GiB of the address space are mapped.
///
/// # Safety
///
/// Must be called once, at boot, with the MMU off and nothing else running.
pub unsafe fn enable(ram: &[(u64, u64)]) {
    let is_ram = |addr: u64| {
        ram.iter()
            .any(|&(start, size)| addr >= start && addr + BLOCK_2M <= start + size)
    };
    // SAFETY: single-threaded boot code; the tables are not in use yet.
    unsafe {
        let level2 = &raw mut LEVEL2;
        for gib in 0..4 {
            let table = &mut (*level2)[gib].0;
            for (i, entry) in table.iter_mut().enumerate() {
                let addr = (gib as u64) << 30 | (i as u64) * BLOCK_2M;
                *entry = if is_ram(addr) {
                    addr | DESC_VALID | ATTR_NORMAL << 2 | DESC_SH_INNER | DESC_AF
                } else {
                    addr | DESC_VALID | ATTR_DEVICE << 2 | DESC_AF | DESC_PXN | DESC_UXN
                };
            }
            (*(&raw mut LEVEL1)).0[gib] =
                (&raw const (*level2)[gib]) as u64 | DESC_VALID | DESC_TABLE;
        }

        // Attr0 = Device-nGnRnE (0x00), Attr1 = Normal write-back RW-allocate (0xFF).
        let mair: u64 = 0xFF << 8;
        // PARange from ID_AA64MMFR0_EL1 becomes TCR_EL1.IPS.
        let mmfr0: u64;
        asm!("mrs {}, id_aa64mmfr0_el1", out(reg) mmfr0);
        let ips = (mmfr0 & 0b111).min(5);
        let tcr: u64 = 32            // T0SZ: 4 GiB of input address space.
            | 0b01 << 8              // IRGN0: inner write-back.
            | 0b01 << 10             // ORGN0: outer write-back.
            | 0b11 << 12             // SH0: inner shareable.
            | 0b00 << 14             // TG0: 4 KiB granule.
            | 1 << 23                // EPD1: no walks through TTBR1.
            | ips << 32;
        let ttbr0 = &raw const LEVEL1 as u64;

        asm!(
            "msr mair_el1, {mair}",
            "msr tcr_el1, {tcr}",
            "msr ttbr0_el1, {ttbr0}",
            "dsb ish",
            "isb",
            "tlbi vmalle1",
            "dsb ish",
            "isb",
            "mrs {tmp}, sctlr_el1",
            "orr {tmp}, {tmp}, #(1 << 0)",   // M: MMU on.
            "orr {tmp}, {tmp}, #(1 << 2)",   // C: data cache.
            "orr {tmp}, {tmp}, #(1 << 12)",  // I: instruction cache.
            "msr sctlr_el1, {tmp}",
            "isb",
            mair = in(reg) mair,
            tcr = in(reg) tcr,
            ttbr0 = in(reg) ttbr0,
            tmp = out(reg) _,
        );
    }
}
