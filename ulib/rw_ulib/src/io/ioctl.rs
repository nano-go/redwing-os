use core::ptr::addr_of;

use rw_ulib_types::ioctl;

use crate::{
    error::{wrap_with_result, Result},
    fs::FileDiscriptor,
    syscall::api::sys_ioctl,
};

fn ioctl(fd: FileDiscriptor, request: ioctl::Request, args: usize) -> Result<usize> {
    wrap_with_result(unsafe { sys_ioctl(fd.0, request as u64, args) })
}

pub fn tcsetpgrp(fd: FileDiscriptor, pid: u64) -> Result<()> {
    ioctl(fd, ioctl::Request::TIOCSPGRP, addr_of!(pid) as usize)?;
    Ok(())
}

pub fn tcgetpgrp(fd: FileDiscriptor) -> Result<u64> {
    let pgid = ioctl(fd, ioctl::Request::TIOCGPGRP, 0)?;
    Ok(pgid as u64)
}
