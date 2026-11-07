//! # llmos kernel for AArch64
//!
//! The ARM port of llmos, for QEMU's `virt` board and Raspberry Pi class
//! boards. It shares the hardware-independent parts with the x86-64 kernel:
//! the [`llm`] inference engine and agent protocol, and the in-memory file
//! system (`kernel/src/fs.rs`). The platform layer is ARM specific:
//!
//! | x86-64 kernel            | this kernel                                  |
//! |--------------------------|----------------------------------------------|
//! | UEFI + `bootloader`      | Linux arm64 image header, self-relocation    |
//! | ACPI + PCI scan          | device tree ([`dtb`])                        |
//! | 16550 UART               | PL011 UART ([`uart`])                        |
//! | paging via `x86_64`      | identity-mapped MMU ([`mmu`])                |
//! | PIT timer                | ARM generic timer counter                    |
//! | RAM disk from bootloader | initrd from the device tree                  |
//!
//! Boot sequence: `boot.rs` (assembly) → [`rust_main`] → device tree → UART →
//! MMU → heap → model → shell.

#![no_std]
#![no_main]
#![warn(missing_docs)]

extern crate alloc;

mod boot;
mod dtb;
#[path = "../../kernel/src/fs.rs"]
mod fs;
mod mmu;
mod platform;
mod shell;
mod uart;

use alloc::vec::Vec;
use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

use linked_list_allocator::LockedHeap;

