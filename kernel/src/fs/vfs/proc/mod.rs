use core::sync::atomic::AtomicU64;

use alloc::sync::Arc;
use redwing_ram::fs::InodeNoAllocatorVfs;
use redwing_vfs::{VfsINodeRef, VfsOps};
use root::ProcRootDirectory;

use crate::sync::spin::Once;

mod info;
mod root;
mod tasks;

#[must_use]
pub fn proc_vfs() -> Arc<dyn VfsOps> {
    ProcFileSystem::new()
}

pub struct ProcFileSystem {
    inode_no: AtomicU64,
    root: Once<Arc<ProcRootDirectory>>,
}

impl ProcFileSystem {
    pub fn new() -> Arc<Self> {
        let fs = Arc::new(Self {
            inode_no: AtomicU64::new(2),
            root: Once::new(),
        });
        fs.root.call_once(|| ProcRootDirectory::new(&fs));
        fs
    }
}

impl VfsOps for ProcFileSystem {
    fn root(&self) -> redwing_vfs::error::Result<VfsINodeRef> {
        Ok(self.root.get().unwrap().clone())
    }

    fn sync(&self) -> redwing_vfs::error::Result<()> {
        Ok(())
    }
}

impl InodeNoAllocatorVfs for ProcFileSystem {
    fn allocate_inode_no(&self) -> u64 {
        self.inode_no
            .fetch_add(1, core::sync::atomic::Ordering::Relaxed)
    }
}
