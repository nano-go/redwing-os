use path::{Component, Path};
use redwing_vfs::{
    error::{FsErrorKind, Result},
    name::ValidLookupName,
    VfsINodeRef,
};

use super::{current_fs, current_task_inode};

#[inline]
pub fn lookup(pathname: &[u8]) -> Result<VfsINodeRef> {
    try_lookup(pathname).and_then(|result| result.ok_or(FsErrorKind::NoSuchFileOrDirectory.into()))
}

#[inline]
pub fn try_lookup(pathname: &[u8]) -> Result<Option<VfsINodeRef>> {
    let parent = if Path::new(pathname).is_absolute() {
        current_fs().root()?
    } else {
        current_task_inode().map_err(|_| FsErrorKind::IOError)?
    };
    try_ilookup(parent, pathname)
}

#[inline]
pub fn ilookup(inode: VfsINodeRef, pathname: &[u8]) -> Result<VfsINodeRef> {
    try_ilookup(inode, pathname)
        .and_then(|result| result.ok_or(FsErrorKind::NoSuchFileOrDirectory.into()))
}

pub fn try_ilookup(mut inode: VfsINodeRef, pathname: &[u8]) -> Result<Option<VfsINodeRef>> {
    let path = Path::new(pathname);
    for comp in path.components() {
        match comp {
            Component::RootDir => (),
            Component::CurDir => (),
            comp => {
                let name = ValidLookupName::try_from(comp.as_bytes())?;
                match inode.try_lookup(name)? {
                    Some(i) => inode = i,
                    None => return Ok(None),
                }
            }
        }
    }

    Ok(Some(inode))
}

pub fn lookup_parent(pathname: &[u8]) -> Result<VfsINodeRef> {
    let path = Path::new(pathname);
    let mut parent = path.components();
    let last_comp = parent.next_back();
    match last_comp {
        Some(Component::CurDir) => lookup(b".."),
        Some(Component::ParentDir) => {
            let dir_inode = lookup(pathname)?;
            ilookup(dir_inode, b"..")
        }
        Some(Component::Normal(_)) => lookup(parent.as_path().as_bytes()),
        Some(Component::RootDir) | None => Err(FsErrorKind::NoSuchFileOrDirectory.into()),
    }
}
