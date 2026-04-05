use core::{ptr, time::Duration};

use alloc::{
    ffi::CString,
    format,
    string::{String, ToString},
    vec::Vec,
};
use rw_ulib_types::{signal::Signal, time::Timespec};

use crate::{
    env,
    error::{wrap_with_result, Error, Result},
    fs,
    syscall::api::{
        sys_execve, sys_exit, sys_fork, sys_getpgid, sys_kill, sys_nanosleep, sys_sched_yield,
        sys_setpgid, sys_wait, sys_waittid,
    },
};
use path::Path;

pub type Tid = u64;

pub fn wait(status: &mut u32) -> Result<Option<Tid>> {
    let code = unsafe { sys_wait(ptr::addr_of!(*status).addr()) };
    let child_tid = wrap_with_result(code)?;
    if child_tid == 0 {
        Ok(None)
    } else {
        Ok(Some(child_tid as Tid))
    }
}

pub fn waittid(tid: Tid, status: &mut u32) -> Result<()> {
    let code = unsafe { sys_waittid(tid.try_into().unwrap(), ptr::addr_of!(*status).addr()) };
    wrap_with_result(code)?;
    Ok(())
}

pub fn fork() -> Result<Option<Tid>> {
    let child_tid = wrap_with_result(unsafe { sys_fork() })?;
    if child_tid == 0 {
        Ok(None)
    } else {
        Ok(Some(child_tid as Tid))
    }
}

pub fn exec<P: AsRef<Path>>(path: P, args: Vec<String>) -> Result<!> {
    execve(
        path,
        args,
        env::vars()
            .map(|(key, value)| format!("{key}={value}"))
            .collect(),
    )
}

pub fn execp<P: AsRef<Path>>(path: P, args: Vec<String>) -> Result<!> {
    let path = path.as_ref();
    if path.as_bytes().starts_with(b"./") || path.is_absolute() {
        return exec(path, args);
    }
    let path_var = env::var("PATH").unwrap_or_else(|| "/bin:/usr/bin".to_string());
    let dirs = path_var.split(':').chain(["./"].into_iter());
    for dir in dirs {
        let exec_path = format!("{dir}/{path}");
        let err = exec(exec_path, args.clone()).unwrap_err();
        match err {
            Error::System(syserr::SysErrorKind::NoSuchFileOrDirectory) => {
                continue;
            }
            err => return Err(err),
        }
    }
    Err(Error::System(syserr::SysErrorKind::NoSuchFileOrDirectory))
}

pub fn execve<P: AsRef<Path>>(path: P, args: Vec<String>, env_vars: Vec<String>) -> Result<!> {
    let path = fs::cstr_path(path.as_ref());

    let c_args: Vec<_> = args
        .into_iter()
        .map(|arg| CString::new(arg).unwrap())
        .collect();
    let c_args_addrs: Vec<_> = c_args.iter().map(|arg| arg.as_ptr()).collect();

    let c_env_vars: Vec<_> = env_vars
        .into_iter()
        .map(|var| CString::new(var).unwrap())
        .collect();
    let c_env_addrs: Vec<_> = c_env_vars.iter().map(|var| var.as_ptr()).collect();

    let code = sys_execve(&path, &c_args_addrs, &c_env_addrs);
    wrap_with_result(code)?;
    unreachable!();
}

#[inline]
pub fn yield_now() -> Result<()> {
    wrap_with_result(unsafe { sys_sched_yield() })?;
    Ok(())
}

#[inline]
pub fn exit(status: i32) -> ! {
    wrap_with_result(unsafe { sys_exit(status) }).unwrap();
    unreachable!()
}

#[inline]
pub fn kill(pid: i64, signal: Signal) -> Result<()> {
    wrap_with_result(unsafe { sys_kill(pid, signal as u32) })?;
    Ok(())
}

#[inline]
pub fn set_pgid(pid: i64, pgid: i64) -> Result<()> {
    wrap_with_result(unsafe { sys_setpgid(pid, pgid) })?;
    Ok(())
}

#[inline]
pub fn get_pgid(pid: i64) -> Result<u64> {
    let pgid = wrap_with_result(unsafe { sys_getpgid(pid) })?;
    Ok(pgid as u64)
}

#[inline]
pub fn sleep(duration: Duration) -> Result<()> {
    wrap_with_result(sys_nanosleep(&Timespec::from(duration), None))?;
    Ok(())
}
