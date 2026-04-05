use alloc::{
    format,
    sync::{Arc, Weak},
};
use hashbrown::HashMap;
use redwing_ram::{
    file::{FileContentProvider, ReadOnlyRamFile},
    fs::InodeNoAllocatorVfs,
};
use redwing_vfs::{
    error::FsErrorKind, impl_dir_default_for_vinode, name::ValidFileName, VfsINodeOps, VfsINodeRef,
    VfsOps,
};
use rw_ulib_types::fcntl::{self, Dirent};

use crate::proc::{
    id::Tid,
    task::{self, TaskRef, TASKS_TABLE},
};

use super::{
    info::{cpuinfo, BuddyInfo, SlabInfo},
    tasks::task_dir,
    ProcFileSystem,
};

pub struct ProcRootDirectory {
    fs: Weak<ProcFileSystem>,
    self_ref: Weak<ProcRootDirectory>,
    children: HashMap<&'static str, VfsINodeRef>,
}

impl ProcRootDirectory {
    pub fn new(fs: &Arc<ProcFileSystem>) -> Arc<Self> {
        let mut children = HashMap::new();

        fn read_only_file<P: FileContentProvider>(
            fs: &Arc<ProcFileSystem>,
            content: P,
        ) -> VfsINodeRef {
            Arc::new(ReadOnlyRamFile::new(
                Arc::downgrade(fs) as Weak<dyn VfsOps>,
                fs.allocate_inode_no(),
                content,
            ))
        }

        children.insert("cpuinfo", read_only_file(fs, cpuinfo()));
        children.insert("slabinfo", read_only_file(fs, SlabInfo {}));
        children.insert("buddyinfo", read_only_file(fs, BuddyInfo {}));

        Arc::new_cyclic(|me| Self {
            children,
            self_ref: me.clone(),
            fs: Arc::downgrade(fs),
        })
    }

    fn current_dirent(&self) -> Dirent {
        let mut dirent = Dirent {
            inode_no: 1,
            typ: rw_ulib_types::fcntl::FileType::Directory,
            name: [0; 256],
            name_len: 0,
        };
        dirent.set_name(".");
        dirent
    }

    fn parent_dirent(&self) -> Dirent {
        let mut dirent = self.current_dirent();
        dirent.set_name("..");
        dirent
    }

    fn task_dirent(&self, task: &TaskRef) -> Dirent {
        let mut dirent = Dirent {
            inode_no: 100 + task.tid.as_u64() * 10,
            typ: rw_ulib_types::fcntl::FileType::Directory,
            name: [0; 256],
            name_len: 0,
        };
        dirent.set_name(&format!("{}", task.tid.as_u64()));
        dirent
    }
}

impl VfsINodeOps for ProcRootDirectory {
    impl_dir_default_for_vinode!();

    fn metadata(&self) -> redwing_vfs::error::Result<fcntl::Stat> {
        let size = self.children.len() + TASKS_TABLE.lock().len() + 1;
        Ok(fcntl::Stat {
            ino: 1,
            dev_no: 0,
            typ: fcntl::FileType::Directory,
            size: size as u64,
            nlink: 2,
        })
    }

    fn create(
        &self,
        _name: ValidFileName,
        _typ: fcntl::FileType,
    ) -> redwing_vfs::error::Result<redwing_vfs::VfsINodeRef> {
        Err(FsErrorKind::PermissionDenied.into())
    }

    fn unlink(&self, _name: ValidFileName) -> redwing_vfs::error::Result<()> {
        Err(FsErrorKind::PermissionDenied.into())
    }

    fn rename(
        &self,
        _old_name: ValidFileName,
        _target: &redwing_vfs::VfsINodeRef,
        _new_name: ValidFileName,
    ) -> redwing_vfs::error::Result<()> {
        Err(FsErrorKind::PermissionDenied.into())
    }

    fn get_dirents(
        &self,
        offset: u64,
        dirents: &mut [fcntl::Dirent],
    ) -> redwing_vfs::error::Result<(u64, usize)> {
        let mut current_offset = offset;
        let mut idx = 0;

        let tasks_table = TASKS_TABLE.lock_irq_save();
        let ntasks = tasks_table.len();

        let mut tasks = tasks_table.values().skip(offset.saturating_sub(2) as usize);

        while idx < dirents.len() {
            dirents[idx] = match current_offset {
                0 => self.current_dirent(),
                1 => self.parent_dirent(),

                _ => {
                    if let Some(task) = tasks.next() {
                        self.task_dirent(task)
                    } else {
                        break;
                    }
                }
            };

            current_offset += 1;
            idx += 1;
        }

        drop(tasks_table);

        let mut children = self
            .children
            .iter()
            .skip(offset.saturating_sub(2).saturating_sub(ntasks as u64) as usize);

        while idx < dirents.len() {
            dirents[idx] = if let Some((name, inode)) = children.next() {
                let mut dirent = Dirent {
                    typ: inode.file_type()?,
                    inode_no: inode.metadata()?.ino,
                    name: [0; 256],
                    name_len: 0,
                };
                dirent.set_name(name);
                dirent
            } else {
                break;
            };

            current_offset += 1;
            idx += 1;
        }

        Ok((current_offset - offset, idx))
    }

    fn try_lookup(
        &self,
        name: redwing_vfs::name::ValidLookupName,
    ) -> redwing_vfs::error::Result<Option<redwing_vfs::VfsINodeRef>> {
        if &*name == "." || &*name == ".." {
            return Ok(self.self_ref.upgrade().map(|inode| inode as _));
        }

        if let Some(child) = self.children.get(&*name).cloned() {
            return Ok(Some(child));
        }

        if let Ok(tid) = name.parse::<u64>() {
            if let Ok(task) = task::get_task_by_tid(&Tid::for_query(tid)) {
                return Ok(Some(task_dir(
                    self.fs.clone(),
                    self.self_ref.clone(),
                    &task,
                )?));
            }
        }

        Ok(None)
    }

    fn fs(&self) -> redwing_vfs::error::Result<Arc<dyn VfsOps>> {
        if let Some(fs) = self.fs.upgrade() {
            Ok(fs)
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }
}
