use alloc::sync::Arc;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    fs_err,
};

use crate::{cache::BlockCacheManager, consts::block::BLOCK_SIZE};

/// Manages a collection of bitmap blocks to track allocations efficiently.
/// This structure enables bitwise allocation and deallocation across multiple
/// disk blocks.
pub struct BitmapBlocks {
    /// The number of the first block containing the bitmap.
    pub start_blk_no: u64,
    /// The number of the last bitmap block (exclusive).
    pub end_blk_no: u64,
    /// Total number of bits managed by the bitmap.
    bits_len: usize,
    /// Cache manager for handling block data.
    cache: Arc<BlockCacheManager>,
}

const U64_BITS: usize = u64::BITS as usize;
const BLOCK_BITS: usize = BLOCK_SIZE * 8;

/// Represents a raw bitmap block stored in disk.
///
/// This struct ensures the block is correctly aligned for bitwise operations.
#[derive(Debug)]
#[repr(C)]
struct RawBitmapBlock([u64; BLOCK_SIZE / 8]);

impl BitmapBlocks {
    /// Creates a new `BitmapBlocks` instance.
    ///
    /// # Arguments
    /// - `start_blk_no` - The first block number of the bitmap.
    /// - `bits_len` - The number of all bits.
    /// - `cache` - A shared cache for block storage.
    #[must_use]
    pub fn new(start_blk_no: u64, bits_len: usize, cache: Arc<BlockCacheManager>) -> Self {
        let block_len = bits_len.div_ceil(BLOCK_BITS);
        Self {
            start_blk_no,
            end_blk_no: start_blk_no + block_len as u64,
            bits_len,
            cache,
        }
    }

    /// Returns the total number of bits managed.
    pub fn bits_len(&self) -> usize {
        self.bits_len
    }

    /// Modifies a bitmap block at `blk_no` using a provided closure.
    fn modify<F, T>(&self, blk_no: u64, f: F) -> Result<T>
    where
        F: FnOnce(&mut RawBitmapBlock) -> T,
    {
        let block = self.cache.get_block(blk_no)?;
        let mut block_lock = block.write();
        let bitmap = block_lock.as_ref_mut_at::<RawBitmapBlock>(0);
        Ok(f(bitmap))
    }

    /// Reads a bitmap block at `blk_no` using a provided closure.
    fn read<F, T>(&self, blk_no: u64, f: F) -> Result<T>
    where
        F: FnOnce(&RawBitmapBlock) -> T,
    {
        let block = self.cache.get_block(blk_no)?;
        let block_lock = block.read();
        let bitmap = block_lock.as_ref_at::<RawBitmapBlock>(0);
        Ok(f(bitmap))
    }

    /// Iterates over all 64-bit segments of the bitmap, invoking `f` on each.
    fn foreach_64_bits<F>(&self, mut f: F) -> Result<()>
    where
        F: FnMut(usize, u64),
    {
        let mut u64_idx = 0;
        for bno in self.start_blk_no..self.end_blk_no {
            let block_lock = self.cache.get_block(bno)?;
            let block = block_lock.read();
            let bitmap = block.as_ref_at::<RawBitmapBlock>(0);
            for bits in &bitmap.0 {
                f(u64_idx, *bits);
                u64_idx += 1;
                if u64_idx * U64_BITS >= self.bits_len {
                    return Ok(());
                }
            }
        }
        Ok(())
    }

    /// Iterates over all 64-bit segments of the bitmap and allows mutation.
    fn foreach_64_bits_mut<F, T>(&self, mut f: F) -> Result<Option<T>>
    where
        F: FnMut(usize, &mut u64) -> Option<T>,
    {
        let mut u64_idx = 0;
        for bid in self.start_blk_no..self.end_blk_no {
            let block_lock = self.cache.get_block(bid)?;
            let mut block = block_lock.write();
            let bitmap = block.as_ref_mut_at::<RawBitmapBlock>(0);
            for bits in &mut bitmap.0 {
                if let Some(ret) = f(u64_idx, bits) {
                    return Ok(Some(ret));
                }
                u64_idx += 1;
                if u64_idx * U64_BITS >= self.bits_len {
                    return Ok(None);
                }
            }
        }
        Ok(None)
    }

    /// Allocates the first available bit and returns its index.
    ///
    /// # Returns
    ///
    /// - `Some(index)` if a bit was allocated.
    /// - `None` if no bits were available.
    pub fn alloc_bit(&self) -> Result<Option<usize>> {
        let mut bit_idx = 0;
        self.foreach_64_bits_mut(|idx, bits| {
            let idx_within_bits = bits.trailing_ones() as usize;
            if idx_within_bits == U64_BITS {
                bit_idx += U64_BITS;
                return None;
            }
            bit_idx += idx_within_bits;
            if bit_idx >= self.bits_len {
                return None;
            }
            *bits |= 1 << idx_within_bits;
            Some(idx * U64_BITS + idx_within_bits)
        })
    }

