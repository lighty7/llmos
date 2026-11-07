//! PCI bus enumeration through the legacy configuration-space I/O ports.
//!
//! Every PCI function has a 256-byte configuration space holding its vendor,
//! device and class codes. We read it through ports `0xCF8` (address) and
//! `0xCFC` (data), which every PC chipset (including QEMU's q35) supports.
//! PCIe's memory-mapped ECAM (from the ACPI `MCFG` table) is a later step.
//!
//! Reference: <https://wiki.osdev.org/PCI>.

use alloc::string::String;
use alloc::vec::Vec;

use serde::Serialize;
use x86_64::instructions::port::Port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/// One PCI function found on the bus.
#[derive(Debug, Clone, Serialize)]
pub struct PciDevice {
    /// Bus number.
    pub bus: u8,
    /// Device (slot) number.
    pub device: u8,
    /// Function number.
    pub function: u8,
    /// Vendor ID, e.g. `0x8086` for Intel.
    pub vendor_id: u16,
    /// Device ID assigned by the vendor.
    pub device_id: u16,
    /// Base class code.
    pub class: u8,
    /// Subclass code.
    pub subclass: u8,
    /// Programming interface.
    pub prog_if: u8,
    /// Human-readable description of the class.
    pub kind: String,
    /// Human-readable vendor name, if known.
    pub vendor: String,
}

/// Configuration-space address of a register.
fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    0x8000_0000
        | (bus as u32) << 16
        | (device as u32) << 11
        | (function as u32) << 8
        | (offset as u32 & 0xFC)
}

/// Reads a 32-bit register from a function's configuration space.
pub fn read_config(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    // SAFETY: 0xCF8/0xCFC are the standard PCI configuration ports; reading
    // configuration space has no side effects.
    unsafe {
        Port::<u32>::new(CONFIG_ADDRESS).write(config_address(bus, device, function, offset));
        Port::<u32>::new(CONFIG_DATA).read()
    }
}

/// Writes a 32-bit register in a function's configuration space.
///
/// # Safety
///
/// Changing configuration registers can remap device memory or enable DMA;
/// the caller must know what the register does for this device.
pub unsafe fn write_config(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    // SAFETY: the ports are the standard configuration mechanism; the effect
    // of the write is the caller's responsibility.
    unsafe {
        Port::<u32>::new(CONFIG_ADDRESS).write(config_address(bus, device, function, offset));
        Port::<u32>::new(CONFIG_DATA).write(value);
    }
}

impl PciDevice {
    /// Reads base address register `index` (0..=5).
    pub fn bar(&self, index: u8) -> u32 {
        read_config(self.bus, self.device, self.function, 0x10 + index * 4)
    }

    /// Enables I/O-space decoding, memory-space decoding and bus mastering
    /// (DMA) in the command register.
    pub fn enable_io_and_dma(&self) {
        let cmd = read_config(self.bus, self.device, self.function, 0x04);
        // SAFETY: setting bits 0 (I/O), 1 (memory) and 2 (bus master) only
        // lets a device we are about to drive respond and perform DMA.
        unsafe { write_config(self.bus, self.device, self.function, 0x04, cmd | 0b111) };
    }
}

/// Scans every bus, device and function and returns those that respond.
pub fn scan() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    for bus in 0..=255u8 {
        for device in 0..32u8 {
            if read_config(bus, device, 0, 0) & 0xFFFF == 0xFFFF {
                continue; // No device in this slot.
            }
            let header_type = (read_config(bus, device, 0, 0x0C) >> 16) as u8;
            let functions = if header_type & 0x80 != 0 { 8 } else { 1 };
            for function in 0..functions {
                let id = read_config(bus, device, function, 0);
                if id & 0xFFFF == 0xFFFF {
                    continue;
                }
                let class_reg = read_config(bus, device, function, 0x08);
                let (class, subclass, prog_if) = (
                    (class_reg >> 24) as u8,
                    (class_reg >> 16) as u8,
                    (class_reg >> 8) as u8,
                );
                let vendor_id = id as u16;
                devices.push(PciDevice {
                    bus,
                    device,
                    function,
                    vendor_id,
                    device_id: (id >> 16) as u16,
                    class,
                    subclass,
                    prog_if,
                    kind: describe_class(class, subclass, prog_if).into(),
                    vendor: vendor_name(vendor_id).into(),
                });
            }
        }
    }
    devices
}

/// Names the most common PCI class/subclass combinations.
pub fn describe_class(class: u8, subclass: u8, prog_if: u8) -> &'static str {
    match (class, subclass, prog_if) {
        (0x01, 0x01, _) => "IDE controller",
        (0x01, 0x06, 0x01) => "SATA controller (AHCI)",
        (0x01, 0x06, _) => "SATA controller",
        (0x01, 0x08, 0x02) => "NVMe controller",
        (0x01, 0x00, _) => "SCSI controller",
        (0x01, _, _) => "storage controller",
        (0x02, 0x00, _) => "Ethernet controller",
        (0x02, 0x80, _) => "network controller",
        (0x02, _, _) => "network controller",
        (0x03, 0x00, _) => "VGA-compatible display controller",
        (0x03, _, _) => "display controller",
        (0x04, 0x03, _) => "audio device",
        (0x04, _, _) => "multimedia controller",
        (0x05, _, _) => "memory controller",
        (0x06, 0x00, _) => "host bridge",
        (0x06, 0x01, _) => "ISA bridge",
        (0x06, 0x04, _) => "PCI-to-PCI bridge",
        (0x06, _, _) => "bridge",
        (0x07, 0x00, _) => "serial controller",
        (0x07, _, _) => "communication controller",
        (0x08, _, _) => "system peripheral",
        (0x09, _, _) => "input device controller",
        (0x0C, 0x03, 0x00) => "USB 1.1 UHCI controller",
        (0x0C, 0x03, 0x10) => "USB 1.1 OHCI controller",
        (0x0C, 0x03, 0x20) => "USB 2.0 EHCI controller",
        (0x0C, 0x03, 0x30) => "USB 3.x xHCI controller",
        (0x0C, 0x05, _) => "SMBus controller",
        (0x0C, _, _) => "serial bus controller",
        (0x0D, _, _) => "wireless controller",
        (0x12, _, _) => "processing accelerator",
        _ => "unknown device",
    }
}

/// Names a handful of well-known PCI vendors.
pub fn vendor_name(vendor_id: u16) -> &'static str {
    match vendor_id {
        0x8086 => "Intel",
        0x1022 => "AMD",
        0x10DE => "NVIDIA",
        0x1002 => "AMD/ATI",
        0x1AF4 => "Red Hat (virtio)",
        0x1B36 => "Red Hat (QEMU)",
        0x1234 => "QEMU/Bochs",
        0x15AD => "VMware",
        0x80EE => "VirtualBox",
        0x10EC => "Realtek",
        0x14E4 => "Broadcom",
        0x144D => "Samsung",
        0x1B4B => "Marvell",
        _ => "unknown",
    }
}
