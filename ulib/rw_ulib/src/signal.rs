use core::ptr;

use rw_ulib_types::signal::{ProcMaskHow, Signal, SignalAction, SignalFlags};

use crate::{
    error::{wrap_with_result, Result},
    syscall::api::{sys_sigaction, sys_sigprocmask, sys_sigreturn},
};

pub fn sigprocmask(
    how: ProcMaskHow,
    mask: &SignalFlags,
    old_mask: Option<&mut SignalFlags>,
) -> Result<()> {
    wrap_with_result(unsafe {
        sys_sigprocmask(
            how.into(),
            ptr::addr_of!(*mask).addr(),
            old_mask
                .map(|mask| ptr::addr_of!(*mask))
                .unwrap_or(ptr::null_mut())
                .addr(),
        )
    })?;
    Ok(())
}

pub fn sigaction(
    signal: Signal,
    action: &SignalAction,
    old_action: Option<&mut SignalAction>,
) -> Result<()> {
    wrap_with_result(unsafe {
        sys_sigaction(
            signal as u32,
            ptr::addr_of!(*action).addr(),
            old_action
                .map(|action| ptr::addr_of!(*action))
                .unwrap_or(ptr::null_mut())
                .addr(),
        )
    })?;
    Ok(())
}

pub fn sigreturn() -> Result<()> {
    wrap_with_result(unsafe { sys_sigreturn() })?;
    Ok(())
}
