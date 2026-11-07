//! Unified text output: every message goes to both the serial port and the
//! framebuffer console (when one exists).
//!
//! Use the [`print!`](crate::print) and [`println!`](crate::println) macros
//! rather than calling these functions directly.

use core::fmt::{self, Write};

use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::framebuffer::{Color, FrameBufferConsole};
use crate::serial;

/// The framebuffer console, if the bootloader provided a framebuffer.
static SCREEN: Mutex<Option<FrameBufferConsole>> = Mutex::new(None);

/// Installs the framebuffer console. Called once during boot.
pub fn init_screen(console: FrameBufferConsole) {
    *SCREEN.lock() = Some(console);
}

/// Writes formatted text to all outputs. Used by the `print!` macros.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    // Interrupts are disabled while the locks are held so an interrupt
    // handler that prints (e.g. on a CPU exception) cannot deadlock.
    interrupts::without_interrupts(|| {
        let _ = serial::COM1_PORT.lock().write_fmt(args);
        if let Some(screen) = SCREEN.lock().as_mut() {
            let _ = screen.write_fmt(args);
        }
    });
}

/// Changes the colour of subsequent framebuffer text. Serial output is
/// unaffected (it stays plain text so logs and tests are easy to parse).
pub fn set_color(fg: Color) {
    interrupts::without_interrupts(|| {
        if let Some(screen) = SCREEN.lock().as_mut() {
            screen.set_color(fg, Color::BLACK);
        }
    });
}

/// Clears the framebuffer console.
pub fn clear() {
    interrupts::without_interrupts(|| {
        if let Some(screen) = SCREEN.lock().as_mut() {
            screen.clear();
        }
    });
}

/// Prints to the serial port and the screen.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/// Prints to the serial port and the screen, followed by a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

/// Prints a boot-progress line such as `[ ok ] heap: 64 MiB`.
#[macro_export]
macro_rules! log_ok {
    ($($arg:tt)*) => {{
        $crate::console::set_color($crate::framebuffer::Color::GREEN);
        $crate::print!("[ ok ] ");
        $crate::console::set_color($crate::framebuffer::Color::WHITE);
        $crate::println!($($arg)*);
    }};
}

/// Prints a warning line such as `[warn] no model found`.
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {{
        $crate::console::set_color($crate::framebuffer::Color::YELLOW);
        $crate::print!("[warn] ");
        $crate::console::set_color($crate::framebuffer::Color::WHITE);
        $crate::println!($($arg)*);
    }};
}
