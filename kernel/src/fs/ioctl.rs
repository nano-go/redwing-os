use alloc::sync::Arc;
use rw_ulib_types::fcntl::FileType;
use syserr::SysErrorKind;

use crate::{
    devices::{
        get_device,
        tty::{TtyDevice, TTY_DEV_NO},
    },
    error::KResult,
    proc::{
        id::Tid,
        task::{self},
    },
};

fn get_tty(fd: u32) -> KResult<Arc<TtyDevice>> {
    let file = task::current_task_or_err()?.lock_irq_save().get_file(fd)?;
    let indoe = file.get_inode().ok_or(SysErrorKind::NoTty)?;
    let metadata = indoe.metadata()?;
    if metadata.typ != FileType::Device || metadata.dev_no != TTY_DEV_NO {
        return Err(SysErrorKind::NoTty.into());
    }
    let dev = get_device(TTY_DEV_NO)?;
    Arc::downcast::<TtyDevice>(dev).map_err(|_| SysErrorKind::NoTty.into())
}

pub fn tcsetpgrp(fd: u32, tid: Tid) -> KResult<()> {
    let tty = get_tty(fd)?;
    tty.set_fg_tgid(tid)
}

pub fn tcgetpgrp(fd: u32) -> KResult<Tid> {
    Ok(get_tty(fd)?.get_fg_tgid())
}
