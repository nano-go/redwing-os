use core::fmt;

use crate::{
    arch::{
        cpu::{cpuid, intr_get, intr_on},
        plic,
    },
    proc::{
        signal::{handle_signals, send_signal_to_current},
        task::{self},
    },
    trap::AccessType,
};

use super::{cpu::intr_off, ctx::Trapframe};

use log::error;
use riscv::register::*;
use rw_ulib_types::signal::SignalFlags;

extern "C" {
    /// This is defined in `trap.S` file.
    pub fn _kernel_trap_vec();
    pub fn _user_trap_vec();
    pub fn _user_trap_ret(a: usize) -> !;
}

/// Enable interrupts and set the entry of trap.
pub(super) unsafe fn trap_init_hart() {
    // Enable software and external intterupts.
    sie::set_sext();
    sie::set_ssoft();

    stvec::write(stvec::Stvec::from_bits(_kernel_trap_vec as _));
}

/// Interrupts or exceptions in kernel code go here via `kernel_trap_vec`.
#[no_mangle]
unsafe fn kernel_trap_handler() {
    let sstatus = sstatus::read();
    let sepc = sepc::read();

    if intr_get() {
        error!("kernel_trap_handler: interrupts enabled");
    }

    if sstatus::read().spp() != sstatus::SPP::Supervisor {
        error!("kernel_trap_handler: the interrupt is not from supervisor.")
    }

    handle_scause();

    // The sstatus and sepc may be changed during the trap.
    sstatus::write(sstatus);
    sepc::write(sepc);
}

/// Interrupts or exceptions in user code go here via `user_trap_vec`.
#[no_mangle]
unsafe fn user_trap_handler(_trapframe: &Trapframe) -> ! {
    {
        if sstatus::read().spp() != sstatus::SPP::User {
            error!("user_trap_handler: the interrupt is not from user.")
        }

        // Sends intterupts and exceptions to kernel_trap_vec since we are now in
        // kernel.
        stvec::write(stvec::Stvec::from_bits(_kernel_trap_vec as _));

        let mut trapframe = task::current_trapframe();
        trapframe.sepc = sepc::read() as u64;

        handle_scause();
    }

    user_trap_ret();
}

pub fn user_trap_ret() -> ! {
    let _ = handle_signals();

    if let Some(task) = task::current_task() {
        let task = task.lock_irq_save();
        if task.is_killed {
            let exit_status = task.exit_status;
            drop(task);
            task::exit(exit_status);
        }
    }

    intr_off();

    let trapframe_addr = {
        let task = task::current_task().unwrap();
        let task = task.lock();
        let mut trapframe = task.trapframe();
        trapframe.hartid = cpuid() as u64;
        trapframe.kernel_stack = (task.kstack.as_ptr().addr() + task.kstack.len()) as u64;
        unsafe { sepc::write(trapframe.sepc as usize) };
        trapframe.addr().get()
    };

    unsafe {
        sstatus::set_spp(sstatus::SPP::User);
        // Sends intterupts and exceptions to user_trap_handler.
        stvec::write(stvec::Stvec::from_bits(_user_trap_vec as _));
        _user_trap_ret(trapframe_addr)
    }
}

fn handle_scause() {
    match Scause::from(scause::read().bits()) {
        Scause::Intr(Interrupt::Timer) => {
            // Set the next time event trigger.
            super::timer::set_next_time();
            crate::trap::timer_intr_handler();
        }

        Scause::Intr(Interrupt::External) => {
            let irq = plic::claim();
            crate::trap::dev_intr_handler(irq);
            if irq != 0 {
                plic::complete(irq);
            }
        }

        Scause::Intr(intr) => {
            error!(
                "Unexpected intterrupt occured: \n\tsepc: {:#x} from user: {}\n\t{}",
                sepc::read(),
                sstatus::read().spp() == sstatus::SPP::User,
                intr
            )
        }

        Scause::Excp(Exception::EnvCallFromUMode) => {
            let mut trapframe = task::current_trapframe();

            // 'ecall' instruction to lead here.
            // skip the 'ecall' instruction to return to the next instruction.
            trapframe.sepc += 4;

            intr_on();
            let syscall_no = trapframe.a7 as usize;
            let ret_value = crate::syscall::syscall(syscall_no, &mut trapframe);
            trapframe.a0 = ret_value as u64;
        }

        Scause::Excp(exception) => {
            handle_exception(exception, sepc::read());
        }
    }
}

fn handle_exception(exp: Exception, sepc: usize) {
    let addr = stval::read();

    match exp {
        Exception::InstructionPageFault => {
            if crate::trap::page_fault_handler(addr, AccessType::Exec) {
                return;
            }
            let _ = send_signal_to_current(SignalFlags::SIGSEGV);
        }

        Exception::LoadPageFault => {
            if crate::trap::page_fault_handler(addr, AccessType::Read) {
                return;
            }
            let _ = send_signal_to_current(SignalFlags::SIGSEGV);
        }

        Exception::StorePageFault => {
            if crate::trap::page_fault_handler(addr, AccessType::Write) {
                return;
            }
            let _ = send_signal_to_current(SignalFlags::SIGSEGV);
        }

        Exception::IllegalInstruction => {
            let _ = send_signal_to_current(SignalFlags::SIGILL);
        }

        _ => {
            let _ = send_signal_to_current(SignalFlags::SIGKILL);
        }
    };

    log::error!("An exception occurs(RISC-V64):");
    log::error!("  {}", exp);
    log::error!("Registers:");
    log::error!(
        "  sepc: {:#x}, scause: {:#x}, stval: {:#x}, CPU: {}",
        sepc,
        usize::from(exp),
        addr,
        cpuid(),
    );
}

