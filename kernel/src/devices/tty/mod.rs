use core::cmp;

use alloc::{sync::Arc, vec::Vec};
use discipline::LineDiscipline;
use rw_ulib_types::signal::{Signal, SignalFlags};
use syserr::SysErrorKind;

use crate::{
    devices::{
        terminal::{InputReceiver, TextScreen},
        Device,
    },
    drivers::uart::{self, UartTextScreen},
    error::KResult,
    proc::{
        id::Tid,
        signal::{kill_group, send_signal},
        task,
    },
    sync::{
        spin::{Once, Spinlock},
        wait::WaitQueue,
    },
    utils::string::format_on_stack,
};

use super::dev_register;

pub mod discipline;

pub const TTY_DEV_NO: u32 = 1;

pub static TTY: Once<Arc<TtyDevice>> = Once::new();

pub fn init() {
    let tty = TTY
        .get_or_init(|| Arc::new(TtyDevice::new(Arc::new(UartTextScreen {}))))
        .clone();
    uart::register_receiver(Arc::downgrade(&tty) as _);
    #[cfg(not(test))]
    tty.text_screen.clear_screen().unwrap();
    dev_register(tty as Arc<dyn Device>);
}

pub struct TtyDevice {
    pub current_tgid: Spinlock<Tid>,
    pub current_sid: Spinlock<Option<Tid>>,
    pub current_read_buf: Spinlock<Vec<u8>>,
    pub line_discipline: Spinlock<LineDiscipline>,
    pub text_screen: Arc<dyn TextScreen>,
    pub wq: WaitQueue,
}

impl TtyDevice {
    #[must_use]
    pub fn new(text_screen: Arc<dyn TextScreen>) -> Self {
        Self {
            current_tgid: Spinlock::new("tty_tgid", Tid::zero()),
            current_sid: Spinlock::new("tty_sid", None),
            current_read_buf: Spinlock::new("tty_current_read_buf", Vec::new()),
            line_discipline: Spinlock::new(
                "line_discipline",
                LineDiscipline::new(text_screen.clone()),
            ),
            text_screen,
            wq: WaitQueue::with_name("tty_wq"),
        }
    }

    #[must_use]
    pub fn get_fg_tgid(&self) -> Tid {
        *self.current_tgid.lock()
    }

    pub fn set_fg_tgid(&self, tgid: Tid) -> KResult<()> {
        let taskref = if tgid.is_zero() {
            task::current_task_or_err()?
        } else {
            task::get_task_by_tid(&tgid)?
        };

        let mut current_tgid = self.current_tgid.lock();
        let mut current_sid = self.current_sid.lock();

        {
            // Calling process must be the leader of the own session.
            if let Some(current_sid) = *current_sid {
                let calling_task = task::current_task_or_err()?;
                let task = calling_task.lock_irq_save();
                if task.sid != current_sid || task.sid != calling_task.tid {
                    return Err(SysErrorKind::NotPermitted.into());
                }
            }
        }

        {
            let task = taskref.lock_irq_save();
            if !current_tgid.is_zero() {
                // The source task group must be in the session.
                if matches!(*current_sid, Some(sid) if sid != task.sid) {
                    return Err(SysErrorKind::NotPermitted.into());
                }
            } else {
                // Binds the session.
                *current_sid = Some(task.sid);
            }

            *current_tgid = taskref.tid;
        }

        send_signal(&taskref, SignalFlags::SIGCONT);
        Ok(())
    }

    fn try_send_signal(&self, input_byte: u8) -> bool {
        let fg_tid = *self.current_tgid.lock();

        let Ok(task) = task::get_task_by_tid(&fg_tid) else {
            return false;
        };

        match input_byte {
            // Ctrl-C
            0x03 => {
                let _ = kill_group(&task, Some(Signal::SIGINT));
                true
            }

            // Ctrl-\
            0x1C => {
                let _ = kill_group(&task, Some(Signal::SIGQUIT));
                true
            }

            _ => false,
        }
    }
}

impl InputReceiver for TtyDevice {
    fn receive_input(&self, buf: &[u8]) {
        let mut line = self.line_discipline.lock_irq_save();
        let mut new_line = false;

        for byte in buf {
            if self.try_send_signal(*byte) {
                continue;
            }

            if line.input_byte(*byte) {
                new_line = true;
            }
        }

        if new_line {
            self.wq.signal_all();
        }
    }
}

impl Device for TtyDevice {
    fn info(&self) -> super::DeviceInfo {
        super::DeviceInfo {
            device_no: TTY_DEV_NO,
            name: "tty",
            file_name: "tty",
        }
    }

    fn dev_read(&self, _offset: u64, buf: &mut [u8]) -> KResult<u64> {
        loop {
            {
                let cur_taskref = task::current_task_or_err()?;

                // Checks whether the current task is the leader of terminal control group.
                if cur_taskref.tid != *self.current_tgid.lock() {
                    // The message usually printed by shell, but we have not implemeneted
                    // stuff about it.
                    let _ = self.text_screen.write(
                        format_on_stack!(
                            128,
                            "+ {} suspended (tty input) {}",
                            cur_taskref.tid,
                            &cur_taskref.lock().name,
                        )
                        .as_bytes(),
                    );

                    send_signal(&cur_taskref, SignalFlags::SIGTTIN);
                    return Err(SysErrorKind::Interrupted.into());
                }
            }

            {
                let mut buffer = self.current_read_buf.lock_irq_save();

                if buffer.is_empty() {
                    let mut line_discipline = self.line_discipline.lock();
                    if let Some(line) = line_discipline.completed_line() {
                        if line.len() == 1 && line[0] == 0 {
                            // read eof.
                            return Ok(0);
                        }
                        buffer.extend(line);
                    }
                }

                if !buffer.is_empty() {
                    let bytes_read = cmp::min(buffer.len(), buf.len());
                    buf[..bytes_read].copy_from_slice(&buffer[..bytes_read]);
                    buffer.drain(..bytes_read);
                    return Ok(bytes_read as u64);
                }
            }

            self.wq.interruptible_wait()?;
        }
    }

    fn dev_write(&self, _offset: u64, buf: &[u8]) -> KResult<u64> {
        self.text_screen.write(buf).map(|len| len as u64)
    }
}
