//! Hardware inventory.
//!
//! Collects what the machine has (CPU, memory, ACPI tables, PCI devices,
//! display, boot RAM disk) into an [`Inventory`] that can be printed for
//! humans ([`Inventory::print_summary`]), serialised to JSON
//! ([`Inventory::to_json`]) or condensed into a short text for the LLM's
//! prompt ([`Inventory::llm_summary`]).
//!
//! Discovery is deterministic code reading firmware tables and hardware
//! registers. The LLM consumes the *result*; it never probes hardware itself.

pub mod acpi;
pub mod pci;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use bootloader_api::BootInfo;
use bootloader_api::info::FrameBufferInfo;
use raw_cpuid::CpuId;
use serde::Serialize;

use crate::memory::MemoryStats;
use crate::{MIB, println};

/// Everything the kernel knows about the machine.
#[derive(Debug, Clone, Serialize)]
pub struct Inventory {
    /// Processor information from CPUID.
    pub cpu: CpuInfo,
    /// RAM totals from the firmware memory map.
    pub memory: MemoryInfo,
    /// ACPI data, or `None` if the tables were missing or invalid.
    pub acpi: Option<acpi::AcpiInfo>,
    /// Devices found on the PCI bus.
    pub pci_devices: Vec<pci::PciDevice>,
    /// Framebuffer resolution, if any.
    pub display: Option<DisplayInfo>,
    /// Size of the boot RAM disk (holds the LLM model), in bytes.
    pub ramdisk_bytes: u64,
    /// Disks found by the storage drivers (filled in after `storage::init`).
    pub disks: Vec<crate::storage::DiskInfo>,
}

/// Processor details.
#[derive(Debug, Clone, Serialize)]
pub struct CpuInfo {
    /// Vendor string, e.g. `GenuineIntel`.
    pub vendor: String,
    /// Marketing name, e.g. `Intel(R) Core(TM) i7 ...`.
    pub brand: String,
    /// Family / model / stepping.
    pub family: u8,
    /// Model number.
    pub model: u8,
    /// Stepping.
    pub stepping: u8,
    /// SIMD and math extensions relevant to inference speed.
    pub features: Vec<&'static str>,
    /// Hypervisor name if running virtualised.
    pub hypervisor: Option<String>,
}

/// Memory totals.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryInfo {
    /// Free RAM at boot, in MiB.
    pub usable_mib: u64,
    /// RAM used by the bootloader (kernel, page tables, RAM disk), in MiB.
    pub bootloader_mib: u64,
    /// Memory reserved by firmware/hardware, in MiB.
    pub reserved_mib: u64,
    /// Kernel heap size, in MiB.
    pub heap_mib: u64,
    /// Number of regions in the memory map.
    pub regions: usize,
}

/// Display details.
#[derive(Debug, Clone, Serialize)]
pub struct DisplayInfo {
    /// Width in pixels.
    pub width: usize,
    /// Height in pixels.
    pub height: usize,
    /// Pixel format.
    pub format: String,
}

impl CpuInfo {
    /// Queries CPUID.
    pub fn detect() -> Self {
        let cpuid = CpuId::new();
        let vendor = cpuid
            .get_vendor_info()
            .map(|v| v.as_str().to_string())
            .unwrap_or_default();
        let brand = cpuid
            .get_processor_brand_string()
            .map(|b| b.as_str().trim().to_string())
            .unwrap_or_default();
        let mut features = Vec::new();
        let (mut family, mut model, mut stepping) = (0, 0, 0);
        if let Some(f) = cpuid.get_feature_info() {
            family = f.family_id();
            model = f.model_id();
            stepping = f.stepping_id();
            let flags = [
                (f.has_sse(), "sse"),
                (f.has_sse2(), "sse2"),
                (f.has_sse3(), "sse3"),
                (f.has_ssse3(), "ssse3"),
                (f.has_sse41(), "sse4.1"),
                (f.has_sse42(), "sse4.2"),
                (f.has_avx(), "avx"),
                (f.has_fma(), "fma"),
            ];
            features.extend(flags.iter().filter(|(on, _)| *on).map(|(_, name)| *name));
        }
        if let Some(e) = cpuid.get_extended_feature_info() {
            if e.has_avx2() {
                features.push("avx2");
            }
            if e.has_avx512f() {
                features.push("avx512f");
            }
        }
        let hypervisor = cpuid
            .get_hypervisor_info()
            .map(|h| format!("{:?}", h.identify()));
        Self {
            vendor,
            brand,
            family,
            model,
            stepping,
            features,
            hypervisor,
        }
    }
}

