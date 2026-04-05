use core::{array, cell::UnsafeCell};

use log::error;

use crate::{
    arch::cpu::{cpuid, intr_get},
    params::MAX_NCPU,
    sync::irq::{self, IrqGuard},
};

/// A CPU-local storage abstraction for managing per-core data.
///
/// `PerCpu<T>` holds a separate instance of `T` for each CPU and provides
/// safe access to the local CPU's instance. This is especially useful in kernel
/// development to avoid locking and contention when accessing CPU-specific
/// data.
///
/// Internally, the data is stored in a fixed-size array indexed by the CPU ID,
/// and access is validated to ensure the current CPU is only accessing its own
/// entry.
///
/// # Examples
///
/// ```rust
/// static PC_COUNTER: PerCpu<u64> = PerCpu::from_fn(|_| 0);
///
/// // In an interrupt-disabled context or with a lock:
/// let mut guard = PC_COUNTER.lock_irq();
/// *guard += 1;
/// ```
#[derive(Debug)]
#[repr(transparent)]
pub struct PerCpu<T> {
    data: [CpuLocalData<T>; MAX_NCPU],
}

impl<T: Default> Default for PerCpu<T> {
    fn default() -> Self {
        Self {
            data: array::from_fn(|id| CpuLocalData::new(id, T::default())),
        }
    }
}

impl<T: Clone> PerCpu<T> {
    #[must_use]
    pub fn new(data: T) -> Self {
        Self {
            data: array::from_fn(|id| CpuLocalData::new(id, data.clone())),
        }
    }
}

impl<T> PerCpu<T> {
    #[must_use]
    pub fn from_fn<F>(f: F) -> Self
    where
        F: Fn(usize) -> T,
    {
        Self {
            data: array::from_fn(|id| CpuLocalData::new(id, f(id))),
        }
    }

    pub fn get(&self) -> &T {
        unsafe { &*self.data[cpuid()].acquire().get() }
    }

    /// Returns a mutable reference to the local CPU's `T`, even on an immutable
    /// `&self`.
    ///
    /// # Safety
    ///
    /// Interrupts must be disabled before calling this, to avoid preemption and
    /// race conditions.
    #[must_use]
    #[allow(clippy::mut_from_ref)]
    pub unsafe fn get_mut_unchecked(&self) -> &mut T {
        assert!(!intr_get());
        &mut *self.data[cpuid()].acquire().get()
    }

    /// Disables interrupts and returns a guard that allows safe mutable access
    /// to the local CPU's data.
    ///
    /// Use this when you're already working in a context where you manage
    /// nested interrupt disabling.
    #[must_use]
    pub fn lock_irq(&self) -> IrqGuard<T> {
        irq::lock_irq(self.data[cpuid()].acquire())
    }

    /// Disables interrupts and returns a guard that allows safe mutable access
    /// to the local CPU's data, restoring the original interrupt state on drop.
    #[must_use]
    pub fn lock_irq_save(&self) -> IrqGuard<T> {
        irq::lock_irq_save(self.data[cpuid()].acquire())
    }

    /// Returns a reference to the data of the CPU specified by `cpuid`.
    ///
    /// # Safety
    ///
    /// Access the data of another CPU is undefined behavior. Ensure that the
    /// access to the data is synchronized.
    pub unsafe fn get_by_cpuid(&self, cpuid: usize) -> &T {
        &*self.data[cpuid].data.get()
    }
}

#[derive(Debug)]
struct CpuLocalData<T> {
    cpuid: usize,
    data: UnsafeCell<T>,
}

impl<T> CpuLocalData<T> {
    #[must_use]
    pub fn new(cpuid: usize, data: T) -> Self {
        Self {
            cpuid,
            data: UnsafeCell::new(data),
        }
    }

    fn acquire(&self) -> &UnsafeCell<T> {
        let cur_cpuid = cpuid();
        if cur_cpuid != self.cpuid {
            error!(
                "can not fetch the reference in different cpu: local {}, current {}",
                self.cpuid, cur_cpuid
            );
        }
        &self.data
    }
}

unsafe impl<T: Send> Sync for CpuLocalData<T> {}
