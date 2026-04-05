use alloc::sync::Arc;
use alloc::sync::Weak;
use hashbrown::HashMap;
use redwing_vfs::fs_err;
use redwing_vfs::name::ValidLookupName;
use redwing_vfs::VfsOps;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    name::ValidFileName,
    VfsINodeOps,
};
use spin::{Mutex, RwLock};

use crate::config::FsConfig;
use crate::consts::inode::ROOT_INODE_NO;
use crate::dev::BlockDevice;
use crate::fs::EfsFileSystem;
use crate::inode::INode;
use crate::superblock::RawSuperBlock;

pub struct VfsImpl {
    self_ref: Weak<VfsImpl>,
    pub inner: Arc<EfsFileSystem>,
    inodes: RwLock<HashMap<u64, Weak<VfsInodeImpl>>>,
}

impl VfsImpl {
    pub fn open(block_dev: Arc<dyn BlockDevice>, config: &FsConfig) -> Result<Arc<Self>> {
        let inner = EfsFileSystem::open(block_dev, config)?;

        Ok(Arc::new_cyclic(|me| Self {
            self_ref: me.clone(),
            inner,
            inodes: RwLock::default(),
        }))
    }

    pub fn make(
        block_dev: Arc<dyn BlockDevice>,
        sb: RawSuperBlock,
        config: &FsConfig,
    ) -> Result<Arc<Self>> {
        let inner = EfsFileSystem::make(block_dev, sb, config)?;
        Ok(Arc::new_cyclic(|me| Self {
            self_ref: me.clone(),
            inner,
            inodes: RwLock::default(),
        }))
    }

    pub fn inode(&self, ino: u64) -> Result<Arc<VfsInodeImpl>> {
        let inodes = self.inodes.upgradeable_read();
        if let Some(inode) = inodes.get(&ino) {
            if let Some(inode) = inode.upgrade() {
                return Ok(inode);
            }
        }
        let inode = VfsInodeImpl::new_arc(self.self_ref.upgrade().unwrap(), ino)?;
        inodes.upgrade().insert(ino, Arc::downgrade(&inode));
        Ok(inode)
    }

    pub fn inode_cast(&self, ino: u64) -> Result<Arc<dyn VfsINodeOps>> {
        self.inode(ino).map(|inode| inode as Arc<dyn VfsINodeOps>)
    }

    fn delete_inode_from_disk(&self, inode: &VfsInodeImpl) -> Result<()> {
        let inode = inode.inner.lock();
        // Deallocates all data blocks.
        inode.raw_truncate(0)?;
        self.inner.dealloc_inode(inode.inode_no())?;
        self.inodes.write().remove(&inode.ino);
        Ok(())
    }
}

impl VfsOps for VfsImpl {
    fn root(&self) -> Result<redwing_vfs::VfsINodeRef> {
        self.inode_cast(ROOT_INODE_NO)
    }

    fn sync(&self) -> Result<()> {
        self.inner.cache.sync_all()
    }
}

pub struct VfsInodeImpl {
    fs: Arc<VfsImpl>,
    inner: Mutex<INode>,
}

impl VfsInodeImpl {
    pub fn new_arc(fs: Arc<VfsImpl>, ino: u64) -> Result<Arc<VfsInodeImpl>> {
        let inode = INode::new(fs.inner.clone(), ino);

        if !fs.inner.is_inode_used(ino)? {
            return Err(FsErrorKind::NoSuchFileOrDirectory.into());
        }

        let nlink = inode.read_raw_inode()?.nlink();
        if nlink == 0 {
            return Err(fs_err!(
                FsErrorKind::NoSuchFileOrDirectory,
                "the nlink of inode {} is zero.",
                inode.inode_no()
            ));
        }

        Ok(Arc::new(Self {
            fs: fs.clone(),
            inner: Mutex::new(inode),
        }))
    }

    pub fn real_lookup_inode(&self, name: ValidLookupName) -> Result<Arc<VfsInodeImpl>> {
        self.real_try_lookup_inode(name)?
            .ok_or(FsErrorKind::NoSuchFileOrDirectory.into())
    }

    pub fn real_try_lookup_inode(
        &self,
        name: ValidLookupName,
    ) -> Result<Option<Arc<VfsInodeImpl>>> {
        let inode = self.inner.lock();
        if let Some(dirent) = inode.lookup(name)? {
            Ok(Some(self.fs.inode(dirent.inode_no)?))
        } else {
            Ok(None)
        }
    }
}

impl VfsINodeOps for VfsInodeImpl {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        self.inner.lock().read(offset, buf)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<u64> {
        self.inner.lock().write(offset, buf)
    }

    fn metadata(&self) -> Result<rw_ulib_types::fcntl::Stat> {
        let inode = self.inner.lock();
        let raw_inode = inode.read_raw_inode()?;
        Ok(rw_ulib_types::fcntl::Stat {
            ino: inode.inode_no(),
            dev_no: raw_inode.dev_no(),
            typ: raw_inode.file_type().try_into()?,
            size: raw_inode.size(),
            nlink: raw_inode.nlink(),
        })
    }

    fn create(
        &self,
        name: ValidFileName,
        typ: rw_ulib_types::fcntl::FileType,
    ) -> Result<redwing_vfs::VfsINodeRef> {
        let ino = match typ {
            rw_ulib_types::fcntl::FileType::Device | rw_ulib_types::fcntl::FileType::Symlink => {
                Err(FsErrorKind::Unsupported.into())
            }
            rw_ulib_types::fcntl::FileType::Directory => self.inner.lock().mkdir(name),
            rw_ulib_types::fcntl::FileType::RegularFile => self.inner.lock().create_file(name),
        }?;
        self.fs.inode_cast(ino)
    }

    fn unlink(&self, name: ValidFileName) -> Result<()> {
        let child = self.real_lookup_inode(name.into())?;
        {
            let parent = self.inner.lock();
            let child = child.inner.lock();
            parent.remove(&child, name)?;
        }
        Ok(())
    }

    fn rename(
        &self,
        _old_name: ValidFileName,
        _target: &redwing_vfs::VfsINodeRef,
        _new_name: ValidFileName,
    ) -> Result<()> {
        Err(FsErrorKind::Unsupported.into())
    }

    fn get_dirents(
        &self,
        offset: u64,
        dirents: &mut [rw_ulib_types::fcntl::Dirent],
    ) -> Result<(u64, usize)> {
        self.inner.lock().get_dirents(offset, dirents)
    }

    fn try_lookup(&self, name: ValidLookupName) -> Result<Option<redwing_vfs::VfsINodeRef>> {
        Ok(self.real_try_lookup_inode(name)?.map(|inode| inode as _))
    }

    fn truncate(&self, new_size: u64) -> Result<u64> {
        self.inner.lock().truncate(new_size)
    }

    fn fs(&self) -> Result<Arc<dyn redwing_vfs::VfsOps>> {
        Ok(self.fs.clone() as _)
    }
}

impl Drop for VfsInodeImpl {
    fn drop(&mut self) {
        let stat = self.metadata();
        match stat {
            Ok(stat) => {
                if stat.nlink == 0 {
                    let _ = self.fs.delete_inode_from_disk(self);
                }
            }
            Err(_err) => {}
        }
    }
}