    /// Determines the position of a bit within the bitmap.
    fn locate_bit_pos(&self, index: usize) -> (u64, usize, usize) {
        let blk_no = index / BLOCK_BITS;
        let bytes_idx = (index - blk_no * BLOCK_BITS) / U64_BITS;
        let idx_in_bit = index % U64_BITS;
        (self.start_blk_no + blk_no as u64, bytes_idx, idx_in_bit)
    }

    /// Returns whether a bit at `index` is set.
    pub fn get_bit(&self, index: usize) -> Result<bool> {
        if index >= self.bits_len {
            return Err(fs_err!(
                FsErrorKind::InvalidArgument,
                "index out of bound for bitmap."
            ));
        }
        let (blk_no, u64_idx, idx_within_bits) = self.locate_bit_pos(index);
        self.read(blk_no, |bitmap| {
            (bitmap.0[u64_idx] >> idx_within_bits) & 1 == 1
        })
    }

    /// Clears a bit at `index`, marking it as unallocated.
    pub fn clear_bit(&self, index: usize) -> Result<()> {
        if index >= self.bits_len {
            return Err(fs_err!(
                FsErrorKind::InvalidArgument,
                "index out of bound for bitmap."
            ));
        }
        let (block_id, bytes_idx, idx_in_bit) = self.locate_bit_pos(index);
        self.modify(block_id, |bitmap| {
            bitmap.0[bytes_idx] &= !(1 << idx_in_bit);
        })
    }

    /// Set a bit at `index`, marking it as allocated.
    pub fn set_bit(&self, index: usize) -> Result<()> {
        if index >= self.bits_len {
            return Err(fs_err!(
                FsErrorKind::InvalidArgument,
                "index out of bound for bitmap."
            ));
        }
        let (block_id, bytes_idx, idx_in_bit) = self.locate_bit_pos(index);
        self.modify(block_id, |bitmap| {
            bitmap.0[bytes_idx] |= 1 << idx_in_bit;
        })
    }

    /// Counts the number of allocated bits.
    pub fn count_allocated_bits(&self) -> Result<usize> {
        let mut total_allocated_bits = 0;
        self.foreach_64_bits(|_, bits| total_allocated_bits += bits.count_ones())?;
        Ok(total_allocated_bits as usize)
    }
}

#[cfg(test)]
mod tests {

    use std::sync::Arc;

    use redwing_vfs::error::{FsErrorKind, Result};

    use super::BitmapBlocks;
    use crate::{
        bitmap::BLOCK_BITS, cache::BlockCacheManager, config::FsConfig, dev::ramdev::RamBlockDevice,
    };

    pub fn mem_cache() -> Arc<BlockCacheManager> {
        Arc::new(BlockCacheManager::new(
            Arc::new(RamBlockDevice::new()),
            &FsConfig::new(),
        ))
    }

    #[test]
    fn test_basic_operations() -> Result<()> {
        let cache = mem_cache();
        let bitmap = BitmapBlocks::new(15, 32, cache);

        assert_eq!(bitmap.get_bit(0)?, false);
        assert_eq!(bitmap.alloc_bit()?, Some(0));
        assert_eq!(bitmap.get_bit(0)?, true);

        bitmap.clear_bit(0)?;
        assert_eq!(bitmap.get_bit(0)?, false);

        Ok(())
    }

    #[test]
    fn test_full_allocation() -> Result<()> {
        let cache = mem_cache();
        let bitmap = BitmapBlocks::new(15, BLOCK_BITS * 2, cache);

        for i in 0..BLOCK_BITS * 2 {
            assert_eq!(bitmap.alloc_bit()?, Some(i));
            assert_eq!(bitmap.count_allocated_bits()?, i + 1);
        }
        assert_eq!(bitmap.alloc_bit()?, None);

        for i in 0..(BLOCK_BITS * 2) {
            assert_eq!(bitmap.get_bit(i)?, true);
        }
        Ok(())
    }

    #[test]
    fn test_bounds_checks() -> Result<()> {
        let cache = mem_cache();
        let bitmap = BitmapBlocks::new(15, 4, cache);

        assert_eq!(
            bitmap.get_bit(4).unwrap_err().kind,
            FsErrorKind::InvalidArgument
        );

        assert_eq!(
            bitmap.clear_bit(4).unwrap_err().kind,
            FsErrorKind::InvalidArgument
        );

        for _ in 0..bitmap.bits_len() {
            bitmap.alloc_bit()?;
        }

        assert_eq!(bitmap.alloc_bit()?, None);
        Ok(())
    }
}
