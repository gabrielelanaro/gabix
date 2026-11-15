//! VGA text-mode buffer driver.
//!
//! This driver assumes an already configured 80x25 text display and access to
//! its character buffer at 0xB8000. It updates cells without programming display
//! timing, selecting a font or changing the video mode.
//!
//! Layout: 25 rows × 80 columns × 2 bytes/cell = 4000 bytes. Each cell is
//! one ASCII byte followed by one color attribute byte.

use crate::irq_mutex::IrqMutex;
use core::fmt;
use core::ptr;
use lazy_static::lazy_static;

const TEXT_BUFFER_ADDRESS: usize = 0xb8000;
const REPLACEMENT_GLYPH: u8 = 0xfe; // Glyph appearance depends on the loaded font.
const BUFFER_HEIGHT: usize = 25;
const BUFFER_WIDTH: usize = 80;

/// The standard 16-color VGA palette.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Color {
    Black = 0,
    Blue = 1,
    Green = 2,
    Cyan = 3,
    Red = 4,
    Magenta = 5,
    Brown = 6,
    LightGray = 7,
    DarkGray = 8,
    LightBlue = 9,
    LightGreen = 10,
    LightCyan = 11,
    LightRed = 12,
    Pink = 13,
    Yellow = 14,
    White = 15,
}

/// A foreground + background color pair, packed into one byte
/// (high 4 bits background, low 4 bits foreground).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct ColorCode(u8);

impl ColorCode {
    const fn new(foreground: Color, background: Color) -> ColorCode {
        ColorCode((background as u8) << 4 | (foreground as u8))
    }
}

/// One cell on screen. `#[repr(C)]` fixes the field order so it matches
/// exactly what the VGA hardware expects: ASCII byte first, color byte second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(C)]
struct ScreenChar {
    ascii_character: u8,
    color_code: ColorCode,
}

/// The full 25×80 text buffer. We access it with `ptr::write_volatile` /
/// `ptr::read_volatile` directly so the compiler can't elide accesses the
/// VGA hardware depends on.
#[repr(transparent)]
struct Buffer {
    chars: [[ScreenChar; BUFFER_WIDTH]; BUFFER_HEIGHT],
}

/// Prints formatted text to the screen. Holds the current column position
/// and a `&'static mut` reference to the VGA buffer (the address never changes).
pub struct Writer {
    column_position: usize,
    color_code: ColorCode,
    buffer: &'static mut Buffer,
}

impl Writer {
    /// Write a single ASCII byte. Handles `\n` (newline) and column wrap.
    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                if self.column_position >= BUFFER_WIDTH {
                    self.new_line();
                }
                let row = BUFFER_HEIGHT - 1;
                let col = self.column_position;
                let color_code = self.color_code;

                // Safe because `row < 25`, `col < 80`, and the buffer lives
                // at a fixed hardware address that we own for the kernel's
                // lifetime.
                unsafe {
                    ptr::write_volatile(
                        &mut self.buffer.chars[row][col],
                        ScreenChar {
                            ascii_character: byte,
                            color_code,
                        },
                    );
                }
                self.column_position += 1;
            }
        }
    }

    /// Write printable ASCII and newlines. Each other input byte becomes glyph
    /// 0xfe; a multi-byte UTF-8 character therefore produces multiple replacements.
    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                _ => self.write_byte(REPLACEMENT_GLYPH),
            }
        }
    }

    /// Scroll the whole buffer up by one row and reset to column 0.
    fn new_line(&mut self) {
        for row in 1..BUFFER_HEIGHT {
            for col in 0..BUFFER_WIDTH {
                let character = unsafe { ptr::read_volatile(&self.buffer.chars[row][col]) };
                unsafe {
                    ptr::write_volatile(&mut self.buffer.chars[row - 1][col], character);
                }
            }
        }
        self.clear_row(BUFFER_HEIGHT - 1);
        self.column_position = 0;
    }

    /// Fill a row with blanks using the current color.
    fn clear_row(&mut self, row: usize) {
        let blank = ScreenChar {
            ascii_character: b' ',
            color_code: self.color_code,
        };
        for col in 0..BUFFER_WIDTH {
            unsafe {
                ptr::write_volatile(&mut self.buffer.chars[row][col], blank);
            }
        }
    }
}

/// Implementing `core::fmt::Write` lets us use `core::write!(writer, ...)`
/// and therefore `format_args!` from the print!/println! macros.
impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

// `lazy_static!` defers construction to first use. This sidesteps the const
// evaluator's strict provenance check (you can't safely create a `&mut Buffer`
// from a raw integer address in const context).
lazy_static! {
    /// Global writer, guarded by a spinlock. Any code in the kernel can
    /// `WRITER.lock()` to print atomically.
    pub static ref WRITER: IrqMutex<Writer> = IrqMutex::new(Writer {
        column_position: 0,
        color_code: ColorCode::new(Color::Yellow, Color::Black),
        buffer: unsafe { &mut *(TEXT_BUFFER_ADDRESS as *mut Buffer) },
    });
}

/// Hidden helper used by the `print!` and `println!` macros.
/// Locks the global WRITER and forwards the format args.
///
/// Also mirrors to the serial port so output can be captured outside QEMU.
/// The two writes take separate locks; another task/interrupt can print between
/// them, so VGA and serial ordering need not match across concurrent calls.
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;
    WRITER.lock().write_fmt(args).unwrap();
    crate::serial::SERIAL1.lock().write_fmt(args).unwrap();
}

/// Print without a trailing newline. Locks WRITER, formats, writes.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

/// Print with a trailing newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
