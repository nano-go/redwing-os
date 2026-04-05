use crate::{
    arch::timer::timer_now,
    mmu::buddy::{BuddyAlloc, BuddyBox},
    params::{TASK_KERNEL_STACK_SIZE, TICK_TIME_DUR},
    timer_events::{self, TimerEventId},
};
use core::{
    array,
    fmt::{Display, Write},
    mem::ManuallyDrop,
    time::Duration,
};

use alloc::{
    boxed::Box,
    collections::linked_list::LinkedList,
    sync::{Arc, Weak},
};
use const_default::ConstDefault;
use intrusive_collections::RBTreeAtomicLink;
use lazy_static::lazy_static;
use log::{error, trace};
use redwing_vfs::VfsINodeRef;
use rw_ulib_types::signal::SignalFlags;
use syserr::{sys_err, SysErrorKind};

use crate::{
    arch::ctx::{Context, Trapframe},
    error::KResult,
    fs::file::File,
    mmu::{types::PhysicalPtr, vm::VM},
    params::MAX_TASKS,
    proc::cpu::mycpu,
    sync::spin::{Once, Spinlock, SpinlockGuard},
};

use super::{
    id::Tid,
    sched::{self, cfs::CFSAttrs, sched},
    session,
    signal::{SignalActionKind, DEFAULT_SIGNAL_ACTIONS, MAX_SIG},
};

pub const MAX_OPEN_FILES: usize = 20;
pub const TASK_NAME_LEN: usize = 64;

lazy_static! {
    /// The global task table containing all tasks.
    pub static ref TASKS_TABLE: Spinlock<heapless::FnvIndexMap<u64, TaskRef, MAX_TASKS>> =
        Spinlock::new("task_table", heapless::FnvIndexMap::new());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskState {
    /// An initialized task has not been started.
    Initialized,

    /// A task is running if it occupied the CPU.
    Running,

    /// A task is ready to run.
    Runable,

    /// Blocked until some events finish.
    Blocked,

    /// Blocked until some events finish or receives a signal.
    Interruptible,

    /// Blocked until a child task exits or receives a signal.
    Waitting,

    /// An exited task.
    Zombie,
}

impl Display for TaskState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Zombie => write!(f, "Z (zombie)"),
            Self::Running => write!(f, "R (runnung)"),
            Self::Runable => write!(f, "r (runnable)"),
            Self::Initialized => write!(f, "i (initialized)"),
            Self::Interruptible => write!(f, "I (interruptible)"),
            Self::Waitting => write!(f, "W (waitting)"),
            Self::Blocked => write!(f, "B (blocked)"),
        }
    }
}

pub type TaskRef = Arc<Task>;

pub struct Task {
    pub tid: Tid,

    /// The reference to the self TaskRef.
    pub self_ref: Weak<Task>,

    /// For CFS scheduler.
    pub cfs_rbtree_link: RBTreeAtomicLink,
    pub cfs_attrs: Spinlock<CFSAttrs>,

    pub inner: Spinlock<TaskInner>,
}

pub struct TaskInner {
    /// The reference to the self TaskRef.
    pub self_ref: Weak<Task>,

    pub name: heapless::String<TASK_NAME_LEN>,

    pub tgid: Tid,
    pub sid: Tid,

    pub entry: Option<Box<dyn FnOnce() + Send + 'static>>,

    pub state: TaskState,

    pub vm: VM,
    pub kstack: BuddyBox<[u8; TASK_KERNEL_STACK_SIZE]>,
    pub context: Context,

    pub exit_status: i32,
    pub is_killed: bool,
    pub is_frozen: bool,
    pub signal_mask: SignalFlags,
    pub signals: SignalFlags,
    pub signal_actions: [SignalActionKind; MAX_SIG as usize + 1],
    pub trapframe_backup: Option<Trapframe>,

    pub sleep_timer_event: Option<TimerEventId>,

    pub parent: Option<Weak<Task>>,
    pub children: LinkedList<Weak<Task>>,

    pub cwd: Option<VfsINodeRef>,
    pub o_files: [Option<Arc<File>>; MAX_OPEN_FILES],
}

impl Drop for Task {
    fn drop(&mut self) {
        trace!("the task {}({}) is dropped.", self.tid, &self.lock().name);
        session::remove_task(self);
    }
}

