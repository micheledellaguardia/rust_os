//! A minimal text console drawn onto the bootloader-provided framebuffer.
//!
//! Replaces the legacy VGA text-mode writer: bootloader 0.11 sets up a linear
//! graphics framebuffer, so we render an 8x8 bitmap font (`font8x8`) directly
//! to pixels and support scrolling.

use core::fmt;

use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use font8x8::{UnicodeFonts, BASIC_FONTS};
use spin::Mutex;

/// Glyph cell size in pixels.
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 8;

/// Foreground (white) and background (black) colors as (r, g, b).
const FG: (u8, u8, u8) = (0xDD, 0xDD, 0xDD);
const BG: (u8, u8, u8) = (0x10, 0x10, 0x10);

pub struct Console {
    buffer: &'static mut [u8],
    info: FrameBufferInfo,
    column: usize,
    row: usize,
}

impl Console {
    pub fn new(framebuffer: FrameBuffer) -> Self {
        let info = framebuffer.info();
        let buffer = framebuffer.into_buffer();
        let mut console = Console {
            buffer,
            info,
            column: 0,
            row: 0,
        };
        console.clear();
        console
    }

    pub fn clear(&mut self) {
        for byte in self.buffer.iter_mut() {
            *byte = 0;
        }
        self.column = 0;
        self.row = 0;
    }

    fn columns(&self) -> usize {
        self.info.width / FONT_WIDTH
    }

    fn rows(&self) -> usize {
        self.info.height / FONT_HEIGHT
    }

    fn write_pixel(&mut self, x: usize, y: usize, color: (u8, u8, u8)) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        let (r, g, b) = color;
        let offset = (y * self.info.stride + x) * self.info.bytes_per_pixel;
        let pixel = &mut self.buffer[offset..offset + self.info.bytes_per_pixel];

        match self.info.pixel_format {
            PixelFormat::Rgb => {
                pixel[0] = r;
                pixel[1] = g;
                pixel[2] = b;
            }
            PixelFormat::Bgr => {
                pixel[0] = b;
                pixel[1] = g;
                pixel[2] = r;
            }
            PixelFormat::U8 => {
                pixel[0] = ((r as u32 + g as u32 + b as u32) / 3) as u8;
            }
            PixelFormat::Unknown {
                red_position,
                green_position,
                blue_position,
            } => {
                let value = ((r as u32) << red_position)
                    | ((g as u32) << green_position)
                    | ((b as u32) << blue_position);
                for (i, byte) in pixel.iter_mut().enumerate() {
                    *byte = (value >> (i * 8)) as u8;
                }
            }
            _ => {}
        }
    }

    fn draw_glyph(&mut self, ch: char) {
        let glyph = BASIC_FONTS.get(ch).unwrap_or([0xFF; 8]);
        let origin_x = self.column * FONT_WIDTH;
        let origin_y = self.row * FONT_HEIGHT;

        for (dy, row_bits) in glyph.iter().enumerate() {
            for dx in 0..FONT_WIDTH {
                let on = (row_bits >> dx) & 1 == 1;
                let color = if on { FG } else { BG };
                self.write_pixel(origin_x + dx, origin_y + dy, color);
            }
        }
    }

    fn clear_cell(&mut self, column: usize, row: usize) {
        let origin_x = column * FONT_WIDTH;
        let origin_y = row * FONT_HEIGHT;
        for dy in 0..FONT_HEIGHT {
            for dx in 0..FONT_WIDTH {
                self.write_pixel(origin_x + dx, origin_y + dy, BG);
            }
        }
    }

    fn new_line(&mut self) {
        self.column = 0;
        if self.row + 1 >= self.rows() {
            self.scroll();
        } else {
            self.row += 1;
        }
    }

    fn scroll(&mut self) {
        let line_bytes = self.info.stride * self.info.bytes_per_pixel * FONT_HEIGHT;
        let len = self.buffer.len();
        self.buffer.copy_within(line_bytes..len, 0);
        for byte in &mut self.buffer[len - line_bytes..] {
            *byte = 0;
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            b'\r' => self.column = 0,
            0x08 => {
                if self.column > 0 {
                    self.column -= 1;
                    let (column, row) = (self.column, self.row);
                    self.clear_cell(column, row);
                }
            }
            byte => {
                if self.column >= self.columns() {
                    self.new_line();
                }
                self.draw_glyph(byte as char);
                self.column += 1;
            }
        }
    }
}

impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for byte in s.bytes() {
            self.write_byte(byte);
        }
        Ok(())
    }
}

static CONSOLE: Mutex<Option<Console>> = Mutex::new(None);

/// Initialize the console with the bootloader framebuffer.
pub fn init(framebuffer: FrameBuffer) {
    *CONSOLE.lock() = Some(Console::new(framebuffer));
}

/// Clears the framebuffer console and resets the cursor.
pub fn clear() {
    if let Some(console) = CONSOLE.lock().as_mut() {
        console.clear();
    }
}

pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    if let Some(console) = CONSOLE.lock().as_mut() {
        let _ = console.write_fmt(args);
    }
    crate::serial::_print(args);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::framebuffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
