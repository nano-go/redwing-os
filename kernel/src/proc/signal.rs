use rw_ulib_types::signal::{ProcMaskHow, Signal, SignalAction, SignalFlags};
use syserr::SysErrorKind;

use crate::error::KResult;

use super::{
    id::Tid,
    session,
    task::{self, TaskRef},
};

pub const MAX_SIG: u64 = 31;

#[derive(Clone, Copy)]
pub enum SignalActionKind {
    Terminate,
    Stop,
    Continue,
    Ignored,
    UserHandler(SignalAction),
}

pub const DEFAULT_SIGNAL_ACTIONS: [SignalActionKind; MAX_SIG as usize + 1] = {
    let mut actions = [SignalActionKind::Terminate; MAX_SIG as usize + 1];
    actions[Signal::SIGDEF as usize] = SignalActionKind::Ignored;
    actions[Signal::SIGCONT as usize] = SignalActionKind::Continue;
    actions[Signal::SIGSTOP as usize] = SignalActionKind::Stop;
    actions[Signal::SIGTTIN as usize] = SignalActionKind::Stop;
    actions
};

pub fn sigprocmask(
    how: ProcMaskHow,
    mask: &SignalFlags,
    old_mask: Option<&mut SignalFlags>,
) -> KResult<()> {
    let task = task::current_task_or_err()?;
    {
        let mut task = task.lock();
        match how {
            ProcMaskHow::BLOCKED => Err(SysErrorKind::Unsupported.into()),
            ProcMaskHow::UNBLOCKED => Err(SysErrorKind::Unsupported.into()),
            ProcMaskHow::SETMASK => {
                if let Some(old_mask) = old_mask {
                    *old_mask = task.signal_mask;
                }
                task.signal_mask = *mask;
                Ok(())
            }
        }
    }
}

pub fn kill(tid: i64, signal: Option<Signal>) -> KResult<()> {
    let calling_task = task::current_task_or_err()?;

    match tid {
        0 => kill_group(&calling_task, signal),

        -1 => Err(SysErrorKind::Unsupported.into()),

        tid if tid < -1 => {
            let tgid = Tid::for_query((-tid) as u64);

            let grp_task = task::get_task_by_tid(&tgid)?;

            let calling_task_sid = calling_task.lock_irq_save().sid;
            let grp_task_sid = grp_task.lock_irq_save().sid;
            if calling_task_sid != grp_task_sid {
                return Err(SysErrorKind::NotPermitted.into());
            }

            kill_group(&grp_task, signal)
        }

        tid => {
            let tid = Tid::for_query(tid as u64);

            let target_task = task::get_task_by_tid(&tid)?;

            let calling_task_sid = calling_task.lock_irq_save().sid;
            let target_task_sid = target_task.lock_irq_save().sid;
            if target_task_sid != calling_task_sid {
                return Err(SysErrorKind::NotPermitted.into());
            }

            if let Some(signal) = signal {
                send_signal(&target_task, signal.to_singal_flags());
            }
            Ok(())
        }
    }
}

pub fn kill_group(task: &TaskRef, signal: Option<Signal>) -> KResult<()> {
    let group = session::get_group(&task.lock())?;
    if let Some(signal) = signal {
        group.signal(signal);
    }
    Ok(())
}

pub fn send_signal_to_current(signal: SignalFlags) -> KResult<()> {
    let task = task::current_task_or_err()?;
    send_signal(&task, signal);
    Ok(())
}

pub fn send_signal(task: &TaskRef, signal: SignalFlags) {
    if signal == SignalFlags::empty() {
        return;
    }
    let mut task = task.lock();
    task.signals |= signal;
    if task.signal_mask.contains(signal) {
        return;
    }
    task.wakeup_if_interruptible();
}

pub fn sigaction(
    signum: Signal,
    act: &SignalAction,
    old_act: Option<&mut SignalAction>,
) -> KResult<()> {
    if signum == Signal::SIGKILL || signum == Signal::SIGSTOP {
        return Err(SysErrorKind::NotPermitted.into());
    }

    let task = task::current_task_or_err()?;
    let mut task = task.lock_irq_save();
    if let SignalActionKind::UserHandler(act) = task.signal_actions[signum as usize] {
        if let Some(old_act) = old_act {
            *old_act = act;
        }
    }

    task.signal_actions[signum as usize] = SignalActionKind::UserHandler(*act);
    Ok(())
}

pub fn handle_signals() -> KResult<()> {
    let taskref = task::current_task_or_err()?;

    loop {
        handle_pendding_signals(&taskref);
        let mut task = taskref.lock();

        if task.is_killed || !task.is_frozen {
            break;
        }

        task.suspend();
    }

    Ok(())
}

fn handle_pendding_signals(taskref: &TaskRef) {
    for signal in 0..MAX_SIG + 1 {
        let task = taskref.lock();

        let Some(flag) = SignalFlags::from_bits(1 << signal) else {
            continue;
        };

        if !task.signals.contains(flag) || task.signal_mask.contains(flag) {
            continue;
        }

        let signal = unsafe { Signal::try_from(signal).unwrap_unchecked() };
        let action = task.signal_actions[signal as usize];

        if matches!(action, SignalActionKind::UserHandler(_)) && task.trapframe_backup.is_some() {
            continue;
        }

        drop(task);
        handle_action(taskref, signal, action);
    }
}

