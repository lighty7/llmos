//! Interrupt Descriptor Table: CPU exception and hardware interrupt handlers.
//!
//! Exceptions that indicate a kernel bug (page fault, general protection
//! fault, double fault, ...) print a diagnostic and halt. The breakpoint
//! exception is recoverable and is used by the self-test. Hardware interrupts
//! from the PIC drive the timer and keyboard.

use core::sync::atomic::{AtomicU64, Ordering};

use spin::Lazy;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::pic::{self, Irq};
use crate::{gdt, keyboard, println, time};

/// Number of breakpoint exceptions handled (checked by the self-test).
pub static BREAKPOINTS: AtomicU64 = AtomicU64::new(0);

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
    idt.general_protection_fault
        .set_handler_fn(general_protection_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.simd_floating_point.set_handler_fn(simd_handler);
    // SAFETY: the IST index refers to a valid, unused stack set up in `gdt`.
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_handler)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt[Irq::Timer.vector()].set_handler_fn(timer_handler);
    idt[Irq::Keyboard.vector()].set_handler_fn(keyboard_handler);
    idt
});

/// Loads the IDT. Requires [`gdt::init`] to have run first.
pub fn init_idt() {
    IDT.load();
}

/// Remaps the PICs, starts the timer and enables interrupts.
pub fn enable_hardware_interrupts() {
    // SAFETY: called once during boot while interrupts are still disabled.
    unsafe { pic::init() };
    time::init();
    x86_64::instructions::interrupts::enable();
}

extern "x86-interrupt" fn breakpoint_handler(_frame: InterruptStackFrame) {
    BREAKPOINTS.fetch_add(1, Ordering::Relaxed);
}

extern "x86-interrupt" fn invalid_opcode_handler(frame: InterruptStackFrame) {
    panic!("EXCEPTION: invalid opcode\n{frame:#?}");
}

extern "x86-interrupt" fn general_protection_handler(frame: InterruptStackFrame, code: u64) {
    panic!("EXCEPTION: general protection fault (code {code:#x})\n{frame:#?}");
}

extern "x86-interrupt" fn simd_handler(frame: InterruptStackFrame) {
    panic!("EXCEPTION: SIMD floating-point error\n{frame:#?}");
}

extern "x86-interrupt" fn page_fault_handler(frame: InterruptStackFrame, code: PageFaultErrorCode) {
    let addr = Cr2::read();
    panic!("EXCEPTION: page fault at {addr:?} ({code:?})\n{frame:#?}");
}

extern "x86-interrupt" fn double_fault_handler(frame: InterruptStackFrame, _code: u64) -> ! {
    println!("EXCEPTION: double fault\n{frame:#?}");
    panic!("double fault (likely a kernel stack overflow)");
}

extern "x86-interrupt" fn timer_handler(_frame: InterruptStackFrame) {
    time::tick();
    pic::end_of_interrupt(Irq::Timer);
}

extern "x86-interrupt" fn keyboard_handler(_frame: InterruptStackFrame) {
    keyboard::handle_interrupt();
    pic::end_of_interrupt(Irq::Keyboard);
}
