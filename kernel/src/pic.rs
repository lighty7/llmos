//! Driver for the two cascaded Intel 8259 Programmable Interrupt Controllers.
//!
//! The PICs deliver legacy hardware interrupts (timer, keyboard, ...). At
//! reset they use vectors 0–15, which collide with CPU exceptions, so we remap
//! them to [`PIC_1_OFFSET`]..`PIC_1_OFFSET + 16`. Every handled interrupt must
//! be acknowledged with [`end_of_interrupt`].
//!
//! Reference: <https://wiki.osdev.org/8259_PIC>.

use x86_64::instructions::port::Port;

/// First interrupt vector used by the primary PIC (IRQ 0).
pub const PIC_1_OFFSET: u8 = 32;
/// First interrupt vector used by the secondary PIC (IRQ 8).
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

const PIC1_COMMAND: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_COMMAND: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

/// "Initialise, expect ICW4" command word.
const ICW1_INIT: u8 = 0x11;
/// 8086/88 mode.
const ICW4_8086: u8 = 0x01;
/// End-of-interrupt command.
const EOI: u8 = 0x20;

/// Hardware IRQ lines the kernel uses.
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum Irq {
    /// Programmable interval timer.
    Timer = 0,
    /// PS/2 keyboard.
    Keyboard = 1,
}

impl Irq {
    /// The interrupt vector this IRQ is delivered on after remapping.
    pub const fn vector(self) -> u8 {
        PIC_1_OFFSET + self as u8
    }
}

/// Writes to a PIC port followed by a short delay, as old PICs require.
///
/// # Safety
///
/// `port` must be one of the PIC ports and `value` a valid command for the
/// current initialisation step.
unsafe fn write(port: u16, value: u8) {
    unsafe {
        Port::<u8>::new(port).write(value);
        // Writing to the unused port 0x80 takes ~1µs: a portable I/O delay.
        Port::<u8>::new(0x80).write(0);
    }
}

/// Remaps both PICs and unmasks only the timer and keyboard IRQs.
///
/// # Safety
///
/// Must be called once with interrupts disabled, before they are enabled.
pub unsafe fn init() {
    unsafe {
        write(PIC1_COMMAND, ICW1_INIT);
        write(PIC2_COMMAND, ICW1_INIT);
        write(PIC1_DATA, PIC_1_OFFSET); // ICW2: vector offsets.
        write(PIC2_DATA, PIC_2_OFFSET);
        write(PIC1_DATA, 4); // ICW3: secondary PIC on IRQ2.
        write(PIC2_DATA, 2); // ICW3: secondary's cascade identity.
        write(PIC1_DATA, ICW4_8086);
        write(PIC2_DATA, ICW4_8086);
        // Masks: a set bit disables the line. Enable IRQ0 (timer), IRQ1
        // (keyboard) and IRQ2 (cascade); disable everything on the secondary.
        write(PIC1_DATA, !0b0000_0111);
        write(PIC2_DATA, 0xFF);
    }
}

/// Signals the end of interrupt `irq` so the PIC can deliver the next one.
pub fn end_of_interrupt(irq: Irq) {
    // SAFETY: sending EOI to the primary PIC is always valid inside its
    // interrupt handler; our IRQs all live on the primary.
    unsafe { Port::<u8>::new(PIC1_COMMAND).write(EOI) };
    let _ = irq;
}
