use alloc::sync::Arc;
use redwing_ram::fs::RamFileSystem;
use redwing_vfs::{
    error::{FsErrorKind, Result},
    impl_file_default_for_vinode, VfsINodeOps, VfsOps,
};
use rw_ulib_types::fcntl::{FileType, Stat};

use crate::devices::{self, get_device};

pub fn dev_vfs() -> Arc<dyn VfsOps> {
    let vfs = RamFileSystem::new();
    let root = vfs.root_dir();
    for dev in devices::get_all_devices() {
        let info = dev.info();
        root.add_inode(info.file_name, Arc::new(DevINode::new(info.device_no)));
    }
    root.make_read_only();
    vfs
}

pub struct DevINode {
    dev_no: u32,
}

impl DevINode {
    #[must_use]
    pub fn new(dev_no: u32) -> Self {
        Self { dev_no }
    }
}

impl VfsINodeOps for DevINode {
    impl_file_default_for_vinode!();

    fn read(&self, offset: u64, buf: &mut [u8]) -> Result<u64> {
        Ok(get_device(self.dev_no)
            .map_err(|_| FsErrorKind::NoSuchDev)?
            .dev_read(offset, buf)
            .map_err(|_| FsErrorKind::IOError)?)
    }

    fn write(&self, offset: u64, buf: &[u8]) -> Result<u64> {
        Ok(get_device(self.dev_no)
            .map_err(|_| FsErrorKind::NoSuchDev)?
            .dev_write(offset, buf)
            .map_err(|_| FsErrorKind::IOError)?)
    }

    fn metadata(&self) -> Result<Stat> {
        Ok(Stat {
            ino: 0,
            dev_no: self.dev_no,
            typ: FileType::Device,
            size: 1,
            nlink: 1,
        })
    }

    fn truncate(&self, _new_size: u64) -> Result<u64> {
        Err(FsErrorKind::Unsupported.into())
    }

    fn fs(&self) -> Result<Arc<dyn VfsOps>> {
        Err(FsErrorKind::Unsupported.into())
    }
}
