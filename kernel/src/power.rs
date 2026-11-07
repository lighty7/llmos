//! Power-off and reboot.

use x86_64::instructions::port::Port;

use crate::halt_loop;

/// I/O port of QEMU's `isa-debug-exit` device (configured by the runner).
const QEMU_DEBUG_EXIT_PORT: u16 = 0xF4;
/// Value written for a successful exit; QEMU exits with `(0x10 << 1) | 1 = 33`.
const QEMU_EXIT_SUCCESS: u32 = 0x10;

/// Turns the machine off.
///
/// Tries, in order: QEMU's debug-exit device, the QEMU/Bochs ACPI power
/// management ports, then halts. Full ACPI shutdown (parsing the `\_S5`
/// object from the DSDT) is future work.
pub fn poweroff() -> ! {
    x86_64::instructions::interrupts::disable();
    // SAFETY: writing to these ports either powers the machine off or is
    // ignored by hardware that does not implement them.
    unsafe {
        Port::<u32>::new(QEMU_DEBUG_EXIT_PORT).write(QEMU_EXIT_SUCCESS);
        Port::<u16>::new(0x604).write(0x2000); // QEMU q35 / newer i440fx ACPI PM.
        Port::<u16>::new(0xB004).write(0x2000); // Bochs / older QEMU.
    }
    halt_loop()
}

/// Reboots via the PS/2 controller's CPU-reset line.
pub fn reboot() -> ! {
    x86_64::instructions::interrupts::disable();
    // SAFETY: command 0xFE pulses the CPU reset line.
    unsafe { Port::<u8>::new(0x64).write(0xFE) };
    halt_loop()
}
