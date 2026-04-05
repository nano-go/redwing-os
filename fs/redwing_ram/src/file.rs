use core::any::Any;

use alloc::borrow::Cow;
use alloc::boxed::Box;
use alloc::sync::{Arc, Weak};
use alloc::vec::Vec;
use redwing_vfs::error::FsErrorKind;
use redwing_vfs::{error::Result, VfsINodeOps, VfsOps};
use redwing_vfs::{fs_err, impl_file_default_for_vinode};
use rw_ulib_types::fcntl::{FileType, Stat};
use spin::RwLock;

pub struct RamFile {
    pub inode_no: u64,
    pub fs: Weak<dyn VfsOps>,
    pub content: RwLock<Vec<u8>>,
}

impl RamFile {
    pub fn new(inode_no: u64, fs: Weak<dyn VfsOps>) -> Self {
        Self {
            inode_no,
            content: RwLock::new(Vec::new()),
            fs,
        }
    }
}

impl VfsINodeOps for RamFile {
    impl_file_default_for_vinode!();

    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        let content = self.content.read();
        if offset >= content.len() as u64 {
            return Ok(0);
        }
        let bytes_read = (content.len() - offset as usize).min(buf.len());
        buf[..bytes_read].copy_from_slice(&content[offset as usize..offset as usize + bytes_read]);
        Ok(bytes_read as u64)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<u64> {
        let mut content = self.content.write();
        let offset = offset as usize;
        if offset > content.len() {
            return Err(fs_err!(
                FsErrorKind::InvalidArgument,
                "ram file: write offset out of file."
            ));
        }

        let new_len = offset + buf.len();
        if new_len > content.len() {
            content.resize(new_len, 0);
        }

        content[offset..new_len].copy_from_slice(buf);
        Ok(buf.len() as u64)
    }

    fn metadata(&self) -> Result<Stat> {
        Ok(Stat {
            dev_no: 0,
            ino: self.inode_no,
            typ: FileType::RegularFile,
            size: self.content.read().len() as u64,
            nlink: 1,
        })
    }

    fn truncate(&self, new_size: u64) -> Result<u64> {
        let mut content = self.content.write();
        if new_size > content.len() as u64 {
            return Err(FsErrorKind::InvalidArgument.into());
        }
        let old_len = content.len() as u64;
        content.resize(new_size as usize, 0);
        Ok(old_len)
    }

    fn fs(&self) -> Result<Arc<dyn VfsOps>> {
        if let Some(fs) = self.fs.upgrade() {
            Ok(fs)
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }
}

pub trait FileContentProvider: Any + Send + Sync {
    fn provide_content(&self) -> Cow<'static, str>;

    fn size_hint(&self) -> Option<usize> {
        None
    }
}

impl FileContentProvider for &'static str {
    fn provide_content(&self) -> Cow<'static, str> {
        Cow::Borrowed(self)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.len())
    }
}

pub struct ReadOnlyRamFile {
    inode_no: u64,
    cotnent_provider: Box<dyn FileContentProvider>,
    buffer: RwLock<Option<Cow<'static, str>>>,
    fs: Weak<dyn VfsOps>,
}

impl ReadOnlyRamFile {
    pub fn new<P: FileContentProvider>(
        fs: Weak<dyn VfsOps>,
        ino: u64,
        cotnent_provider: P,
    ) -> Self {
        Self {
            inode_no: ino,
            cotnent_provider: Box::new(cotnent_provider),
            buffer: RwLock::new(None),
            fs,
        }
    }
}

impl VfsINodeOps for ReadOnlyRamFile {
    impl_file_default_for_vinode!();

    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        if buf.len() == 0 {
            return Ok(0);
        }

        let mut buffer = self.buffer.upgradeable_read();

        if buffer.is_none() {
            let mut buffer_mut = buffer.upgrade();
            *buffer_mut = Some(self.cotnent_provider.provide_content());
            buffer = buffer_mut.downgrade_to_upgradeable();
        }

        let content = buffer.as_ref().unwrap();
        let content_bytes = content.as_bytes();

        if offset >= content.len() as u64 {
            let mut buffer_mut = buffer.upgrade();
            *buffer_mut = None;
            return Ok(0);
        }

        let bytes_read = (content_bytes.len() - offset as usize).min(buf.len());
        buf[..bytes_read]
            .copy_from_slice(&content_bytes[offset as usize..offset as usize + bytes_read]);

        if offset + bytes_read as u64 >= content_bytes.len() as u64 {
            let mut buffer_mut = buffer.upgrade();
            *buffer_mut = None;
        }

        Ok(bytes_read as u64)
    }

    fn write(&self, _offset: u64, _buf: &[u8]) -> Result<u64> {
        Err(FsErrorKind::PermissionDenied.into())
    }

    fn metadata(&self) -> Result<Stat> {
        let buffer = self.buffer.read();

        let len;
        if let Some(buf) = &*buffer {
            len = buf.as_bytes().len();
        } else if let Some(sz) = self.cotnent_provider.size_hint() {
            len = sz;
        } else {
            len = self.cotnent_provider.provide_content().len();
        }

        Ok(Stat {
            dev_no: 0,
            ino: self.inode_no,
            typ: FileType::RegularFile,
            size: len as u64,
            nlink: 1,
        })
    }

    fn truncate(&self, _new_size: u64) -> Result<u64> {
        Err(FsErrorKind::PermissionDenied.into())
    }

    fn fs(&self) -> Result<Arc<dyn VfsOps>> {
        if let Some(fs) = self.fs.upgrade() {
            Ok(fs)
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use redwing_vfs::{
        error::Result,
        name::{ValidFileName, ValidLookupName},
        VfsOps,
    };

    use crate::fs::RamFileSystem;

    #[test]
    pub fn test_file_read_write() -> Result<()> {
        let fs = RamFileSystem::new();
        let root = fs.root()?;
        root.create(
            ValidFileName::try_from("test").unwrap(),
            rw_ulib_types::fcntl::FileType::RegularFile,
        )?;
        let inode = root.lookup(ValidLookupName::try_from("test").unwrap())?;

        let len = inode.write(0, b"hello world")?;
        assert_eq!(len, 11);

        let mut buf = [0_u8; 1024];
        let len = inode.read(0, &mut buf[..11])? as usize;
        assert_eq!(len, 11);
        assert_eq!(&buf[..len], b"hello world");

        let len = inode.write(6, b"rust")?;
        assert_eq!(len, 4);

        let len = inode.read(0, &mut buf[..11])? as usize;
        assert_eq!(len, 11);
        assert_eq!(&buf[..len], b"hello rustd");

        let len = inode.read(6, &mut buf[..11])? as usize;
        assert_eq!(len, 5);
        assert_eq!(&buf[..len], b"rustd");

        Ok(())
    }
}