impl Task {
    /// Creates a new task. You can run the task by the [`Task::start`]
    /// function.
    pub fn create() -> KResult<TaskRef> {
        let task = Self::with_name(heapless::String::new())?;
        task.lock()
            .name
            .write_fmt(format_args!("task-{}", task.tid.as_u64()))
            .unwrap();
        Ok(task)
    }

    /// Creates a new task with a specified name.
    pub fn with_name(name: heapless::String<TASK_NAME_LEN>) -> KResult<TaskRef> {
        let vm = VM::with_kernel_vm()?;
        vm.lock().alloc_trapframe(Trapframe::DEFAULT)?;

        let kstack = unsafe {
            // SAFETY: the byte array can be initialized with zero bytes.
            BuddyBox::<[u8; TASK_KERNEL_STACK_SIZE]>::try_new_zeroed_in(BuddyAlloc {})?
                .assume_init()
        };

        let tid = Tid::next_tid();
        Ok(Arc::new_cyclic(|me| Self {
            tid,
            self_ref: me.clone(),
            cfs_rbtree_link: RBTreeAtomicLink::new(),
            cfs_attrs: Spinlock::new("task_cfs_attrs", CFSAttrs::default()),
            inner: Spinlock::new("task", TaskInner::new(me.clone(), name, tid, vm, kstack)),
        }))
    }

    #[must_use]
    pub fn lock(&self) -> SpinlockGuard<TaskInner> {
        self.inner.lock_irq()
    }

    #[must_use]
    pub fn lock_irq_save(&self) -> SpinlockGuard<TaskInner> {
        self.inner.lock_irq_save()
    }

    /// This is a convenience wrapper around [`Task::start_with_parent`] with
    /// the [`current_task`].
    pub fn start<F>(&self, entry: F) -> KResult<()>
    where
        F: FnOnce() + Send + 'static,
    {
        self.start_with_parent(current_task(), entry)
    }

    /// Starts a new task and optionally sets its parent.
    ///
    /// If the parent is present, the task will inherit resources from it:
    /// 1. Open files
    /// 2. Current work directory
    /// 3. The group ID and session ID.
    ///
    /// # Parameters
    ///
    /// - `parent`: An optional reference to the parent task. If provided:
    ///   - The new task inherits file descriptors and session/group
    ///     information.
    ///   - The new task is added to the parent's list of children.
    /// - `entry`: A closure representing the starting point of the task.
    ///
    /// # Returns
    ///
    /// - `Ok(())` if the task is successfully started.
    /// - `Err(SysError)` if the task table is full or any internal operation
    ///   fails.
    ///
    /// # Panics
    ///
    /// Panics if this function is called on a task that has already been
    /// started.
    ///
    /// # Notes
    ///
    /// - Once started, the task will be managed by the scheduler.
    /// - The task must eventually be cleaned up by its parent via `wait`, or it
    ///   may leak memory.
    pub fn start_with_parent<F>(&self, parent: Option<TaskRef>, entry: F) -> KResult<()>
    where
        F: FnOnce() + Send + 'static,
    {
        let mut task = self.lock();
        if task.state != TaskState::Initialized {
            panic!("starts a task multiple times");
        }

        {
            let mut task_table = TASKS_TABLE.lock();
            let self_ref = task.self_ref.upgrade().unwrap();

            // Adds the new task into the global task table.
            // This may causes memory leak. This task should be removed from the table
            // when a parent task collects it by `wait`.
            if task_table.insert(*self.tid, self_ref).is_err() {
                return Err(sys_err!(
                    SysErrorKind::TooManyTasks,
                    "fail to start a task because the global task table is full.",
                ));
            }
        }

        if let Some(parent) = parent {
            let mut parent = parent.lock();
            task.copy_o_files(&parent);
            task.copy_cwd(&parent);
            task.parent = Some(parent.self_ref.clone());
            parent.children.push_back(task.self_ref.clone());
            task.tgid = parent.tgid;
            task.sid = parent.sid;
            session::try_add_task(&task);
        }

        task.entry = Some(Box::new(entry));
        task.wakeup();

        Ok(())
    }

    fn parent(&self) -> Option<TaskRef> {
        self.lock()
            .parent
            .clone()
            .and_then(|parent_weak| parent_weak.upgrade())
    }

    /// If this task has no parent task, reparents the task to the init task.
    ///
    /// Returns the parent of this task.
    fn parent_or_insert_init(&self) -> TaskRef {
        self.parent().unwrap_or_else(|| {
            let init_task_ref = get_init_task();
            self.reparent(&init_task_ref);
            init_task_ref
        })
    }

