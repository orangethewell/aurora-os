use core::fmt::{self, Write};
use embedded_graphics::{pixelcolor::Rgb888, prelude::*};
use font8x8::UnicodeFonts;
use lazy_static::lazy_static;
use spin::Mutex;

use crate::framebuffer::Display;

lazy_static! {
    pub static ref ACTIVE_TTY: Mutex<Option<TTY<'static>>> = Mutex::new(None);
}

pub fn activate_tty(mut tty: TTY<'static>) {
    tty.display.clear_buf();
    tty.display.flush();

    let mut active_tty = ACTIVE_TTY.lock();
    *active_tty = Some(tty);
}

// Define o tamanho do terminal
pub const TTY_WIDTH: usize = 80;
pub const TTY_HEIGHT: usize = 25;

pub struct TTY<'a> {
    display: Display<'a>,
    buffer: [[char; TTY_WIDTH]; TTY_HEIGHT],
    cursor_x: usize,
    cursor_y: usize,
}

unsafe impl Send for TTY<'_> {}
unsafe impl Sync for TTY<'_> {}

impl<'a> TTY<'a> {
    pub const fn new(display: Display<'a>) -> Self {
        Self {
            display,
            buffer: [[' '; TTY_WIDTH]; TTY_HEIGHT],
            cursor_x: 0,
            cursor_y: 0,
        }
    }

    pub fn write_char(&mut self, c: char) {
        match c {
            '\n' => {
                self.cursor_x = 0;
                self.cursor_y += 1;
            }
            _ => {
                if self.cursor_x >= TTY_WIDTH {
                    self.cursor_x = 0;
                    self.cursor_y += 1;
                }
                if self.cursor_y >= TTY_HEIGHT {
                    self.scroll_up();
                    self.cursor_y = TTY_HEIGHT - 1;
                }
                self.buffer[self.cursor_y][self.cursor_x] = c;
                self.cursor_x += 1;
            }
        }
    }

    pub fn write_str(&mut self, s: &str) {
        for c in s.chars() {
            self.write_char(c);
        }
    }

    fn scroll_up(&mut self) {
        for y in 1..TTY_HEIGHT {
            self.buffer[y - 1] = self.buffer[y];
        }
        self.buffer[TTY_HEIGHT - 1] = [' '; TTY_WIDTH];
        self.display.clear_buf();
        self.render(2, Rgb888::new(255, 255, 255));
        self.display.flush();
    }

    /// Renderiza no framebuffer
    pub fn render(
        &mut self,
        scale: usize,
        color: Rgb888,
    ) {
        for y in 0..TTY_HEIGHT {
            for x in 0..TTY_WIDTH {
                let c = self.buffer[y][x];
                if let Some(glyph) = font8x8::BASIC_FONTS.get(c) {
                    for (row, byte) in glyph.iter().enumerate() {
                        for bit in 0..8 {
                            if (byte >> bit) & 1 == 1 {
                                // Calcular pixel base
                                let px = x * 8 * scale + bit * scale;
                                let py = y * 8 * scale + row * scale;

                                // Desenhar pixels com o scale
                                for dy in 0..scale {
                                    for dx in 0..scale {
                                        self.display.draw_pixel(Pixel(Point::new((px + dx) as i32, (py + dy) as i32), color));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

// IMPLEMENTA fmt::Write pra usar write! / writeln!
impl Write for TTY<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.chars() {
            self.write_char(c);
        }
        Ok(())
    }
}

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;
    use x86_64::instructions::interrupts;

    interrupts::without_interrupts(|| { 
        if let Some(tty) = ACTIVE_TTY.lock().as_mut() {
            serial_print!("AURORA::KERNEL::TTY::PRINT > {}", args);
            let _ = tty.write_fmt(args);
            tty.render(2, Rgb888::new(255, 255, 255));
            tty.display.flush();
        } else {
            serial_println!("AURORA::KERNEL::TTY > No active TTY for printing! Falling to UART");
            serial_println!("AURORA::KERNEL::UART::PRINT > {}", args);
        }
    }); 
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => {
        $crate::tty::_print(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($fmt:expr) => ($crate::kprint!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::kprint!(
        concat!($fmt, "\n"), $($arg)*));
}