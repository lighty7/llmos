//! Text console rendered onto the UEFI linear framebuffer.
//!
//! UEFI machines have no VGA text mode, so text is drawn pixel by pixel
//! using a pre-rasterised bitmap font ([`noto_sans_mono_bitmap`]). The console
//! keeps a character cursor, wraps long lines and scrolls when the bottom of
//! the screen is reached.

use bootloader_api::info::{FrameBufferInfo, PixelFormat};
use noto_sans_mono_bitmap::{
    FontWeight, RasterHeight, RasterizedChar, get_raster, get_raster_width,
};

/// Height of one text row in pixels.
const CHAR_HEIGHT: RasterHeight = RasterHeight::Size16;
/// Font weight used for all text.
const FONT_WEIGHT: FontWeight = FontWeight::Regular;
/// Width of one character cell in pixels.
const CHAR_WIDTH: usize = get_raster_width(FONT_WEIGHT, CHAR_HEIGHT);
/// Extra vertical space between rows, in pixels.
const LINE_SPACING: usize = 2;
/// Blank margin around the text area, in pixels.
const BORDER: usize = 4;

/// A 24-bit RGB colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// Red intensity.
    pub r: u8,
    /// Green intensity.
    pub g: u8,
    /// Blue intensity.
    pub b: u8,
}

impl Color {
    /// Builds a colour from its components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Default foreground colour.
    pub const WHITE: Color = Color::rgb(0xE0, 0xE0, 0xE0);
    /// Background colour.
    pub const BLACK: Color = Color::rgb(0x00, 0x00, 0x00);
    /// Banner / success colour.
    pub const GREEN: Color = Color::rgb(0x55, 0xFF, 0x55);
    /// Prompt colour.
    pub const CYAN: Color = Color::rgb(0x55, 0xFF, 0xFF);
    /// Headings and LLM output.
    pub const YELLOW: Color = Color::rgb(0xFF, 0xFF, 0x55);
    /// Errors.
    pub const RED: Color = Color::rgb(0xFF, 0x55, 0x55);
}

/// A text console drawing into a linear framebuffer.
pub struct FrameBufferConsole {
    buffer: &'static mut [u8],
    info: FrameBufferInfo,
    /// Pixel column of the next character.
    x: usize,
    /// Pixel row (top edge) of the current text line.
    y: usize,
    fg: Color,
    bg: Color,
}

impl FrameBufferConsole {
    /// Wraps the framebuffer handed over by the bootloader and clears it.
    pub fn new(buffer: &'static mut [u8], info: FrameBufferInfo) -> Self {
        let mut console = Self {
            buffer,
            info,
            x: BORDER,
            y: BORDER,
            fg: Color::WHITE,
            bg: Color::BLACK,
        };
        console.clear();
        console
    }

    /// Height of a text row including spacing.
    const fn line_height() -> usize {
        CHAR_HEIGHT.val() + LINE_SPACING
    }

    /// Sets the colours used for subsequently written text.
    pub fn set_color(&mut self, fg: Color, bg: Color) {
        self.fg = fg;
        self.bg = bg;
    }

    /// Fills the screen with the background colour and homes the cursor.
    pub fn clear(&mut self) {
        let bg = self.bg;
        let bytes_per_pixel = self.info.bytes_per_pixel;
        let pixel = self.encode(bg);
        for chunk in self.buffer.chunks_exact_mut(bytes_per_pixel) {
            chunk.copy_from_slice(&pixel[..bytes_per_pixel]);
        }
        self.x = BORDER;
        self.y = BORDER;
    }

    /// Converts a colour into the framebuffer's native pixel bytes.
    fn encode(&self, color: Color) -> [u8; 4] {
        match self.info.pixel_format {
            PixelFormat::Rgb => [color.r, color.g, color.b, 0],
            PixelFormat::Bgr => [color.b, color.g, color.r, 0],
            PixelFormat::U8 => {
                let grey = (color.r as u16 + color.g as u16 + color.b as u16) / 3;
                [grey as u8, 0, 0, 0]
            }
            // Unknown layouts are rare; fall back to the common BGR order.
            _ => [color.b, color.g, color.r, 0],
        }
    }

    /// Writes a single pixel. Out-of-bounds coordinates are ignored.
    fn put_pixel(&mut self, x: usize, y: usize, color: Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        let bpp = self.info.bytes_per_pixel;
        let offset = (y * self.info.stride + x) * bpp;
        let pixel = self.encode(color);
        self.buffer[offset..offset + bpp].copy_from_slice(&pixel[..bpp]);
    }

    /// Blends foreground and background by the glyph's coverage value.
    fn blend(&self, intensity: u8) -> Color {
        let mix = |fg: u8, bg: u8| -> u8 {
            ((fg as u16 * intensity as u16 + bg as u16 * (255 - intensity as u16)) / 255) as u8
        };
        Color::rgb(
            mix(self.fg.r, self.bg.r),
            mix(self.fg.g, self.bg.g),
            mix(self.fg.b, self.bg.b),
        )
    }

    /// Moves the cursor to the start of the next line, scrolling if needed.
    fn newline(&mut self) {
        self.x = BORDER;
        self.y += Self::line_height();
        if self.y + Self::line_height() + BORDER > self.info.height {
            self.scroll();
        }
    }

    /// Shifts the screen contents up by one text row and blanks the last row.
    fn scroll(&mut self) {
        let row_bytes = self.info.stride * self.info.bytes_per_pixel;
        let shift = Self::line_height() * row_bytes;
        let len = self.buffer.len();
        self.buffer.copy_within(shift.., 0);
        let pixel = self.encode(self.bg);
        let bpp = self.info.bytes_per_pixel;
        for chunk in self.buffer[len - shift..].chunks_exact_mut(bpp) {
            chunk.copy_from_slice(&pixel[..bpp]);
        }
        self.y -= Self::line_height();
    }

    /// Draws a rasterised glyph at the cursor position.
    fn draw_glyph(&mut self, glyph: &RasterizedChar) {
        for (dy, row) in glyph.raster().iter().enumerate() {
            for (dx, &intensity) in row.iter().enumerate() {
                let color = self.blend(intensity);
                self.put_pixel(self.x + dx, self.y + dy, color);
            }
        }
    }

    /// Erases the character cell immediately left of the cursor and moves
    /// the cursor onto it. Does nothing at the start of a line.
    fn backspace(&mut self) {
        if self.x < BORDER + CHAR_WIDTH {
            return;
        }
        self.x -= CHAR_WIDTH;
        for dy in 0..CHAR_HEIGHT.val() {
            for dx in 0..CHAR_WIDTH {
                self.put_pixel(self.x + dx, self.y + dy, self.bg);
            }
        }
    }

    /// Writes one character, handling `\n`, `\r` and backspace (`\x08`).
    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => self.newline(),
            '\r' => self.x = BORDER,
            '\x08' => self.backspace(),
            c => {
                if self.x + CHAR_WIDTH + BORDER > self.info.width {
                    self.newline();
                }
                let glyph = get_raster(c, FONT_WEIGHT, CHAR_HEIGHT)
                    .or_else(|| get_raster('?', FONT_WEIGHT, CHAR_HEIGHT))
                    .expect("the font always contains '?'");
                self.draw_glyph(&glyph);
                self.x += CHAR_WIDTH;
            }
        }
    }
}

impl core::fmt::Write for FrameBufferConsole {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}
