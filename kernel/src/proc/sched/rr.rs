use core::time::Duration;

use alloc::{boxed::Box, collections::vec_deque::VecDeque};
use lazy_static::lazy_static;

use crate::{
    proc::task::{self, TaskRef},
    sync::spin::Spinlock,
};

use super::Scheduler;

lazy_static! {
    pub static ref RR_SHCEDULER: Box<RRScheduler> = Box::new(RRScheduler::new());
}

pub struct RRScheduler {
    rq: Spinlock<VecDeque<TaskRef>>,
}

impl RRScheduler {
    pub fn new() -> Self {
        Self {
            rq: Spinlock::new("fifo", VecDeque::new()),
        }
    }
}

impl Scheduler for RRScheduler {
    fn enqueue_ready(&self, task: TaskRef) {
        self.rq.lock_irq_save().push_back(task);
    }

    fn tick(&self, _jiffies: Duration) {
        // Assume every task has 1 timeslice.
        task::yield_now();
    }

    fn pick_next_task(&self) -> Option<TaskRef> {
        self.rq.lock_irq_save().pop_front()
    }
}
