//! llmfs: the on-disk format of the llmos data partition.
//!
//! The whole file system is one checksummed snapshot of the in-memory
//! [`fs`](crate::fs), rewritten on every change. That is simple and robust
//! for the small text files the shell creates. It is not suited to large or
//! frequently changing data, and a power cut during a write can lose the
//! latest change (the checksum detects the damage).
//!
//! ```text
//! offset 0   u8[8]  magic "LLMOSFS1"
//!            u32    version (1)
//!            u32    number of files
//!            u64    payload length in bytes
//!            u32    CRC-32 of the payload
//! offset 32  payload: for each file { u8 name_len; name; u32 size; data }
//! ```

use alloc::vec;
use alloc::vec::Vec;

use super::gpt::{Partition, crc32};
use super::{BlockDevice, BlockError, SECTOR_SIZE};
use crate::fs;

const MAGIC: &[u8; 8] = b"LLMOSFS1";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 32;

/// Errors specific to llmfs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmfsError {
    /// Underlying I/O failure.
    Io(BlockError),
    /// The partition does not contain an llmfs image (e.g. never formatted).
    NotFormatted,
    /// The checksum does not match: the data is damaged.
    Corrupt,
    /// The files do not fit in the partition.
    Full,
}

impl From<BlockError> for LlmfsError {
    fn from(e: BlockError) -> Self {
        LlmfsError::Io(e)
    }
}

impl core::fmt::Display for LlmfsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LlmfsError::Io(e) => write!(f, "{e}"),
            LlmfsError::NotFormatted => f.write_str("partition is not formatted"),
            LlmfsError::Corrupt => f.write_str("file system checksum mismatch"),
            LlmfsError::Full => f.write_str("data partition is full"),
        }
    }
}

/// Serialises every file of the in-memory file system.
fn serialize() -> Vec<u8> {
    let files = fs::list();
    let mut payload = Vec::new();
    for (name, _) in &files {
        let data = fs::read(name).unwrap_or_default();
        payload.push(name.len() as u8);
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
        payload.extend_from_slice(&data);
    }
    let mut image = vec![0u8; HEADER_LEN];
    image[0..8].copy_from_slice(MAGIC);
    image[8..12].copy_from_slice(&VERSION.to_le_bytes());
    image[12..16].copy_from_slice(&(files.len() as u32).to_le_bytes());
    image[16..24].copy_from_slice(&(payload.len() as u64).to_le_bytes());
    image[24..28].copy_from_slice(&crc32(&payload).to_le_bytes());
    image.extend_from_slice(&payload);
    image.resize(image.len().div_ceil(SECTOR_SIZE) * SECTOR_SIZE, 0);
    image
}

/// Writes the current in-memory files to `part` on `disk`.
///
/// # Errors
///
/// Fails on I/O errors or if the files do not fit.
pub fn save(disk: &mut dyn BlockDevice, part: &Partition) -> Result<(), LlmfsError> {
    let image = serialize();
    if (image.len() / SECTOR_SIZE) as u64 > part.sectors() {
        return Err(LlmfsError::Full);
    }
    disk.write(part.first_lba, &image)?;
    Ok(())
}

/// Loads the files stored in `part` into the in-memory file system.
/// Returns the number of files loaded.
///
/// # Errors
///
/// Fails on I/O errors, an unformatted partition or a checksum mismatch.
pub fn load(disk: &mut dyn BlockDevice, part: &Partition) -> Result<usize, LlmfsError> {
    let mut first = vec![0u8; SECTOR_SIZE];
    disk.read(part.first_lba, &mut first)?;
    if &first[0..8] != MAGIC || u32::from_le_bytes(first[8..12].try_into().unwrap()) != VERSION {
        return Err(LlmfsError::NotFormatted);
    }
    let count = u32::from_le_bytes(first[12..16].try_into().unwrap()) as usize;
    let len = u64::from_le_bytes(first[16..24].try_into().unwrap()) as usize;
    let crc = u32::from_le_bytes(first[24..28].try_into().unwrap());
    let total = (HEADER_LEN + len).div_ceil(SECTOR_SIZE) * SECTOR_SIZE;
    if (total / SECTOR_SIZE) as u64 > part.sectors() {
        return Err(LlmfsError::Corrupt);
    }
    let mut image = vec![0u8; total];
    disk.read(part.first_lba, &mut image)?;
    let payload = &image[HEADER_LEN..HEADER_LEN + len];
    if crc32(payload) != crc {
        return Err(LlmfsError::Corrupt);
    }

    let mut pos = 0;
    let mut loaded = 0;
    for _ in 0..count {
        let name_len = *payload.get(pos).ok_or(LlmfsError::Corrupt)? as usize;
        let name = payload
            .get(pos + 1..pos + 1 + name_len)
            .ok_or(LlmfsError::Corrupt)?;
        pos += 1 + name_len;
        let size_bytes = payload.get(pos..pos + 4).ok_or(LlmfsError::Corrupt)?;
        let size = u32::from_le_bytes(size_bytes.try_into().unwrap()) as usize;
        pos += 4;
        let data = payload.get(pos..pos + size).ok_or(LlmfsError::Corrupt)?;
        pos += size;
        let name = core::str::from_utf8(name).map_err(|_| LlmfsError::Corrupt)?;
        if fs::write(name, data).is_ok() {
            loaded += 1;
        }
    }
    Ok(loaded)
}