    /// Changes the parent of the task to the given task.
    ///
    /// Note: this does not remove the task from the children link of the old
    /// parent, so make sure that the old parent is `None` or this task has been
    /// removed from its children link.
    fn reparent(&self, parent: &TaskRef) {
        let mut parent = parent.lock();
        let mut child = self.lock();
        parent.children.push_back(self.self_ref.clone());
        child.parent = Some(parent.self_ref.clone());
    }

    /// Removes all children and reparents theme to the init task. This also
    /// wakes up the init task if any child task is zombie.
    ///
    /// This is useful for clearing children when a task exits.
    fn clear_and_reparent_children(&self) {
        let old_children = {
            let mut task = self.lock();
            core::mem::take(&mut task.children)
        };

        let init_task_ref = get_init_task();
        let contains_zombie = old_children
            .into_iter()
            .filter_map(|child_weak| child_weak.upgrade())
            .fold(false, |contains_zombie: bool, child| {
                child.reparent(&init_task_ref);
                contains_zombie || child.lock().state == TaskState::Zombie
            });

        if contains_zombie {
            init_task_ref.lock().wakeup_if_waiting();
        }
    }
}

impl TaskInner {
    #[must_use]
    fn new(
        self_ref: Weak<Task>,
        name: heapless::String<TASK_NAME_LEN>,
        tid: Tid,
        vm: VM,
        kstack: BuddyBox<[u8; TASK_KERNEL_STACK_SIZE]>,
    ) -> Self {
        Self {
            name,
            self_ref,
            tgid: tid,
            sid: tid,
            state: TaskState::Initialized,
            context: Context::new(
                Self::task_entry as usize as u64,
                (kstack.as_ptr().addr() + kstack.len()) as u64,
            ),
            entry: None,
            parent: None,
            is_killed: false,
            is_frozen: false,
            signal_mask: SignalFlags::empty(),
            signals: SignalFlags::empty(),
            signal_actions: DEFAULT_SIGNAL_ACTIONS,
            trapframe_backup: None,
            kstack,
            vm,
            sleep_timer_event: None,
            exit_status: 0,
            cwd: None,
            o_files: array::from_fn(|_| None),
            children: LinkedList::new(),
        }
    }

    fn task_entry() -> ! {
        let entry = {
            let task = current_task().unwrap();

            // unlock spinlock::lock_irq().
            task.inner.force_unlock();
            // manually make interrupt off.
            super::cpu::pop_off("task");

            let mut task = task.lock_irq_save();
            task.entry.take().unwrap()
        };
        entry();
        exit(0);
    }

    #[inline]
    pub fn set_task_name(&mut self, name: heapless::String<TASK_NAME_LEN>) {
        self.name = name;
    }

    /// A convenience wrapper around `vm.lock().trapframe()`.
    #[inline]
    pub fn trapframe(&self) -> PhysicalPtr<Trapframe> {
        self.vm.lock().trapframe()
    }

    #[inline]
    pub fn copy_o_files(&mut self, another: &Self) {
        for (i, o_file) in another.o_files.iter().enumerate() {
            if let Some(o_file) = o_file {
                self.o_files[i] = Some(Arc::new(o_file.dup()));
            }
        }
    }

    #[inline]
    pub fn copy_cwd(&mut self, another: &Self) {
        self.cwd = another.cwd.clone();
    }

    pub fn wakeup(&mut self) {
        match self.state {
            TaskState::Blocked
            | TaskState::Waitting
            | TaskState::Initialized
            | TaskState::Interruptible => {
                unsafe { self.wakeup_unchecked() };
            }

            _ => error!(
                "wake up the task {} in unexpected state {:?}",
                self.self_ref.upgrade().unwrap().tid,
                self.state
            ),
        }
    }

    #[inline]
    pub fn wakeup_if_interruptible(&mut self) {
        if self.state == TaskState::Waitting || self.state == TaskState::Interruptible {
            if let Some(timer_event_id) = self.sleep_timer_event.take() {
                timer_events::remove_event(timer_event_id);
            }
            unsafe { self.wakeup_unchecked() };
        }
    }

    #[inline]
    pub fn wakeup_if_waiting(&mut self) {
        if self.state == TaskState::Waitting {
            unsafe { self.wakeup_unchecked() };
        }
    }

