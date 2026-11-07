//! CPU feature setup.
//!
//! The kernel is compiled with SSE2 enabled (see `x86_64-llmos.json`), so the
//! SSE unit must be switched on before any floating-point code runs. UEFI
//! firmware already does this on every x86-64 machine, but we set the bits
//! explicitly so the kernel does not depend on firmware behaviour.

use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};

/// Enables the FPU and SSE instructions.
///
/// * `CR0.EM` cleared: no x87 emulation (use the real FPU).
/// * `CR0.MP` set: `WAIT`/`FWAIT` honour `CR0.TS`.
/// * `CR4.OSFXSR` set: the OS supports `FXSAVE`/`FXRSTOR`; enables SSE.
/// * `CR4.OSXMMEXCPT` set: unmasked SIMD floating-point errors raise `#XM`.
pub fn enable_sse() {
    // SAFETY: these flag changes only enable architectural features that every
    // x86-64 CPU implements; they do not affect memory safety.
    unsafe {
        Cr0::update(|cr0| {
            cr0.remove(Cr0Flags::EMULATE_COPROCESSOR);
            cr0.insert(Cr0Flags::MONITOR_COPROCESSOR);
        });
        Cr4::update(|cr4| {
            cr4.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
        });
    }
}
