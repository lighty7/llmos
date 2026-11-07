//! Driver for virtio block devices (legacy / transitional PCI interface).
//!
//! virtio is the standard paravirtual device interface of QEMU, KVM, cloud
//! hypervisors and many emulators. The *legacy* interface used here exposes
//! its registers through an I/O-port BAR, which needs no MMIO mapping, and
//! is still offered by "transitional" devices (QEMU's default on the root
//! PCIe bus).
//!
//! Requests are submitted through one virtqueue and completion is polled:
//!
//! ```text
//! descriptor 0: request header  {type, reserved, sector}   (device reads)
//! descriptor 1: data buffer     n × 512 bytes             (device reads or writes)
//! descriptor 2: status byte                               (device writes)
//! ```
//!
//! Specification: "Virtual I/O Device (VIRTIO) Version 1.2", sections 2.7
//! (split virtqueues), 4.1.4.8 (legacy PCI interface) and 5.2 (block device).

use core::sync::atomic::{Ordering, fence};

use x86_64::instructions::port::Port;

use super::{BlockDevice, BlockError, SECTOR_SIZE};
use crate::inventory::pci::PciDevice;
use crate::memory::DmaBuffer;

/// PCI vendor ID of virtio devices.
pub const VIRTIO_VENDOR: u16 = 0x1AF4;
/// PCI device ID of a transitional virtio block device.
pub const VIRTIO_BLK_TRANSITIONAL: u16 = 0x1001;

/// Legacy register offsets from BAR0.
mod reg {
    pub const GUEST_FEATURES: u16 = 0x04;
    pub const QUEUE_PFN: u16 = 0x08;
    pub const QUEUE_SIZE: u16 = 0x0C;
    pub const QUEUE_SELECT: u16 = 0x0E;
    pub const QUEUE_NOTIFY: u16 = 0x10;
    pub const STATUS: u16 = 0x12;
    /// Device-specific configuration (without MSI-X): block capacity.
    pub const CAPACITY: u16 = 0x14;
}

/// Device status bits.
mod status {
    pub const ACKNOWLEDGE: u8 = 1;
    pub const DRIVER: u8 = 2;
    pub const DRIVER_OK: u8 = 4;
    pub const FAILED: u8 = 128;
}

const DESC_F_NEXT: u16 = 1;
const DESC_F_WRITE: u16 = 2;
const AVAIL_F_NO_INTERRUPT: u16 = 1;
const REQ_IN: u32 = 0; // Read from the device.
const REQ_OUT: u32 = 1; // Write to the device.
const QUEUE_ALIGN: usize = 4096;

/// Largest transfer per request (bounce-buffer size).
pub const MAX_SECTORS_PER_REQUEST: usize = 64; // 32 KiB

