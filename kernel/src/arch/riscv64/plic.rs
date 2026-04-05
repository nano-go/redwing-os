//! The RISC-V platform-level-interrupt-controller system priorties and
//! distributes global interrupts in a RISC-V system.

use core::ptr;

use crate::drivers::{uart::UART_IRQ, virtio::VIRTIO_IRQ};

use super::{cpu::cpuid, memlayout::PLIC_BASE_VADDR};

pub fn init_hart() {
    let hart_id = cpuid();
    set_intr_priority(VIRTIO_IRQ, 1);
    set_intr_priority(UART_IRQ, 1);
    enable_intr(hart_id, &[VIRTIO_IRQ, UART_IRQ]);
    set_priority_threshold(hart_id, 0);
}

fn w_reg(offset: usize, value: u32) {
    unsafe {
        ptr::write_volatile((PLIC_BASE_VADDR + offset) as *mut u32, value);
    }
}

fn r_reg(offset: usize) -> u32 {
    unsafe { ptr::read_volatile((PLIC_BASE_VADDR + offset) as *mut u32) }
}

/// The priority value 0 is reserved to mean "never interrupt", and interrupt
/// priority increases with increasing integer values.
fn set_intr_priority(irq: u32, priority: u32) {
    assert!((1..1024).contains(&irq));
    w_reg(irq as usize * 4, priority);
}

/// Each global interrupt can be enabled by this.
///
/// The PLIC spec defines separate enable contexts per mode(M-mode, S-mode).
/// This is for S-mode.
fn enable_intr(hart_id: usize, irqs: &[u32]) {
    const EANBLE_BASE_ADDR: usize = 0x2080;
    const SIZE_PER_CTX: usize = 0x100;

    let base = EANBLE_BASE_ADDR + SIZE_PER_CTX * hart_id;
    for irq in irqs {
        let addr = base + *irq as usize / 32;
        let bit_pos = *irq as usize % 32;
        // To enable a interrupt for a hart, set the bit correspoding to the
        // interrupt.
        w_reg(addr, r_reg(addr) | (1 << bit_pos));
    }
}

/// PLIC provides context based threshold register for the settings of a
/// interrupt priority threshold of each context.
///
/// Usally set 0 to accept all interrupts.
///
/// The PLIC spec defines separate priority-threshold contexts per mode(M-mode,
/// S-mode). This is for S-mode.
fn set_priority_threshold(hart_id: usize, value: u32) {
    const PRIORITY_THRESHOLD_BASE_ADDR: usize = 0x201000;
    const SIZE_PER_CTX: usize = 0x2000;
    w_reg(PRIORITY_THRESHOLD_BASE_ADDR + hart_id * SIZE_PER_CTX, value);
}

/// Ask PLIC what interrupt we should serve.
pub fn claim() -> u32 {
    const CLAIM_BASE_ADDR: usize = 0x201000;
    const SIZE_PER_CTX: usize = 0x2000;
    r_reg(CLAIM_BASE_ADDR + cpuid() * SIZE_PER_CTX + 4)
}

/// Tell PLIC this intterupt request has handled.
pub fn complete(irq: u32) {
    const CLAIM_BASE_ADDR: usize = 0x201000;
    const SIZE_PER_CTX: usize = 0x2000;
    w_reg(CLAIM_BASE_ADDR + cpuid() * SIZE_PER_CTX + 4, irq)
}
