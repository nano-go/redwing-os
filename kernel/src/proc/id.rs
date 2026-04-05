use core::{
    fmt,
    ops::Deref,
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(6300);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct Tid(u64);

impl Tid {
    #[must_use]
    pub fn next_tid() -> Self {
        Self(next_id())
    }

    #[must_use]
    pub fn for_query(tid: u64) -> Self {
        Self(tid)
    }

    #[must_use]
    pub fn zero() -> Self {
        Self(0)
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.0 == 0
    }

    #[must_use]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Deref for Tid {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl fmt::Display for Tid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tid <{}>", self.0)
    }
}
