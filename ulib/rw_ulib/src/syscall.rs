pub const SYSCALL_NO_OPEN: usize = 0;
pub const SYSCALL_NO_CLOSE: usize = 1;
pub const SYSCALL_NO_READ: usize = 2;
pub const SYSCALL_NO_WRITE: usize = 3;
pub const SYSCALL_NO_SEEK: usize = 4;
pub const SYSCALL_NO_RMDIR: usize = 5;
pub const SYSCALL_NO_MKDIR: usize = 6;
pub const SYSCALL_NO_UNLINK: usize = 7;
pub const SYSCALL_NO_GETDIRENTS: usize = 8;
pub const SYSCALL_NO_STAT: usize = 9;
pub const SYSCALL_NO_CD: usize = 10;
pub const SYSCALL_NO_SYNC: usize = 11;
pub const SYSCALL_NO_PIPE: usize = 12;
pub const SYSCALL_NO_DUP2: usize = 13;
pub const SYSCALL_NO_IOCTL: usize = 14;

pub const SYSCALL_NO_EXIT: usize = 30;
pub const SYSCALL_NO_WAIT: usize = 31;
pub const SYSCALL_NO_WAITTID: usize = 32;
pub const SYSCALL_NO_SCHED_YIELD: usize = 33;
pub const SYSCALL_NO_FORK: usize = 34;
pub const SYSCALL_NO_EXECVE: usize = 35;
pub const SYSCALL_NO_SETPGID: usize = 36;
pub const SYSCALL_NO_GETPGID: usize = 37;
pub const SYSCALL_NO_KILL: usize = 38;
pub const SYSCALL_NO_SIGPROCMASK: usize = 39;
pub const SYSCALL_NO_SIGACTION: usize = 40;
pub const SYSCALL_NO_SIGRETURN: usize = 41;
pub const SYSCALL_NO_NANOSLEEP: usize = 42;

pub const SYSCALL_NO_BRK: usize = 50;

#[cfg(feature = "user")]
pub mod api {
    use super::*;
    use alloc::ffi::CString;
    use core::ffi::c_char;

    use rw_ulib_types::{
        fcntl::{Dirent, OpenFlags, SeekFrom, Stat},
        time::Timespec,
    };

    macro_rules! syscall_asm {
        ($syscall_no:expr, $name:ident ( $( $arg_name:ident : $arg_type:ty),*) ) => {
            #[naked]
            pub(crate) unsafe extern "C" fn $name($( $arg_name : $arg_type, )*) -> isize {
                core::arch::naked_asm!(
                    "li a7, {sysca_no}",
                    "ecall",
                    "ret",
                    sysca_no = const $syscall_no)
            }
        };
    }

    syscall_asm!(SYSCALL_NO_OPEN, _sys_open(path_ptr: *const c_char, flags: u32));
    syscall_asm!(SYSCALL_NO_CLOSE, sys_close(fd: u32));
    syscall_asm!(SYSCALL_NO_READ, _sys_read(fd: u32, buf_ptr: usize, buf_len: usize));
    syscall_asm!(SYSCALL_NO_WRITE, _sys_write(fd: u32, buf_ptr: usize, buf_len: usize));
    syscall_asm!(SYSCALL_NO_SEEK, _sys_seek(fd: u32, offset: i64, whence: u32));
    syscall_asm!(SYSCALL_NO_RMDIR, _sys_rmdir(path_ptr: *const c_char));
    syscall_asm!(SYSCALL_NO_MKDIR, _sys_mkdir(path_ptr: *const c_char));
    syscall_asm!(SYSCALL_NO_UNLINK, _sys_unlink(path_ptr: *const c_char));
    syscall_asm!(SYSCALL_NO_GETDIRENTS, _sys_getdirents(fd: u32, dirent_ptr: usize, dirent_len:usize));
    syscall_asm!(SYSCALL_NO_STAT, _sys_stat(path_ptr: *const c_char, stat_ptr: usize));
    syscall_asm!(SYSCALL_NO_CD, _sys_cd(path_ptr: *const c_char));
    syscall_asm!(SYSCALL_NO_SYNC, sys_sync());
    syscall_asm!(SYSCALL_NO_PIPE, _sys_pipe(fds_addr: *const u32));
    syscall_asm!(SYSCALL_NO_DUP2, sys_dup2(src_fd: u32, dst_fd: u32));
    syscall_asm!(SYSCALL_NO_IOCTL, sys_ioctl(fd: u32, request: u64, args: usize));

