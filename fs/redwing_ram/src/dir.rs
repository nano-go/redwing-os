use core::sync::atomic::AtomicBool;

use alloc::{
    string::{String, ToString},
    sync::{Arc, Weak},
};
use hashbrown::HashMap;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    impl_dir_default_for_vinode,
    name::{ValidFileName, ValidLookupName},
    VfsINodeOps, VfsINodeRef, VfsOps,
};
use rw_ulib_types::fcntl::{Dirent, FileType, Stat};
use spin::RwLock;

use crate::{
    file::{FileContentProvider, RamFile, ReadOnlyRamFile},
    fs::{InodeNoAllocatorVfs, ROOT_INODE_NO},
};

/// A virtual directory in memory.
pub struct RamDirectory {
    inode_no: u64,
    self_ref: Weak<dyn VfsINodeOps>,
    parent: Weak<dyn VfsINodeOps>,
    fs: Weak<dyn InodeNoAllocatorVfs>,
    children: RwLock<HashMap<String, VfsINodeRef>>,
    is_read_only: AtomicBool,
}

impl RamDirectory {
    #[must_use]
    pub fn new(fs: Weak<dyn InodeNoAllocatorVfs>, parent: Weak<dyn VfsINodeOps>) -> Arc<Self> {
        let inode_no = if let Some(fs) = fs.upgrade() {
            fs.allocate_inode_no()
        } else {
            ROOT_INODE_NO
        };

        Arc::new_cyclic(|me| Self {
            inode_no,
            parent,
            fs,
            children: RwLock::default(),
            self_ref: me.clone() as _,
            is_read_only: AtomicBool::new(false),
        })
    }

    pub fn make_read_only(&self) {
        self.is_read_only
            .store(true, core::sync::atomic::Ordering::Relaxed);
    }

    pub fn add_readonly_file<P: FileContentProvider>(&self, name: &str, provider: P) {
        self.children.write().insert(
            name.to_string(),
            Arc::new(ReadOnlyRamFile::new(
                self.fs.clone(),
                self.fs.upgrade().unwrap().allocate_inode_no(),
                provider,
            )),
        );
    }

    pub fn add_inode(&self, name: &str, inode: VfsINodeRef) {
        self.children.write().insert(
            name.to_string(),
            Arc::new(RamInodeWrapper {
                inode_no: self.fs.upgrade().unwrap().allocate_inode_no(),
                fs: self.fs.clone(),
                inner: inode,
            }),
        );
    }

    #[must_use]
    pub fn parent(&self) -> Option<VfsINodeRef> {
        self.parent.upgrade()
    }

    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.is_read_only
            .load(core::sync::atomic::Ordering::Relaxed)
    }
}

impl VfsINodeOps for RamDirectory {
    impl_dir_default_for_vinode!();

    fn metadata(&self) -> Result<Stat> {
        Ok(Stat {
            dev_no: 0,
            ino: self.inode_no,
            typ: FileType::Directory,
            // extra '.' and '..'
            size: self.children.read().len() as u64 + 2,
            nlink: 2,
        })
    }

    fn create(&self, name: ValidFileName, typ: FileType) -> Result<VfsINodeRef> {
        if self.is_read_only() {
            return Err(FsErrorKind::PermissionDenied.into());
        }

        let mut children = self.children.write();
        if children.contains_key(&*name) {
            return Err(FsErrorKind::AlreadyExists.into());
        }

        let inode = match typ {
            FileType::Directory => {
                RamDirectory::new(self.fs.clone(), self.self_ref.clone()) as VfsINodeRef
            }
            FileType::RegularFile => Arc::new(RamFile::new(
                self.fs
                    .upgrade()
                    .map(|fs| fs.allocate_inode_no())
                    .unwrap_or(0),
                self.fs.clone(),
            )),
            FileType::Device | FileType::Symlink => return Err(FsErrorKind::Unsupported.into()),
        };

        children.insert(name.to_string(), inode.clone());
        Ok(inode)
    }

    fn unlink(&self, name: ValidFileName) -> Result<()> {
        if self.is_read_only() {
            return Err(FsErrorKind::PermissionDenied.into());
        }

        let mut children = self.children.write();
        if let Some(_) = children.remove(&*name) {
            Ok(())
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }

    fn rename(
        &self,
        _old_name: ValidFileName,
        _target: &VfsINodeRef,
        _new_name: ValidFileName,
    ) -> Result<()> {
        Err(FsErrorKind::Unsupported.into())
    }

    fn get_dirents(&self, offset: u64, dirents: &mut [Dirent]) -> Result<(u64, usize)> {
        let children = self.children.read();

        let mut iter = children.iter().skip(offset.saturating_sub(2) as usize);

        let mut current_offset = offset;
        let mut idx = 0;

        while idx < dirents.len() {
            dirents[idx] = if current_offset == 0 {
                Dirent::with_stat(&self.metadata()?, ".")
            } else if current_offset == 1 {
                if let Some(parent) = self.parent() {
                    Dirent::with_stat(&parent.metadata()?, "..")
                } else {
                    Dirent::with_stat(&self.metadata()?, "..")
                }
            } else if let Some((name, inode)) = iter.next() {
                Dirent::with_stat(&inode.metadata()?, &name)
            } else {
                break;
            };

            current_offset += 1;
            idx += 1;
        }

        Ok((current_offset - offset, idx))
    }

    fn try_lookup(&self, name: ValidLookupName) -> Result<Option<VfsINodeRef>> {
        if &*name == "." {
            return Ok(self.self_ref.upgrade());
        }

        if &*name == ".." {
            return Ok(self.parent.upgrade());
        }

        Ok(self.children.read().get(&*name).cloned())
    }

    fn fs(&self) -> Result<Arc<dyn VfsOps>> {
        if let Some(fs) = self.fs.upgrade() {
            Ok(fs)
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }
}

struct RamInodeWrapper {
    inode_no: u64,
    fs: Weak<dyn VfsOps>,
    inner: VfsINodeRef,
}

impl VfsINodeOps for RamInodeWrapper {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        self.inner.read(offset, buf)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<u64> {
        self.inner.write(offset, buf)
    }

    fn metadata(&self) -> Result<Stat> {
        let mut stat = self.inner.metadata()?;
        stat.ino = self.inode_no;
        Ok(stat)
    }

    fn create(&self, name: ValidFileName, typ: FileType) -> Result<VfsINodeRef> {
        self.inner.create(name, typ)
    }

    fn unlink(&self, name: ValidFileName) -> Result<()> {
        self.inner.unlink(name)
    }

    fn rename(
        &self,
        old_name: ValidFileName,
        target: &VfsINodeRef,
        new_name: ValidFileName,
    ) -> Result<()> {
        self.inner.rename(old_name, target, new_name)
    }

    fn get_dirents(&self, offset: u64, dirents: &mut [Dirent]) -> Result<(u64, usize)> {
        self.inner.get_dirents(offset, dirents)
    }

    fn try_lookup(&self, name: ValidLookupName) -> Result<Option<VfsINodeRef>> {
        self.inner.try_lookup(name)
    }

    fn truncate(&self, new_size: u64) -> Result<u64> {
        self.inner.truncate(new_size)
    }

    fn fs(&self) -> Result<Arc<dyn VfsOps>> {
        if let Some(fs) = self.fs.upgrade() {
            Ok(fs)
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }
}