/// A virtqueue descriptor.
#[repr(C)]
#[derive(Clone, Copy)]
struct Descriptor {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

/// A virtio block device.
pub struct VirtioBlk {
    io_base: u16,
    sectors: u64,
    queue_size: u16,
    /// Descriptor table, available ring and used ring (legacy layout).
    queue: DmaBuffer,
    /// Request header, status byte and data bounce buffer.
    bounce: DmaBuffer,
    /// Our next index into the available ring.
    avail_idx: u16,
    /// Last used-ring index we consumed.
    used_idx: u16,
}

// Offsets inside `bounce`.
const HEADER_OFF: usize = 0;
const STATUS_OFF: usize = 16;
const DATA_OFF: usize = 512;

impl VirtioBlk {
    /// Initialises the device described by `pci`.
    ///
    /// Returns `None` if it is not a legacy-capable virtio block device or
    /// DMA memory cannot be allocated.
    pub fn init(pci: &PciDevice) -> Option<Self> {
        if pci.vendor_id != VIRTIO_VENDOR || pci.device_id != VIRTIO_BLK_TRANSITIONAL {
            return None;
        }
        let bar0 = pci.bar(0);
        if bar0 & 1 == 0 {
            return None; // Not an I/O BAR: modern-only device.
        }
        pci.enable_io_and_dma();
        let io_base = (bar0 & !0b11) as u16;

        // SAFETY (for all port accesses in this function): `io_base` is the
        // device's own legacy register block, decoded since I/O was enabled.
        let w8 = |off: u16, v: u8| unsafe { Port::<u8>::new(io_base + off).write(v) };
        w8(reg::STATUS, 0); // Reset.
        w8(reg::STATUS, status::ACKNOWLEDGE);
        w8(reg::STATUS, status::ACKNOWLEDGE | status::DRIVER);
        // We need no optional features.
        unsafe { Port::<u32>::new(io_base + reg::GUEST_FEATURES).write(0) };

        unsafe { Port::<u16>::new(io_base + reg::QUEUE_SELECT).write(0) };
        let queue_size = unsafe { Port::<u16>::new(io_base + reg::QUEUE_SIZE).read() };
        if queue_size == 0 {
            w8(reg::STATUS, status::FAILED);
            return None;
        }
        let queue = DmaBuffer::new(Self::queue_bytes(queue_size).div_ceil(4096))?;
        let bounce =
            DmaBuffer::new((DATA_OFF + MAX_SECTORS_PER_REQUEST * SECTOR_SIZE).div_ceil(4096))?;
        unsafe { Port::<u32>::new(io_base + reg::QUEUE_PFN).write((queue.phys / 4096) as u32) };

        let lo = unsafe { Port::<u32>::new(io_base + reg::CAPACITY).read() } as u64;
        let hi = unsafe { Port::<u32>::new(io_base + reg::CAPACITY + 4).read() } as u64;

        let mut dev = Self {
            io_base,
            sectors: hi << 32 | lo,
            queue_size,
            queue,
            bounce,
            avail_idx: 0,
            used_idx: 0,
        };
        // We poll for completion, so ask the device not to interrupt.
        dev.write_avail_u16(0, AVAIL_F_NO_INTERRUPT);
        w8(
            reg::STATUS,
            status::ACKNOWLEDGE | status::DRIVER | status::DRIVER_OK,
        );
        Some(dev)
    }

    /// Bytes needed by a legacy split virtqueue of `n` entries.
    fn queue_bytes(n: u16) -> usize {
        let n = n as usize;
        Self::used_offset(n as u16) + 6 + 8 * n
    }

    fn avail_offset(n: u16) -> usize {
        16 * n as usize
    }

    fn used_offset(n: u16) -> usize {
        (Self::avail_offset(n) + 6 + 2 * n as usize).div_ceil(QUEUE_ALIGN) * QUEUE_ALIGN
    }

    fn write_avail_u16(&mut self, index: usize, value: u16) {
        let off = Self::avail_offset(self.queue_size) + 2 * index;
        // SAFETY: the offset lies inside the queue buffer (see `queue_bytes`).
        unsafe { core::ptr::write_volatile(self.queue.virt.add(off) as *mut u16, value) };
    }

    fn read_used_idx(&self) -> u16 {
        let off = Self::used_offset(self.queue_size) + 2;
        // SAFETY: as above; the device updates this field, hence volatile.
        unsafe { core::ptr::read_volatile(self.queue.virt.add(off) as *const u16) }
    }

    fn set_descriptor(&mut self, i: u16, desc: Descriptor) {
        // SAFETY: `i < 3 <= queue_size`, inside the descriptor table.
        unsafe {
            core::ptr::write_volatile((self.queue.virt as *mut Descriptor).add(i as usize), desc)
        };
    }

