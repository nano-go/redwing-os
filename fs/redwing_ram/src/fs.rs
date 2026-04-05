use core::sync::atomic::AtomicU64;

use alloc::sync::{Arc, Weak};
use redwing_vfs::{VfsINodeOps, VfsOps};

use crate::dir::RamDirectory;

pub const ROOT_INODE_NO: u64 = 1;

pub struct RamFileSystem {
    root: Arc<RamDirectory>,
    inode_no: AtomicU64,
}

impl RamFileSystem {
    #[must_use]
    pub fn new() -> Arc<Self> {
        Self::with_parent(Weak::<RamDirectory>::new())
    }

    #[must_use]
    pub fn with_parent(parent: Weak<dyn VfsINodeOps>) -> Arc<Self> {
        Arc::<Self>::new_cyclic(|me| Self {
            root: RamDirectory::new(me.clone(), parent),
            inode_no: AtomicU64::new(ROOT_INODE_NO + 1),
        })
    }

    #[must_use]
    pub fn root_dir(&self) -> Arc<RamDirectory> {
        self.root.clone()
    }
}

impl VfsOps for RamFileSystem {
    fn root(&self) -> redwing_vfs::error::Result<redwing_vfs::VfsINodeRef> {
        Ok(self.root.clone())
    }

    fn sync(&self) -> redwing_vfs::error::Result<()> {
        Ok(())
    }
}

pub trait InodeNoAllocatorVfs: VfsOps {
    fn allocate_inode_no(&self) -> u64;
}

impl InodeNoAllocatorVfs for RamFileSystem {
    fn allocate_inode_no(&self) -> u64 {
        self.inode_no
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    }
}
