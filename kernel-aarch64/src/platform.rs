//! Board information, time and power control on AArch64.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;

use serde::Serialize;

use crate::dtb::DeviceTree;
use crate::println;

/// What the kernel knows about the board (the ARM counterpart of the x86-64
/// `Inventory`).
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    /// Board model from the device tree.
    pub model: String,
    /// CPU core type, e.g. `arm,cortex-a72`.
    pub cpu: String,
    /// Number of CPU cores.
    pub cores: usize,
    /// RAM in MiB.
    pub ram_mib: u64,
    /// Distinct `compatible` strings of the peripherals.
    pub devices: Vec<String>,
    /// PSCI conduit (`hvc` / `smc`) used for power control, if any.
    pub psci: Option<String>,
}

impl Info {
    /// Collects the information from the device tree.
    pub fn from_device_tree(dt: &DeviceTree, ram_mib: u64) -> Self {
        let cpu = dt
            .nodes
            .iter()
            .find(|n| n.device_type == "cpu")
            .and_then(|n| n.compatible.first().cloned())
            .unwrap_or_else(|| "unknown".into());
        let mut devices: Vec<String> = dt
            .nodes
            .iter()
            .filter(|n| n.device_type != "cpu" && n.parent.is_some())
            .filter_map(|n| n.compatible.first().cloned())
            .collect();
        devices.sort_unstable();
        devices.dedup();
        Self {
            model: dt.model(),
            cpu,
            cores: dt.cpu_count().max(1),
            ram_mib,
            devices,
            psci: dt.psci_method(),
        }
    }

    /// Prints a human-readable summary.
    pub fn print(&self) {
        println!("  board:   {}", self.model);
        println!("  cpu:     {} x{}", self.cpu, self.cores);
        println!("  memory:  {} MiB", self.ram_mib);
        println!("  devices: {}", self.devices.join(" "));
        println!(
            "  power:   {}",
            self.psci
                .as_deref()
                .map_or("no PSCI".into(), |m| format!("PSCI via {m}"))
        );
    }

    /// A compact description for the LLM's prompt.
    pub fn llm_summary(&self) -> String {
        format!(
            "Board: {}. CPU: {} with {} core(s). RAM: {} MiB. Devices: {}. Disks: none.",
            self.model,
            self.cpu,
            self.cores,
            self.ram_mib,
            self.devices
                .iter()
                .take(12)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

/// Milliseconds since the counter started (ARM generic timer).
pub fn uptime_ms() -> u64 {
    let (count, freq): (u64, u64);
    // SAFETY: the virtual counter and its frequency are readable at EL1.
    unsafe {
        asm!("mrs {}, cntvct_el0", out(reg) count);
        asm!("mrs {}, cntfrq_el0", out(reg) freq);
    }
    if freq == 0 {
        0
    } else {
        count / (freq / 1000).max(1)
    }
}

/// PSCI function IDs (SMC32 calling convention).
const PSCI_SYSTEM_OFF: u64 = 0x8400_0008;
const PSCI_SYSTEM_RESET: u64 = 0x8400_0009;

fn psci_call(method: Option<&str>, function: u64) {
    // SAFETY: PSCI calls with these IDs do not return when they succeed and
    // touch no memory the kernel owns.
    unsafe {
        match method {
            Some("hvc") => asm!("hvc #0", inout("x0") function => _),
            Some("smc") => asm!("smc #0", inout("x0") function => _),
            _ => {}
        }
    }
}

/// Powers the board off (PSCI), or halts if that is unavailable.
pub fn poweroff(method: Option<&str>) -> ! {
    psci_call(method, PSCI_SYSTEM_OFF);
    halt()
}

/// Resets the board (PSCI), or halts if that is unavailable.
pub fn reboot(method: Option<&str>) -> ! {
    psci_call(method, PSCI_SYSTEM_RESET);
    halt()
}

/// Stops the CPU.
pub fn halt() -> ! {
    loop {
        // SAFETY: waiting for an event has no side effects.
        unsafe { asm!("wfe") };
    }
}
