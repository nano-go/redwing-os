use alloc::{borrow::Cow, collections::vec_deque::VecDeque, format, sync::Arc};
use log::{error, warn};
use syserr::SysErrorKind;

use crate::{
    error::KResult,
    proc::{
        sched::sched,
        task::{self, TaskRef, TaskState},
    },
};

use super::spin::{Spinlock, SpinlockGuard};

pub struct WaitQueue {
    inner: WaitQueueLock,
}

impl Default for WaitQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitQueue {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            inner: WaitQueueLock::new(),
        }
    }
    #[must_use]
    pub const fn with_name(name: &'static str) -> Self {
        Self {
            inner: WaitQueueLock::with_name(name),
        }
    }

    pub fn wait(&self) {
        self.inner.lock_irq().wait();
    }

    pub fn interruptible_wait(&self) -> KResult<()> {
        self.inner.lock_irq().interruptible_wait()
    }

    pub fn signal(&self) -> bool {
        self.inner.lock_irq().signal()
    }

    pub fn signal_all(&self) -> usize {
        self.inner.lock_irq().signal_all()
    }
}

pub struct WaitQueueLock {
    queue: Spinlock<VecDeque<TaskRef>>,
}

impl Default for WaitQueueLock {
    fn default() -> Self {
        Self::new()
    }
}

impl WaitQueueLock {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            queue: Spinlock::new("wait_queue", VecDeque::new()),
        }
    }

    #[must_use]
    pub const fn with_name(name: &'static str) -> Self {
        Self {
            queue: Spinlock::new(name, VecDeque::new()),
        }
    }

    #[must_use]
    pub fn new_debug<N>(name: N) -> Self
    where
        N: Into<Cow<'static, str>>,
    {
        let name = format!("{}->wq", name.into());
        Self {
            queue: Spinlock::with_cow_name(name, VecDeque::new()),
        }
    }

    pub fn lock_irq(&self) -> WaitQueueGuard {
        WaitQueueGuard {
            guard_queue: self.queue.lock_irq(),
        }
    }

    pub fn lock(&self) -> WaitQueueGuard {
        WaitQueueGuard {
            guard_queue: self.queue.lock(),
        }
    }
}

impl Drop for WaitQueueLock {
    fn drop(&mut self) {
        let guard = self.lock();
        if !guard.guard_queue.is_empty() {
            error!(
                "the wait queue({}) lock is dropped but the wait queue is not empty.",
                self.queue.name()
            );
            guard.signal_all();
        }
    }
}

pub struct WaitQueueGuard<'a> {
    guard_queue: SpinlockGuard<'a, VecDeque<TaskRef>>,
}

impl WaitQueueGuard<'_> {
    pub fn wait(mut self) {
        if let Some(task) = task::current_task() {
            self.guard_queue.push_back(task.clone());
            let mut task = task.lock();
            assert_eq!(task.state, TaskState::Running);
            task.state = TaskState::Blocked;
            drop(self);
            sched(&task.context);
        } else {
            warn!("wait queue(wait): the current task is None.");
        }
    }

    pub fn interruptible_wait(mut self) -> KResult<()> {
        let queue = self.guard_queue.get_raw_spinlock();

        if let Some(taskref) = task::current_task() {
            self.guard_queue.push_back(taskref.clone());
            let mut task = taskref.lock();
            assert_eq!(task.state, TaskState::Running);
            task.state = TaskState::Interruptible;
            drop(self);
            sched(&task.context);

            if task.is_interrupted_by_signal() {
                let mut queue = queue.lock();
                queue.retain(|elem| !Arc::ptr_eq(elem, &taskref));
                Err(SysErrorKind::Interrupted.into())
            } else {
                Ok(())
            }
        } else {
            warn!("wait queue(interruptible_wait): the current task is None.");
            Ok(())
        }
    }

    pub fn signal(mut self) -> bool {
        if let Some(task) = self.guard_queue.pop_front() {
            drop(self);
            Self::wakeup(task);
            true
        } else {
            false
        }
    }

    pub fn signal_all(mut self) -> usize {
        let size = self.guard_queue.len();
        while let Some(task) = self.guard_queue.pop_front() {
            Self::wakeup(task);
        }
        size
    }

    fn wakeup(task: TaskRef) {
        let mut task = task.lock();
        if task.state == TaskState::Zombie {
            return;
        }
        task.wakeup();
    }
}