    /// Wakeup the task.
    ///
    /// # Safety
    ///
    /// Caller should ensure the task is blocked.
    #[inline]
    pub unsafe fn wakeup_unchecked(&mut self) {
        let taskref = self.self_ref.upgrade().unwrap();
        self.state = TaskState::Runable;
        sched::enqueue_ready(taskref);
    }

    /// Suspends the task until it receives a signal.
    #[inline]
    pub fn suspend(&mut self) {
        self.state = TaskState::Interruptible;
        sched(&self.context);
    }

    #[must_use]
    #[inline]
    pub fn is_interrupted_by_signal(&self) -> bool {
        (self.signals | self.signal_mask) != self.signal_mask
    }

    /// Find a zombie child task with matched `tid` and remove it. This is
    /// used by the `wait` function.
    ///
    /// This also clears tasks which has since been dropped(weak reference).
    ///
    /// # Parameters
    ///
    /// - `qtid` - The `None` matches all tids or `Some(tid)` matches specified
    ///   tid.
    /// - `have_kids` - This will modify `have_kids` to `true` if a child task
    ///   is matched with the `qtid` even it is not a zombie task.
    ///
    /// # Returns
    ///
    /// The removed task or `None` if such child does not exist.
    fn remove_zombie_child(&mut self, qtid: Option<Tid>, have_kids: &mut bool) -> Option<TaskRef> {
        let mut cursor = self.children.cursor_front_mut();

        while let Some(childref) = cursor.current() {
            let Some(childref) = childref.upgrade() else {
                // Clear child task which has been dropped.
                cursor.remove_current();
                continue;
            };

            if matches!(qtid, Some(tid) if tid != childref.tid) {
                // Don't remove tasks with unmatched tid.
                cursor.move_next();
                continue;
            }

            // Found a child task with matched tid!.
            *have_kids = true;

            let mut child = childref.lock();
            if child.state != TaskState::Zombie {
                if qtid.is_some() {
                    // The tid is unique.
                    return None;
                }

                cursor.move_next();
                continue;
            }

            // Found a zombie task! Remove it from the link.
            child.parent = None;
            cursor.remove_current();
            drop(child);
            return Some(childref);
        }

        None
    }

    pub fn allocate_fd_for(&mut self, file: File) -> KResult<u32> {
        for (i, o_file) in self.o_files.iter_mut().enumerate() {
            if o_file.is_none() {
                *o_file = Some(Arc::new(file));
                return Ok(i as u32);
            }
        }
        Err(sys_err!(SysErrorKind::TooManyOpenFiles))
    }

    pub fn get_file(&mut self, fd: u32) -> KResult<Arc<File>> {
        if let Some(Some(file)) = self.o_files.get(fd as usize) {
            Ok(file.clone())
        } else {
            Err(SysErrorKind::BadFileDescriptor.into())
        }
    }

    pub fn set_file(&mut self, fd: u32, file: File) -> KResult<Option<Arc<File>>> {
        if let Some(f) = self.o_files.get_mut(fd as usize) {
            Ok(f.replace(Arc::new(file)))
        } else {
            // Index out of bound.
            Err(SysErrorKind::BadFileDescriptor.into())
        }
    }
}

/// The init task reference.
pub static INIT_TASK: Once<TaskRef> = Once::new();

pub fn set_init_task(tid: Tid) {
    INIT_TASK.call_once(|| TASKS_TABLE.lock().get(&tid).unwrap().clone());
}

#[must_use]
#[inline]
pub fn get_init_task() -> TaskRef {
    INIT_TASK.get().unwrap().clone()
}

#[inline]
pub fn get_task_by_tid(tid: &Tid) -> KResult<TaskRef> {
    TASKS_TABLE
        .lock()
        .get(tid)
        .cloned()
        .ok_or(SysErrorKind::NoSuchProcess.into())
}

pub fn spawn<F>(entry: F) -> Tid
where
    F: FnOnce() + Send + 'static,
{
    let task = Task::create().unwrap();
    task.start(entry).unwrap();
    task.tid
}

#[must_use]
#[inline]
pub fn current_task() -> Option<TaskRef> {
    mycpu().current_task.clone()
}

#[inline]
pub fn current_task_or_err() -> KResult<TaskRef> {
    current_task().ok_or_else(|| sys_err!(SysErrorKind::IOError, "the current task is none."))
}

/// Returns a pointer to the trapframe of the current task.
#[must_use]
#[inline]
pub fn current_trapframe() -> PhysicalPtr<Trapframe> {
    current_task().unwrap().lock_irq_save().trapframe()
}