/// The real heap, set up once the device tree says where RAM is.
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Size of the bootstrap region used before the real heap exists (parsing
/// the device tree already needs `Vec`s).
const BOOTSTRAP_SIZE: usize = 1024 * 1024;

#[repr(C, align(16))]
struct Bootstrap([u8; BOOTSTRAP_SIZE]);
static mut BOOTSTRAP: Bootstrap = Bootstrap([0; BOOTSTRAP_SIZE]);
/// Bytes of the bootstrap region handed out so far.
static BOOTSTRAP_USED: AtomicUsize = AtomicUsize::new(0);
/// Set once [`ALLOCATOR`] is initialised.
static HEAP_READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Global allocator: a bump allocator over a static region until the real
/// heap is ready, then the linked-list heap. Bootstrap memory is never
/// reused (it holds the device tree, which lives forever anyway).
struct KernelAllocator;

// SAFETY: both strategies return unique, suitably aligned memory.
unsafe impl GlobalAlloc for KernelAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if HEAP_READY.load(Ordering::Acquire) {
            // SAFETY: forwarded with the caller's guarantees.
            return unsafe { ALLOCATOR.alloc(layout) };
        }
        let base = &raw mut BOOTSTRAP as usize;
        let mut used = BOOTSTRAP_USED.load(Ordering::Relaxed);
        loop {
            let start = (base + used).next_multiple_of(layout.align());
            let end = start + layout.size();
            if end > base + BOOTSTRAP_SIZE {
                return core::ptr::null_mut();
            }
            match BOOTSTRAP_USED.compare_exchange(
                used,
                end - base,
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return start as *mut u8,
                Err(now) => used = now,
            }
        }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let base = &raw mut BOOTSTRAP as usize;
        if (base..base + BOOTSTRAP_SIZE).contains(&(ptr as usize)) {
            return; // Bootstrap memory is not recycled.
        }
        // SAFETY: the pointer came from `ALLOCATOR` with this layout.
        unsafe { ALLOCATOR.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: KernelAllocator = KernelAllocator;

/// Largest heap the kernel creates.
const MAX_HEAP: u64 = 512 * 1024 * 1024;
/// Bytes per mebibyte.
pub const MIB: u64 = 1024 * 1024;

unsafe extern "C" {
    static __image_start: u8;
    static __image_end: u8;
}

/// Rust entry point, called by `boot.rs` with the device-tree address.
#[unsafe(no_mangle)]
pub extern "C" fn rust_main(dtb_addr: usize) -> ! {
    // SAFETY: the firmware passes the device tree in x0 (Linux boot protocol).
    let Some(dt) = (unsafe { dtb::DeviceTree::parse(dtb_addr) }) else {
        platform::halt()
    };

    // Console first. Without a PL011 there is nowhere to report anything.
    let Some(uart_node) = dt.find_compatible("arm,pl011") else {
        platform::halt()
    };
    let Some(&(uart_base, _)) = dt.regs(uart_node).first() else {
        platform::halt()
    };
    // SAFETY: the device tree says a PL011 lives at this address.
    unsafe { uart::init(uart_base as usize) };
    println!("llmos v{} (aarch64) booting", env!("CARGO_PKG_VERSION"));

    let ram = dt.memory();
    // SAFETY: boot-time, single core, MMU still off.
    unsafe { mmu::enable(&ram) };
    println!("[ ok ] mmu: identity map, caches on");

    // Heap: the largest free RAM range not used by the kernel, the device
    // tree or the initrd.
    // Linker-provided symbols: only their addresses are used.
    let (image_start, image_end) = (
        &raw const __image_start as u64,
        &raw const __image_end as u64,
    );
    let mut reserved = Vec::new();
    reserved.push((image_start, image_end));
    reserved.push((dtb_addr as u64, dtb_addr as u64 + dt.size as u64));
    let initrd = dt.initrd();
    if let Some(r) = initrd {
        reserved.push(r);
    }
    let (heap_start, heap_len) = largest_free_range(&ram, &reserved);
    let heap_len = heap_len.min(MAX_HEAP);
    // SAFETY: the range is RAM that nothing else uses (see above).
    unsafe {
        ALLOCATOR
            .lock()
            .init(heap_start as *mut u8, heap_len as usize)
    };
    HEAP_READY.store(true, Ordering::Release);
    let ram_mib: u64 = ram.iter().map(|r| r.1).sum::<u64>() / MIB;
    println!(
        "[ ok ] memory: {ram_mib} MiB RAM, {} MiB heap at {heap_start:#x}",
        heap_len / MIB
    );

    let info = platform::Info::from_device_tree(&dt, ram_mib);
    info.print();

    let agent = match initrd {
        None => {
            println!("[warn] llm: no initrd; natural-language commands are disabled");
            None
        }
        Some((start, end)) => {
            // SAFETY: the bootloader placed the initrd here and nothing else uses it.
            let bytes: &'static [u8] =
                unsafe { core::slice::from_raw_parts(start as *const u8, (end - start) as usize) };
            match shell::Agent::load(bytes, &info.llm_summary()) {
                Ok(agent) => {
                    println!("[ ok ] llm: {}", agent.summary());
                    Some(agent)
                }
                Err(e) => {
                    println!("[warn] llm: could not load the model from the initrd: {e}");
                    None
                }
            }
        }
    };
    shell::run(shell::Context { info, agent })
}

/// Returns the largest `(start, len)` inside `ram` that overlaps nothing in
/// `reserved`, aligned to 2 MiB.
fn largest_free_range(ram: &[(u64, u64)], reserved: &[(u64, u64)]) -> (u64, u64) {
    const ALIGN: u64 = 2 * MIB;
    let mut best = (0, 0);
    for &(start, size) in ram {
        // Cut the RAM range at every reserved range, keep the gaps.
        let mut points: Vec<(u64, u64)> = reserved
            .iter()
            .filter(|&&(s, e)| e > start && s < start + size)
            .copied()
            .collect();
        points.sort_unstable();
        let mut cursor = start;
        for (s, e) in points
            .iter()
            .copied()
            .chain(core::iter::once((start + size, start + size)))
        {
            let gap_start = cursor.div_ceil(ALIGN) * ALIGN;
            if s > gap_start && s - gap_start > best.1 {
                best = (gap_start, (s - gap_start) & !(ALIGN - 1));
            }
            cursor = cursor.max(e);
        }
    }
    best
}

/// Bytes used and free on the heap.
pub fn heap_usage() -> (usize, usize) {
    let heap = ALLOCATOR.lock();
    (heap.used(), heap.free())
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("\nKERNEL PANIC: {info}");
    platform::halt()
}
