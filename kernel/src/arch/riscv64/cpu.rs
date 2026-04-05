use core::{
    arch::asm,
    fmt::{self},
};

use riscv::register::*;

pub(super) fn init(hartid: usize) {
    set_cpuid(hartid);

    let info = CpuInfo {
        processor: hartid,
        vendor_id: mvendorid::read().bits(),
        arch_id: marchid::read().bits(),
        impl_id: mimpid::read().bits(),
        isa_id: misa::read().bits(),
        isa: parse_misa(misa::read().bits()),
    };

    crate::proc::cpu::CPU_INFOS[hartid].call_once(|| info);
}

#[inline]
fn set_cpuid(id: usize) {
    unsafe {
        asm!("mv tp, {}", in(reg) id);
    }
}

/// Returns the id of current CPU. This is only avaliable after `_start`.
#[inline]
#[must_use]
pub fn cpuid() -> usize {
    let val: usize;
    unsafe {
        asm!("mv {}, tp",out(reg) val);
    }
    val
}

#[inline]
pub fn halt() {
    unsafe { asm!("wfi") };
}

/// Return whether the intterupt is enable.
#[inline]
pub fn intr_get() -> bool {
    sstatus::read().sie()
}

/// Enable interrupts.
#[inline]
pub fn intr_on() {
    unsafe { sstatus::set_sie() };
}

/// Disable interrupts.
#[inline]
pub fn intr_off() {
    unsafe { sstatus::clear_sie() };
}

#[inline]
pub fn exit_in_qemu() -> ! {
    unsafe {
        (0x100000 as *mut u32).write_volatile(0x5555);
    }
    loop {
        core::hint::spin_loop();
    }
}

#[derive(Default)]
pub struct CpuInfo {
    pub processor: usize,
    pub vendor_id: usize,
    pub arch_id: usize,
    pub impl_id: usize,
    pub isa_id: usize,
    isa: heapless::String<128>,
}

impl fmt::Display for CpuInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let vendor = match self.vendor_id {
            0x0 => "Unknown / Custom",
            0x489 => "SiFive",
            0x101 => "Andes Technology",
            _ => "Unrecognized",
        };

        let arch = match self.arch_id {
            0x0 => "Unknown",
            0x21 => "SiFive U74",
            val if val >> 63 == 1 => "Custom arch ID",
            _ => "Unrecognized",
        };

        writeln!(f, "{:<22}: {}", "Processor", self.processor)?;
        writeln!(f, "{:<22}: {} ({:#x})", "Vendor", vendor, self.vendor_id)?;
        writeln!(f, "{:<22}: {} ({:#x})", "Architecture", arch, self.arch_id)?;
        writeln!(f, "{:<22}: {:#x}", "Implementation ID", self.impl_id)?;
        write!(f, "{:<22}: {} ({:#x})", "ISA", self.isa, self.isa_id)
    }
}

fn parse_misa(misa: usize) -> heapless::String<128> {
    let mut info = heapless::String::new();

    // Extract XLEN (bits 62–63)
    let xlen = match misa >> (usize::BITS - 2) {
        1 => "rv32",
        2 => "rv64",
        3 => "rv128",
        _ => "rv??",
    };

    info.push_str(xlen).unwrap();

    info
}
