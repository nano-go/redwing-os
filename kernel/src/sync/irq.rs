use core::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
};

use crate::proc::cpu::{intr_off_store, intr_restore, pop_off, push_off};

enum CloseIntr {
    PopOff,
    IntrRestore(bool),
}

pub struct IrqGuard<'a, T> {
    close_intr: CloseIntr,
    inner: &'a UnsafeCell<T>,
}

pub fn lock_irq<'a, T>(data: &'a UnsafeCell<T>) -> IrqGuard<'a, T> {
    push_off();
    IrqGuard {
        close_intr: CloseIntr::PopOff,
        inner: data,
    }
}

pub fn lock_irq_save<'a, T>(data: &'a UnsafeCell<T>) -> IrqGuard<'a, T> {
    let flag = intr_off_store();
    IrqGuard {
        close_intr: CloseIntr::IntrRestore(flag),
        inner: data,
    }
}

impl<'a, T> Deref for IrqGuard<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.inner.get() }
    }
}

impl<'a, T> DerefMut for IrqGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.inner.get() }
    }
}

impl<'a, T> Drop for IrqGuard<'a, T> {
    fn drop(&mut self) {
        match self.close_intr {
            CloseIntr::PopOff => pop_off("irq_guard"),
            CloseIntr::IntrRestore(flag) => intr_restore(flag),
        }
    }
}
