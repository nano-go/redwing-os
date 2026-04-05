use alloc::sync::{Arc, Weak};
use hashbrown::HashMap;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    fs_err,
    name::{ValidFileName, ValidLookupName},
    VfsINodeOps, VfsINodeRef, VfsOps,
};
use rw_ulib_types::fcntl::{Dirent, FileType, Stat};

use crate::{fs::pathname, sync::spin::Spinlock};

pub struct MountFileSystem {
    self_ref: Weak<MountFileSystem>,
    main_fs: Arc<dyn VfsOps>,
    mount_points: Spinlock<HashMap<u64, Arc<dyn VfsOps>>>,
}

impl MountFileSystem {
    pub fn new(inner: Arc<dyn VfsOps>) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            self_ref: me.clone(),
            main_fs: inner,
            mount_points: Spinlock::new("mount_points", HashMap::default()),
        })
    }

    pub fn mount(&self, path: &str, fs: Arc<dyn VfsOps>) -> Result<()> {
        let inode = pathname::ilookup(self.main_fs.root()?, path.as_bytes())?;
        if !inode.is_directory()? {
            return Err(FsErrorKind::NotADirectory.into());
        }
        self.mount_points.lock().insert(inode.metadata()?.ino, fs);
        Ok(())
    }

    pub fn unmount(&self, path: &str) -> Result<()> {
        let inode = pathname::ilookup(self.main_fs.root()?, path.as_bytes())?;
        self.mount_points.lock().remove(&inode.metadata()?.ino);
        Ok(())
    }
}

impl VfsOps for MountFileSystem {
    fn root(&self) -> Result<VfsINodeRef> {
        let origin = self.main_fs.root()?;
        let inode_no = origin.metadata()?.ino;

        let (inner, raw_inode) = if let Some(fs) = self.mount_points.lock().get(&inode_no) {
            (fs.root()?, Some(origin))
        } else {
            (origin, None)
        };

        Ok(Arc::new(MountFsINode {
            inode_no,
            mountfs: self.self_ref.upgrade().unwrap(),
            inner,
            raw_inode,
        }))
    }

    fn sync(&self) -> Result<()> {
        self.main_fs.sync()?;
        for fs in self.mount_points.lock().values() {
            fs.sync()?;
        }
        Ok(())
    }
}

pub struct MountFsINode {
    inode_no: u64,
    mountfs: Arc<MountFileSystem>,
    inner: VfsINodeRef,

    /// The inode reference of mount directory if the inode is the root of a
    /// mounted file system.
    raw_inode: Option<VfsINodeRef>,
}

impl MountFsINode {
    #[inline]
    pub fn inode(&self) -> Result<VfsINodeRef> {
        if let Some(fs) = self.get_mount_fs() {
            fs.root()
        } else {
            Ok(self.inner.clone())
        }
    }

    pub fn get_mount_fs(&self) -> Option<Arc<dyn VfsOps>> {
        if self.raw_inode.is_some() {
            None
        } else {
            self.mountfs
                .mount_points
                .lock()
                .get(&self.inode_no)
                .cloned()
        }
    }

    #[must_use]
    pub fn is_mount_point_root(&self) -> bool {
        self.inode_no == 1 && self.raw_inode.is_some()
    }
}

impl VfsINodeOps for MountFsINode {
    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        self.inode()?.read(offset, buf)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<u64> {
        self.inode()?.write(offset, buf)
    }

    fn metadata(&self) -> Result<Stat> {
        self.inode()?.metadata()
    }

    fn file_type(&self) -> Result<FileType> {
        self.inode()?.file_type()
    }

    fn create(&self, name: ValidFileName, typ: FileType) -> Result<VfsINodeRef> {
        let new_inode = if let Some(fs) = self.get_mount_fs() {
            fs.root()?.create(name, typ)?
        } else {
            self.inner.clone().create(name, typ)?
        };

        let inode_no = new_inode.metadata()?.ino;
        Ok(Arc::new(MountFsINode {
            mountfs: self.mountfs.clone(),
            inode_no,
            inner: new_inode,
            raw_inode: self.raw_inode.clone(),
        }))
    }

    fn unlink(&self, name: ValidFileName) -> Result<()> {
        if let Some(fs) = self.get_mount_fs() {
            fs.root()?.unlink(name)
        } else {
            if self.raw_inode.is_none() {
                if let Some(child) = self.inner.try_lookup(name.into())? {
                    if self
                        .mountfs
                        .mount_points
                        .lock()
                        .get(&child.metadata()?.ino)
                        .is_some()
                    {
                        return Err(fs_err!(
                            FsErrorKind::PermissionDenied,
                            "can not remove a mount point."
                        ));
                    }
                }
            }
            self.inner.unlink(name)
        }
    }

    fn rename(
        &self,
        old_name: ValidFileName,
        target: &VfsINodeRef,
        new_name: ValidFileName,
    ) -> Result<()> {
        self.inode()?.rename(old_name, target, new_name)
    }

    fn get_dirents(&self, offset: u64, dirents: &mut [Dirent]) -> Result<(u64, usize)> {
        self.inode()?.get_dirents(offset, dirents)
    }

    fn try_lookup(&self, name: ValidLookupName) -> Result<Option<VfsINodeRef>> {
        if let Some(fs) = self.get_mount_fs() {
            // Is a mount point.

            if &*name != ".." && &*name != "." {
                let Some(inode) = fs.root()?.try_lookup(name)? else {
                    return Ok(None);
                };
                return Ok(Some(Arc::new(MountFsINode {
                    mountfs: self.mountfs.clone(),
                    inode_no: inode.metadata()?.ino,
                    inner: inode,
                    raw_inode: Some(self.inner.clone()),
                })));
            }
        }

        let (inner, raw_inode) = if self.is_mount_point_root() && &*name == ".." {
            (self.raw_inode.as_ref().unwrap(), None)
        } else {
            (&self.inner, self.raw_inode.clone())
        };

        if let Some(inode) = inner.try_lookup(name)? {
            Ok(Some(Arc::new(MountFsINode {
                mountfs: self.mountfs.clone(),
                inode_no: inode.metadata()?.ino,
                inner: inode,
                raw_inode,
            })))
        } else {
            Ok(None)
        }
    }

    fn truncate(&self, new_size: u64) -> Result<u64> {
        self.inode()?.truncate(new_size)
    }

    fn fs(&self) -> Result<Arc<dyn VfsOps>> {
        Ok(self.mountfs.clone())
    }
}
