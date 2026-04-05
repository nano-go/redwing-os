use core::{arch::asm, time::Duration};

use crate::params::TIMER_FREQ_HZ;

use riscv::register::*;

const MENVCFG_STCE_BIT: usize = 1 << 63;
const MCOUNTEREN_TM_BIT: usize = 1 << 1;

/// Initialize the timer intterupt.
pub fn init() {
    unsafe {
        // Enable timer intterupt in supervisor mode.
        sie::set_stimer();
        // Enable sstc extension.
        set_stce();
        // Makes `stimecmp` accessible.
        set_mcounteren_time_bit();
    }
    set_next_time();
}

#[inline]
#[must_use]
pub fn read_menvcfg() -> usize {
    let val: usize;
    unsafe {
        // asm!("csrr {}, menvcfg", out(reg) val);
        asm!("csrr {}, 0x30A", out(reg) val);
    }
    val
}

#[inline]
pub fn write_menvcfg(menvcfg: usize) {
    unsafe {
        // asm!("csrw menvcfg, {}", in(reg) menvcfg);
        asm!("csrw 0x30A, {}", in(reg) menvcfg);
    }
}

/// Enable the `stimecmp` in supervisor mode.
pub fn set_stce() {
    write_menvcfg(read_menvcfg() | MENVCFG_STCE_BIT);
}

/// If the `TM` bit in `mcounteren` is set, access to the `stimecmp` is
/// premitted in supervisor mode if implemented.
#[inline]
fn set_mcounteren_time_bit() {
    unsafe {
        asm!("csrsi mcounteren, {}", const MCOUNTEREN_TM_BIT);
    }
}

#[inline]
#[must_use]
pub fn get_cycle() -> u64 {
    time::read() as u64
}

#[inline]
#[must_use]
pub fn timer_now() -> Duration {
    Duration::from_nanos(get_cycle() * 100)
}

#[inline]
pub fn set_next_time() {
    let interval = 10_000_000 / TIMER_FREQ_HZ;

    fn write(stimecmp: usize) {
        unsafe {
            asm!("csrw 0x14d, {}", in(reg) stimecmp);
        }
    }

    write(get_cycle() as usize + interval);
}
