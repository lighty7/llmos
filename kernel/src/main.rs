//! # llmos kernel
//!
//! Entry point and boot sequence of the llmos kernel.
//!
//! The UEFI firmware starts the [`bootloader`](https://docs.rs/bootloader)
//! UEFI application, which loads this kernel, sets up 64-bit paging, maps all
//! physical memory and jumps to [`kernel_main`] with a [`BootInfo`] describing
//! the machine. From there the kernel initialises itself in this order:
//!
//! 1. serial port and framebuffer console (so we can report progress),
//! 2. CPU features (SSE), GDT/TSS and the interrupt descriptor table,
//! 3. paging helpers, the physical frame allocator and the kernel heap,
//! 4. interrupt controllers, timer and keyboard,
//! 5. hardware inventory (ACPI, PCI, CPUID),
//! 6. the LLM model (if one was packed into the boot RAM disk),
//! 7. the interactive shell.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![warn(missing_docs)]

extern crate alloc;

use bootloader_api::config::{BootloaderConfig, Mapping};
use bootloader_api::{BootInfo, entry_point};
use x86_64::VirtAddr;

mod actions;
mod agent;
mod allocator;
mod console;
mod cpu;
mod framebuffer;
mod fs;
mod gdt;
mod interrupts;
mod inventory;
mod keyboard;
mod memory;
mod pic;
mod power;
mod selftest;
mod serial;
mod shell;
mod time;

/// Bootloader configuration embedded in the kernel binary.
///
/// We ask for all physical memory to be mapped at a bootloader-chosen virtual
/// offset, which lets the kernel access page tables and MMIO regions directly.
pub static BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    // The model's inference code is deeply nested; give it a roomy stack.
    config.kernel_stack_size = 1024 * 1024;
    config
};

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

/// Kernel entry point, called by the bootloader.
fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    // 1. Output first, so every later step can report progress or failure.
    serial::init();
    let mut display = None;
    if let Some(fb) = boot_info.framebuffer.take() {
        let info = fb.info();
        display = Some(info);
        console::init_screen(framebuffer::FrameBufferConsole::new(fb.into_buffer(), info));
    }
    // Everything else in `BootInfo` is only read from now on.
    let boot_info: &'static BootInfo = boot_info;

    console::set_color(framebuffer::Color::GREEN);
    println!("llmos v{} booting", env!("CARGO_PKG_VERSION"));
    console::set_color(framebuffer::Color::WHITE);

    // 2. CPU state.
    cpu::enable_sse();
    gdt::init();
    interrupts::init_idt();
    log_ok!("cpu: SSE enabled, GDT/TSS and IDT loaded");

    // 3. Memory.
    let phys_offset = boot_info
        .physical_memory_offset
        .into_option()
        .expect("bootloader did not map physical memory");
    // SAFETY: the bootloader mapped all physical memory at `phys_offset`
    // (requested in BOOTLOADER_CONFIG) and this is the only call.
    let mut mapper = unsafe { memory::init(VirtAddr::new(phys_offset)) };
    // SAFETY: the bootloader marks only genuinely free memory as `Usable`.
    let mut frames = unsafe { memory::BootInfoFrameAllocator::new(&boot_info.memory_regions) };
    let mem = memory::stats(&boot_info.memory_regions);
    let heap_size = allocator::heap_size_for(mem.usable_bytes);
    allocator::init(heap_size, &mut mapper, &mut frames).expect("heap initialisation failed");
    log_ok!(
        "memory: {} MiB usable, {} MiB heap at {:#x}",
        mem.usable_bytes / MIB,
        heap_size / MIB,
        allocator::HEAP_START
    );

    // 4. Hardware interrupts.
    interrupts::enable_hardware_interrupts();
    log_ok!(
        "interrupts: PIC remapped, timer at {} Hz, PS/2 keyboard",
        time::TICK_HZ
    );

    // 5. Hardware inventory.
    let inventory = inventory::Inventory::collect(boot_info, mem, heap_size, display);
    log_ok!(
        "inventory: {} PCI device(s), {} CPU(s)",
        inventory.pci_devices.len(),
        inventory.processors()
    );
    inventory.print_summary();

    // 6. The LLM, if the bootloader loaded a model as the RAM disk.
    let agent = load_agent(boot_info, &inventory);

    // 7. Interactive shell (never returns).
    shell::run(shell::Context { inventory, agent })
}

/// Loads the model from the boot RAM disk, if there is one.
fn load_agent(boot_info: &BootInfo, inventory: &inventory::Inventory) -> Option<agent::Agent> {
    let Some(addr) = boot_info.ramdisk_addr.into_option() else {
        log_warn!("llm: no model in the boot RAM disk; natural-language commands are disabled");
        return None;
    };
    // SAFETY: the bootloader mapped the RAM disk at `addr` with this length
    // and nothing else uses that memory.
    let ramdisk: &'static [u8] =
        unsafe { core::slice::from_raw_parts(addr as *const u8, boot_info.ramdisk_len as usize) };
    match agent::Agent::load(ramdisk, &inventory.llm_summary()) {
        Ok(agent) => {
            log_ok!("llm: {}", agent.summary());
            if !agent.has_prompt_cache() {
                log_warn!(
                    "llm: no baked prompt cache; the first request will be slow (see models/README.md)"
                );
            }
            Some(agent)
        }
        Err(e) => {
            log_warn!("llm: could not load the model: {e}");
            None
        }
    }
}

/// Bytes per mebibyte.
pub const MIB: u64 = 1024 * 1024;

/// Halts the CPU forever, waking only to service interrupts.
pub fn halt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

/// Called on any Rust panic: report it and stop the machine.
#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    x86_64::instructions::interrupts::disable();
    console::set_color(framebuffer::Color::RED);
    println!("\nKERNEL PANIC: {info}");
    halt_loop()
}
