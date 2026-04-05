use core::time::Duration;

use rw_ulib_types::{
    signal::{ProcMaskHow, Signal, SignalAction, SignalFlags},
    time::Timespec,
};
use syscall_macro::syscall;
use syserr::SysErrorKind;

use crate::{
    error::KResult,
    proc::{exec, id::Tid, signal, task},
    syscall::CStringArg,
};

use super::check_sys_arg_fn;

#[syscall(name = "sys_exit")]
fn _sys_exit(status: i32) -> KResult<isize> {
    task::exit(status);
}

#[syscall(name = "sys_wait")]
fn _sys_wait(exit_status: &mut i32) -> KResult<isize> {
    let tid = task::wait_with_xstatus(None, exit_status)?;
    Ok(tid.map(|tid| *tid as isize).unwrap_or(0))
}

#[syscall(name = "sys_waittid")]
fn _sys_waittid(tid: u64, exit_status: &mut i32) -> KResult<isize> {
    let tid = task::wait_with_xstatus(Some(Tid::for_query(tid)), exit_status)?;
    Ok(if tid.is_some() { 0 } else { -1 })
}

#[syscall(name = "sys_sched_yield")]
fn _sys_sched_yield() -> KResult<isize> {
    task::yield_now();
    Ok(0)
}

#[syscall(name = "sys_fork")]
fn _sys_fork() -> KResult<isize> {
    Ok(*exec::fork()? as isize)
}

#[syscall(name = "sys_execve")]
fn _sys_execve(path: CStringArg, args: &[usize], env_vars: &[usize]) -> KResult<isize> {
    fn from_cstring_array(cstrs: &[usize]) -> KResult<heapless::Vec<&str, 256>> {
        let mut str_vec = heapless::Vec::new();

        for cstr_addr in cstrs {
            let cstr = CStringArg::new(*cstr_addr)?;
            let str = str::from_utf8(cstr.get()).map_err(|_| SysErrorKind::InvalidUt8Str)?;
            str_vec
                .push(str)
                .map_err(|_| SysErrorKind::InvalidArgument)?;
        }
        Ok(str_vec)
    }

    exec::execve(
        path.get(),
        &from_cstring_array(args)?,
        &from_cstring_array(env_vars)?,
    )?;
}

#[syscall(name = "sys_setpgid")]
fn _sys_setpgid(pid: i64, pgid: i64) -> KResult<isize> {
    let target_tid = Tid::for_query(u64::try_from(pid)?);
    let new_tgid = Tid::for_query(u64::try_from(pgid)?);

    task::set_tgid(&target_tid, &new_tgid)?;
    Ok(0)
}

#[syscall(name = "sys_getpgid")]
fn _sys_getpgid(pid: i64) -> KResult<isize> {
    let tid = Tid::for_query(u64::try_from(pid)?);
    let tgid = task::get_tgid(&tid)?;
    Ok(tgid.as_u64() as isize)
}

#[syscall(name = "sys_kill")]
fn _sys_kill(pid: i64, signal: u32) -> KResult<isize> {
    let signal = if signal == 0 {
        None
    } else {
        Some(Signal::try_from(signal as u64).map_err(|_| SysErrorKind::InvalidArgument)?)
    };
    signal::kill(pid, signal)?;
    Ok(0)
}

#[syscall(name = "sys_sigprocmask")]
fn _sys_sigprocmask(
    how: u32,
    mask: &SignalFlags,
    old_mask: Option<&mut SignalFlags>,
) -> KResult<isize> {
    let how = ProcMaskHow::try_from(how).map_err(|_| SysErrorKind::InvalidArgument)?;
    signal::sigprocmask(how, mask, old_mask)?;
    Ok(0)
}

#[syscall(name = "sys_sigaction")]
fn _sys_sigaction(
    signal: u32,
    act: &SignalAction,
    old_act: Option<&mut SignalAction>,
) -> KResult<isize> {
    // Checks address is valid.
    check_sys_arg_fn(act.sig_handler as usize)?;
    let signum = Signal::try_from(signal as u64).map_err(|_| SysErrorKind::InvalidArgument)?;
    signal::sigaction(signum, act, old_act)?;
    Ok(0)
}

#[syscall(name = "sys_sigreturn")]
fn _sys_sigreturn() -> KResult<isize> {
    signal::sigreturn()?;
    Ok(0)
}

#[syscall(name = "sys_nanosleep")]
fn _sys_nanosleep(req: &Timespec, rem: Option<&mut Timespec>) -> KResult<isize> {
    let mut dur_rem = Duration::new(0, 0);
    let result = task::sleep_rem(
        Duration::try_from(*req).map_err(|_| SysErrorKind::InvalidArgument)?,
        Some(&mut dur_rem),
    );
    if result.is_err() {
        if let Some(rem) = rem {
            *rem = dur_rem.into();
        }
    }
    result.map(|_| 0)
}
