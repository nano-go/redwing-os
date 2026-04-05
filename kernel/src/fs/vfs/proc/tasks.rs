use alloc::{
    borrow::Cow,
    format,
    sync::{Arc, Weak},
};
use redwing_ram::{dir::RamDirectory, file::FileContentProvider};
use redwing_vfs::{
    error::{FsErrorKind, Result},
    VfsINodeOps, VfsINodeRef,
};

use crate::proc::{
    id::Tid,
    task::{Task, TaskRef},
};

use super::ProcFileSystem;

pub(super) fn task_dir(
    fs: Weak<ProcFileSystem>,
    parent: Weak<dyn VfsINodeOps>,
    task: &TaskRef,
) -> Result<VfsINodeRef> {
    if let Some(fs) = fs.upgrade() {
        let dir = RamDirectory::new(Arc::downgrade(&fs) as _, parent);
        dir.add_readonly_file(
            "status",
            TaksStatus {
                tid: task.tid,
                task: Arc::downgrade(task),
            },
        );
        dir.make_read_only();
        Ok(dir)
    } else {
        Err(FsErrorKind::NoSuchFileOrDirectory.into())
    }
}

struct TaksStatus {
    tid: Tid,
    task: Weak<Task>,
}

impl FileContentProvider for TaksStatus {
    fn provide_content(&self) -> Cow<'static, str> {
        if let Some(taskref) = self.task.upgrade() {
            let task = taskref.lock_irq_save();
            format!(
                "Name: {}\nTid: {}\nTgid: {}\nSid: {}\nState: {}",
                task.name,
                taskref.tid.as_u64(),
                task.tgid.as_u64(),
                task.sid.as_u64(),
                task.state
            )
            .into()
        } else {
            format!("the task {} does not exist.", self.tid).into()
        }
    }
}
