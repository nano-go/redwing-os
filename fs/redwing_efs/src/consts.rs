pub mod inode {
    use core::ops::Range;

    use super::block::{BLOCK_SIZE, INVALID_DATA_BLOCK_NO};
    use crate::inode::RAW_INODE_SIZE;

    /// The inode number is reserved for representing no inode.
    pub const INVALID_INODE_NO: u64 = 0;

    /// The inode number is reserved for representing the root inode.
    pub const ROOT_INODE_NO: u64 = 1;

    pub const REVERSED_INODE_NUM_RANGE: Range<u64> = 0..20;

    /// Number of direct data block pointers.
    pub const NUM_DIRECT_DATA_BLKS: usize = 12;

    pub const BLK_NUMS_PER_BLOCK: usize =
        BLOCK_SIZE / core::mem::size_of_val(&INVALID_DATA_BLOCK_NO);

    pub const MAX_FILE_SIZE: u64 = (NUM_DIRECT_DATA_BLKS * BLOCK_SIZE
        + BLK_NUMS_PER_BLOCK * BLOCK_SIZE
        + BLK_NUMS_PER_BLOCK * BLK_NUMS_PER_BLOCK * BLOCK_SIZE)
        as u64;

    pub const NINODES_PER_BLOCK: usize = BLOCK_SIZE / RAW_INODE_SIZE;

    pub const FILE_NAME_LEN: usize = 64;
}

pub mod block {
    pub const BLOCK_SIZE: usize = 4096;

    /// Represent an invalid data block number (used for unallocated data
    /// blocks)
    pub const INVALID_DATA_BLOCK_NO: u64 = 0;
}
