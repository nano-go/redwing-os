use redwing_vfs::{
    error::{FsErrorKind, Result},
    fs_err,
};

use crate::{
    bitmap::BitmapBlocks,
    buffer::BlockBuffer,
    cache::BlockCacheManager,
    config::FsConfig,
    consts::inode::{INVALID_INODE_NO, REVERSED_INODE_NUM_RANGE, ROOT_INODE_NO},
    dev::BlockDevice,
    inode::INode,
    superblock::RawSuperBlock,
};
use alloc::sync::Arc;

pub struct EfsFileSystem {
    pub(crate) dev: Arc<dyn BlockDevice>,
    pub(crate) cache: Arc<BlockCacheManager>,
    pub(crate) superblock: RawSuperBlock,
    pub(crate) inode_bitmap: BitmapBlocks,
    pub(crate) data_bitmap: BitmapBlocks,
}

impl EfsFileSystem {
    pub fn open(dev: Arc<dyn BlockDevice>, config: &FsConfig) -> Result<Arc<Self>> {
        let fs = Self::new_self_ref_without_verify(dev, config)?;
        fs.verify()?;
        Ok(fs)
    }

    pub fn make(
        dev: Arc<dyn BlockDevice>,
        sb: RawSuperBlock,
        config: &FsConfig,
    ) -> Result<Arc<Self>> {
        {
            let cache = Arc::new(BlockCacheManager::new(dev.clone(), config));
            sb.verify()?;
            sb.write(&cache)?;
        }

        let fs = Self::new_self_ref_without_verify(dev, config)?;
        for ino in REVERSED_INODE_NUM_RANGE {
            fs.set_inode_bit(ino)?;
        }

        // Init root directory.
        let root_inode = INode::new(fs.clone(), ROOT_INODE_NO);
        root_inode.init_root_dir()?;

        fs.verify()?;
        Ok(fs)
    }

    pub(self) fn new_self_ref_without_verify(
        dev: Arc<dyn BlockDevice>,
        config: &FsConfig,
    ) -> Result<Arc<Self>> {
        let cache = Arc::new(BlockCacheManager::new(dev.clone(), config));

        let superblock = RawSuperBlock::read(&cache)?;
        let inode_bitmap = BitmapBlocks::new(
            superblock.inode_bitmap_start_bno(),
            superblock.inode_counts(),
            cache.clone(),
        );
        let data_bitmap = BitmapBlocks::new(
            superblock.data_bitmap_start_bno(),
            superblock.data_blocks() as usize,
            cache.clone(),
        );

        Ok(Arc::try_new(Self {
            dev,
            cache,
            superblock,
            inode_bitmap,
            data_bitmap,
        })?)
    }

    pub fn verify(&self) -> Result<()> {
        if !self.is_inode_used(INVALID_INODE_NO)? {
            return Err(fs_err!(
                FsErrorKind::FileSystemCorruption,
                "efs: the inode index 0 should be reserved for representing no inode",
            ));
        }

        if !self.is_inode_used(ROOT_INODE_NO)? {
            return Err(fs_err!(
                FsErrorKind::FileSystemCorruption,
                "efs: the root is not present",
            ));
        }

        self.superblock.verify()?;
        Ok(())
    }

    pub fn superblock(&self) -> &RawSuperBlock {
        &self.superblock
    }

    pub fn alloc_data_block(&self) -> Result<u64> {
        self.data_bitmap
            .alloc_bit()?
            .ok_or_else(|| FsErrorKind::NoSpaceLeft.into())
            .map(|v| v as u64 + self.superblock().data_start_bno())
    }

    pub fn dealloc_data_block(&self, bno: u64) -> Result<()> {
        self.data_bitmap
            .clear_bit((bno - self.superblock().data_start_bno()) as usize)
    }

    pub fn is_data_block_used(&self, bno: u64) -> Result<bool> {
        self.data_bitmap
            .get_bit((bno - self.superblock().data_start_bno()) as usize)
    }

    pub fn count_allocated_data_blocks(&self) -> Result<usize> {
        self.data_bitmap.count_allocated_bits()
    }

    pub fn alloc_inode(&self) -> Result<u64> {
        self.inode_bitmap
            .alloc_bit()?
            .ok_or_else(|| FsErrorKind::NoSpaceLeft.into())
            .map(|idx| idx as u64)
    }

    pub fn set_inode_bit(&self, inode_no: u64) -> Result<()> {
        self.inode_bitmap.set_bit(inode_no as usize)
    }

    pub fn dealloc_inode(&self, inode_no: u64) -> Result<()> {
        self.inode_bitmap.clear_bit(inode_no as usize)
    }

    pub fn is_inode_used(&self, inode_no: u64) -> Result<bool> {
        self.inode_bitmap.get_bit(inode_no as usize)
    }

    pub fn count_allocated_inodes(&self) -> Result<usize> {
        self.inode_bitmap.count_allocated_bits()
    }

    pub fn get_block(&self, blk_no: u64) -> Result<BlockBuffer> {
        self.cache.get_block(blk_no)
    }

    #[must_use]
    pub fn cache(&self) -> &BlockCacheManager {
        &self.cache
    }

    #[must_use]
    pub fn dev(&self) -> Arc<dyn BlockDevice> {
        self.dev.clone()
    }
}
