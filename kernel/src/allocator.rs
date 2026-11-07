//! Kernel heap.
//!
//! A contiguous virtual range starting at [`HEAP_START`] is backed by
//! physical frames and handed to a linked-list allocator, which makes the
//! `alloc` crate (`Box`, `Vec`, `String`, `BTreeMap`, ...) available.

use linked_list_allocator::LockedHeap;
use x86_64::VirtAddr;
use x86_64::structures::paging::mapper::MapToError;
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB};

/// Virtual address where the heap begins. Chosen far away from the kernel
/// image, the bootloader's mappings and the physical-memory window.
pub const HEAP_START: u64 = 0x4444_4444_0000;
/// Smallest heap the kernel will boot with.
pub const MIN_HEAP_SIZE: u64 = 16 * 1024 * 1024;
/// Largest heap the kernel creates.
pub const MAX_HEAP_SIZE: u64 = 512 * 1024 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Picks a heap size for a machine with `usable_bytes` of free RAM: half of
/// it, clamped to [`MIN_HEAP_SIZE`]..=[`MAX_HEAP_SIZE`] and rounded to pages.
pub fn heap_size_for(usable_bytes: u64) -> u64 {
    let size = (usable_bytes / 2).clamp(MIN_HEAP_SIZE, MAX_HEAP_SIZE);
    size & !(crate::memory::PAGE_SIZE - 1)
}

/// Maps `size` bytes of heap and initialises the global allocator.
///
/// # Errors
///
/// Fails if physical frames run out or a page is already mapped.
pub fn init(
    size: u64,
    mapper: &mut impl Mapper<Size4KiB>,
    frames: &mut impl FrameAllocator<Size4KiB>,
) -> Result<(), MapToError<Size4KiB>> {
    let start = Page::containing_address(VirtAddr::new(HEAP_START));
    let end = Page::containing_address(VirtAddr::new(HEAP_START + size - 1));
    let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::NO_EXECUTE;
    for page in Page::range_inclusive(start, end) {
        let frame = frames
            .allocate_frame()
            .ok_or(MapToError::FrameAllocationFailed)?;
        // SAFETY: the heap range is reserved for this purpose and the frame
        // is freshly allocated, so the mapping aliases nothing.
        unsafe { mapper.map_to(page, frame, flags, frames)?.ignore() };
    }
    // The mappings above are new (never cached), so no TLB flush is needed.
    // SAFETY: the range is now mapped, writable and used by nothing else.
    unsafe { ALLOCATOR.lock().init(HEAP_START as *mut u8, size as usize) };
    Ok(())
}

/// Returns `(used, free)` heap bytes.
pub fn usage() -> (usize, usize) {
    let heap = ALLOCATOR.lock();
    (heap.used(), heap.free())
}
