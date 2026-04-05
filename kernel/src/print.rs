use crate::drivers::uart::uart_write_sync;
use crate::sync::spin::Spinlock;
use core::fmt::Arguments;
use core::fmt::{self, Write};

#[macro_export]
macro_rules! printk {
    ($($arg:tt)*) => {
        $crate::print::_printk(format_args!( $( $arg )* ))
    };
}

#[macro_export]
macro_rules! printkln {
    () =>{
        $crate::console::kprint!("\n")
    };
    ($($arg:tt)*) => {
        $crate::printk!("{}\n", format_args!( $( $arg )* ))
    };
}

#[macro_export]
macro_rules! printk_with_color {
    ($color:expr, $($arg:tt)*) => {
        $crate::printk!("\u{1B}[{}m{}\u{1B}[0m", $color as u8, format_args!( $( $arg )* ))
    };
}

static PRINTER: Spinlock<UartWriter> = Spinlock::new("printk", UartWriter);

struct UartWriter;

impl fmt::Write for UartWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        uart_write_sync(s.as_bytes());
        Ok(())
    }
}

#[repr(C)]
pub enum FGColor {
    Black = 30,
    Red = 31,
    Green = 32,
    Yellow = 33,
    Blue = 34,
    Magenta = 35,
    Cyan = 36,
    White = 37,
    BrightBlack = 90,
    BrightRed = 91,
    BrightGreen = 92,
    BrightYellow = 93,
    BrightBlue = 94,
    BrightMagenta = 95,
    BrightCyan = 96,
    BrightWhite = 97,
}

pub fn _printk(args: Arguments) {
    let mut printer = PRINTER.lock_irq_save();
    printer.write_fmt(args).unwrap();
    drop(printer);
}
