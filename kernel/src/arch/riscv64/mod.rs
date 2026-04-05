use core::arch::asm;

use riscv::register::{medeleg::Medeleg, mideleg::Mideleg, satp::Satp, *};

use crate::kernel_main;

pub mod asm;
pub mod cpu;
pub mod ctx;
pub mod memlayout;
pub mod memory;
pub mod pgtable;
pub mod plic;
pub mod timer;
pub mod trap;

#[no_mangle]
pub unsafe extern "C" fn arch_init() -> ! {
    mstatus::set_mpp(mstatus::MPP::Supervisor);

    // Set the return pc.
    mepc::write(kernel_main as usize);

    // Disable virtual memory.
    satp::write(Satp::from_bits(0));

    // Delegate interrupts to supervisor mode.
    mideleg::write(Mideleg::from_bits(0xFFFF));
    medeleg::write(Medeleg::from_bits(0xFFFF));

    pmpaddr0::write((!0) >> 10);
    pmpcfg0::write(0xF);

    cpu::init(mhartid::read());
    timer::init();

    asm!("mret");

    unreachable!()
}

pub fn arch_init_hart() {
    unsafe {
        // Set SUM bit of sstatus to allow that we can access user pages in
        // supervisor mode.
        sstatus::set_sum();
        trap::trap_init_hart();
    }

    plic::init_hart();
}