pub fn set_name_for_kernel_task(name: &str) {
    if let Some(taskref) = current_task() {
        let mut task = taskref.lock();
        task.name.clear();
        let _ = task.name.write_str("__");
        let _ = task.name.write_str(name);
    }
}

pub fn yield_now() {
    if let Some(taskref) = current_task() {
        let mut task = taskref.lock();
        task.state = TaskState::Runable;
        sched(&task.context);
    }
}

#[inline]
pub fn sleep(dur: Duration) -> KResult<()> {
    sleep_rem(dur, None)
}

pub fn sleep_rem(dur: Duration, rem: Option<&mut Duration>) -> KResult<()> {
    let taskref = current_task_or_err()?;

    let mut task = taskref.lock();

    if task.is_interrupted_by_signal() {
        return Err(SysErrorKind::Interrupted.into());
    }

    let now = timer_now();

    let taskref_clone = taskref.clone();
    let timer_id = timer_events::add_event(
        // TICK_TIME_DUR, scheduling time, for more precise sleep duration.
        dur.saturating_sub(TICK_TIME_DUR),
        move || {
            let mut task = taskref_clone.lock();
            if task.sleep_timer_event.is_none() {
                return;
            }
            task.sleep_timer_event = None;
            if task.state == TaskState::Interruptible {
                task.wakeup();
            }
        },
    );

    task.sleep_timer_event = Some(timer_id);
    task.state = TaskState::Interruptible;
    sched(&task.context);

    assert_eq!(task.sleep_timer_event, None);

    if task.is_interrupted_by_signal() {
        if let Some(rem) = rem {
            *rem = dur.saturating_sub(timer_now().saturating_sub(now));
        }
        Err(SysErrorKind::Interrupted.into())
    } else {
        Ok(())
    }
}

pub fn exit(status: i32) -> ! {
    let mut taskref = ManuallyDrop::new(current_task().unwrap());

    // Release open files and the current inode.
    let (o_files, cur_inode) = {
        let mut task = taskref.lock();
        (
            core::mem::replace(&mut task.o_files, array::from_fn(|_| None)),
            task.cwd.take(),
        )
    };
    drop(o_files);
    drop(cur_inode);

    taskref.clear_and_reparent_children();
    let parentref = taskref.parent_or_insert_init();

    unsafe {
        // SAFETY: We need to manually drop the task `Arc` because the `drop(taskref)`
        // after`sched` never be executed.
        ManuallyDrop::drop(&mut taskref);
    }

    let mut parent = parentref.lock();
    let mut task = taskref.lock();

    task.exit_status = status;
    task.state = TaskState::Zombie;

    parent.wakeup_if_waiting();

    drop(parent);
    drop(parentref);

    sched(&task.context);

    unreachable!()

    // drop(task);
    // drop(taskref);
}

pub fn wait(tid: Option<Tid>) -> Option<Tid> {
    let mut _status = 0;
    wait_with_xstatus(tid, &mut _status).unwrap()
}

pub fn wait_with_xstatus(tid: Option<Tid>, status: &mut i32) -> KResult<Option<Tid>> {
    let cur_task_ref = current_task_or_err()?;
    loop {
        let mut cur_task = cur_task_ref.lock();

        if cur_task.is_interrupted_by_signal() {
            return Err(SysErrorKind::Interrupted.into());
        }

        let mut have_kids = false;
        if let Some(child) = cur_task.remove_zombie_child(tid, &mut have_kids) {
            drop(cur_task);

            // The child task is ended.

            // Remove it from the global table.
            TASKS_TABLE.lock_irq_save().remove(&child.tid);
            *status = child.lock_irq_save().exit_status;

            trace!(
                "deallocated the task {} by the parent task {}",
                child.tid,
                cur_task_ref.tid
            );

            return Ok(Some(child.tid));
        }

        if !have_kids {
            return Ok(None);
        }

        cur_task.state = TaskState::Waitting;
        sched(&cur_task.context);

        if cur_task.is_interrupted_by_signal() {
            return Err(SysErrorKind::Interrupted.into());
        }
    }
}

pub fn set_sid() -> KResult<()> {
    let taskref = current_task_or_err()?;
    session::create_session(&taskref)
}

pub fn set_tgid(target_tid: &Tid, new_tgid: &Tid) -> KResult<()> {
    let target_task_ref = if target_tid.is_zero() {
        current_task_or_err()?
    } else {
        get_task_by_tid(target_tid)?
    };

    session::set_tgid(&target_task_ref, *new_tgid)
}

