use syscall_macro::syscall;

use crate::{error::KResult, proc::task};

#[syscall(name = "sys_brk")]
fn _sys_brk(new_brk_ptr: usize) -> KResult<isize> {
    task::current_task_or_err()?
        .lock()
        .vm
        .lock()
        .brk(new_brk_ptr)
        .map(|addr| addr as isize)
}
