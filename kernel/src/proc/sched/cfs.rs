use core::time::Duration;

use alloc::boxed::Box;
use intrusive_collections::{intrusive_adapter, KeyAdapter, RBTree, RBTreeAtomicLink};
use lazy_static::lazy_static;

use crate::{
    arch::timer::timer_now,
    proc::task::{self, Task, TaskRef},
    sync::spin::Spinlock,
};

use super::Scheduler;

lazy_static! {
    pub static ref CF_SHCEDULER: Box<CFScheduler> = Box::new(CFScheduler::new());
}

intrusive_adapter!(RBTreeAdapter = TaskRef: Task { cfs_rbtree_link: RBTreeAtomicLink } );

impl<'a> KeyAdapter<'a> for RBTreeAdapter {
    type Key = u64;

    fn get_key(&self, task: &'a Task) -> Self::Key {
        task.cfs_attrs.lock().vruntime
    }
}

#[derive(Default)]
pub struct CFSAttrs {
    pub nice: i32,

    // in nanoseconds
    pub vruntime: u64,

    // in nanoseconds
    pub start_execution_time: u64,

    // in nanoseconds
    pub slice: u64,
}

pub struct CFScheduler {
    inner: Spinlock<CFSchedulerInner>,
}

struct CFSchedulerInner {
    rbtree: RBTree<RBTreeAdapter>,
    min_vruntime: u64,
    total_weight: u64,

    /// How long it should take for all runnable tasks to get a turn once.
    sched_latency: u64,
    sched_min_granularity: u64,
}

impl Default for CFScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl CFScheduler {
    pub fn new() -> Self {
        Self {
            inner: Spinlock::new(
                "cfs",
                CFSchedulerInner {
                    rbtree: RBTree::new(RBTreeAdapter::default()),
                    min_vruntime: 0,
                    total_weight: 0,
                    sched_latency: 20_000_000,        /* 20 ms */
                    sched_min_granularity: 4_000_000, /* 4ms */
                },
            ),
        }
    }
}

impl Scheduler for CFScheduler {
    fn enqueue_ready(&self, task: TaskRef) {
        let mut scheduler = self.inner.lock_irq_save();
        let min_vruntime = if let Some(task) = scheduler.rbtree.front().get() {
            task.cfs_attrs.lock().vruntime
        } else {
            0
        };
        scheduler.min_vruntime = min_vruntime;
        {
            let mut attrs = task.cfs_attrs.lock();
            attrs.vruntime = u64::max(attrs.vruntime, min_vruntime);

            let weight = nice_to_weight(attrs.nice);
            scheduler.total_weight += weight;
        }
        scheduler.rbtree.insert(task);
    }

    fn tick(&self, jiffies: Duration) {
        if let Some(current_task) = task::current_task() {
            let now = jiffies.as_nanos() as u64;
            let do_yield = {
                let mut attrs = current_task.cfs_attrs.lock();
                let delta = now - attrs.start_execution_time;
                let weight = nice_to_weight(attrs.nice);
                attrs.vruntime += delta * 1024 / weight;
                delta >= attrs.slice
            };
            drop(current_task);
            if do_yield {
                task::yield_now();
            }
        }
    }

    fn pick_next_task(&self) -> Option<TaskRef> {
        let mut scheduler = self.inner.lock_irq_save();
        let task = scheduler.rbtree.front_mut().remove();
        if let Some(task) = &task {
            let mut attrs = task.cfs_attrs.lock();
            let weight = nice_to_weight(attrs.nice);

            attrs.start_execution_time = timer_now().as_nanos() as u64;
            attrs.slice = u64::max(
                scheduler.sched_latency * weight / scheduler.total_weight,
                scheduler.sched_min_granularity,
            );

            scheduler.total_weight -= weight;
        }
        task
    }
}

pub fn nice_to_weight(nice: i32) -> u64 {
    // Precomputed weights from Linux kernel (sched/prio.h)
    #[rustfmt::skip]
    const NICE_TO_WEIGHT: [u64; 40] = [
        /* -20 */ 88761, 71755, 56483, 46273, 36291, 
        /* -15 */ 29154, 23254, 18705, 14949, 11916, 
        /* -10 */ 9548, 7620, 6100, 4904, 3906, 
        /* -5  */ 3121, 2501, 1991, 1586, 1277, 
        /*  0  */ 1024, 820, 655, 526, 423, 
        /*  5  */ 335, 272, 215, 172, 137,
        /*  10 */ 110, 87, 70, 56, 45, 
        /*  15 */ 36, 29, 23, 18, 15,
    ];

    let clamped = nice.clamp(-20, 19);
    NICE_TO_WEIGHT[(clamped + 20) as usize]
}
