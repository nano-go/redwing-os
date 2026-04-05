use bitflags::bitflags;
use num_enum::{IntoPrimitive, TryFromPrimitive};

pub const MAX_SIG: u64 = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u64)]
pub enum Signal {
    SIGDEF = 0,
    SIGHUP = 1,
    SIGINT = 2,
    SIGQUIT = 3,
    SIGILL = 4,
    SIGABRT = 6,
    SIGKILL = 9,
    SIGSEGV = 11,
    SIGCONT = 18,
    SIGSTOP = 19,
    SIGTTIN = 21,
}

impl Signal {
    #[must_use]
    #[inline]
    pub fn to_singal_flags(self) -> SignalFlags {
        SignalFlags::from_bits_retain(1 << self as u64)
    }
}

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct SignalFlags: u64 {
        const SIGDEF = 1 << (Signal::SIGDEF as u64);
        const SIGHUP = 1 << (Signal::SIGHUP as u64);
        const SIGINT = 1 << (Signal::SIGINT as u64);
        const SIGQUIT = 1 << (Signal::SIGQUIT as u64);
        const SIGILL = 1 << (Signal::SIGILL as u64);
        const SIGABRT = 1 << (Signal::SIGABRT as u64);
        const SIGKILL = 1 << (Signal::SIGKILL as u64);
        const SIGSEGV = 1 << (Signal::SIGSEGV as u64);
        const SIGCONT = 1 << (Signal::SIGCONT as u64);
        const SIGSTOP = 1 << (Signal::SIGSTOP as u64);
        const SIGTTIN = 1 << (Signal::SIGTTIN as u64);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, IntoPrimitive, TryFromPrimitive)]
#[repr(u32)]
pub enum ProcMaskHow {
    BLOCKED = 0,
    UNBLOCKED,
    SETMASK,
}

#[derive(Clone, Copy)]
#[repr(C)]
pub struct SignalAction {
    pub sig_handler: fn(u32),
    pub mask: SignalFlags,
}
