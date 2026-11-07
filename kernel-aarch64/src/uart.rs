//! Driver for the ARM PL011 UART.
//!
//! The PL011 is the console UART of QEMU's `virt` board (at `0x0900_0000`)
//! and of the Raspberry Pi (UART0 at `0x3F20_1000` on the Pi 3, `0xFE20_1000`
//! on the Pi 4). The base address comes from the device tree. The firmware
//! (or QEMU) has already configured the baud rate, so the driver only moves
//! bytes, by polling.
//!
//! Reference: ARM PrimeCell UART (PL011) Technical Reference Manual.

use core::fmt;

use spin::Mutex;

/// Data register.
const DR: usize = 0x00;
/// Flag register.
const FR: usize = 0x18;
/// FR bit: receive FIFO empty.
const FR_RXFE: u32 = 1 << 4;
/// FR bit: transmit FIFO full.
const FR_TXFF: u32 = 1 << 5;

/// A PL011 at a fixed MMIO base address.
pub struct Pl011 {
    base: usize,
}

impl Pl011 {
    fn read(&self, reg: usize) -> u32 {
        // SAFETY: `base` is a PL011 register block (see `init`).
        unsafe { core::ptr::read_volatile((self.base + reg) as *const u32) }
    }

    fn write(&self, reg: usize, value: u32) {
        // SAFETY: as above.
        unsafe { core::ptr::write_volatile((self.base + reg) as *mut u32, value) }
    }

    /// Sends one byte, waiting while the transmit FIFO is full.
    pub fn write_byte(&self, byte: u8) {
        while self.read(FR) & FR_TXFF != 0 {
            core::hint::spin_loop();
        }
        self.write(DR, byte as u32);
    }

    /// Returns a received byte, if any.
    pub fn try_read_byte(&self) -> Option<u8> {
        (self.read(FR) & FR_RXFE == 0).then(|| self.read(DR) as u8)
    }
}

impl fmt::Write for Pl011 {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
        Ok(())
    }
}

/// The console UART, once found.
pub static CONSOLE: Mutex<Option<Pl011>> = Mutex::new(None);

/// Starts using the PL011 at `base` as the console.
///
/// # Safety
///
/// `base` must be the MMIO base of a PL011 that nothing else drives.
pub unsafe fn init(base: usize) {
    *CONSOLE.lock() = Some(Pl011 { base });
}

/// Returns a received console byte, if any.
pub fn try_read_byte() -> Option<u8> {
    CONSOLE.lock().as_ref().and_then(Pl011::try_read_byte)
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use fmt::Write;
    if let Some(uart) = CONSOLE.lock().as_mut() {
        let _ = uart.write_fmt(args);
    }
}

/// Prints to the console UART.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::uart::_print(format_args!($($arg)*)));
}

/// Prints to the console UART, followed by a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
