#![no_std]
#![feature(allocator_api)]

extern crate alloc;

use core::any::Any;

use alloc::sync::Arc;
use error::{FsErrorKind, Result};
use name::{ValidFileName, ValidLookupName};
use rw_ulib_types::fcntl::{Dirent, FileType, Stat};

pub mod error;
pub mod name;

pub type VfsINodeRef = Arc<dyn VfsINodeOps>;

pub trait VfsOps: Any + Sync + Send {
    fn root(&self) -> Result<VfsINodeRef>;
    fn sync(&self) -> Result<()>;
}

pub trait VfsINodeOps: Any + Sync + Send {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64>;

    fn write(&self, offset: u64, buf: &[u8]) -> Result<u64>;

    fn metadata(&self) -> Result<Stat>;

    fn file_type(&self) -> Result<FileType> {
        Ok(self.metadata()?.typ)
    }

    fn create(&self, _name: ValidFileName, _typ: FileType) -> Result<VfsINodeRef>;

    fn unlink(&self, _name: ValidFileName) -> Result<()>;

    fn rename(
        &self,
        _old_name: ValidFileName,
        _target: &VfsINodeRef,
        _new_name: ValidFileName,
    ) -> Result<()>;

    fn get_dirents(&self, _offset: u64, _dirents: &mut [Dirent]) -> Result<(u64, usize)>;

    fn try_lookup(&self, _name: ValidLookupName) -> Result<Option<VfsINodeRef>>;

    fn lookup(&self, name: ValidLookupName) -> Result<VfsINodeRef> {
        Ok(self
            .try_lookup(name)?
            .ok_or(FsErrorKind::NoSuchFileOrDirectory)?)
    }

    fn truncate(&self, new_size: u64) -> Result<u64>;

    fn fs(&self) -> Result<Arc<dyn VfsOps>>;
}

impl dyn VfsINodeOps {
    /// Returns an iterator over all dirents.
    #[must_use]
    pub fn list(&self) -> ReadDir {
        ReadDir::new(self)
    }

    /// # Safety
    ///
    /// Caller must ensure that the type `T` has `[repr(C)]`
    pub unsafe fn read_struct<T>(&self, offset: u64, dst: &mut T) -> Result<()> {
        let buf =
            core::slice::from_raw_parts_mut(dst as *mut T as *mut u8, core::mem::size_of::<T>());
        let size = self.read(offset, buf)?;
        if size != buf.len() as u64 {
            return Err(FsErrorKind::InvalidArgument.into());
        }
        Ok(())
    }

    #[inline]
    pub fn check_type_is_file(&self) -> Result<()> {
        match self.file_type()? {
            FileType::Directory => Err(FsErrorKind::IsADirectory.into()),
            FileType::Symlink => Err(FsErrorKind::Unsupported.into()),
            FileType::RegularFile | FileType::Device => Ok(()),
        }
    }

    #[inline]
    pub fn is_file(&self) -> Result<bool> {
        match self.file_type()? {
            FileType::Directory => Ok(false),
            FileType::Symlink => Err(FsErrorKind::Unsupported.into()),
            FileType::RegularFile | FileType::Device => Ok(true),
        }
    }

    #[inline]
    pub fn is_directory(&self) -> Result<bool> {
        Ok(self.file_type()? == FileType::Directory)
    }
}

#[macro_export]
macro_rules! impl_dir_default_for_vinode {
    () => {
        fn read(&self, _offset: u64, _buf: &mut [u8]) -> $crate::error::Result<u64> {
            Err($crate::error::FsErrorKind::IsADirectory.into())
        }

        fn write(&self, _offset: u64, _buf: &[u8]) -> $crate::error::Result<u64> {
            Err($crate::error::FsErrorKind::IsADirectory.into())
        }

        fn truncate(&self, _new_size: u64) -> $crate::error::Result<u64> {
            Err($crate::error::FsErrorKind::IsADirectory.into())
        }
    };
}

#[macro_export]
macro_rules! impl_file_default_for_vinode {
    () => {
        fn create(
            &self,
            _name: $crate::name::ValidFileName,
            _typ: rw_ulib_types::fcntl::FileType,
        ) -> $crate::error::Result<$crate::VfsINodeRef> {
            Err($crate::error::FsErrorKind::NotADirectory.into())
        }

        fn unlink(&self, _name: $crate::name::ValidFileName) -> $crate::error::Result<()> {
            Err($crate::error::FsErrorKind::NotADirectory.into())
        }

        fn rename(
            &self,
            _old_name: $crate::name::ValidFileName,
            _target: &$crate::VfsINodeRef,
            _new_name: $crate::name::ValidFileName,
        ) -> $crate::error::Result<()> {
            Err($crate::error::FsErrorKind::NotADirectory.into())
        }

        fn get_dirents(
            &self,
            _offset: u64,
            _dirents: &mut [rw_ulib_types::fcntl::Dirent],
        ) -> $crate::error::Result<(u64, usize)> {
            Err($crate::error::FsErrorKind::NotADirectory.into())
        }

        fn try_lookup(
            &self,
            _name: $crate::name::ValidLookupName,
        ) -> $crate::error::Result<Option<$crate::VfsINodeRef>> {
            Err($crate::error::FsErrorKind::NotADirectory.into())
        }

        fn lookup(
            &self,
            _name: $crate::name::ValidLookupName,
        ) -> $crate::error::Result<$crate::VfsINodeRef> {
            Err($crate::error::FsErrorKind::NotADirectory.into())
        }
    };
}

pub struct ReadDir<'a> {
    inode: &'a dyn VfsINodeOps,
    buffer: [Dirent; 32],
    cur_offset: u64,
    idx_in_buf: usize,
    buf_len: usize,
}

impl<'a> ReadDir<'a> {
    #[must_use]
    pub fn new(inode: &'a dyn VfsINodeOps) -> Self {
        Self {
            inode,
            buffer: [Dirent::default(); 32],
            cur_offset: 0,
            idx_in_buf: 0,
            buf_len: 1, // fake len
        }
    }
}

impl<'a> Iterator for ReadDir<'a> {
    type Item = Result<Dirent>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.buf_len == 0 {
            return None;
        }

        if self.idx_in_buf == 0 {
            let result = self.inode.get_dirents(self.cur_offset, &mut self.buffer);
            match result {
                Ok((off, buf_len)) => {
                    self.cur_offset += off;
                    self.buf_len = buf_len;
                }
                Err(err) => {
                    return Some(Err(err));
                }
            }

            if self.buf_len == 0 {
                return None;
            }
        }

        let dirent = self.buffer[self.idx_in_buf];
        self.idx_in_buf = (self.idx_in_buf + 1) % self.buf_len;
        Some(Ok(dirent))
    }
}
