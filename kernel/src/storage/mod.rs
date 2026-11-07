//! Block storage: disk drivers, partition tables, persistent files and the
//! installer.
//!
//! At boot, [`init`] probes the PCI devices for disks it can drive (currently
//! virtio-blk), reads their GPTs and, if it finds an *llmos data* partition,
//! loads the files stored there into the in-memory [`fs`](crate::fs). After
//! that, [`sync`] writes the files back after every change.

pub mod gpt;
pub mod install;
pub mod llmfs;
pub mod virtio_blk;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;
use spin::Mutex;

use crate::inventory::pci::PciDevice;
use gpt::Partition;

/// Bytes per sector.
pub const SECTOR_SIZE: usize = 512;

/// Errors from block devices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockError {
    /// The request extends past the end of the device.
    OutOfRange,
    /// The buffer length is not a multiple of [`SECTOR_SIZE`].
    Unaligned,
    /// The device reported an error.
    Device,
    /// The device did not complete the request.
    Timeout,
}

impl core::fmt::Display for BlockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            BlockError::OutOfRange => "sector out of range",
            BlockError::Unaligned => "buffer is not a whole number of sectors",
            BlockError::Device => "device reported an I/O error",
            BlockError::Timeout => "device timed out",
        })
    }
}

/// A device addressed in 512-byte sectors.
pub trait BlockDevice: Send {
    /// Total number of sectors.
    fn sector_count(&self) -> u64;

    /// Reads `buf.len() / 512` sectors starting at `sector`.
    ///
    /// # Errors
    ///
    /// See [`BlockError`].
    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockError>;

    /// Writes `buf.len() / 512` sectors starting at `sector`.
    ///
    /// # Errors
    ///
    /// See [`BlockError`].
    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockError>;
}

/// A disk known to the kernel.
pub struct Disk {
    /// Short name, e.g. `vd0`.
    pub name: String,
    /// The driver.
    pub device: Box<dyn BlockDevice>,
    /// GPT partitions, or `None` if the disk has no (valid) partition table.
    pub partitions: Option<Vec<Partition>>,
}

impl Disk {
    /// Capacity in MiB.
    pub fn size_mib(&self) -> u64 {
        self.device.sector_count() * SECTOR_SIZE as u64 / (1024 * 1024)
    }

    /// Whether the disk carries an EFI system partition (it can boot).
    pub fn is_bootable(&self) -> bool {
        self.partitions
            .as_ref()
            .is_some_and(|p| p.iter().any(|p| p.type_guid == gpt::ESP_TYPE))
    }

    /// One-word description of what is on the disk.
    pub fn role(&self) -> &'static str {
        match &self.partitions {
            None => "empty",
            Some(parts) if parts.iter().any(|p| p.type_guid == gpt::LLMOS_DATA_TYPE) => {
                "llmos installed"
            }
            Some(_) if self.is_bootable() => "llmos boot media",
            Some(_) => "partitioned",
        }
    }
}

/// Summary of a disk for the inventory and the LLM.
#[derive(Debug, Clone, Serialize)]
pub struct DiskInfo {
    /// Disk name.
    pub name: String,
    /// Capacity in MiB.
    pub size_mib: u64,
    /// What is on it (see [`Disk::role`]).
    pub role: &'static str,
    /// Partitions as `"<kind> <size> MiB"` strings.
    pub partitions: Vec<String>,
}

/// All disks found at boot.
pub static DISKS: Mutex<Vec<Disk>> = Mutex::new(Vec::new());

/// Where the persistent files live: `(disk index, partition)`.
static MOUNTED: Mutex<Option<(usize, Partition)>> = Mutex::new(None);

/// Probes `pci_devices` for supported disks and mounts the data partition.
/// Returns a one-line status message for the boot log.
pub fn init(pci_devices: &[PciDevice]) -> String {
    let mut disks = DISKS.lock();
    for dev in pci_devices {
        if let Some(blk) = virtio_blk::VirtioBlk::init(dev) {
            let mut device: Box<dyn BlockDevice> = Box::new(blk);
            let partitions = gpt::read(device.as_mut()).ok().flatten();
            let name = format!("vd{}", disks.len());
            disks.push(Disk {
                name,
                device,
                partitions,
            });
        }
    }
    if disks.is_empty() {
        return "no supported disks (virtio-blk); files are kept in RAM only".into();
    }
    let names: Vec<String> = disks
        .iter()
        .map(|d| format!("{} ({} MiB, {})", d.name, d.size_mib(), d.role()))
        .collect();
    let mut status = format!("disks: {}", names.join(", "));
    drop(disks);
    if let Some(msg) = mount_data_partition() {
        status.push_str("; ");
        status.push_str(&msg);
    }
    status
}

/// Finds the first llmos data partition and loads its files.
fn mount_data_partition() -> Option<String> {
    let mut disks = DISKS.lock();
    for (i, disk) in disks.iter_mut().enumerate() {
        let Some(part) = disk
            .partitions
            .as_ref()
            .and_then(|parts| parts.iter().find(|p| p.type_guid == gpt::LLMOS_DATA_TYPE))
            .cloned()
        else {
            continue;
        };
        let msg = match llmfs::load(disk.device.as_mut(), &part) {
            Ok(n) => format!("{n} file(s) loaded from {}", disk.name),
            Err(e) => format!("data partition on {} unreadable: {e}", disk.name),
        };
        *MOUNTED.lock() = Some((i, part));
        return Some(msg);
    }
    None
}

/// Makes `part` on disk `index` the persistent store (used by the installer).
pub fn mount(index: usize, part: Partition) {
    *MOUNTED.lock() = Some((index, part));
}

/// Name of the disk holding the persistent files, if any.
pub fn mounted_disk() -> Option<String> {
    let mounted = MOUNTED.lock();
    let (index, _) = mounted.as_ref()?;
    DISKS.lock().get(*index).map(|d| d.name.clone())
}

/// Writes the in-memory files to the data partition, if one is mounted.
///
/// # Errors
///
/// Returns the llmfs error message on failure.
pub fn sync() -> Result<bool, llmfs::LlmfsError> {
    let mounted = MOUNTED.lock();
    let Some((index, part)) = mounted.as_ref() else {
        return Ok(false);
    };
    let mut disks = DISKS.lock();
    let disk = disks
        .get_mut(*index)
        .ok_or(llmfs::LlmfsError::Io(BlockError::Device))?;
    llmfs::save(disk.device.as_mut(), part)?;
    Ok(true)
}

/// Describes every disk.
pub fn disk_info() -> Vec<DiskInfo> {
    DISKS
        .lock()
        .iter()
        .map(|d| DiskInfo {
            name: d.name.clone(),
            size_mib: d.size_mib(),
            role: d.role(),
            partitions: d
                .partitions
                .iter()
                .flatten()
                .map(|p| {
                    format!(
                        "{} {} MiB",
                        p.kind(),
                        p.sectors() * SECTOR_SIZE as u64 / (1024 * 1024)
                    )
                })
                .collect(),
        })
        .collect()
}

/// A pseudo-random number (for GUIDs). Seeded from the time-stamp counter;
/// not suitable for cryptography.
pub fn random_u64() -> u64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    // SAFETY: `rdtsc` is always available on x86-64 and has no side effects.
    let tsc = unsafe { core::arch::x86_64::_rdtsc() };
    let mut z = STATE
        .fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed)
        .wrapping_add(tsc);
    // splitmix64 finaliser.
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