impl Inventory {
    /// Gathers the inventory. Requires memory management to be initialised.
    pub fn collect(
        boot_info: &BootInfo,
        mem: MemoryStats,
        heap_bytes: u64,
        display: Option<FrameBufferInfo>,
    ) -> Self {
        let acpi = boot_info.rsdp_addr.into_option().and_then(|rsdp| {
            // SAFETY: the address comes from the firmware via the bootloader,
            // and physical memory is mapped.
            unsafe { acpi::parse(rsdp) }.ok()
        });
        Self {
            cpu: CpuInfo::detect(),
            memory: MemoryInfo {
                usable_mib: mem.usable_bytes / MIB,
                bootloader_mib: mem.bootloader_bytes / MIB,
                reserved_mib: mem.reserved_bytes / MIB,
                heap_mib: heap_bytes / MIB,
                regions: mem.regions,
            },
            acpi,
            pci_devices: pci::scan(),
            display: display.map(|d| DisplayInfo {
                width: d.width,
                height: d.height,
                format: format!("{:?}", d.pixel_format),
            }),
            ramdisk_bytes: boot_info.ramdisk_len,
            disks: Vec::new(),
        }
    }

    /// Number of logical processors (from ACPI; 1 if unknown).
    pub fn processors(&self) -> usize {
        self.acpi
            .as_ref()
            .map(|a| a.processors)
            .filter(|&n| n > 0)
            .unwrap_or(1)
    }

    /// Pretty-printed JSON report.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_else(|e| format!("{{\"error\": \"{e}\"}}"))
    }

    /// Prints a short human-readable summary.
    pub fn print_summary(&self) {
        let cpu = &self.cpu;
        println!(
            "  cpu:     {} ({}), {} logical processor(s)",
            cpu.brand,
            cpu.vendor,
            self.processors()
        );
        println!("  simd:    {}", cpu.features.join(" "));
        if let Some(h) = &cpu.hypervisor {
            println!("  virt:    running under {h}");
        }
        println!(
            "  memory:  {} MiB usable, {} MiB heap",
            self.memory.usable_mib, self.memory.heap_mib
        );
        if let Some(a) = &self.acpi {
            println!(
                "  acpi:    rev {} by {}, tables: {}",
                a.revision,
                a.oem_id,
                a.tables.join(" ")
            );
        }
        println!("  pci:     {} device(s)", self.pci_devices.len());
        for d in &self.pci_devices {
            println!(
                "           {:02x}:{:02x}.{} {:04x}:{:04x} {} ({})",
                d.bus, d.device, d.function, d.vendor_id, d.device_id, d.kind, d.vendor
            );
        }
        for d in &self.disks {
            println!(
                "  disk:    {} {} MiB, {} [{}]",
                d.name,
                d.size_mib,
                d.role,
                d.partitions.join(", ")
            );
        }
        if let Some(d) = &self.display {
            println!("  display: {}x{} {}", d.width, d.height, d.format);
        }
    }

    /// A compact, one-paragraph description for the LLM's system prompt.
    pub fn llm_summary(&self) -> String {
        let mut kinds: Vec<&str> = self.pci_devices.iter().map(|d| d.kind.as_str()).collect();
        kinds.sort_unstable();
        kinds.dedup();
        let disks: Vec<String> = self
            .disks
            .iter()
            .map(|d| format!("{} {} MiB ({})", d.name, d.size_mib, d.role))
            .collect();
        let disks = if disks.is_empty() {
            String::from("none")
        } else {
            disks.join(", ")
        };
        format!(
            "CPU: {} with {} core(s). RAM: {} MiB. Devices: {}. Disks: {}.",
            if self.cpu.brand.is_empty() {
                &self.cpu.vendor
            } else {
                &self.cpu.brand
            },
            self.processors(),
            self.memory.usable_mib,
            kinds.join(", "),
            disks
        )
    }
}
