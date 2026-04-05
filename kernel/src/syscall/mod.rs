use lazy_static::lazy_static;
use log::{trace, warn};

use crate::{
    arch::{self, ctx::Trapframe},
    error::{KResult, SysError, SysErrorKind},
};

use rw_ulib::syscall::*;

mod fs;
mod mmu;
mod proc;

use fs::*;
use mmu::*;
use proc::*;

pub type SystemCallee = fn(&mut Trapframe) -> KResult<isize>;

macro_rules! register_syscall {
    ($table:tt, $syscall_no:tt, $syscall_fn:tt) => {
        $table[$syscall_no] = Some(($syscall_fn as SystemCallee, stringify!($syscall_fn)));
    };
}

lazy_static! {
    pub static ref SYS_TABLE: [Option<(SystemCallee, &'static str)>; 128] = {
        let mut table = [None; 128];

        register_syscall!(table, SYSCALL_NO_OPEN, sys_open);
        register_syscall!(table, SYSCALL_NO_CLOSE, sys_close);
        register_syscall!(table, SYSCALL_NO_READ, sys_read);
        register_syscall!(table, SYSCALL_NO_WRITE, sys_write);
        register_syscall!(table, SYSCALL_NO_SEEK, sys_seek);
        register_syscall!(table, SYSCALL_NO_RMDIR, sys_rmdir);
        register_syscall!(table, SYSCALL_NO_MKDIR, sys_mkdir);
        register_syscall!(table, SYSCALL_NO_UNLINK, sys_unlink);
        register_syscall!(table, SYSCALL_NO_GETDIRENTS, sys_getdirents);
        register_syscall!(table, SYSCALL_NO_STAT, sys_stat);
        register_syscall!(table, SYSCALL_NO_CD, sys_cd);
        register_syscall!(table, SYSCALL_NO_PIPE, sys_pipe);
        register_syscall!(table, SYSCALL_NO_DUP2, sys_dup2);
        register_syscall!(table, SYSCALL_NO_SYNC, sys_sync);
        register_syscall!(table, SYSCALL_NO_IOCTL, sys_ioctl);

        register_syscall!(table, SYSCALL_NO_EXIT, sys_exit);
        register_syscall!(table, SYSCALL_NO_WAIT, sys_wait);
        register_syscall!(table, SYSCALL_NO_WAITTID, sys_waittid);
        register_syscall!(table, SYSCALL_NO_SCHED_YIELD, sys_sched_yield);
        register_syscall!(table, SYSCALL_NO_FORK, sys_fork);
        register_syscall!(table, SYSCALL_NO_EXECVE, sys_execve);
        register_syscall!(table, SYSCALL_NO_SETPGID, sys_setpgid);
        register_syscall!(table, SYSCALL_NO_GETPGID, sys_getpgid);
        register_syscall!(table, SYSCALL_NO_KILL, sys_kill);
        register_syscall!(table, SYSCALL_NO_SIGPROCMASK, sys_sigprocmask);
        register_syscall!(table, SYSCALL_NO_SIGACTION, sys_sigaction);
        register_syscall!(table, SYSCALL_NO_SIGRETURN, sys_sigreturn);
        register_syscall!(table, SYSCALL_NO_NANOSLEEP, sys_nanosleep);

        register_syscall!(table, SYSCALL_NO_BRK, sys_brk);

        table
    };
}

pub fn syscall(syscall_no: usize, trapframe: &mut Trapframe) -> isize {
    let syscall = SYS_TABLE.get(syscall_no).cloned().flatten();

    if let Some((syscall_fn, name)) = syscall {
        trace!(target: "syscall", "system call {syscall_no}({name})");
        match syscall_fn(trapframe) {
            Ok(code) => code,
            Err(err) => {
                trace!(target: "syscall", 
                    "system call {syscall_no}({name}) return an error: {}({err})",
                    err.errno());
                -(err.errno() as isize)
            }
        }
    } else {
        warn!(target: "syscall", "unknown syscall call {syscall_no}");
        -(SysError::from(SysErrorKind::NoSys).errno() as isize)
    }
}

pub struct SysArg(u64);

macro_rules! define_sys_arg_fn {
    ($name:ident, $register:ident) => {
        #[must_use]
        #[inline]
        pub fn $name(trapframe: &Trapframe) -> SysArg {
            SysArg(trapframe.$register)
        }
    };
}

define_sys_arg_fn!(sys_arg0, a0);
define_sys_arg_fn!(sys_arg1, a1);
define_sys_arg_fn!(sys_arg2, a2);
define_sys_arg_fn!(sys_arg3, a3);
define_sys_arg_fn!(sys_arg4, a4);
define_sys_arg_fn!(sys_arg5, a5);
define_sys_arg_fn!(sys_arg6, a6);

macro_rules! impl_tryfrom_for {
    ($num_ty:ty) => {
        impl TryFrom<SysArg> for $num_ty {
            type Error = SysError;

            fn try_from(value: SysArg) -> Result<Self, Self::Error> {
                <$num_ty>::try_from(value.0).map_err(|_| {
                    syserr::sys_err!(
                        SysErrorKind::InvalidArgument,
                        "can not convert a number to {}: {}({:#x})",
                        stringify!($num_ty),
                        value.0,
                        value.0
                    )
                })
            }
        }
    };
}

impl_tryfrom_for!(isize);
impl_tryfrom_for!(usize);
impl_tryfrom_for!(i64);
impl_tryfrom_for!(u64);
impl_tryfrom_for!(i32);
impl_tryfrom_for!(u32);
impl_tryfrom_for!(i16);
impl_tryfrom_for!(u16);
impl_tryfrom_for!(i8);
impl_tryfrom_for!(u8);

pub struct CStringArg {
    base_addr: usize,
    len: usize,
}

impl TryFrom<SysArg> for CStringArg {
    type Error = SysError;

    fn try_from(value: SysArg) -> Result<Self, Self::Error> {
        let base_addr = usize::try_from(value)?;
        Self::new(base_addr)
    }
}

impl CStringArg {
    pub fn new(base_addr: usize) -> KResult<Self> {
        let mut addr = base_addr;
        loop {
            let byte_ptr = sys_arg_ref::<u8>(addr)?;
            if *byte_ptr == 0 {
                break;
            }
            addr += 1;
        }
        Ok(Self {
            base_addr,
            len: addr - base_addr,
        })
    }

    #[must_use]
    #[inline]
    pub fn get(&self) -> &'static [u8] {
        unsafe { core::slice::from_raw_parts(self.base_addr as *mut u8, self.len) }
    }

    #[must_use]
    #[inline]
    pub fn get_mut(&mut self) -> &'static mut [u8] {
        unsafe { core::slice::from_raw_parts_mut(self.base_addr as *mut u8, self.len) }
    }
}

