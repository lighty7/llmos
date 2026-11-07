//! GUID Partition Table (GPT): reading and writing.
//!
//! Disk layout for `n` sectors (512 bytes each):
//!
//! ```text
//! LBA 0          protective MBR (one partition of type 0xEE covering the disk)
//! LBA 1          primary GPT header
//! LBA 2..=33     partition entry array (128 entries × 128 bytes)
//! ...            partitions
//! LBA n-33..n-2  backup partition entry array
//! LBA n-1        backup GPT header
//! ```
//!
//! Specification: UEFI 2.10, chapter 5 "GUID Partition Table (GPT) Disk Layout".

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use super::{BlockDevice, BlockError, SECTOR_SIZE};

/// Number of entries in the partition array we write.
const ENTRY_COUNT: usize = 128;
/// Size of one partition entry.
const ENTRY_SIZE: usize = 128;
/// Sectors occupied by the partition entry array.
const ENTRY_SECTORS: u64 = (ENTRY_COUNT * ENTRY_SIZE / SECTOR_SIZE) as u64;
/// Size of the GPT header structure.
const HEADER_SIZE: usize = 92;

/// A GUID in its on-disk (mixed-endian) byte order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guid(pub [u8; 16]);

impl Guid {
    /// Builds a GUID from its textual fields, e.g.
    /// `C12A7328-F81F-11D2-BA4B-00A0C93EC93B` → `(0xC12A7328, 0xF81F, 0x11D2, [0xBA, 0x4B, ...])`.
    pub const fn new(a: u32, b: u16, c: u16, d: [u8; 8]) -> Self {
        let a = a.to_le_bytes();
        let b = b.to_le_bytes();
        let c = c.to_le_bytes();
        Guid([
            a[0], a[1], a[2], a[3], b[0], b[1], c[0], c[1], d[0], d[1], d[2], d[3], d[4], d[5],
            d[6], d[7],
        ])
    }

    /// The all-zero GUID marks an unused partition entry.
    pub fn is_zero(&self) -> bool {
        self.0 == [0; 16]
    }

    /// A random (version 4) GUID.
    pub fn random() -> Self {
        let mut bytes = [0u8; 16];
        for chunk in bytes.chunks_mut(8) {
            chunk.copy_from_slice(&super::random_u64().to_le_bytes());
        }
        bytes[7] = (bytes[7] & 0x0F) | 0x40; // Version 4.
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // RFC 4122 variant.
        Guid(bytes)
    }
}

/// Partition type of an EFI System Partition.
pub const ESP_TYPE: Guid = Guid::new(
    0xC12A7328,
    0xF81F,
    0x11D2,
    [0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B],
);
/// Partition type of an llmos data partition (llmfs). Chosen for this
/// project: `4C4C4D4F-5344-4154-8000-6C6C6D6F7331`.
pub const LLMOS_DATA_TYPE: Guid = Guid::new(
    0x4C4C4D4F,
    0x5344,
    0x4154,
    [0x80, 0x00, 0x6C, 0x6C, 0x6D, 0x6F, 0x73, 0x31],
);

/// One partition.
#[derive(Debug, Clone)]
pub struct Partition {
    /// Partition type.
    pub type_guid: Guid,
    /// Unique partition GUID.
    pub unique_guid: Guid,
    /// First sector.
    pub first_lba: u64,
    /// Last sector (inclusive).
    pub last_lba: u64,
    /// Name (UTF-16 in the table; ASCII here).
    pub name: String,
}

impl Partition {
    /// Number of sectors.
    pub fn sectors(&self) -> u64 {
        self.last_lba + 1 - self.first_lba
    }

    /// Human-readable role of the partition.
    pub fn kind(&self) -> &'static str {
        if self.type_guid == ESP_TYPE {
            "EFI system"
        } else if self.type_guid == LLMOS_DATA_TYPE {
            "llmos data"
        } else {
            "other"
        }
    }
}

/// CRC-32 (IEEE 802.3, reflected, polynomial 0xEDB88320) as used by GPT.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Reads the primary GPT. Returns `Ok(None)` if the disk has no valid GPT.
///
/// # Errors
///
/// Propagates I/O errors.
pub fn read(disk: &mut dyn BlockDevice) -> Result<Option<Vec<Partition>>, BlockError> {
    let mut header = vec![0u8; SECTOR_SIZE];
    disk.read(1, &mut header)?;
    if &header[0..8] != b"EFI PART" {
        return Ok(None);
    }
    let header_size = u32_at(&header, 12) as usize;
    if !(HEADER_SIZE..=SECTOR_SIZE).contains(&header_size) {
        return Ok(None);
    }
    let stored_crc = u32_at(&header, 16);
    let mut copy = header[..header_size].to_vec();
    copy[16..20].fill(0);
    if crc32(&copy) != stored_crc {
        return Ok(None);
    }
    let entries_lba = u64_at(&header, 72);
    let count = u32_at(&header, 80) as usize;
    let entry_size = u32_at(&header, 84) as usize;
    if entry_size < 128 || count == 0 || count > 1024 {
        return Ok(None);
    }
    let mut table = vec![0u8; (count * entry_size).div_ceil(SECTOR_SIZE) * SECTOR_SIZE];
    disk.read(entries_lba, &mut table)?;
    if crc32(&table[..count * entry_size]) != u32_at(&header, 88) {
        return Ok(None);
    }

    let mut partitions = Vec::new();
    for e in table[..count * entry_size].chunks_exact(entry_size) {
        let type_guid = Guid(e[0..16].try_into().unwrap());
        if type_guid.is_zero() {
            continue;
        }
        let name: String = e[56..128]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .map(|c| char::from_u32(c as u32).unwrap_or('?'))
            .collect();
        partitions.push(Partition {
            type_guid,
            unique_guid: Guid(e[16..32].try_into().unwrap()),
            first_lba: u64_at(e, 32),
            last_lba: u64_at(e, 40),
            name,
        });
    }
    Ok(Some(partitions))
}

