//! System time based on the 8253/8254 Programmable Interval Timer.
//!
//! The PIT is programmed to fire IRQ 0 at [`TICK_HZ`]; each interrupt
//! increments a global tick counter. That gives the kernel uptime, delays and
//! the tokens-per-second figures shown by the LLM.

use core::sync::atomic::{AtomicU64, Ordering};

use x86_64::instructions::port::Port;

/// Timer interrupt frequency.
pub const TICK_HZ: u64 = 100;
/// Base oscillator frequency of the PIT.
const PIT_BASE_HZ: u64 = 1_193_182;

/// Ticks since the timer was started.
static TICKS: AtomicU64 = AtomicU64::new(0);

/// Programs PIT channel 0 as a periodic rate generator at [`TICK_HZ`].
pub fn init() {
    let divisor = (PIT_BASE_HZ / TICK_HZ) as u16;
    // SAFETY: ports 0x43/0x40 belong to the PIT; mode 2 on channel 0 is the
    // standard periodic configuration.
    unsafe {
        Port::<u8>::new(0x43).write(0b0011_0100); // Channel 0, lo/hi byte, mode 2.
        Port::<u8>::new(0x40).write((divisor & 0xFF) as u8);
        Port::<u8>::new(0x40).write((divisor >> 8) as u8);
    }
}

/// Called by the timer interrupt handler.
pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Ticks elapsed since boot.
pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

/// Milliseconds elapsed since boot (resolution: 1000 / [`TICK_HZ`] ms).
pub fn uptime_ms() -> u64 {
    ticks() * 1000 / TICK_HZ
}

/// Busy-waits (sleeping between ticks) for at least `ms` milliseconds.
///
/// Interrupts must be enabled, otherwise this never returns.
pub fn sleep_ms(ms: u64) {
    let target = ticks() + ms.div_ceil(1000 / TICK_HZ);
    while ticks() < target {
        x86_64::instructions::hlt();
    }
}
