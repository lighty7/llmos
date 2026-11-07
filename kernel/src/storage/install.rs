//! Installing llmos onto a disk.
//!
//! The installer copies the running system's EFI system partition (which
//! holds the UEFI bootloader, the kernel and the model RAM disk) onto an
//! **empty** disk and adds an llmos data partition for persistent files:
//!
//! ```text
//! target disk:  [GPT][ EFI system (copy of the boot media) ][ llmos data ][backup GPT]
//!                     1 MiB aligned                          up to 64 MiB
//! ```
//!
//! Safety rules:
//!
//! * only disks without a partition table are eligible, so existing data
//!   is never overwritten (wiping a disk is deliberately not offered);
//! * the plan is shown and the user must confirm before anything is written.

use alloc::format;
use alloc::string::String;
use alloc::vec;

use super::gpt::{self, Guid, Partition};
use super::{DISKS, SECTOR_SIZE, llmfs};
use crate::print;

/// Partition alignment: 1 MiB.
const ALIGN: u64 = 2048;
/// Preferred size of the data partition: 64 MiB.
const DATA_SECTORS: u64 = 64 * 2048;
/// Smallest acceptable data partition: 1 MiB.
const MIN_DATA_SECTORS: u64 = 2048;
/// Sectors copied per I/O round.
const COPY_CHUNK: u64 = 2048;

/// What the installer will do.
#[derive(Debug, Clone)]
pub struct InstallPlan {
    source: usize,
    source_esp: Partition,
    target: usize,
    target_name: String,
    target_mib: u64,
    esp: Partition,
    data: Partition,
}

fn align_up(lba: u64) -> u64 {
    lba.div_ceil(ALIGN) * ALIGN
}

/// Works out an installation onto `target` (a disk name such as `vd1`), or
/// onto the only eligible disk if `target` is `None`.
///
/// # Errors
///
/// Returns a human-readable reason when no safe installation is possible.
pub fn plan(target: Option<&str>) -> Result<InstallPlan, String> {
    let disks = DISKS.lock();
    let (source, source_esp) = disks
        .iter()
        .enumerate()
        .find_map(|(i, d)| {
            d.partitions
                .as_ref()?
                .iter()
                .find(|p| p.type_guid == gpt::ESP_TYPE)
                .map(|p| (i, p.clone()))
        })
        .ok_or("no boot media found: none of the disks has an EFI system partition")?;

    let empty: alloc::vec::Vec<usize> = (0..disks.len())
        .filter(|&i| i != source && disks[i].partitions.is_none())
        .collect();
    let target = match target {
        Some(name) => {
            let i = disks
                .iter()
                .position(|d| d.name == name)
                .ok_or_else(|| format!("no disk named {name}"))?;
            if i == source {
                return Err(format!("{name} is the boot media llmos is running from"));
            }
            if disks[i].partitions.is_some() {
                return Err(format!(
                    "{name} already has partitions; llmos only installs onto empty disks"
                ));
            }
            i
        }
        None => match empty.as_slice() {
            [only] => *only,
            [] => return Err("no empty disk to install onto".into()),
            _ => return Err("several empty disks; say which one, e.g. `install vd1`".into()),
        },
    };

    let sectors = disks[target].device.sector_count();
    let esp_first = align_up(gpt::FIRST_USABLE_LBA);
    let esp_last = esp_first + source_esp.sectors() - 1;
    let data_first = align_up(esp_last + 1);
    let last_usable = gpt::last_usable_lba(sectors);
    if data_first + MIN_DATA_SECTORS - 1 > last_usable {
        return Err(format!(
            "{} is too small: needs at least {} MiB",
            disks[target].name,
            (data_first + MIN_DATA_SECTORS + 64) * SECTOR_SIZE as u64 / (1024 * 1024) + 1
        ));
    }
    let data_last = (data_first + DATA_SECTORS - 1).min(last_usable);

    Ok(InstallPlan {
        source,
        target,
        target_name: disks[target].name.clone(),
        target_mib: disks[target].size_mib(),
        esp: Partition {
            type_guid: gpt::ESP_TYPE,
            unique_guid: Guid::random(),
            first_lba: esp_first,
            last_lba: esp_last,
            name: "llmos boot".into(),
        },
        data: Partition {
            type_guid: gpt::LLMOS_DATA_TYPE,
            unique_guid: Guid::random(),
            first_lba: data_first,
            last_lba: data_last,
            name: "llmos data".into(),
        },
        source_esp,
    })
}

impl InstallPlan {
    /// Describes the plan for the confirmation prompt.
    pub fn describe(&self) -> String {
        let mib = |p: &Partition| p.sectors() * SECTOR_SIZE as u64 / (1024 * 1024);
        format!(
            "install llmos onto {} ({} MiB): EFI system partition {} MiB (boot loader, kernel, model) + data partition {} MiB",
            self.target_name,
            self.target_mib,
            mib(&self.esp),
            mib(&self.data)
        )
    }

    /// Name of the target disk.
    pub fn target_name(&self) -> &str {
        &self.target_name
    }

    /// Performs the installation, printing progress.
    ///
    /// # Errors
    ///
    /// Returns a description of the failed step.
    pub fn execute(&self) -> Result<(), String> {
        // 1. Copy the EFI system partition in chunks.
        let total = self.source_esp.sectors();
        let mut buf = vec![0u8; COPY_CHUNK as usize * SECTOR_SIZE];
        let mut done = 0;
        let mut last_pct = u64::MAX;
        while done < total {
            let n = (total - done).min(COPY_CHUNK);
            let chunk = &mut buf[..n as usize * SECTOR_SIZE];
            {
                let mut disks = DISKS.lock();
                disks[self.source]
                    .device
                    .read(self.source_esp.first_lba + done, chunk)
                    .map_err(|e| format!("reading the boot media: {e}"))?;
                disks[self.target]
                    .device
                    .write(self.esp.first_lba + done, chunk)
                    .map_err(|e| format!("writing {}: {e}", self.target_name))?;
            }
            done += n;
            let pct = done * 100 / total;
            if pct / 10 != last_pct / 10 {
                print!("\r  copying boot partition: {pct}%");
                last_pct = pct;
            }
        }
        crate::println!();

        // 2. Partition table (written after the copy, so an interrupted
        //    install leaves a disk that is still recognised as empty).
        let mut disks = DISKS.lock();
        let target = &mut disks[self.target];
        let parts = [self.esp.clone(), self.data.clone()];
        gpt::write(target.device.as_mut(), &parts)
            .map_err(|e| format!("writing the partition table: {e}"))?;
        target.partitions = Some(parts.to_vec());

        // 3. Format the data partition with the current files and use it.
        llmfs::save(target.device.as_mut(), &self.data)
            .map_err(|e| format!("formatting the data partition: {e}"))?;
        drop(disks);
        super::mount(self.target, self.data.clone());
        Ok(())
    }
}
