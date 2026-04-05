use core::{
    any::Any,
    sync::atomic::{AtomicUsize, Ordering},
};

pub trait IdAllocator: Any + Send + Sync {
    type IdType: PartialEq + Eq + Clone;

    fn allocate_id(&self) -> Self::IdType;
}

#[derive(Default)]
pub struct SelfIncIdAllocator {
    id: AtomicUsize,
}

impl SelfIncIdAllocator {
    #[must_use]
    pub const fn new() -> Self {
        Self::with_init_id(0)
    }

    #[must_use]
    pub const fn with_init_id(id: usize) -> Self {
        Self {
            id: AtomicUsize::new(id),
        }
    }
}

impl IdAllocator for SelfIncIdAllocator {
    type IdType = usize;

    fn allocate_id(&self) -> Self::IdType {
        self.id.fetch_add(1, Ordering::Relaxed)
    }
}
