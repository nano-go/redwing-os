use core::sync::atomic::{AtomicBool, Ordering};

use alloc::{collections::vec_deque::VecDeque, sync::Arc};

use crate::{
    error::{KResult, SysErrorKind},
    sync::{spin::Spinlock, wait::WaitQueueLock},
    utils::vectool::{self},
};

pub const PIPE_BUFFER_LEN: usize = 4096;

#[derive(Clone)]
pub struct SharedPipe {
    pipe_end: Arc<PipeRwEnd>,
}

impl SharedPipe {
    pub fn create() -> KResult<(Self, Self)> {
        Self::with_buffer_size(PIPE_BUFFER_LEN)
    }

    pub fn with_buffer_size(size: usize) -> KResult<(Self, Self)> {
        let buffer = VecDeque::try_with_capacity(size).map_err(|_| SysErrorKind::OutOfMemory)?;
        let buffer = Arc::try_new(Spinlock::new("pipe", buffer))?;
        let state = Arc::new(PipeState::default());

        let read_end_pipe = Arc::try_new(PipeRwEnd {
            buffer: buffer.clone(),
            state: state.clone(),
            is_read_end: true,
        })?;

        let write_end_pipe = Arc::try_new(PipeRwEnd {
            buffer,
            state,
            is_read_end: false,
        })?;

        Ok((
            Self {
                pipe_end: read_end_pipe,
            },
            Self {
                pipe_end: write_end_pipe,
            },
        ))
    }

    pub fn read(&self, mut buf: &mut [u8]) -> KResult<usize> {
        let pipe = &self.pipe_end;
        if !pipe.is_read_end {
            // We can not read from the write-end pipe.
            return Err(SysErrorKind::Pipe.into());
        }

        let mut buffer = pipe.buffer.lock_irq();
        let mut len = 0;
        loop {
            let bytes_read = vectool::drain_vecdeque_to_slice(&mut buffer, buf);

            // Wake up tasks waiting on the write.
            if bytes_read != 0 {
                pipe.state.w_wq.lock().signal_all();
            }

            len += bytes_read;
            if bytes_read == buf.len() || pipe.state.is_closed() {
                break;
            }
            buf = &mut buf[bytes_read..];

            let wq = pipe.state.r_wq.lock();
            if pipe.state.is_closed() {
                // The pipe may be closed during r_wq.lock().
                // See drop() and close()
                break;
            }
            drop(buffer);
            // Blocks until a task writes data to the pipe or the pipe is closed.
            wq.interruptible_wait()?;
            buffer = pipe.buffer.lock_irq();
        }
        Ok(len)
    }

    pub fn write(&self, mut buf: &[u8]) -> KResult<usize> {
        let pipe = &self.pipe_end;
        if pipe.is_read_end {
            // We can not writes to the read-end pipe.
            return Err(SysErrorKind::Pipe.into());
        }

        let mut buffer = pipe.buffer.lock_irq();
        let mut len = 0;
        loop {
            let bytes_write = buf.len().min(buffer.capacity() - buffer.len());
            for byte in &buf[..bytes_write] {
                buffer.push_back(*byte);
            }

            // Wake up tasks waiting on the read.
            if bytes_write != 0 {
                pipe.state.r_wq.lock().signal_all();
            }

            len += bytes_write;
            if bytes_write == buf.len() || pipe.state.is_closed() {
                break;
            }
            buf = &buf[bytes_write..];

            let wq = pipe.state.w_wq.lock();
            if pipe.state.is_closed() {
                // The pipe may be closed during w_wq.lock().
                // See drop() and close()
                break;
            }
            drop(buffer);
            // Blocks until a task reads data from the pipe or the pipe is closed.
            wq.interruptible_wait()?;
            buffer = pipe.buffer.lock_irq();
        }
        Ok(len)
    }
}

struct PipeRwEnd {
    buffer: Arc<Spinlock<VecDeque<u8>>>,
    state: Arc<PipeState>,
    is_read_end: bool,
}

impl Drop for PipeRwEnd {
    fn drop(&mut self) {
        let name = if self.is_read_end {
            "read end"
        } else {
            "write end"
        };
        log::trace!(target: "pipe", "pipe: the {name} pipe is closed.");
        // The pipe should be closed when no one references to any of write and read end
        // pipe
        self.state.close();
    }
}

#[derive(Default)]
struct PipeState {
    is_closed: AtomicBool,
    r_wq: WaitQueueLock,
    w_wq: WaitQueueLock,
}

impl PipeState {
    #[must_use]
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.is_closed.load(Ordering::Relaxed)
    }

    pub fn close(&self) {
        if self
            .is_closed
            .compare_exchange(false, true, Ordering::Release, Ordering::Acquire)
            .is_ok()
        {
            self.r_wq.lock_irq().signal_all();
            self.w_wq.lock_irq().signal_all();
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, sync::Arc};

    use crate::{proc::task, sync::spin::Spinlock};

    use super::SharedPipe;

    #[test_case]
    pub fn test_rw_with_tiny_buffer_size() {
        let (r_pipe, w_pipe) = SharedPipe::with_buffer_size(4).unwrap();
        let tid0 = {
            task::spawn(move || {
                w_pipe.write(b"hello world\n").unwrap();
                w_pipe.write(b"hello rust").unwrap();
            })
        };

        let buf = Arc::new(Spinlock::new("buffer_for_test", String::new()));
        let tid1 = {
            let pipe = r_pipe.clone();
            let str = buf.clone();
            task::spawn(move || {
                let mut buf = [0; 5];
                let mut len;
                while {
                    len = pipe.read(&mut buf).unwrap();
                    len != 0
                } {
                    str.lock()
                        .extend(str::from_utf8(&buf[..len]).unwrap().chars());
                }
            })
        };

        task::wait(Some(tid0));
        task::wait(Some(tid1));

        assert_eq!(*buf.lock(), "hello world\nhello rust");
    }

    #[test_case]
    pub fn test_close() {
        let (r_pipe, w_pipe) = SharedPipe::with_buffer_size(10).unwrap();
        let test_r_pipe = r_pipe.clone();

        let tid0 = {
            task::spawn(move || {
                w_pipe.write(b"hello world").unwrap();
            })
        };

        let buf = Arc::new(Spinlock::new("buffer_for_test", String::new()));
        let tid1 = {
            let r_pipe = r_pipe.clone();
            let str = buf.clone();
            task::spawn(move || {
                let mut buf = [0; 11];
                let len = r_pipe.read(&mut buf).unwrap();
                assert_eq!(len, 11);
                str.lock()
                    .extend(str::from_utf8(&buf[..len]).unwrap().chars());

                let len = r_pipe.read(&mut buf).unwrap();
                assert_eq!(len, 0);
            })
        };

        task::wait(Some(tid0));
        task::wait(Some(tid1));

        assert_eq!(test_r_pipe.read(&mut [0; 4]).unwrap(), 0);
        assert_eq!(*buf.lock(), "hello world");
    }
}
