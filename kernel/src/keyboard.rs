//! PS/2 keyboard driver (scancode set 1, US layout).
//!
//! The keyboard interrupt handler reads one scancode per IRQ, translates it
//! into a character and pushes it onto a lock-free ring buffer. The shell pops
//! characters with [`read_char`]. Using a lock-free single-producer /
//! single-consumer queue means the interrupt handler can never deadlock
//! against the code it interrupted.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use x86_64::instructions::port::Port;

/// PS/2 controller data port.
const DATA_PORT: u16 = 0x60;
/// Capacity of the input queue (must be a power of two).
const QUEUE_SIZE: usize = 256;

/// Scancode → ASCII for unshifted keys (index = make code).
const MAP_LOWER: [u8; 58] =
    *b"\0\x1b1234567890-=\x08\tqwertyuiop[]\n\0asdfghjkl;'`\0\\zxcvbnm,./\0*\0 ";
/// Scancode → ASCII with Shift held.
const MAP_UPPER: [u8; 58] =
    *b"\0\x1b!@#$%^&*()_+\x08\tQWERTYUIOP{}\n\0ASDFGHJKL:\"~\0|ZXCVBNM<>?\0*\0 ";

const SC_LEFT_SHIFT: u8 = 0x2A;
const SC_RIGHT_SHIFT: u8 = 0x36;
const SC_CAPS_LOCK: u8 = 0x3A;
/// Bit set in a scancode when the key is released ("break" code).
const RELEASE_BIT: u8 = 0x80;
/// Prefix byte of extended scancodes (arrow keys, right Ctrl, ...).
const EXTENDED_PREFIX: u8 = 0xE0;

static SHIFT: AtomicBool = AtomicBool::new(false);
static CAPS_LOCK: AtomicBool = AtomicBool::new(false);
static EXTENDED: AtomicBool = AtomicBool::new(false);

/// Ring buffer storage. Written only by the IRQ handler (producer).
static QUEUE: [AtomicU8; QUEUE_SIZE] = [const { AtomicU8::new(0) }; QUEUE_SIZE];
/// Index of the next slot to write (owned by the producer).
static HEAD: AtomicUsize = AtomicUsize::new(0);
/// Index of the next slot to read (owned by the consumer).
static TAIL: AtomicUsize = AtomicUsize::new(0);

/// Translates a scancode into a character, updating modifier state.
/// Returns `None` for modifier keys, releases and unmapped keys.
fn translate(scancode: u8) -> Option<u8> {
    if scancode == EXTENDED_PREFIX {
        EXTENDED.store(true, Ordering::Relaxed);
        return None;
    }
    if EXTENDED.swap(false, Ordering::Relaxed) {
        return None; // Arrow keys etc. are not supported yet.
    }
    let released = scancode & RELEASE_BIT != 0;
    let code = scancode & !RELEASE_BIT;
    match code {
        SC_LEFT_SHIFT | SC_RIGHT_SHIFT => {
            SHIFT.store(!released, Ordering::Relaxed);
            None
        }
        SC_CAPS_LOCK if !released => {
            CAPS_LOCK.fetch_xor(true, Ordering::Relaxed);
            None
        }
        _ if released => None,
        _ => {
            let lower = *MAP_LOWER.get(code as usize)?;
            if lower == 0 {
                return None;
            }
            let shift = SHIFT.load(Ordering::Relaxed);
            let caps = CAPS_LOCK.load(Ordering::Relaxed) && lower.is_ascii_lowercase();
            Some(if shift ^ caps {
                MAP_UPPER[code as usize]
            } else {
                lower
            })
        }
    }
}

/// Handles one keyboard interrupt. Called from the IRQ 1 handler.
pub fn handle_interrupt() {
    // SAFETY: reading the PS/2 data port fetches the pending scancode.
    let scancode = unsafe { Port::<u8>::new(DATA_PORT).read() };
    if let Some(c) = translate(scancode) {
        let head = HEAD.load(Ordering::Relaxed);
        let next = (head + 1) % QUEUE_SIZE;
        if next != TAIL.load(Ordering::Acquire) {
            QUEUE[head].store(c, Ordering::Relaxed);
            HEAD.store(next, Ordering::Release);
        } // else: queue full, drop the key.
    }
}

/// Pops the next typed character, if any (non-blocking).
pub fn read_char() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let c = QUEUE[tail].load(Ordering::Relaxed);
    TAIL.store((tail + 1) % QUEUE_SIZE, Ordering::Release);
    Some(c)
}

/// Exposes [`translate`] to the in-kernel `selftest` command (this module is
/// bare-metal only, so it cannot be unit-tested on the host).
pub fn translate_for_test(scancode: u8) -> Option<u8> {
    translate(scancode)
}
