use core::time::Duration;

use crate::{
    arch::{
        cpu::{halt, intr_on},
        ctx::{switch, Context},
    },
    mmu::vm,
    proc::cpu::mycpu_mut,
};

use super::{
    cpu::mycpu,
    task::{TaskRef, TaskState},
};

pub mod cfs;
mod rr;

pub trait Scheduler: Send + Sync + 'static {
    fn enqueue_ready(&self, task: TaskRef);
    fn pick_next_task(&self) -> Option<TaskRef>;
    fn tick(&self, jiffies: Duration);
}

/// Switch to scheduler from the task context.
#[inline]
pub fn sched(task_ctx: &Context) {
    unsafe { switch(task_ctx, &mycpu().context) };
}

#[inline]
#[must_use]
pub fn get_current_scheduler() -> &'static dyn Scheduler {
    cfs::CF_SHCEDULER.as_ref()
}

#[inline]
pub fn enqueue_ready(task: TaskRef) {
    get_current_scheduler().enqueue_ready(task);
}

pub fn scheduler() -> ! {
    intr_on();
    let scheduler = get_current_scheduler();
    loop {
        if let Some(task) = scheduler.pick_next_task() {
            let mut tx = task.lock();
            let cpu = unsafe {
                // SAFETY: the intterupt state is disabled by `task.lock()`.
                mycpu_mut()
            };
            assert_eq!(tx.state, TaskState::Runable, "id: {}", task.tid);

            tx.state = TaskState::Running;
            cpu.current_task = Some(task.clone());

            vm::switch_vm(&tx.vm);
            unsafe { switch(&cpu.context, &tx.context) };
            vm::switch_vm_to_kernel();

            if tx.state == TaskState::Runable {
                enqueue_ready(task.clone());
            }

            // The task is done for running now.
            cpu.current_task = None;
            drop(tx);
        } else {
            intr_on();
            halt();
        }
    }
}