    syscall_asm!(SYSCALL_NO_EXIT, sys_exit(status: i32));
    syscall_asm!(SYSCALL_NO_WAIT, sys_wait(status_addr: usize));
    syscall_asm!(SYSCALL_NO_WAITTID, sys_waittid(tid: usize, status_addr: usize));
    syscall_asm!(SYSCALL_NO_SCHED_YIELD, sys_sched_yield());
    syscall_asm!(SYSCALL_NO_FORK, sys_fork());
    syscall_asm!(SYSCALL_NO_EXECVE, _sys_execve(
        path_ptr: *const c_char,
        args_base_addr: usize,
        args_len: usize,
        env_vars_base_addr: usize,
        env_vars_len: usize
    ));
    syscall_asm!(SYSCALL_NO_KILL, sys_kill(pid: i64, signal: u32));
    syscall_asm!(SYSCALL_NO_SETPGID, sys_setpgid(pid: i64, pgid: i64));
    syscall_asm!(SYSCALL_NO_GETPGID, sys_getpgid(pid: i64));
    syscall_asm!(SYSCALL_NO_SIGPROCMASK, sys_sigprocmask(how: u32, mask: usize, old_mask: usize));
    syscall_asm!(SYSCALL_NO_SIGACTION, sys_sigaction(signal: u32, act: usize, old_act: usize));
    syscall_asm!(SYSCALL_NO_SIGRETURN, sys_sigreturn());
    syscall_asm!(SYSCALL_NO_SIGRETURN, _sys_nanosleep(req_ptr: usize, rem: usize));

    syscall_asm!(SYSCALL_NO_BRK, sys_brk(ptr: usize));

    pub fn sys_open(pathname: &CString, flags: OpenFlags) -> isize {
        unsafe { _sys_open(pathname.as_ptr(), flags.bits()) }
    }

    pub fn sys_read(fd: u32, buf: &mut [u8]) -> isize {
        unsafe { _sys_read(fd, buf.as_ptr().addr(), buf.len()) }
    }

    pub fn sys_write(fd: u32, buf: &[u8]) -> isize {
        unsafe { _sys_write(fd, buf.as_ptr().addr(), buf.len()) }
    }

    pub fn sys_seek(fd: u32, offset: i64, whence: SeekFrom) -> isize {
        unsafe { _sys_seek(fd, offset, u32::from(whence)) }
    }

    pub fn sys_rmdir(pathname: &CString) -> isize {
        unsafe { _sys_rmdir(pathname.as_ptr()) }
    }

    pub fn sys_mkdir(pathname: &CString) -> isize {
        unsafe { _sys_mkdir(pathname.as_ptr()) }
    }

    pub fn sys_unlink(pathname: &CString) -> isize {
        unsafe { _sys_unlink(pathname.as_ptr()) }
    }

    pub fn sys_getdirents(fd: u32, dirents: &mut [Dirent]) -> isize {
        unsafe { _sys_getdirents(fd, dirents.as_ptr().addr(), dirents.len()) }
    }

    pub fn sys_stat(pathname: &CString, stat: &mut Stat) -> isize {
        unsafe { _sys_stat(pathname.as_ptr(), core::ptr::addr_of!(*stat).addr()) }
    }

    pub fn sys_cd(pathname: &CString) -> isize {
        unsafe { _sys_cd(pathname.as_ptr()) }
    }

    pub fn sys_pipe(fds: &mut [u32; 2]) -> isize {
        unsafe { _sys_pipe(fds.as_ptr()) }
    }

    pub fn sys_execve(
        pathname: &CString,
        args: &[*const c_char],
        env_vars: &[*const c_char],
    ) -> isize {
        unsafe {
            _sys_execve(
                pathname.as_ptr(),
                args.as_ptr().addr(),
                args.len(),
                env_vars.as_ptr().addr(),
                env_vars.len(),
            )
        }
    }

    pub fn sys_nanosleep(req: &Timespec, rem: Option<&mut Timespec>) -> isize {
        unsafe {
            _sys_nanosleep(
                core::ptr::addr_of!(*req).addr(),
                if let Some(rem) = rem {
                    core::ptr::addr_of!(rem).addr()
                } else {
                    0
                },
            )
        }
    }
}
