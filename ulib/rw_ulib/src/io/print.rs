use crate::io::stdout;
use core::fmt::Arguments;

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::io::print::print_fmt(format_args!( $( $arg )* ))
    };
}

#[macro_export]
macro_rules! println {
    () =>{
        $crate::print!("\n")
    };
    ($($arg:tt)*) => {
        $crate::print!("{}\n", format_args!( $( $arg )* ))
    };
}

/// Provides for `print!` family macros.
///
/// See [`stdout`].print_fmt
#[inline]
pub fn print_fmt(args: Arguments) {
    stdout().print_fmt(args);
}