pub fn get_tgid(tid: &Tid) -> KResult<Tid> {
    let task = if tid.is_zero() {
        current_task_or_err()?
    } else {
        get_task_by_tid(tid)?
    };

    let task = task.lock();
    Ok(task.tgid)
}

#[cfg(test)]
mod tests {
    use core::{sync::atomic::AtomicUsize, time::Duration};

    use alloc::sync::Arc;

    use rw_ulib_types::signal::Signal;
    use syserr::SysErrorKind;

    use crate::{
        arch::timer::timer_now,
        proc::{
            id::Tid,
            session,
            signal::kill,
            task::{self, get_init_task, get_tgid, set_sid, set_tgid, TASKS_TABLE},
        },
    };

    #[test_case]
    pub fn test_wait_basic() {
        use core::sync::atomic::Ordering;
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = counter.clone();
        let tid = task::spawn(move || {
            counter_clone.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(task::wait(Some(tid)), Some(tid));
        assert!(!TASKS_TABLE.lock_irq().contains_key(&tid));
        assert_eq!(counter.load(Ordering::Relaxed), 1);
    }

    #[test_case]
    pub fn test_wait_unavailable_tid() {
        assert_eq!(task::wait(Some(Tid::next_tid())), None);
        assert_eq!(task::wait(Some(Tid::next_tid())), None);
        assert_eq!(task::wait(Some(get_init_task().tid)), None);
    }

    #[test_case]
    pub fn test_exit() {
        let tid = task::spawn(|| {
            task::exit(4);
        });

        let mut status = 0;
        let wait_tid = task::wait_with_xstatus(Some(tid), &mut status).unwrap();
        assert_eq!(wait_tid, Some(tid));
        assert_eq!(status, 4);
    }

    #[test_case]
    pub fn test_sleep_rem() {
        let tid = task::spawn(|| {
            let mut rem = Duration::default();
            let now = timer_now();
            task::sleep_rem(Duration::from_secs(5), Some(&mut rem)).unwrap_err();
            if (timer_now() - now).abs_diff(Duration::from_secs(5) - rem)
                > Duration::from_millis(100)
            {
                panic!("unexpected rem: {}ms", rem.as_millis());
            }
        });
        task::sleep(Duration::from_millis(300)).unwrap();
        kill(tid.as_u64() as i64, Some(Signal::SIGKILL)).unwrap();
        assert_eq!(task::wait(Some(tid)), Some(tid));
    }

    #[test_case]
    pub fn test_settpid_perm_err() {
        let tid0 = task::spawn(|| {
            set_sid().unwrap();
            // Can not change the task that is the session leader.
            let err = set_tgid(&Tid::zero(), &Tid::zero()).unwrap_err();
            assert_eq!(err.kind, SysErrorKind::NotPermitted.into());
        });

        let tid1 = task::spawn(|| {
            // Can not change the task that is not same session ID with the calling task.
            let err = set_tgid(&get_init_task().tid, &Tid::zero()).unwrap_err();
            assert_eq!(err.kind, SysErrorKind::NotPermitted.into());
        });

        let tid2 = task::spawn(|| {
            set_sid().unwrap();
            let tid = task::spawn(|| {
                // Can not change the group ID of the task to another task group in
                // different session.
                let err = set_tgid(&Tid::zero(), &get_init_task().tid).unwrap_err();
                assert_eq!(err.kind, SysErrorKind::NotPermitted.into());
            });
            assert_eq!(task::wait(Some(tid)), Some(tid));
        });

        assert_eq!(task::wait(Some(tid0)), Some(tid0));
        assert_eq!(task::wait(Some(tid1)), Some(tid1));
        assert_eq!(task::wait(Some(tid2)), Some(tid2));
    }

    #[test_case]
    pub fn test_settpid_basic() {
        let parent = task::current_task().unwrap();
        let tid0 = task::spawn(move || {
            set_tgid(&Tid::zero(), &Tid::zero()).unwrap();
            let tid = task::current_task().unwrap().tid;
            assert_eq!(get_tgid(&Tid::zero()).unwrap(), tid);

            let grp = session::get_group(&parent.lock()).unwrap();
            assert!(!grp.contain_task(&tid));
        });
        assert_eq!(task::wait(Some(tid0)), Some(tid0));
    }
}
