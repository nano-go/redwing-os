use core::fmt::Display;

use endian_num::{le32, le64};
use human_size::human_size;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    fs_err,
};

use crate::{
    cache::BlockCacheManager,
    consts::{block::BLOCK_SIZE, inode::NINODES_PER_BLOCK},
};

pub const EFS_SB_MAGIC: u32 = 0xEFFF;
pub const SUPERBLOCK_BLOCK_NO: u64 = 1;

/// The superblock contains various information about file system such as inode
/// blocks, bitmap blocks, data blocks...
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct RawSuperBlock {
    pub magic: le32,

    pub size: le64,
    pub inode_bitmap_blocks: le64,
    pub inode_blocks: le64,
    pub data_bitmap_blocks: le64,
    pub data_blocks: le64,
}

impl RawSuperBlock {
    /// Creates a new `RawSuperBlock` structure based on the expected size and
    /// inode usage.
    ///
    /// This function is responsible for computing the layout of the essential
    /// file system metadata blocks (inode bitmaps, inodes, data bitmaps,
    /// and data blocks) given a total potential file system size and inode
    /// space requirement.
    ///
    /// # Parameters
    ///
    /// - `potential_size`: Total size in bytes that the file system can
    ///   potentially use. Must be:
    ///   - A multiple of `BLOCK_SIZE`.
    ///   - Larger than `BLOCK_SIZE * 128`.
    ///
    /// - `inode_size`: Total number of bytes allocated for inodes. Must be
    ///   greater than 8 KiB (1024 * 8).
    ///
    /// # Layout Calculation
    ///
    /// The block layout is computed in the following order:
    ///
    /// 1. **Inode Bitmap Blocks** – Tracks inode allocation.
    /// 2. **Inode Blocks** – Stores inode metadata.
    /// 3. **Data Bitmap Blocks** – Tracks data block usage.
    /// 4. **Data Blocks** – Actual blocks used for file data.
    ///
    /// The function ensures there's enough room for all components and
    /// calculates:
    /// - Total number of blocks (`potential_block_cnt`)
    /// - Number of blocks needed for inode storage and bitmaps
    /// - Remaining blocks for data, accounting for data block bitmaps
    ///
    /// # Panics
    ///
    /// This function will panic if any of the following conditions are
    /// violated:
    /// - `potential_size` is not a multiple of `BLOCK_SIZE`.
    /// - `potential_size` is too small to fit at least 128 blocks.
    /// - `inode_size` is too small or exceeds the file system size.
    #[must_use]
    pub fn new(potential_size: usize, inode_size: usize) -> Self {
        assert!(potential_size % BLOCK_SIZE == 0);
        assert!(potential_size > BLOCK_SIZE * 128);
        assert!(potential_size > inode_size);
        assert!(inode_size > 1024 * 8);

        let potential_block_cnt = potential_size / BLOCK_SIZE;

        let inode_blocks = inode_size.div_ceil(BLOCK_SIZE) as u64;
        let inode_bitmap_blocks = inode_size.div_ceil(BLOCK_SIZE * 8) as u64;

        let remainning_blocks = potential_block_cnt as u64 - inode_blocks - inode_bitmap_blocks - 2;

        let mut data_blocks = remainning_blocks;
        let data_bitmap_blocks = remainning_blocks.div_ceil(BLOCK_SIZE as u64 * 8);
        data_blocks -= data_bitmap_blocks;

        Self {
            magic: EFS_SB_MAGIC.into(),
            size: ((potential_block_cnt * BLOCK_SIZE) as u64).into(),
            inode_bitmap_blocks: inode_bitmap_blocks.into(),
            inode_blocks: inode_blocks.into(),
            data_bitmap_blocks: data_bitmap_blocks.into(),
            data_blocks: data_blocks.into(),
        }
    }

    pub fn read(cache: &BlockCacheManager) -> Result<Self> {
        Ok(*cache
            .get_block(SUPERBLOCK_BLOCK_NO)?
            .read()
            .as_ref_at::<Self>(0))
    }

    pub fn write(self, cache: &BlockCacheManager) -> Result<()> {
        let buf_lock = cache.get_block(SUPERBLOCK_BLOCK_NO)?;
        let mut block = buf_lock.write();
        *block.as_ref_mut_at::<Self>(0) = self;
        block.sync()?;
        Ok(())
    }

    pub fn verify(&self) -> Result<()> {
        if self.magic.to_ne() != EFS_SB_MAGIC {
            return Err(fs_err!(
                FsErrorKind::FileSystemCorruption,
                "efs superblock: invalid magic number"
            ));
        }

        if !Self::verify_bitmap(self.inode_bitmap_blocks() as usize, self.inode_counts()) {
            return Err(fs_err!(
                FsErrorKind::FileSystemCorruption,
                "efs superblock: bad inode bitmap"
            ));
        }

        if !Self::verify_bitmap(
            self.data_bitmap_blocks() as usize,
            self.data_blocks() as usize,
        ) {
            return Err(fs_err!(
                FsErrorKind::FileSystemCorruption,
                "efs superblock: bad data bitmap"
            ));
        }
        Ok(())
    }

    fn verify_bitmap(n_bitmap_blocks: usize, n_bits: usize) -> bool {
        let bitmap_bits = n_bitmap_blocks * BLOCK_SIZE * 8;
        bitmap_bits >= n_bits
    }

    #[must_use]
    pub fn size(&self) -> u64 {
        self.size.to_ne()
    }

    #[must_use]
    pub fn inode_bitmap_blocks(&self) -> u64 {
        self.inode_bitmap_blocks.to_ne()
    }

    #[must_use]
    pub fn inode_blocks(&self) -> u64 {
        self.inode_blocks.to_ne()
    }

    #[must_use]
    pub fn data_bitmap_blocks(&self) -> u64 {
        self.data_bitmap_blocks.to_ne()
    }

    #[must_use]
    pub fn data_blocks(&self) -> u64 {
        self.data_blocks.to_ne()
    }

    #[must_use]
    pub fn inode_bitmap_start_bno(&self) -> u64 {
        2
    }

    #[must_use]
    pub fn inode_start_bno(&self) -> u64 {
        2 + self.inode_bitmap_blocks()
    }

    #[must_use]
    pub fn data_bitmap_start_bno(&self) -> u64 {
        self.inode_start_bno() + self.inode_blocks()
    }

    #[must_use]
    pub fn data_start_bno(&self) -> u64 {
        self.data_bitmap_start_bno() + self.data_bitmap_blocks()
    }

    /// The number of all inodes in this file system.
    #[must_use]
    pub fn inode_counts(&self) -> usize {
        self.inode_blocks() as usize * NINODES_PER_BLOCK
    }
}

impl Display for RawSuperBlock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "magic: {:#X}", self.magic.to_ne())?;
        writeln!(f, "size: {}", human_size(self.size()))?;
        writeln!(
            f,
            "inode bitmap blocks: {}",
            self.inode_bitmap_blocks.to_ne()
        )?;
        writeln!(f, "inode blocks: {}", self.inode_blocks.to_ne())?;
        writeln!(f, "data bitmap blocks: {}", self.data_bitmap_blocks.to_ne())?;
        writeln!(f, "data blocks: {}", self.data_blocks.to_ne())
    }
}
