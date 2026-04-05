use alloc::sync::{Arc, Weak};
use hashbrown::HashMap;
use rw_ulib_types::signal::Signal;

use crate::{error::KResult, sync::spin::Spinlock};

use super::{
    id::Tid,
    signal::send_signal,
    task::{self, Task, TaskRef},
};

pub struct TaskGroup {
    leader_tid: Tid,
    tasks: Spinlock<HashMap<Tid, Weak<Task>>>,
}

impl TaskGroup {
    pub fn new(leader: &TaskRef) -> Self {
        let mut tasks = HashMap::new();
        tasks.insert(leader.tid, Arc::downgrade(leader));
        Self {
            leader_tid: leader.tid,
            tasks: Spinlock::new("task_group", tasks),
        }
    }

    #[must_use]
    pub fn leader_tid(&self) -> Tid {
        self.leader_tid
    }

    pub fn leader_task(&self) -> KResult<TaskRef> {
        task::get_task_by_tid(&self.leader_tid)
    }

    pub fn add_task(&self, task: &TaskRef) {
        self.tasks.lock().insert(task.tid, Arc::downgrade(task));
    }

    pub fn remove_task(&self, tid: &Tid) -> Option<Weak<Task>> {
        self.tasks.lock().remove(tid)
    }

    pub fn contain_task(&self, tid: &Tid) -> bool {
        self.tasks.lock().contains_key(tid)
    }

    pub fn is_emtpy(&self) -> bool {
        self.tasks.lock().is_empty()
    }

    pub fn signal(&self, signal: Signal) {
        for task in self.tasks.lock().values() {
            if let Some(task) = task.upgrade() {
                send_signal(&task, signal.to_singal_flags());
            }
        }
    }
}
