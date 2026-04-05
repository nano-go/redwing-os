use core::array;

use lazy_static::lazy_static;
use log::error;

use crate::{
    arch::{
        cpu::{intr_get, intr_off, intr_on},
        ctx::Context,
    },
    mmu::vm::VM,
    params::MAX_NCPU,
    sync::{percpu::PerCpu, spin::Once},
};

use super::task::TaskRef;

#[derive(Default)]
pub struct CPU {
    pub context: Context,

    pub current_task: Option<TaskRef>,
    pub current_vm: Option<VM>,

    /// The depth of `push_off` nesting.
    pub depth_off: usize,

    /// Were interrupts enable before `push_off`.
    pub intena: bool,
}

lazy_static! {
    static ref MYCPU: PerCpu<CPU> = PerCpu::default();
}

lazy_static! {
    pub static ref CPU_INFOS: [Once<crate::arch::cpu::CpuInfo>; MAX_NCPU] =
        array::from_fn(|_| Once::new());
}

#[must_use]
#[inline]
pub fn mycpu() -> &'static CPU {
    MYCPU.get()
}

/// Returns a mutable reference to the current CPU.
///
/// # Safety
///
/// Interrupts must be disabled before calling this, to avoid preemption and
/// race conditions.
#[must_use]
#[inline]
pub unsafe fn mycpu_mut() -> &'static mut CPU {
    PerCpu::get_mut_unchecked(&MYCPU)
}

pub fn push_off() {
    let state = intr_off_store();
    let cpu = unsafe {
        // SAFETY: the interrupt is disabled.
        mycpu_mut()
    };
    if cpu.depth_off == 0 {
        cpu.intena = state;
    }
    cpu.depth_off += 1;
}

pub fn pop_off(dbg_name: &str) {
    if intr_get() {
        error!("{dbg_name}: the interrupt status should be off.")
    }
    let cpu = unsafe {
        // SAFETY: the interrupt is disabled in expected.
        mycpu_mut()
    };
    if cpu.depth_off == 0 {
        error!("{dbg_name}: cpu.depth_off is zero.");
        intr_on();
        return;
    }
    cpu.depth_off -= 1;
    if cpu.depth_off == 0 && cpu.intena {
        intr_on();
    }
}

pub fn intr_off_store() -> bool {
    let state = intr_get();
    intr_off();
    state
}

pub fn intr_on_store() -> bool {
    let state = intr_get();
    intr_on();
    state
}

pub fn intr_restore(flag: bool) {
    if flag {
        intr_on();
    } else {
        intr_off();
    }
}
