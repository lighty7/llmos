//! In-memory file system.
//!
//! A flat namespace of named files kept on the kernel heap. Contents are lost
//! at power-off; persistent storage arrives with the disk driver (Phase 6).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use spin::Mutex;

/// Longest allowed file name, in bytes.
pub const MAX_NAME_LEN: usize = 64;
/// Largest allowed file, in bytes.
pub const MAX_FILE_SIZE: usize = 1024 * 1024;

/// Reasons a file-system operation can fail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// No file with that name exists.
    NotFound,
    /// The name is empty, too long or contains invalid characters.
    InvalidName,
    /// The contents exceed [`MAX_FILE_SIZE`].
    TooLarge,
}

impl core::fmt::Display for FsError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            FsError::NotFound => "file not found",
            FsError::InvalidName => {
                "invalid file name (use 1-64 characters: letters, digits, . _ -)"
            }
            FsError::TooLarge => "file too large",
        })
    }
}

static FILES: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());

/// Checks that `name` is 1..=64 characters of `[A-Za-z0-9._-]`.
pub fn validate_name(name: &str) -> Result<(), FsError> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(FsError::InvalidName)
    }
}

/// Creates or replaces `name` with `data`.
pub fn write(name: &str, data: &[u8]) -> Result<(), FsError> {
    validate_name(name)?;
    if data.len() > MAX_FILE_SIZE {
        return Err(FsError::TooLarge);
    }
    FILES.lock().insert(String::from(name), Vec::from(data));
    Ok(())
}

/// Returns a copy of the contents of `name`.
pub fn read(name: &str) -> Result<Vec<u8>, FsError> {
    FILES.lock().get(name).cloned().ok_or(FsError::NotFound)
}

/// Returns `true` if `name` exists.
pub fn exists(name: &str) -> bool {
    FILES.lock().contains_key(name)
}

/// Deletes `name`.
pub fn remove(name: &str) -> Result<(), FsError> {
    FILES
        .lock()
        .remove(name)
        .map(|_| ())
        .ok_or(FsError::NotFound)
}

/// Lists `(name, size)` pairs in alphabetical order.
pub fn list() -> Vec<(String, usize)> {
    FILES
        .lock()
        .iter()
        .map(|(name, data)| (name.clone(), data.len()))
        .collect()
}
