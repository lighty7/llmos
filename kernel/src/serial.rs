//! Driver for the 16550 UART serial port (COM1).
//!
//! The serial port is the kernel's most reliable I/O channel: it works before
//! the framebuffer is set up, QEMU forwards it to the host terminal
//! (`-serial stdio`), and the automated test-suite drives the shell through
//! it. Both output ([`write_byte`]) and polled input ([`try_read_byte`]) are
//! supported.
//!
//! Register reference: <https://wiki.osdev.org/Serial_Ports>.

use core::fmt;

use spin::Mutex;
use x86_64::instructions::port::Port;

/// I/O port base address of COM1 on every PC-compatible machine.
const COM1: u16 = 0x3F8;

/// Offsets of the UART registers relative to the base port.
mod reg {
    /// Data register (read: receive buffer, write: transmit holding).
    pub const DATA: u16 = 0;
    /// Interrupt enable register.
    pub const INT_ENABLE: u16 = 1;
    /// FIFO control register.
    pub const FIFO_CTRL: u16 = 2;
    /// Line control register (word length, divisor latch access).
    pub const LINE_CTRL: u16 = 3;
    /// Modem control register.
    pub const MODEM_CTRL: u16 = 4;
    /// Line status register.
    pub const LINE_STATUS: u16 = 5;
}

/// Line-status bit: a received byte is waiting in the data register.
const LSR_DATA_READY: u8 = 1 << 0;
/// Line-status bit: the transmit holding register can accept a byte.
const LSR_TX_EMPTY: u8 = 1 << 5;

/// A 16550-compatible UART at a fixed I/O port base.
pub struct SerialPort {
    base: u16,
}

impl SerialPort {
    /// Creates a handle for the UART at `base`. Does not touch the hardware.
    ///
    /// # Safety
    ///
    /// `base` must be the I/O base of a real 16550 UART and no other code may
    /// drive the same device concurrently.
    pub const unsafe fn new(base: u16) -> Self {
        Self { base }
    }

    fn port(&self, offset: u16) -> Port<u8> {
        Port::new(self.base + offset)
    }

    /// Programs the UART for 115200 baud, 8 data bits, no parity, one stop
    /// bit, with FIFOs enabled and interrupts disabled (the kernel polls).
    pub fn init(&mut self) {
        // SAFETY: `new`'s contract guarantees these ports belong to the UART.
        unsafe {
            self.port(reg::INT_ENABLE).write(0x00); // Disable UART interrupts.
            self.port(reg::LINE_CTRL).write(0x80); // Enable divisor latch.
            self.port(reg::DATA).write(0x01); // Divisor low byte: 115200 baud.
            self.port(reg::INT_ENABLE).write(0x00); // Divisor high byte.
            self.port(reg::LINE_CTRL).write(0x03); // 8N1, latch off.
            self.port(reg::FIFO_CTRL).write(0xC7); // Enable + clear FIFOs, 14-byte threshold.
            self.port(reg::MODEM_CTRL).write(0x0B); // DTR, RTS, OUT2.
        }
    }

    fn line_status(&self) -> u8 {
        // SAFETY: reading the line-status register has no side effects.
        unsafe { self.port(reg::LINE_STATUS).read() }
    }

    /// Blocks until the transmitter is ready, then sends `byte`.
    pub fn write_byte(&mut self, byte: u8) {
        while self.line_status() & LSR_TX_EMPTY == 0 {
            core::hint::spin_loop();
        }
        // SAFETY: the transmitter is ready; writing DATA sends one byte.
        unsafe { self.port(reg::DATA).write(byte) }
    }

    /// Returns the next received byte, or `None` if nothing is waiting.
    pub fn try_read_byte(&mut self) -> Option<u8> {
        if self.line_status() & LSR_DATA_READY == 0 {
            return None;
        }
        // SAFETY: data is ready; reading DATA pops one byte from the FIFO.
        Some(unsafe { self.port(reg::DATA).read() })
    }
}

impl fmt::Write for SerialPort {
    /// Writes `s`, translating `\n` to `\r\n` so host terminals render
    /// newlines correctly.
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            if byte == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(byte);
        }
        Ok(())
    }
}

/// The global COM1 port, shared by all kernel code.
// SAFETY: COM1 is the standard PC serial port and only this static drives it.
pub static COM1_PORT: Mutex<SerialPort> = Mutex::new(unsafe { SerialPort::new(COM1) });

/// Initialises COM1. Call once, as early as possible during boot.
pub fn init() {
    COM1_PORT.lock().init();
}

/// Returns the next byte received on COM1, if any (non-blocking).
pub fn try_read_byte() -> Option<u8> {
    x86_64::instructions::interrupts::without_interrupts(|| COM1_PORT.lock().try_read_byte())
}