pub fn handle_action(task: &TaskRef, signal: Signal, action: SignalActionKind) {
    let mut task = task.lock();
    let signal_bit = signal.to_singal_flags();

    match action {
        SignalActionKind::Terminate => {
            task.is_killed = true;
            task.exit_status = 128 + signal as i32;
        }

        SignalActionKind::Stop => {
            task.is_frozen = true;
        }

        SignalActionKind::Continue => {
            task.is_frozen = false;
        }

        SignalActionKind::Ignored => {}

        SignalActionKind::UserHandler(action) => {
            if action.mask.contains(signal_bit) {
                return;
            }
            let mut tf = task.trapframe();
            task.trapframe_backup = Some(*tf);
            tf.set_ret_pc(action.sig_handler as usize as u64);
            tf.set_arg0(signal as u64);
        }
    }

    task.signals.remove(signal_bit);
}

pub fn sigreturn() -> KResult<()> {
    let taskref = task::current_task_or_err()?;
    let mut task = taskref.lock();
    if let Some(tf) = task.trapframe_backup.take() {
        *task.trapframe() = tf;
        Ok(())
    } else {
        Err(SysErrorKind::NotPermitted.into())
    }
}

#[cfg(test)]
mod tests {
    use core::time::Duration;

    use alloc::sync::Arc;
    use rw_ulib_types::signal::{Signal, SignalAction, SignalFlags};
    use syserr::SysErrorKind;

    use crate::{
        proc::{
            id::Tid,
            signal::{kill, sigaction, sigprocmask},
            task,
        },
        sync::condvar::BoolCondvar,
    };

    #[test_case]
    pub fn test_kill_wait() {
        let tid = task::spawn(|| loop {
            let mut _status = 0;
            let result = task::wait_with_xstatus(None, &mut _status);
            if let Err(error) = result {
                assert_eq!(error.kind, SysErrorKind::Interrupted);
                break;
            }
        });
        kill(tid.as_u64() as i64, Some(Signal::SIGKILL)).unwrap();
        assert_eq!(task::wait(Some(tid)), Some(tid));
    }

    #[test_case]
    pub fn test_kill_sleep() {
        let tid = task::spawn(|| {
            let error = task::sleep(Duration::from_secs(3)).unwrap_err();
            assert_eq!(error.kind, SysErrorKind::Interrupted);
        });
        kill(tid.as_u64() as i64, Some(Signal::SIGKILL)).unwrap();
        assert_eq!(task::wait(Some(tid)), Some(tid));
    }

    #[test_case]
    pub fn test_kill0() {
        let tid = task::spawn(|| {
            task::set_sid().unwrap();
            let tid0 = task::spawn(|| {
                let error = task::sleep(Duration::from_secs(10)).unwrap_err();
                assert_eq!(error.kind, SysErrorKind::Interrupted);
            });

            let tid1 = task::spawn(|| {
                let error = task::sleep(Duration::from_secs(10)).unwrap_err();
                assert_eq!(error.kind, SysErrorKind::Interrupted);
            });

            sigprocmask(
                rw_ulib_types::signal::ProcMaskHow::SETMASK,
                &SignalFlags::SIGKILL,
                None,
            )
            .unwrap();

            kill(0, Some(Signal::SIGKILL)).unwrap();

            task::wait(Some(tid0)).unwrap();
            task::wait(Some(tid1)).unwrap();
        });

        assert_eq!(task::wait(Some(tid)), Some(tid));
    }

    #[test_case]
    pub fn test_kill_specified_grp() {
        let cond = Arc::new(BoolCondvar::new_debug("test_kill_specified_grp", false));

        let cond_cloned = cond.clone();
        let tid = task::spawn(move || {
            sigprocmask(
                rw_ulib_types::signal::ProcMaskHow::SETMASK,
                &SignalFlags::SIGKILL,
                None,
            )
            .unwrap();

            task::set_tgid(&Tid::zero(), &Tid::zero()).unwrap();

            let tid0 = task::spawn(|| {
                let error = task::sleep(Duration::from_secs(10)).unwrap_err();
                assert_eq!(error.kind, SysErrorKind::Interrupted);
            });

            let tid1 = task::spawn(|| {
                let error = task::sleep(Duration::from_secs(10)).unwrap_err();
                assert_eq!(error.kind, SysErrorKind::Interrupted);
            });

            cond_cloned.notify();

            task::wait(Some(tid0)).unwrap();
            task::wait(Some(tid1)).unwrap();
        });

        cond.wait();
        kill(-(tid.as_u64() as i64), Some(Signal::SIGKILL)).unwrap();

        assert_eq!(task::wait(Some(tid)), Some(tid));
    }

    #[test_case]
    pub fn test_kill_init() {
        let error = kill(
            task::get_init_task().tid.as_u64() as i64,
            Some(Signal::SIGKILL),
        )
        .unwrap_err();
        assert_eq!(error.kind, SysErrorKind::NotPermitted);
    }

    #[test_case]
    pub fn test_sigaction_perm() {
        fn sig_handler(_sig: u32) {}
        let action = SignalAction {
            sig_handler,
            mask: SignalFlags::empty(),
        };

        assert_eq!(
            sigaction(Signal::SIGSTOP, &action, None).unwrap_err().kind,
            SysErrorKind::NotPermitted
        );
        assert_eq!(
            sigaction(Signal::SIGKILL, &action, None).unwrap_err().kind,
            SysErrorKind::NotPermitted
        );
    }
}