impl<T: Sized> TryFrom<SysArg> for Option<&T> {
    type Error = SysError;

    fn try_from(value: SysArg) -> Result<Self, Self::Error> {
        if value.0 == 0 {
            return Ok(None);
        }
        sys_arg_ref(value.0 as usize).map(|mutt| Some(mutt as &T))
    }
}

impl<T: Sized> TryFrom<SysArg> for Option<&mut T> {
    type Error = SysError;

    fn try_from(value: SysArg) -> Result<Self, Self::Error> {
        if value.0 == 0 {
            return Ok(None);
        }
        sys_arg_ref(value.0 as usize).map(Some)
    }
}

/// Attempts to create a mutable slice `&'a mut [T]` from a user-provided
/// address and length.
///
/// This function performs crucial safety checks to ensure the memory region
/// is valid and safe for kernel access:
/// 1. It verifies that the entire memory region (`addr` to `addr + size`) is
///    located within the user space.
/// 2. It checks if the starting address `addr` is properly aligned for type
///    `T`.
///
/// For the special case, `len=0`, the base address of an empty string in Rust
/// likes `""` points to `0x1` which is safed with zero length.
///
/// This is usually called by code that are automatically generated by
/// `syscall_macro`;
///
/// # Arguments
///
/// * `addr` - The starting memory address in user space.
/// * `len` - The number of elements of type `T` in the slice.
///
/// # Returns
///
/// * `Ok(&'a mut [T])` if the memory access is safe and the slice is
///   successfully created.
/// * `Err(SysErrorKind::Fault)` if the memory region is not entirely from user
///   space, or if the address is not properly aligned for type `T`.
pub fn sys_arg_slice_mut<'a, T>(addr: usize, len: usize) -> KResult<&'a mut [T]> {
    let size = len * core::mem::size_of::<T>();
    if size != 0
        && (!arch::memlayout::is_from_user(addr, size) || addr % core::mem::align_of::<T>() != 0)
    {
        return Err(SysErrorKind::Fault.into());
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(addr as *mut T, len) })
}

/// Likes [`sys_arg_slice_mut`] but this returns an immutable slice.
pub fn sys_arg_slice<'a, T>(addr: usize, len: usize) -> KResult<&'a [T]> {
    let size = len * core::mem::size_of::<T>();
    if size != 0
        && (!arch::memlayout::is_from_user(addr, size) || addr % core::mem::align_of::<T>() != 0)
    {
        return Err(SysErrorKind::Fault.into());
    }
    Ok(unsafe { core::slice::from_raw_parts(addr as *const T, len) })
}

/// Converts a user-provided address to a mutable reference `&'a mut T`,
/// ensuring it is safe for kernel access.
///
/// This function performs the following critical safety checks:
/// 1. It verifies that the memory region occupied by a single `T` at `addr` is
///    located entirely within the user space.
/// 2. It checks if the address `addr` is properly aligned for type `T`.
///
/// This is usually called by code that are automatically generated by
/// `syscall_macro`;
///
/// # Arguments
///
/// * `addr` - The starting memory address of the `T` instance in user space.
///
/// # Returns
///
/// * `Ok(&'a mut T)` if the memory access is safe and the reference is
///   successfully created.
/// * `Err(SysErrorKind::Fault)` if the memory region is not entirely from user
///   space, or if the address is not properly aligned for type `T`.
pub fn sys_arg_ref<'a, T>(addr: usize) -> KResult<&'a mut T> {
    let size = core::mem::size_of::<T>();
    if !arch::memlayout::is_from_user(addr, size) || addr % core::mem::align_of::<T>() != 0 {
        return Err(SysErrorKind::Fault.into());
    }
    Ok(unsafe { &mut *(addr as *mut T) })
}

/// Checks whether the address of a function is safed for kernel(from user).
pub fn check_sys_arg_fn(addr: usize) -> KResult<()> {
    if !arch::memlayout::is_from_user_elf(addr) {
        Err(SysErrorKind::Fault.into())
    } else {
        Ok(())
    }
}
