//! Global Descriptor Table and Task State Segment.
//!
//! In 64-bit mode segmentation is mostly unused, but the CPU still needs a
//! GDT with a code segment, and a TSS whose *Interrupt Stack Table* lets the
//! double-fault handler run on a known-good stack. Without that, a kernel
//! stack overflow would cause a triple fault and an immediate reboot instead
//! of a readable error message.

use spin::Lazy;
use x86_64::VirtAddr;
use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

/// IST slot used by the double-fault handler.
pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

/// Size of the dedicated double-fault stack.
const IST_STACK_SIZE: usize = 5 * 4096;

/// Backing memory for the double-fault stack.
static mut DOUBLE_FAULT_STACK: [u8; IST_STACK_SIZE] = [0; IST_STACK_SIZE];

static TSS: Lazy<TaskStateSegment> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    // Stacks grow downwards, so the IST entry points at the *end* of the array.
    let stack_start = VirtAddr::from_ptr(&raw const DOUBLE_FAULT_STACK);
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] =
        stack_start + IST_STACK_SIZE as u64;
    tss
});

/// Segment selectors created alongside the GDT.
struct Selectors {
    code: SegmentSelector,
    data: SegmentSelector,
    tss: SegmentSelector,
}

static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let code = gdt.append(Descriptor::kernel_code_segment());
    let data = gdt.append(Descriptor::kernel_data_segment());
    let tss = gdt.append(Descriptor::tss_segment(&TSS));
    (gdt, Selectors { code, data, tss })
});

/// Loads the kernel's GDT and TSS and reloads the segment registers.
pub fn init() {
    GDT.0.load();
    // SAFETY: the selectors refer to valid descriptors in the GDT just loaded.
    unsafe {
        CS::set_reg(GDT.1.code);
        DS::set_reg(GDT.1.data);
        ES::set_reg(GDT.1.data);
        SS::set_reg(GDT.1.data);
        load_tss(GDT.1.tss);
    }
}
