use core::num::NonZeroUsize;

use alloc::sync::Arc;

use lru::LruCache;
use redwing_vfs::error::Result;
use spin::{rwlock::RwLock, Mutex};

use crate::{
    buffer::{BlockBuffer, BlockBufferAllocator, SharedBlockBuffer},
    config::FsConfig,
    dev::BlockDevice,
};

pub struct BlockCacheManager {
    pub(crate) dev: Arc<dyn BlockDevice + Send>,
    pub(crate) cache: Mutex<LruCache<u64, BlockBuffer>>,
    allocator: Arc<dyn BlockBufferAllocator + Send + 'static>,
}

impl BlockCacheManager {
    pub fn new(dev: Arc<dyn BlockDevice>, config: &FsConfig) -> Self {
        Self {
            dev,
            cache: Mutex::new(LruCache::unbounded()),
            allocator: config.block_buffer_allocator.clone(),
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.cache.lock().len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.cache.lock().is_empty()
    }

    #[must_use]
    pub fn dev(&self) -> &dyn BlockDevice {
        self.dev.as_ref()
    }

    #[must_use]
    pub fn dev_arc(&self) -> Arc<dyn BlockDevice> {
        self.dev.clone()
    }

    pub fn trim_to_size(&self, s: usize) {
        self.cache.lock().resize(NonZeroUsize::new(s).unwrap());
    }

    pub fn get_block(&self, blk_no: u64) -> Result<BlockBuffer> {
        let mut map = self.cache.lock();
        if let Some(block) = map.get(&blk_no) {
            Ok(block.clone())
        } else {
            let block = Arc::new(RwLock::new(SharedBlockBuffer::new(
                self.dev.clone(),
                self.allocator.clone(),
                blk_no,
            )?));
            map.push(blk_no, block.clone());
            Ok(block)
        }
    }

    pub fn sync_all(&self) -> Result<()> {
        let list = self
            .cache
            .lock()
            .iter()
            .map(|(_, block)| block.clone())
            .collect::<alloc::vec::Vec<_>>();
        for block in list {
            block.write().sync()?;
        }
        Ok(())
    }
}
