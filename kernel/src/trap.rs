use log::{error, warn};

use crate::{
    arch::timer::timer_now,
    drivers::{
        uart::{uart_intr, UART_IRQ},
        virtio::{block::virtio_blk_intr, VIRTIO_IRQ},
    },
    proc::{cpu::mycpu, sched::get_current_scheduler},
    timer_events,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessType {
    Write,
    Read,
    Exec,
}

pub fn timer_intr_handler() {
    let jiffies = timer_now();
    timer_events::tick(jiffies);
    get_current_scheduler().tick(jiffies);
}

pub fn dev_intr_handler(irq: u32) {
    match irq {
        VIRTIO_IRQ => {
            virtio_blk_intr();
        }
        UART_IRQ => {
            uart_intr();
        }
        0 => {}
        _ => warn!("unknown irq {}", irq),
    }
}

pub fn page_fault_handler(addr: usize, access: AccessType) -> bool {
    if let Some(vm) = &mycpu().current_vm {
        let mut vm = vm.lock();
        match vm.on_page_fault(addr, access) {
            Ok(()) => return true,
            Err(err) => error!("vm on_pafe_fault: {err}"),
        }
    }
    error!(
        "EXCEPTION: Page fault @ {:#x} with access type {:?}",
        addr, access
    );
    false
}
