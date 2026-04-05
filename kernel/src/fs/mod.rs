use alloc::sync::Arc;
use redwing_efs::config::FsConfig;
use redwing_ram::fs::RamFileSystem;
use redwing_vfs::{VfsINodeRef, VfsOps};
use vfs::mount::MountFileSystem;

use crate::{devices::block::get_block_device, error::KResult, proc::task, sync::spin::Once};

pub mod fcntl;
pub mod file;
pub mod ioctl;
pub mod pathname;
pub mod vfs;

#[cfg(test)]
pub mod tests;

pub static FS: Once<Arc<MountFileSystem>> = Once::new();

pub fn fs_init() {
    FS.call_once(|| {
        let efs = redwing_efs::vfs_impl::VfsImpl::open(get_block_device(), &FsConfig::new())
            .expect("fail to initialize efs");
        vfs::mount::MountFileSystem::new(efs)
    });

    mount("/tmp", RamFileSystem::new()).expect("fail to mount on 'tmp'");
    mount("/proc", vfs::proc::proc_vfs()).expect("fail to mount on 'proc'");
    mount("/dev", vfs::dev::dev_vfs()).expect("fail to mount on 'dev'");
}

#[must_use]
#[inline]
pub fn current_fs() -> Arc<dyn VfsOps> {
    FS.get()
        .expect("you should call this after fs_init.")
        .clone()
}

#[inline]
pub fn mount(path: &str, fs: Arc<dyn VfsOps>) -> KResult<()> {
    FS.get()
        .expect("you should call this after fs_init.")
        .mount(path, fs)?;
    Ok(())
}

#[inline]
pub fn unmount(path: &str) -> KResult<()> {
    FS.get()
        .expect("you should call this after fs_init.")
        .unmount(path)?;
    Ok(())
}

#[inline]
pub fn current_task_inode() -> KResult<VfsINodeRef> {
    let current_inode = task::current_task_or_err()?.lock().cwd.clone();
    Ok(match current_inode {
        Some(current_inode) => current_inode,
        None => current_fs().root()?,
    })
}
