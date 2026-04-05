use rw_ulib_types::fcntl::{Dirent, OpenFlags, SeekFrom, Stat};
use syscall_macro::syscall;
use syserr::{sys_err, SysErrorKind};

use crate::{
    error::KResult,
    fs::{fcntl, ioctl},
    proc::id::Tid,
    syscall::{sys_arg_ref, CStringArg},
};

#[syscall(name = "sys_open")]
fn _open(pathname: CStringArg, flags: u32) -> KResult<isize> {
    let flags = OpenFlags::from_bits(flags).ok_or(SysErrorKind::InvalidArgument)?;
    let fd = fcntl::open(pathname.get(), flags)?;
    fcntl::sync()?;
    Ok(fd as isize)
}

#[syscall(name = "sys_close")]
fn _close(fd: u32) -> KResult<isize> {
    fcntl::close(fd)?;
    Ok(0)
}

#[syscall(name = "sys_read")]
fn _read(fd: u32, buf: &mut [u8]) -> KResult<isize> {
    fcntl::read(fd, buf).map(|len| len as isize)
}

#[syscall(name = "sys_write")]
fn _write(fd: u32, buf: &[u8]) -> KResult<isize> {
    let result = fcntl::write(fd, buf)?;
    fcntl::sync()?;
    Ok(result as isize)
}

#[syscall(name = "sys_seek")]
fn _seek(fd: u32, offset: i64, whence: u32) -> KResult<isize> {
    let seekfrom = SeekFrom::try_from(whence)
        .map_err(|_| sys_err!(SysErrorKind::InvalidArgument, "seek: 'whence' is not valid"))?;
    fcntl::seek(fd, offset, seekfrom).map(|old| old as isize)
}

#[syscall(name = "sys_rmdir")]
fn _rmdir(pathname: CStringArg) -> KResult<isize> {
    fcntl::rmdir(pathname.get())?;
    fcntl::sync()?;
    Ok(0)
}

#[syscall(name = "sys_mkdir")]
fn _mkdir(pathname: CStringArg) -> KResult<isize> {
    fcntl::mkdir(pathname.get())?;
    fcntl::sync()?;
    Ok(0)
}

#[syscall(name = "sys_unlink")]
fn _unlink(pathname: CStringArg) -> KResult<isize> {
    fcntl::unlink(pathname.get())?;
    fcntl::sync()?;
    Ok(0)
}

#[syscall(name = "sys_getdirents")]
fn _get_dirents(fd: u32, dirents: &mut [Dirent]) -> KResult<isize> {
    Ok(fcntl::get_dirents(fd, dirents)? as isize)
}

#[syscall(name = "sys_stat")]
fn _stat(pathname: CStringArg, stat: &mut Stat) -> KResult<isize> {
    fcntl::stat(pathname.get(), stat)?;
    Ok(0)
}

#[syscall(name = "sys_cd")]
fn _cd(pathname: CStringArg) -> KResult<isize> {
    fcntl::cd(pathname.get())?;
    Ok(0)
}

#[syscall(name = "sys_sync")]
fn _sync() -> KResult<isize> {
    fcntl::sync()?;
    Ok(0)
}

#[syscall(name = "sys_pipe")]
fn _pipe(fds: &mut [u32; 2]) -> KResult<isize> {
    fcntl::pipe(fds)?;
    Ok(0)
}

#[syscall(name = "sys_dup2")]
fn _dup2(src_fd: u32, dst_fd: u32) -> KResult<isize> {
    fcntl::dup2(src_fd, dst_fd)?;
    Ok(0)
}

#[syscall(name = "sys_ioctl")]
fn _ioctl(fd: u32, request: u64, argp: usize) -> KResult<isize> {
    use rw_ulib_types::ioctl::Request;

    let req = Request::try_from(request).map_err(|_| SysErrorKind::InvalidArgument)?;
    match req {
        Request::TIOCSPGRP => {
            let tid = Tid::for_query(*sys_arg_ref::<u64>(argp)?);
            ioctl::tcsetpgrp(fd, tid)?;
            Ok(0)
        }

        Request::TIOCGPGRP => Ok(ioctl::tcgetpgrp(fd)?.as_u64() as isize),
    }
}