    /// Performs one request of `count` sectors through the bounce buffer.
    fn request(&mut self, kind: u32, sector: u64, count: usize) -> Result<(), BlockError> {
        if sector + count as u64 > self.sectors {
            return Err(BlockError::OutOfRange);
        }
        let phys = self.bounce.phys;
        let buf = self.bounce.as_mut_slice();
        buf[HEADER_OFF..HEADER_OFF + 4].copy_from_slice(&kind.to_le_bytes());
        buf[HEADER_OFF + 4..HEADER_OFF + 8].copy_from_slice(&0u32.to_le_bytes());
        buf[HEADER_OFF + 8..HEADER_OFF + 16].copy_from_slice(&sector.to_le_bytes());
        buf[STATUS_OFF] = 0xFF;

        let data_flags = if kind == REQ_IN { DESC_F_WRITE } else { 0 };
        self.set_descriptor(
            0,
            Descriptor {
                addr: phys + HEADER_OFF as u64,
                len: 16,
                flags: DESC_F_NEXT,
                next: 1,
            },
        );
        self.set_descriptor(
            1,
            Descriptor {
                addr: phys + DATA_OFF as u64,
                len: (count * SECTOR_SIZE) as u32,
                flags: data_flags | DESC_F_NEXT,
                next: 2,
            },
        );
        self.set_descriptor(
            2,
            Descriptor {
                addr: phys + STATUS_OFF as u64,
                len: 1,
                flags: DESC_F_WRITE,
                next: 0,
            },
        );

        // Publish descriptor chain 0 in the available ring, then the index.
        let slot = (self.avail_idx % self.queue_size) as usize;
        self.write_avail_u16(2 + slot, 0);
        fence(Ordering::SeqCst);
        self.avail_idx = self.avail_idx.wrapping_add(1);
        self.write_avail_u16(1, self.avail_idx);
        fence(Ordering::SeqCst);
        // SAFETY: notifying queue 0 tells the device to process the request.
        unsafe { Port::<u16>::new(self.io_base + reg::QUEUE_NOTIFY).write(0) };

        // Poll for completion (bounded, so a broken device cannot hang us).
        let mut spins: u64 = 0;
        while self.read_used_idx() == self.used_idx {
            core::hint::spin_loop();
            spins += 1;
            if spins > 2_000_000_000 {
                return Err(BlockError::Timeout);
            }
        }
        self.used_idx = self.used_idx.wrapping_add(1);
        fence(Ordering::SeqCst);
        match self.bounce.as_slice()[STATUS_OFF] {
            0 => Ok(()),
            _ => Err(BlockError::Device),
        }
    }
}

impl BlockDevice for VirtioBlk {
    fn sector_count(&self) -> u64 {
        self.sectors
    }

    fn read(&mut self, sector: u64, buf: &mut [u8]) -> Result<(), BlockError> {
        if buf.len() % SECTOR_SIZE != 0 {
            return Err(BlockError::Unaligned);
        }
        for (i, chunk) in buf
            .chunks_mut(MAX_SECTORS_PER_REQUEST * SECTOR_SIZE)
            .enumerate()
        {
            let lba = sector + (i * MAX_SECTORS_PER_REQUEST) as u64;
            self.request(REQ_IN, lba, chunk.len() / SECTOR_SIZE)?;
            chunk.copy_from_slice(&self.bounce.as_slice()[DATA_OFF..DATA_OFF + chunk.len()]);
        }
        Ok(())
    }

    fn write(&mut self, sector: u64, buf: &[u8]) -> Result<(), BlockError> {
        if buf.len() % SECTOR_SIZE != 0 {
            return Err(BlockError::Unaligned);
        }
        for (i, chunk) in buf
            .chunks(MAX_SECTORS_PER_REQUEST * SECTOR_SIZE)
            .enumerate()
        {
            let lba = sector + (i * MAX_SECTORS_PER_REQUEST) as u64;
            self.bounce.as_mut_slice()[DATA_OFF..DATA_OFF + chunk.len()].copy_from_slice(chunk);
            self.request(REQ_OUT, lba, chunk.len() / SECTOR_SIZE)?;
        }
        Ok(())
    }
}
