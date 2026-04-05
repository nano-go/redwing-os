use core::panic::PanicInfo;

use crate::{arch, print::FGColor, printk_with_color};

pub trait TestCase {
    fn run(&self);
}

impl<T> TestCase for T
where
    T: Fn(),
{
    fn run(&self) {
        let name = [core::any::type_name::<T>(), "..."].concat();
        printk_with_color!(FGColor::Green, "Test {:<65}", name);
        let start = arch::timer::timer_now();
        self();
        let dur = arch::timer::timer_now() - start;
        printk_with_color!(FGColor::Green, "[{:>4}ms] OK\n", dur.as_millis());
    }
}

pub fn test_runner(tests: &[&dyn TestCase]) {
    printk_with_color!(FGColor::Magenta, "Running {} tests\n\n", tests.len());
    for test in tests {
        test.run();
    }
}

pub fn test_panic_handler(info: &PanicInfo) -> ! {
    printk_with_color!(FGColor::BrightRed, "FAILED\n\n");
    printk_with_color!(FGColor::BrightRed, "1 task failed.\n\n");
    printk_with_color!(FGColor::BrightRed, "{info}\n");
    arch::cpu::exit_in_qemu();
}
