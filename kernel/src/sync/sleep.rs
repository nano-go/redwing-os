use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
};

use crate::proc::task::{self, TaskRef};

use super::wait::WaitQueueLock;

pub struct SleepMutex<T> {
    value: UnsafeCell<T>,
    holder: UnsafeCell<Option<TaskRef>>,
    wait: WaitQueueLock,
}

impl<T> SleepMutex<T> {
    #[must_use]
    pub fn new(value: T) -> Self {
        Self {
            value: UnsafeCell::new(value),
            holder: UnsafeCell::new(None),
            wait: WaitQueueLock::with_name("sleep_mutex"),
        }
    }

    pub fn lock(&self) -> MutexGuard<T> {
        loop {
            let q = self.wait.lock_irq();
            let holder = unsafe { &mut *self.holder.get() };
            if holder.is_none() {
                *holder = task::current_task();
                break;
            }
            q.wait();
        }
        MutexGuard { mutex: self }
    }

    /// Force to release this lock.
    ///
    /// # Safety
    ///
    /// Caller must ensure that the mutex is locked.
    pub unsafe fn force_unlock(&self) {
        let q = self.wait.lock_irq();
        {
            let holder = unsafe { &mut *self.holder.get() };
            if holder.is_some() {
                *holder = None;
            }
        }
        q.signal_all();
    }

    /// Get the inner data.
    ///
    /// # Safety
    ///
    /// Caller must ensure that the access is mutex.
    pub unsafe fn get_unchecked(&self) -> &T {
        &*self.value.get()
    }
}

pub struct MutexGuard<'a, T> {
    mutex: &'a SleepMutex<T>,
}

impl<'a, T> MutexGuard<'a, T> {}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.mutex.value.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.mutex.value.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        unsafe { self.mutex.force_unlock() };
    }
}

unsafe impl<T: Send> Send for SleepMutex<T> {}
unsafe impl<T: Send> Sync for SleepMutex<T> {}

#[cfg(test)]
pub mod tests {

    use alloc::sync::Arc;

    use crate::proc::{
        id::Tid,
        task::{self},
    };

    use super::SleepMutex;

    #[test_case]
    pub fn test_basic() {
        const N_TASKS: usize = 12;
        let counter = Arc::new(SleepMutex::new(0));
        let mut task_tids = [Tid::zero(); N_TASKS];

        for i in 0..N_TASKS {
            let counter_ref = counter.clone();
            task_tids[i] = task::spawn(|| {
                let counter = counter_ref;
                for _ in 0..10 {
                    let mut counter = counter.lock();
                    for _ in 0..10000 {
                        *counter += 1;
                    }
                }
            })
        }

        for tid in task_tids {
            task::wait(Some(tid)).unwrap();
        }

        assert_eq!(*counter.lock(), 100000 * N_TASKS);
    }
}
