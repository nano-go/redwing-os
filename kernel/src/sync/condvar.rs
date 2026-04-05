use alloc::{borrow::Cow, format};

use super::{spin::Spinlock, wait::WaitQueueLock};

pub struct BoolCondvar {
    cond: Spinlock<bool>,
    wq: WaitQueueLock,
}

impl BoolCondvar {
    #[must_use]
    pub const fn new(init_cond: bool) -> Self {
        Self {
            cond: Spinlock::new("bool_cond", init_cond),
            wq: WaitQueueLock::new(),
        }
    }

    #[must_use]
    pub fn new_debug<N>(name: N, init_cond: bool) -> Self
    where
        N: Into<Cow<'static, str>>,
    {
        let name = name.into();
        let cond_name = format!("{name}->bool_cond->cond");
        let wq_name = format!("{name}->bool_cond");
        Self {
            cond: Spinlock::with_cow_name(cond_name, init_cond),
            wq: WaitQueueLock::new_debug(wq_name),
        }
    }

    /// Wait until the `condition` is `true`.
    pub fn wait(&self) {
        let mut cond = self.cond.lock_irq();
        while !*cond {
            let wq = self.wq.lock_irq();
            drop(cond);
            wq.wait();
            cond = self.cond.lock_irq();
        }
    }

    /// Change `condition` to `true` and notify all tasks waiting on the lock.
    pub fn notify(&self) {
        let mut cond = self.cond.lock_irq();
        let wq = self.wq.lock_irq();
        *cond = true;
        drop(cond);
        wq.signal_all();
    }

    /// Change cond to `false`.
    pub fn reset(&self) {
        *self.cond.lock() = false;
    }
}
