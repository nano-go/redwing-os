mod once;
mod spinlock;

pub type Once<T> = once::Once<T>;

pub type Spinlock<T> = spinlock::Spinlock<T>;
pub type SpinlockGuard<'a, T> = spinlock::SpinlockGuard<'a, T>;
