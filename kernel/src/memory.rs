//! Physical and virtual memory management.
//!
//! * [`BootInfoFrameAllocator`] hands out unused 4 KiB physical frames, taken
//!   from the `Usable` regions of the firmware memory map.
//! * [`init`] wraps the active level-4 page table in an
//!   [`OffsetPageTable`], which works because the bootloader maps all of
//!   physical memory at a fixed virtual offset.

use core::sync::atomic::{AtomicU64, Ordering};

use bootloader_api::info::{MemoryRegionKind, MemoryRegions};
use spin::Mutex;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

/// Size of one page / frame.
pub const PAGE_SIZE: u64 = 4096;

/// Virtual address at which the bootloader mapped physical address 0.
static PHYSICAL_MEMORY_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Translates a physical address into a pointer through the physical-memory
/// window set up by the bootloader.
///
/// Dereferencing the pointer is only sound if `phys` is backed by RAM or MMIO
/// that may be read, and only after [`init`] has run.
pub fn phys_to_virt(phys: u64) -> *const u8 {
    (PHYSICAL_MEMORY_OFFSET.load(Ordering::Relaxed) + phys) as *const u8
}

/// Creates an [`OffsetPageTable`] for the currently active page tables.
///
/// # Safety
///
/// All physical memory must be mapped at `physical_memory_offset`, and this
/// function must only be called once (to avoid aliasing `&mut PageTable`s).
pub unsafe fn init(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    PHYSICAL_MEMORY_OFFSET.store(physical_memory_offset.as_u64(), Ordering::Relaxed);
    let (level_4_frame, _) = Cr3::read();
    let virt = physical_memory_offset + level_4_frame.start_address().as_u64();
    // SAFETY: guaranteed by the caller (see above).
    unsafe {
        let level_4_table = &mut *virt.as_mut_ptr::<PageTable>();
        OffsetPageTable::new(level_4_table, physical_memory_offset)
    }
}

/// A frame allocator that walks the usable regions of the boot memory map.
///
/// Frames are never freed; that is sufficient for the kernel heap and page
/// tables, which live for the whole uptime.
pub struct BootInfoFrameAllocator {
    regions: &'static MemoryRegions,
    /// Index of the region currently being consumed.
    region: usize,
    /// Next frame address to hand out within that region.
    next: u64,
}

impl BootInfoFrameAllocator {
    /// Creates an allocator over `regions`.
    ///
    /// # Safety
    ///
    /// Every region marked `Usable` must really be unused.
    pub unsafe fn new(regions: &'static MemoryRegions) -> Self {
        Self {
            regions,
            region: 0,
            next: 0,
        }
    }
}

// SAFETY: each frame is returned at most once and comes from a usable region.
unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame> {
        while let Some(region) = self.regions.get(self.region) {
            if region.kind == MemoryRegionKind::Usable {
                // Round the region start up to a frame boundary.
                let start = region.start.div_ceil(PAGE_SIZE) * PAGE_SIZE;
                self.next = self.next.max(start);
                if self.next + PAGE_SIZE <= region.end {
                    let frame = PhysFrame::containing_address(PhysAddr::new(self.next));
                    self.next += PAGE_SIZE;
                    return Some(frame);
                }
            }
            self.region += 1;
        }
        None
    }
}

/// Summary of the firmware memory map.
#[derive(Debug, Clone, Copy, Default)]
pub struct MemoryStats {
    /// Bytes in `Usable` regions (free RAM at boot).
    pub usable_bytes: u64,
    /// Bytes used by the bootloader (kernel image, page tables, boot info, RAM disk).
    pub bootloader_bytes: u64,
    /// Bytes reserved by the firmware or hardware.
    pub reserved_bytes: u64,
    /// Number of regions in the map.
    pub regions: usize,
}

/// Computes [`MemoryStats`] from the boot memory map.
pub fn stats(regions: &MemoryRegions) -> MemoryStats {
    let mut s = MemoryStats {
        regions: regions.len(),
        ..Default::default()
    };
    for r in regions.iter() {
        let len = r.end - r.start;
        match r.kind {
            MemoryRegionKind::Usable => s.usable_bytes += len,
            MemoryRegionKind::Bootloader => s.bootloader_bytes += len,
            _ => s.reserved_bytes += len,
        }
    }
    s
}

/// The kernel's frame allocator after boot (used for DMA buffers).
static FRAMES: Mutex<Option<BootInfoFrameAllocator>> = Mutex::new(None);

/// Hands the boot-time frame allocator over to the kernel for later use.
pub fn install_frame_allocator(frames: BootInfoFrameAllocator) {
    *FRAMES.lock() = Some(frames);
}

/// A physically contiguous, zeroed buffer for device DMA.
pub struct DmaBuffer {
    /// Physical address of the first byte (what devices are given).
    pub phys: u64,
    /// Kernel virtual address of the first byte.
    pub virt: *mut u8,
    /// Length in bytes.
    pub len: usize,
}

// SAFETY: the buffer is plain memory owned by one driver at a time.
unsafe impl Send for DmaBuffer {}

impl DmaBuffer {
    /// Allocates `pages` physically contiguous, zeroed 4 KiB pages.
    ///
    /// Frames are never freed, so drivers should allocate once at start-up.
    /// Returns `None` if no contiguous run is available.
    pub fn new(pages: usize) -> Option<Self> {
        let mut guard = FRAMES.lock();
        let frames = guard.as_mut()?;
        // The allocator hands out frames in address order, so runs are
        // usually contiguous; restart the run whenever there is a gap.
        let mut start = frames.allocate_frame()?.start_address().as_u64();
        let mut count = 1;
        while count < pages {
            let next = frames.allocate_frame()?.start_address().as_u64();
            if next == start + count as u64 * PAGE_SIZE {
                count += 1;
            } else {
                start = next;
                count = 1;
            }
        }
        let len = pages * PAGE_SIZE as usize;
        let virt = phys_to_virt(start) as *mut u8;
        // SAFETY: the frames were just allocated, so this memory is unused.
        unsafe { core::ptr::write_bytes(virt, 0, len) };
        Some(Self {
            phys: start,
            virt,
            len,
        })
    }

    /// The buffer as a byte slice.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: `virt..virt+len` is mapped memory owned by this buffer.
        unsafe { core::slice::from_raw_parts(self.virt, self.len) }
    }

    /// The buffer as a mutable byte slice.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: as above, and `&mut self` guarantees exclusive access.
        unsafe { core::slice::from_raw_parts_mut(self.virt, self.len) }
    }
}
