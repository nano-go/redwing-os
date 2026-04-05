use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicUsize, Ordering};

/// A cell which can nominally be written to only once.
///
/// This is similar to [`core::cell::OnceCell`] but this is thread-safe for
/// initialization.
pub struct Once<T> {
    state: AtomicUsize,           // Tracks initialization state
    value: UnsafeCell<Option<T>>, // Holds the actual value
}

// Constants for the state machine
const UNINITIALIZED: usize = 0;
const INITIALIZING: usize = 1;
const INITIALIZED: usize = 2;

unsafe impl<T: Send + Sync> Sync for Once<T> {}

impl<T> Default for Once<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Once<T> {
    /// Creates a new uninitialized `Once`.
    pub const fn new() -> Self {
        Self {
            state: AtomicUsize::new(UNINITIALIZED),
            value: UnsafeCell::new(None),
        }
    }

    /// Initializes the value only once.
    pub fn call_once<F: FnOnce() -> T>(&self, f: F) {
        if self.state.load(Ordering::Acquire) == INITIALIZED {
            return;
        }

        if self
            .state
            .compare_exchange(
                UNINITIALIZED,
                INITIALIZING,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            // We're the first to initialize.
            let val = f();
            unsafe { *self.value.get() = Some(val) };

            // Mark as initialized.
            self.state.store(INITIALIZED, Ordering::Release);
        } else {
            // Wait for another core to finish initialization.
            while self.state.load(Ordering::Acquire) != INITIALIZED {
                core::hint::spin_loop();
            }
        }
    }

    /// Returns a reference to the initialized value if available.
    pub fn get(&self) -> Option<&T> {
        if self.state.load(Ordering::Acquire) == INITIALIZED {
            unsafe { (*self.value.get()).as_ref() }
        } else {
            None
        }
    }

    /// Gets the contents of the cell, initializing it to `f()` if the cell was
    /// uninitialized.
    pub fn get_or_init<F: FnOnce() -> T>(&self, f: F) -> &T {
        self.call_once(f);
        unsafe { (*self.value.get()).as_ref().unwrap() }
    }

    pub unsafe fn get_unchecked(&self) -> &T {
        (*self.value.get()).as_ref().unwrap_unchecked()
    }

    pub unsafe fn get_mut_unchecked(&mut self) -> &mut T {
        (*self.value.get_mut()).as_mut().unwrap_unchecked()
    }

    /// Spins until this contains a value.
    pub fn wait(&self) -> &T {
        while self.state.load(Ordering::Acquire) != INITIALIZED {
            core::hint::spin_loop();
        }
        unsafe { (*self.value.get()).as_ref().unwrap() }
    }
}
