use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, AtomicIsize, Ordering},
};

use alloc::borrow::Cow;

use crate::{
    arch::cpu::{cpuid, intr_get},
    proc::cpu::{intr_off_store, intr_restore, pop_off, push_off},
};

#[derive(Debug)]
pub struct Spinlock<T> {
    name: Cow<'static, str>,
    is_locked: AtomicBool,
    cpuid: AtomicIsize,
    data: UnsafeCell<T>,
}

enum CloseIntr {
    Dont,
    PopOff,
    IntrRestore(bool),
}

unsafe impl<T: Send> Send for Spinlock<T> {}
unsafe impl<T: Send> Sync for Spinlock<T> {}

impl<T> Spinlock<T> {
    pub const fn new(name: &'static str, data: T) -> Self {
        Self {
            name: Cow::Borrowed(name),
            is_locked: AtomicBool::new(false),
            cpuid: AtomicIsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn with_cow_name<N>(name: N, data: T) -> Self
    where
        N: Into<Cow<'static, str>>,
    {
        Self {
            name: name.into(),
            is_locked: AtomicBool::new(false),
            cpuid: AtomicIsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn lock(&self) -> SpinlockGuard<T> {
        let flag = intr_get();
        if !flag && self.is_held() {
            log::error!(
                "spinlock {}(lock): nesting lock by the cpu {}.",
                self.name,
                cpuid()
            );
        }
        self._lock(CloseIntr::Dont)
    }

    pub fn lock_irq(&self) -> SpinlockGuard<T> {
        push_off();
        if self.is_held() {
            log::error!(
                "spinlock {}(lock_irq): nesting lock by the cpu {}.",
                self.name,
                cpuid()
            );
            return SpinlockGuard {
                lock: self,
                close_intr: CloseIntr::Dont,
            };
        }
        self._lock(CloseIntr::PopOff)
    }

    pub fn lock_irq_save(&self) -> SpinlockGuard<T> {
        let flag = intr_off_store();
        if self.is_held() {
            log::error!(
                "spinlock {}(lock_irq_save): nesting lock by the cpu {}.",
                self.name,
                cpuid()
            );
            return SpinlockGuard {
                lock: self,
                close_intr: CloseIntr::Dont,
            };
        }
        self._lock(CloseIntr::IntrRestore(flag))
    }

    fn _lock(&self, close_intr: CloseIntr) -> SpinlockGuard<T> {
        while self
            .is_locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }

        self.cpuid.store(cpuid() as isize, Ordering::Release);
        SpinlockGuard {
            lock: self,
            close_intr,
        }
    }

    #[must_use]
    pub fn is_held(&self) -> bool {
        let cur_cpuid = self.cpuid.load(Ordering::Acquire);
        self.is_locked() && cur_cpuid == cpuid() as isize
    }

    #[must_use]
    pub fn is_locked(&self) -> bool {
        self.is_locked.load(Ordering::Acquire)
    }

    /// Force to unlock this spinlock.
    ///
    /// # Panics
    ///
    /// If the spinlock is not locked.
    pub fn force_unlock(&self) {
        if !self.is_locked() {
            panic!("spinlock {}: unlocked.", self.name);
        }
        unsafe { self.force_unlock_unchecked() };
    }

    /// Force to unlock this spinlock.
    ///
    /// # Safety
    ///
    /// Caller must ensure the the spinlock is locked.
    unsafe fn force_unlock_unchecked(&self) {
        self.cpuid.store(-1, Ordering::Release);
        self.is_locked.store(false, Ordering::Release);
    }

    pub fn get(&self) -> &T {
        assert!(self.is_locked());
        unsafe { &*self.data.get() }
    }

    /// Returns the reference to the inner data.
    ///
    /// # Safety
    ///
    /// Caller must ensure the access is mutex.
    #[must_use]
    pub unsafe fn get_unchecked(&self) -> &T {
        &*self.data.get()
    }
}

pub struct SpinlockGuard<'a, T> {
    lock: &'a Spinlock<T>,
    close_intr: CloseIntr,
}

impl<'a, T> SpinlockGuard<'a, T> {
    #[must_use]
    pub fn get_raw_spinlock(&self) -> &'a Spinlock<T> {
        self.lock
    }
}

impl<'a, T> Deref for SpinlockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.data.get() }
    }
}

impl<'a, T> DerefMut for SpinlockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<'a, T> Drop for SpinlockGuard<'a, T> {
    fn drop(&mut self) {
        unsafe { self.lock.force_unlock_unchecked() }
        match self.close_intr {
            CloseIntr::Dont => (),
            CloseIntr::PopOff => pop_off(&self.lock.name),
            CloseIntr::IntrRestore(flag) => intr_restore(flag),
        }
    }
}
