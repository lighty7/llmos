//! Minimal ACPI table parser.
//!
//! Reads the tables the firmware publishes in memory, starting from the RSDP
//! whose address the bootloader passes in [`BootInfo::rsdp_addr`]:
//!
//! ```text
//! RSDP ──► XSDT (or RSDT on ACPI 1.0) ──► FACP, APIC (MADT), HPET, MCFG, ...
//! ```
//!
//! Only static tables are parsed. Interpreting AML bytecode (DSDT/SSDT) is
//! out of scope; we only list those tables.
//!
//! Specification: ACPI 6.5, chapter 5.2 "ACPI System Description Tables".
//!
//! [`BootInfo::rsdp_addr`]: bootloader_api::BootInfo::rsdp_addr

use alloc::string::String;
use alloc::vec::Vec;

use serde::Serialize;

use crate::memory::phys_to_virt;

/// Length of the common header shared by every system description table.
const SDT_HEADER_LEN: usize = 36;
/// MADT entry type: processor local APIC.
const MADT_LOCAL_APIC: u8 = 0;
/// MADT entry type: I/O APIC.
const MADT_IO_APIC: u8 = 1;
/// MADT entry type: processor local x2APIC.
const MADT_LOCAL_X2APIC: u8 = 9;

/// What we learned from the ACPI tables.
#[derive(Debug, Clone, Serialize)]
pub struct AcpiInfo {
    /// ACPI revision from the RSDP (0 = ACPI 1.0, 2 = ACPI 2.0+).
    pub revision: u8,
    /// Firmware vendor ID from the RSDP.
    pub oem_id: String,
    /// Signatures of all tables listed in the RSDT/XSDT.
    pub tables: Vec<String>,
    /// Usable (enabled or online-capable) processors listed in the MADT.
    pub processors: usize,
    /// I/O APICs listed in the MADT.
    pub io_apics: usize,
    /// Physical address of the local APIC.
    pub local_apic_address: u64,
}

/// Errors encountered while parsing ACPI tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpiError {
    /// The RSDP signature was wrong.
    BadRsdpSignature,
    /// A table checksum did not add up to zero.
    BadChecksum(&'static str),
}

/// Reads `N` bytes of physical memory.
///
/// # Safety
///
/// `phys..phys + N` must be readable firmware-provided memory.
unsafe fn read_bytes<const N: usize>(phys: u64) -> [u8; N] {
    // SAFETY: guaranteed by the caller.
    unsafe { core::ptr::read_unaligned(phys_to_virt(phys) as *const [u8; N]) }
}

/// Returns a byte slice over `len` bytes of physical memory.
///
/// # Safety
///
/// The range must be readable firmware-provided memory that is not modified
/// while the slice is alive.
unsafe fn phys_slice<'a>(phys: u64, len: usize) -> &'a [u8] {
    // SAFETY: guaranteed by the caller.
    unsafe { core::slice::from_raw_parts(phys_to_virt(phys), len) }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap())
}

/// ACPI checksums: all bytes of a structure must sum to 0 (mod 256).
fn checksum_ok(bytes: &[u8]) -> bool {
    bytes.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) == 0
}

fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| {
            if b.is_ascii_graphic() || b == b' ' {
                b as char
            } else {
                '?'
            }
        })
        .collect::<String>()
        .trim_end()
        .into()
}

/// A validated system description table in physical memory.
struct Sdt {
    signature: [u8; 4],
    bytes: &'static [u8],
}

/// Reads and validates the table at `phys`.
///
/// # Safety
///
/// `phys` must point at an ACPI table published by the firmware.
unsafe fn read_sdt(phys: u64) -> Result<Sdt, AcpiError> {
    // SAFETY: guaranteed by the caller; the header is always 36 bytes.
    let header: [u8; SDT_HEADER_LEN] = unsafe { read_bytes(phys) };
    let len = u32_at(&header, 4) as usize;
    // SAFETY: the header's length field covers the whole table.
    let bytes = unsafe { phys_slice(phys, len.max(SDT_HEADER_LEN)) };
    if !checksum_ok(bytes) {
        return Err(AcpiError::BadChecksum("system description table"));
    }
    Ok(Sdt {
        signature: header[0..4].try_into().unwrap(),
        bytes,
    })
}

/// Parses the ACPI tables reachable from the RSDP at `rsdp_phys`.
///
/// # Safety
///
/// `rsdp_phys` must be the RSDP address reported by the firmware, and all
/// physical memory must be mapped (see [`crate::memory::init`]).
pub unsafe fn parse(rsdp_phys: u64) -> Result<AcpiInfo, AcpiError> {
    // SAFETY: the RSDP is at least 36 bytes in ACPI 2.0+ and 20 in 1.0;
    // reading 36 bytes stays within the firmware's reserved area.
    let rsdp: [u8; 36] = unsafe { read_bytes(rsdp_phys) };
    if &rsdp[0..8] != b"RSD PTR " {
        return Err(AcpiError::BadRsdpSignature);
    }
    if !checksum_ok(&rsdp[0..20]) {
        return Err(AcpiError::BadChecksum("RSDP"));
    }
    let revision = rsdp[15];
    let oem_id = ascii(&rsdp[9..15]);

    // ACPI 2.0+ provides the 64-bit XSDT; fall back to the 32-bit RSDT.
    let (root_phys, entry_size) =
        if revision >= 2 && checksum_ok(&rsdp[0..36]) && u64_at(&rsdp, 24) != 0 {
            (u64_at(&rsdp, 24), 8)
        } else {
            (u32_at(&rsdp, 16) as u64, 4)
        };
    // SAFETY: the root table address comes from the validated RSDP.
    let root = unsafe { read_sdt(root_phys)? };

    let mut info = AcpiInfo {
        revision,
        oem_id,
        tables: Vec::new(),
        processors: 0,
        io_apics: 0,
        local_apic_address: 0,
    };
    let entries = &root.bytes[SDT_HEADER_LEN..];
    for entry in entries.chunks_exact(entry_size) {
        let phys = if entry_size == 8 {
            u64_at(entry, 0)
        } else {
            u32_at(entry, 0) as u64
        };
        // SAFETY: entries of the validated root table point at ACPI tables.
        let Ok(table) = (unsafe { read_sdt(phys) }) else {
            continue;
        };
        info.tables.push(ascii(&table.signature));
        if &table.signature == b"APIC" {
            parse_madt(table.bytes, &mut info);
        }
    }
    Ok(info)
}

/// Counts processors and I/O APICs in the Multiple APIC Description Table.
fn parse_madt(madt: &[u8], info: &mut AcpiInfo) {
    info.local_apic_address = u32_at(madt, SDT_HEADER_LEN) as u64;
    // Entries start after the header, the LAPIC address and the flags.
    let mut offset = SDT_HEADER_LEN + 8;
    while offset + 2 <= madt.len() {
        let kind = madt[offset];
        let len = madt[offset + 1] as usize;
        if len < 2 || offset + len > madt.len() {
            break;
        }
        let entry = &madt[offset..offset + len];
        match kind {
            // Flags bit 0: enabled, bit 1: online capable.
            MADT_LOCAL_APIC if len >= 8 && u32_at(entry, 4) & 0b11 != 0 => info.processors += 1,
            MADT_LOCAL_X2APIC if len >= 16 && u32_at(entry, 8) & 0b11 != 0 => info.processors += 1,
            MADT_IO_APIC => info.io_apics += 1,
            _ => {}
        }
        offset += len;
    }
}