/// First sector usable for partitions on any disk we format.
pub const FIRST_USABLE_LBA: u64 = 2 + ENTRY_SECTORS;

/// Last sector usable for partitions on a disk of `sectors` sectors.
pub fn last_usable_lba(sectors: u64) -> u64 {
    sectors - 2 - ENTRY_SECTORS
}

/// Writes a protective MBR plus primary and backup GPTs describing
/// `partitions`, replacing any existing partition table.
///
/// # Errors
///
/// Propagates I/O errors; fails with [`BlockError::OutOfRange`] if a
/// partition does not fit in the usable area or there are too many.
pub fn write(disk: &mut dyn BlockDevice, partitions: &[Partition]) -> Result<(), BlockError> {
    let sectors = disk.sector_count();
    let last = sectors - 1;
    let last_usable = last_usable_lba(sectors);
    if partitions.len() > ENTRY_COUNT
        || partitions.iter().any(|p| {
            p.first_lba < FIRST_USABLE_LBA || p.last_lba > last_usable || p.last_lba < p.first_lba
        })
    {
        return Err(BlockError::OutOfRange);
    }

    // Partition entry array.
    let mut table = vec![0u8; ENTRY_COUNT * ENTRY_SIZE];
    for (p, e) in partitions.iter().zip(table.chunks_exact_mut(ENTRY_SIZE)) {
        e[0..16].copy_from_slice(&p.type_guid.0);
        e[16..32].copy_from_slice(&p.unique_guid.0);
        e[32..40].copy_from_slice(&p.first_lba.to_le_bytes());
        e[40..48].copy_from_slice(&p.last_lba.to_le_bytes());
        for (i, c) in p.name.encode_utf16().take(36).enumerate() {
            e[56 + 2 * i..58 + 2 * i].copy_from_slice(&c.to_le_bytes());
        }
    }
    let table_crc = crc32(&table);
    let disk_guid = Guid::random();

    let header = |my_lba: u64, alternate: u64, entries_lba: u64| -> Vec<u8> {
        let mut h = vec![0u8; SECTOR_SIZE];
        h[0..8].copy_from_slice(b"EFI PART");
        h[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes()); // Revision 1.0.
        h[12..16].copy_from_slice(&(HEADER_SIZE as u32).to_le_bytes());
        h[24..32].copy_from_slice(&my_lba.to_le_bytes());
        h[32..40].copy_from_slice(&alternate.to_le_bytes());
        h[40..48].copy_from_slice(&FIRST_USABLE_LBA.to_le_bytes());
        h[48..56].copy_from_slice(&last_usable.to_le_bytes());
        h[56..72].copy_from_slice(&disk_guid.0);
        h[72..80].copy_from_slice(&entries_lba.to_le_bytes());
        h[80..84].copy_from_slice(&(ENTRY_COUNT as u32).to_le_bytes());
        h[84..88].copy_from_slice(&(ENTRY_SIZE as u32).to_le_bytes());
        h[88..92].copy_from_slice(&table_crc.to_le_bytes());
        let crc = crc32(&h[..HEADER_SIZE]);
        h[16..20].copy_from_slice(&crc.to_le_bytes());
        h
    };

    // Protective MBR.
    let mut mbr = vec![0u8; SECTOR_SIZE];
    let entry = &mut mbr[446..462];
    entry[1..4].copy_from_slice(&[0x00, 0x02, 0x00]); // CHS start (unused).
    entry[4] = 0xEE; // GPT protective.
    entry[5..8].copy_from_slice(&[0xFF, 0xFF, 0xFF]); // CHS end (unused).
    entry[8..12].copy_from_slice(&1u32.to_le_bytes());
    entry[12..16].copy_from_slice(&(last.min(u32::MAX as u64) as u32).to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;

    disk.write(0, &mbr)?;
    disk.write(2, &table)?;
    disk.write(1, &header(1, last, 2))?;
    disk.write(last - ENTRY_SECTORS, &table)?;
    disk.write(last, &header(last, 1, last - ENTRY_SECTORS))?;
    Ok(())
}

/// Self-test vector: CRC-32 of "123456789" is 0xCBF43926.
pub fn crc32_self_test() -> bool {
    crc32(b"123456789") == 0xCBF4_3926
}
