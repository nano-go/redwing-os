#![no_std]
#![no_main]
#![reexport_test_harness_main = "test_main"]
#![test_runner(crate::test::test_runner)]
#![feature(custom_test_frameworks)]
#![feature(alloc_error_handler)]
#![feature(decl_macro)]
#![feature(linked_list_cursors)]
#![feature(negative_impls)]
#![feature(never_type)]
#![feature(const_trait_impl)]
#![feature(allocator_api)]
#![feature(try_with_capacity)]
#![feature(box_as_ptr)]

use core::{
    panic::PanicInfo,
    sync::atomic::{AtomicBool, Ordering},
};

use arch::{
    arch_init_hart,
    cpu::{self},
};
use devices::dev_init;
use drivers::{uart::uart_init, virtio::block::virtio_block_device_init};
use init::setup_init_task;
use log::info;
use logging::log_init;
use mmu::{mmu_init, vm::kvm_init_hart};
use proc::sched::scheduler;

extern crate alloc;

pub mod devices;
pub mod drivers;
pub mod elf;
pub mod error;
pub mod fs;
pub mod init;
pub mod io;
pub mod logging;
pub mod mmu;
pub mod params;
pub mod pipe;
pub mod print;
pub mod proc;
pub mod sync;
pub mod syscall;
pub mod timer_events;
pub mod trap;
pub mod utils;

#[cfg(test)]
pub mod test;

#[path = "arch/riscv64/mod.rs"]
pub(crate) mod arch;

static INIT_LOCK: AtomicBool = AtomicBool::new(false);

#[no_mangle]
extern "C" fn kernel_main() -> ! {
    let cpuid = cpu::cpuid();
    if cpuid == 0 {
        log_init();
        mmu_init();
        kvm_init_hart();
        uart_init();
        dev_init();
        virtio_block_device_init();
        arch_init_hart();
        setup_init_task();

        #[cfg(not(test))]
        printkln!("{}", params::WELCOME_MSG);

        INIT_LOCK.store(true, Ordering::Release);
    } else {
        while !INIT_LOCK.load(Ordering::Acquire) {
            core::hint::spin_loop();
        }
        kvm_init_hart();
        arch_init_hart();
    }

    info!("CPU {} is initialized", cpuid);

    scheduler();
}

#[cfg(test)]
pub(crate) fn spawn_test_task() {
    use arch::cpu::exit_in_qemu;
    use proc::task;

    task::spawn(|| {
        task::set_name_for_kernel_task("test");
        task::set_sid().unwrap();
        test_main();
        exit_in_qemu();
    });
}

#[panic_handler]
pub fn panic(info: &PanicInfo) -> ! {
    #[cfg(test)]
    test::test_panic_handler(info);

    #[cfg(not(test))]
    {
        printk_with_color!(print::FGColor::BrightRed, "{info}");
        loop {
            cpu::halt();
        }
    }
}
