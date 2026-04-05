use alloc::sync::Arc;

use crate::buffer::{BlockBufferAllocator, DefaultBlockBufferAllocator};

pub struct FsConfig {
    pub(crate) max_transactions: usize,
    pub(crate) max_cached_blocks: usize,
    pub(crate) block_buffer_allocator: Arc<dyn BlockBufferAllocator + Send + 'static>,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self {
            max_transactions: 8,
            max_cached_blocks: 128,
            block_buffer_allocator: Arc::new(DefaultBlockBufferAllocator {}),
        }
    }
}

impl FsConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn max_transactions(mut self, max_transactions: usize) -> Self {
        self.max_transactions = max_transactions;
        self
    }

    #[must_use]
    pub fn max_cached_blocks(mut self, max_cached_blocks: usize) -> Self {
        self.max_cached_blocks = max_cached_blocks;
        self
    }

    pub fn block_buffer_allocator(mut self, allocator: impl BlockBufferAllocator) -> Self {
        self.block_buffer_allocator = Arc::new(allocator);
        self
    }
}
