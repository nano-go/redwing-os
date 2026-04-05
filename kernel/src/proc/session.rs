use alloc::sync::Arc;
use hashbrown::HashMap;
use lazy_static::lazy_static;
use syserr::SysErrorKind;

use crate::{
    error::KResult,
    sync::spin::{Spinlock, SpinlockGuard},
};

use super::{
    group::TaskGroup,
    id::Tid,
    task::{self, Task, TaskInner, TaskRef},
};

lazy_static! {
    pub static ref SESSIONS: Spinlock<HashMap<Tid, Session>> =
        Spinlock::new("sessions", HashMap::new());
}

pub struct Session {
    leader_tid: Tid,
    groups: Spinlock<HashMap<Tid, Arc<TaskGroup>>>,
}

impl Session {
    #[must_use]
    pub fn new(leader: &TaskRef) -> Self {
        let mut groups = HashMap::new();
        groups.insert(leader.tid, Arc::new(TaskGroup::new(leader)));
        Self {
            leader_tid: leader.tid,
            groups: Spinlock::new("session_group_map", groups),
        }
    }

    #[must_use]
    pub fn leader_tid(&self) -> Tid {
        self.leader_tid
    }

    pub fn leader_task(&self) -> KResult<TaskRef> {
        task::get_task_by_tid(&self.leader_tid)
    }

    #[must_use]
    pub fn get_group(&self, tid: &Tid) -> Option<Arc<TaskGroup>> {
        self.groups.lock().get(tid).cloned()
    }

    pub fn try_add_task(&self, task: &TaskInner) -> bool {
        let groups = self.groups.lock();
        if let Some(group) = groups.get(&task.tgid) {
            group.add_task(&task.self_ref.upgrade().unwrap());
            true
        } else {
            false
        }
    }

    pub fn remove_task(&self, task: &TaskInner, tid: &Tid) -> bool {
        let mut groups = self.groups.lock();
        if let Some(group) = groups.get(&task.tgid) {
            group.remove_task(tid);
            if group.is_emtpy() {
                groups.remove(&task.tgid);
            }
        }
        groups.is_empty()
    }

    pub fn move_task_or_create_grp(
        &self,
        mut task_guard: SpinlockGuard<TaskInner>,
        new_pgid: Tid,
    ) -> KResult<()> {
        let taskref = task_guard.self_ref.upgrade().unwrap();
        let mut groups = self.groups.lock();

        let Some(old_group) = groups.get(&task_guard.tgid) else {
            return Err(SysErrorKind::NoSuchProcess.into());
        };

        let new_group = groups.get(&new_pgid);

        if new_group.is_none() && new_pgid != taskref.tid {
            // The target group does not exists and we couldn't create a new group because
            // the task is not group leader.
            return Err(SysErrorKind::NoSuchProcess.into());
        }

        old_group
            .remove_task(&taskref.tid)
            .ok_or(SysErrorKind::NoSuchProcess)?;

        task_guard.tgid = new_pgid;
        if let Some(new_group) = new_group {
            new_group.add_task(&taskref);
        } else {
            groups.insert(taskref.tid, Arc::new(TaskGroup::new(&taskref)));
        }
        Ok(())
    }
}

pub fn get_group(task: &TaskInner) -> KResult<Arc<TaskGroup>> {
    Ok(SESSIONS
        .lock()
        .get(&task.sid)
        .ok_or(SysErrorKind::NoSuchProcess)?
        .get_group(&task.tgid)
        .ok_or(SysErrorKind::NoSuchProcess)?)
}

pub fn try_add_task(task: &TaskInner) -> bool {
    if let Some(session) = SESSIONS.lock().get(&task.sid) {
        session.try_add_task(task)
    } else {
        false
    }
}

pub fn remove_task(taskref: &Task) {
    let task = taskref.lock_irq_save();
    let mut sessions = SESSIONS.lock();
    if let Some(session) = sessions.get(&task.sid) {
        if session.remove_task(&task, &taskref.tid) {
            sessions.remove(&task.sid);
        }
    }
}

pub fn create_session(taskref: &TaskRef) -> KResult<()> {
    let mut task = taskref.lock_irq_save();
    let mut sessions = SESSIONS.lock();

    if let Some(session) = sessions.get(&task.sid) {
        let group = session.get_group(&task.tgid);
        if let Some(group) = group {
            // We can not create session for the task if it is a group leader.
            if group.leader_tid() == taskref.tid {
                return Err(SysErrorKind::NotPermitted.into());
            }
            group.remove_task(&taskref.tid);
        }
    }

    let new_session = Session::new(taskref);

    if sessions.try_insert(taskref.tid, new_session).is_err() {
        // We can not create session for the task if it is a session leader.
        return Err(SysErrorKind::NotPermitted.into());
    }

    task.tgid = taskref.tid;
    task.sid = taskref.tid;

    Ok(())
}

pub fn set_tgid(taskref: &TaskRef, new_tgid: Tid) -> KResult<()> {
    let calling_task_sid = task::current_task_or_err()?.lock_irq_save().sid;

    let task = taskref.lock_irq_save();

    // The target task is a session leader.
    if task.sid == taskref.tid {
        return Err(SysErrorKind::NotPermitted.into());
    }

    // The session ID of calling task is not same with the target task.
    if calling_task_sid != task.sid {
        return Err(SysErrorKind::NotPermitted.into());
    }

    let tgid = if new_tgid.is_zero() {
        taskref.tid
    } else {
        let new_group_leader = task::get_task_by_tid(&new_tgid)?;
        if new_group_leader.tid != taskref.tid {
            let new_group_leader = new_group_leader.lock_irq_save();
            if new_group_leader.sid != task.sid {
                // We can not move a task between groups where are not present in a same
                // session.
                return Err(SysErrorKind::NotPermitted.into());
            }
        }
        new_tgid
    };

    let sessions = SESSIONS.lock();

    let Some(session) = sessions.get(&task.sid) else {
        return Err(SysErrorKind::NotPermitted.into());
    };

    session.move_task_or_create_grp(task, tgid)
}