// Hanlde scause register

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(usize)]
enum Scause {
    Intr(Interrupt),
    Excp(Exception),
}

impl From<usize> for Scause {
    fn from(value: usize) -> Self {
        if value & (1 << (usize::BITS - 1)) != 0 {
            Scause::Intr(Interrupt::from(value & 0xFFFF))
        } else {
            Scause::Excp(Exception::from(value & 0xFFFF))
        }
    }
}

impl From<Scause> for usize {
    fn from(scause: Scause) -> Self {
        match scause {
            Scause::Intr(interrupt) => usize::from(interrupt) | (1 << (usize::BITS - 1)),
            Scause::Excp(exception) => usize::from(exception),
        }
    }
}

impl fmt::Display for Scause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Intr(intr) => write!(f, "INTR: {}", intr),
            Self::Excp(excp) => write!(f, "EXCEPTION: {}", excp),
        }
    }
}

const EXCEPTION_CODE_DESC: [Option<&'static str>; 16] = [
    Some("Instruction address misaligned"),
    Some("Instruction access fault"),
    Some("Instruction illgeal instruction"),
    Some("Break point"),
    Some("Load address misaligned"),
    Some("Load access fault"),
    Some("Store/AMO address misaligned"),
    Some("Store/AMO access fault"),
    Some("Environment call from U-mode"),
    Some("Environment call from S-mode"),
    None,
    None,
    Some("Instruction page fault"),
    Some("Load page fault"),
    None,
    Some("Store/AMO page fault"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, usize)]
enum Exception {
    InstructionAddrMisalined = 0,
    InstructionAccessFault = 1,
    IllegalInstruction = 2,
    BreakPoint = 3,
    LoadAddrMisalined = 4,
    LoadAccessFault = 5,
    StoreAddrMisaligned = 6,
    StoreAccessFault = 7,
    EnvCallFromUMode = 8,
    EnvCallFromSMode = 9,
    InstructionPageFault = 12,
    LoadPageFault = 13,
    StorePageFault = 15,
    SoftWareCheck = 18,
    HardwareError = 19,
    Reserved(usize),
    Custom(usize),
}

impl From<usize> for Exception {
    fn from(value: usize) -> Self {
        match value {
            0 => Exception::InstructionAddrMisalined,
            1 => Exception::InstructionAccessFault,
            2 => Exception::IllegalInstruction,
            3 => Exception::BreakPoint,
            4 => Exception::LoadAddrMisalined,
            5 => Exception::LoadAccessFault,
            6 => Exception::StoreAddrMisaligned,
            7 => Exception::StoreAccessFault,
            8 => Exception::EnvCallFromUMode,
            9 => Exception::EnvCallFromSMode,
            12 => Exception::InstructionPageFault,
            13 => Exception::LoadPageFault,
            15 => Exception::StorePageFault,
            18 => Exception::SoftWareCheck,
            19 => Exception::HardwareError,
            24..=31 | 48..=63 => Exception::Custom(value),
            _ => Exception::Reserved(value),
        }
    }
}

impl From<Exception> for usize {
    fn from(exception: Exception) -> Self {
        match exception {
            Exception::InstructionAddrMisalined => 0,
            Exception::InstructionAccessFault => 1,
            Exception::IllegalInstruction => 2,
            Exception::BreakPoint => 3,
            Exception::LoadAddrMisalined => 4,
            Exception::LoadAccessFault => 5,
            Exception::StoreAddrMisaligned => 6,
            Exception::StoreAccessFault => 7,
            Exception::EnvCallFromUMode => 8,
            Exception::EnvCallFromSMode => 9,
            Exception::InstructionPageFault => 12,
            Exception::LoadPageFault => 13,
            Exception::StorePageFault => 15,
            Exception::SoftWareCheck => 18,
            Exception::HardwareError => 19,
            Exception::Reserved(code) | Exception::Custom(code) => code,
        }
    }
}

impl fmt::Display for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code: usize = usize::from(*self);
        match self {
            Self::Reserved(code) => write!(f, "Reserved {}", code),
            Self::Custom(code) => write!(f, "Custom {}", code),
            _ => {
                let desc = EXCEPTION_CODE_DESC.get(code).and_then(|x| *x).unwrap();
                write!(f, "{}", desc)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C, usize)]
enum Interrupt {
    Software = 1,
    Timer = 5,
    External = 9,
    CounterFlow = 13,
    Other(usize),
}

impl From<usize> for Interrupt {
    fn from(value: usize) -> Self {
        match value {
            1 => Interrupt::Software,
            5 => Interrupt::Timer,
            9 => Interrupt::External,
            13 => Interrupt::CounterFlow,
            _ => Interrupt::Other(value),
        }
    }
}

impl From<Interrupt> for usize {
    fn from(interrupt: Interrupt) -> Self {
        match interrupt {
            Interrupt::Software => 1,
            Interrupt::Timer => 5,
            Interrupt::External => 9,
            Interrupt::CounterFlow => 13,
            Interrupt::Other(code) => code,
        }
    }
}

impl fmt::Display for Interrupt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Interrupt::Software => write!(f, "Software Interrupt"),
            Interrupt::Timer => write!(f, "Timer Interrupt"),
            Interrupt::External => write!(f, "External Interrupt"),
            Interrupt::CounterFlow => write!(f, "CounterFlow Interrupt"),
            Interrupt::Other(code) => write!(f, "Other Interrupt ({})", code),
        }
    }
}
