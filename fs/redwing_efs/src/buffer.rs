use core::{alloc::Layout, any::Any, ptr::NonNull};

use alloc::sync::Arc;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    fs_err,
};
use spin::RwLock;

use crate::{consts::block::BLOCK_SIZE, dev::BlockDevice};

macro_rules! assert_out_of_block {
    ($type:ty, $offset:expr) => {
        let offset = $offset;
        assert!(
            offset + core::mem::size_of::<$type>() <= $crate::consts::block::BLOCK_SIZE,
            "data out of bound: offset({}), type_size({}), block_size({})",
            offset,
            core::mem::size_of::<$type>(),
            $crate::consts::block::BLOCK_SIZE
        );
    };
}

pub trait BlockBufferAllocator: Send + Sync + Any {
    fn allocate(&self) -> Option<NonNull<[u8; BLOCK_SIZE]>>;
    fn deallocate(&self, buf: NonNull<[u8; BLOCK_SIZE]>);
}

#[derive(Debug, Clone, Copy)]
pub struct DefaultBlockBufferAllocator {}
impl BlockBufferAllocator for DefaultBlockBufferAllocator {
    fn allocate(&self) -> Option<NonNull<[u8; BLOCK_SIZE]>> {
        unsafe {
            NonNull::new(alloc::alloc::alloc_zeroed(Layout::new::<[u8; BLOCK_SIZE]>()).cast())
        }
    }

    fn deallocate(&self, buf: NonNull<[u8; BLOCK_SIZE]>) {
        unsafe {
            alloc::alloc::dealloc(buf.as_ptr().cast(), Layout::new::<[u8; BLOCK_SIZE]>());
        };
    }
}

pub type BlockBuffer = Arc<RwLock<SharedBlockBuffer>>;

pub struct SharedBlockBuffer {
    pub(crate) data: NonNull<[u8; BLOCK_SIZE]>,
    pub(crate) is_dirty: bool,
    pub(crate) blk_no: u64,
    allocator: Arc<dyn BlockBufferAllocator>,
    dev: Arc<dyn BlockDevice>,
}

unsafe impl Send for SharedBlockBuffer {}
unsafe impl Sync for SharedBlockBuffer {}

impl SharedBlockBuffer {
    pub fn new(
        dev: Arc<dyn BlockDevice>,
        allocator: Arc<dyn BlockBufferAllocator>,
        blk_no: u64,
    ) -> Result<Self> {
        let mut buf = allocator.allocate().ok_or(FsErrorKind::OutOfMemory)?;
        dev.read_block(blk_no, unsafe { buf.as_mut() })
            .map_err(|err| fs_err!(FsErrorKind::IOError, "{err}"))?;
        Ok(Self {
            data: buf,
            allocator,
            blk_no,
            dev,
            is_dirty: false,
        })
    }

    pub fn sync(&mut self) -> Result<()> {
        if self.is_dirty {
            self.dev
                .write_block(self.blk_no, self.data())
                .map_err(|err| fs_err!(FsErrorKind::IOError, "{err}"))?;
            self.is_dirty = false;
        }
        Ok(())
    }

    #[must_use]
    pub fn data(&self) -> &[u8] {
        unsafe { self.data.as_ref() }
    }

    #[must_use]
    pub fn data_mut(&mut self) -> &mut [u8] {
        unsafe { self.data.as_mut() }
    }

    fn ptr_at(&self, offset: usize) -> *const u8 {
        &self.data()[offset] as *const u8
    }

    fn ptr_mut_at(&mut self, offset: usize) -> *mut u8 {
        &mut self.data_mut()[offset] as *mut u8
    }

    pub fn as_ref_at<T>(&self, offset: usize) -> &T
    where
        T: Sized,
    {
        assert_out_of_block!(T, offset);
        unsafe { &*(self.ptr_at(offset) as *const T) }
    }

    pub fn as_ref_mut_at<T>(&mut self, offset: usize) -> &mut T
    where
        T: Sized,
    {
        assert_out_of_block!(T, offset);
        self.is_dirty = true;
        unsafe { &mut *(self.ptr_mut_at(offset) as *mut T) }
    }

    pub fn as_slice_mut_at(&mut self, offset: usize, len: usize) -> &mut [u8] {
        assert!(offset + len <= self.data().len());
        self.is_dirty = true;
        &mut self.data_mut()[offset..offset + len]
    }

    pub fn as_slice_at(&self, offset: usize, len: usize) -> &[u8] {
        assert!(offset + len <= self.data().len());
        &self.data()[offset..offset + len]
    }
}

impl Drop for SharedBlockBuffer {
    fn drop(&mut self) {
        self.allocator.deallocate(self.data);
        let _ = self.sync();
    }
}
