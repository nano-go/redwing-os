#![no_std]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::test_runner)]
#![feature(custom_test_frameworks)]
#![feature(strict_provenance_lints)]
#![feature(decl_macro)]
#![feature(naked_functions)]
#![feature(alloc_error_handler)]
#![feature(never_type)]

#[cfg(feature = "user")]
use core::panic::PanicInfo;

#[cfg(feature = "alloc")]
extern crate alloc;

#[cfg(feature = "syscall")]
pub mod syscall;

#[cfg(feature = "user")]
pub mod env;

#[cfg(feature = "user")]
pub mod fs;

#[cfg(feature = "user")]
pub mod heap;

#[cfg(feature = "user")]
pub mod io;

#[cfg(feature = "user")]
pub mod process;

#[cfg(feature = "user")]
pub mod signal;

#[cfg(feature = "user")]
pub mod error;

#[cfg(feature = "user")]
extern "Rust" {
    fn main();
}

#[cfg(feature = "user")]
pub(crate) static mut ARGS_PTR: usize = 0;

#[cfg(feature = "user")]
#[no_mangle]
pub fn _start(args_ptr: usize, env_vars_ptr: usize) {
    use env::init_env_vars;

    unsafe {
        ARGS_PTR = args_ptr;
        heap::init();
        init_env_vars(env_vars_ptr);
        main();
        syscall::api::sys_exit(0);
    }
}

#[cfg(feature = "user")]
#[cfg(test)]
pub trait TestCase {
    fn run(&self);
}

#[cfg(feature = "user")]
#[cfg(test)]
impl<T> TestCase for T
where
    T: Fn(),
{
    fn run(&self) {
        print!("Test {:<80}", core::any::type_name::<T>());
        self();
        println!("[Ok]");
    }
}

#[cfg(feature = "user")]
#[cfg(test)]
pub fn test_runner(tests: &[&dyn TestCase]) {
    println!("Running {} tests.", tests.len());
    for test in tests {
        test.run();
    }
}

#[cfg(feature = "user")]
#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    use core::fmt::Arguments;

    use fs::File;
    use rw_ulib_types::fcntl::OpenFlags;

    fn print(args: Arguments) -> error::Result<()> {
        io::stdout().replace_with(&File::with_flags("/dev/tty", OpenFlags::RDWR)?)?;
        io::stdout().print_fmt_err(args)?;
        Ok(())
    }

    let _ = print(format_args!("panic: {info}"));
    process::exit(1);
}
