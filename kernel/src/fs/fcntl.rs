use alloc::sync::Arc;
use path::{Component, Path};
use redwing_vfs::{name::ValidFileName, VfsINodeRef};

use crate::{
    error::{KResult, SysErrorKind},
    pipe::SharedPipe,
    proc::task,
};

use super::{
    current_fs, current_task_inode,
    file::File,
    pathname::{self},
};

// Including OpenFlags, Stat, Dirent...
use rw_ulib_types::fcntl::*;

fn get_file(fd: u32) -> KResult<Arc<File>> {
    let task_ref = task::current_task_or_err()?;
    let mut task = task_ref.lock();
    task.get_file(fd)
}

pub fn open(pathname: &[u8], flags: OpenFlags) -> KResult<u32> {
    let (inode, already_exists) = open_inode(pathname, flags)?;

    if already_exists && flags.contains(OpenFlags::EXCL | OpenFlags::CREAT) {
        return Err(SysErrorKind::AlreadyExists.into());
    }

    check_file_type_for_open(inode.file_type()?, flags, pathname)?;
    if already_exists && flags.contains(OpenFlags::TRUNC) {
        inode.truncate(0)?;
    }

    let file = File::with_inode(inode, flags);
    if flags.contains(OpenFlags::APPEND) {
        file.seek(0, SeekFrom::End)?;
    }

    task::current_task_or_err()?.lock().allocate_fd_for(file)
}

fn open_inode(pathname: &[u8], flags: OpenFlags) -> KResult<(VfsINodeRef, bool)> {
    if !flags.is_valid() {
        return Err(SysErrorKind::InvalidArgument.into());
    }

    let path = Path::new(pathname);
    let last_comp = path.components().next_back();
    match last_comp {
        Some(Component::RootDir) => Ok((current_fs().root()?, true)),
        Some(Component::CurDir) => Ok((current_task_inode()?, true)),
        Some(Component::ParentDir) => Ok((pathname::lookup(pathname)?, true)),
        Some(Component::Normal(name)) => {
            let name = ValidFileName::try_from(name)?;
            let parent = pathname::lookup_parent(pathname)?;
            if let Some(inode) = parent.try_lookup(name.into())? {
                return Ok((inode, true));
            }

            // The file does not exist.
            if !flags.contains(OpenFlags::CREAT) {
                return Err(SysErrorKind::NoSuchFileOrDirectory.into());
            }

            // Checks whether the path is a directory likes "abc/", "abc/."
            if path.is_dir() {
                // Can not creates a directory by 'open'.
                Err(SysErrorKind::IsADirectory.into())
            } else {
                let new_inode = parent.create(name, FileType::RegularFile)?;
                Ok((new_inode, false))
            }
        }
        // The path is an empty string.
        None => Err(SysErrorKind::NoSuchFileOrDirectory.into()),
    }
}

fn check_file_type_for_open<P: AsRef<Path>>(
    file_type: FileType,
    flags: OpenFlags,
    path: P,
) -> KResult<()> {
    let path = path.as_ref();
    match file_type {
        FileType::RegularFile => {
            if path.is_dir() {
                return Err(SysErrorKind::NotADirectory.into());
            }
        }

        FileType::Device => {
            if path.is_dir() {
                return Err(SysErrorKind::NotADirectory.into());
            }
            if flags.intersects(OpenFlags::CREAT | OpenFlags::TRUNC) {
                return Err(SysErrorKind::InvalidArgument.into());
            }
        }

        FileType::Directory => {
            if flags.access_mode() != OpenFlags::RDONLY
                || flags.intersects(OpenFlags::CREAT | OpenFlags::TRUNC | OpenFlags::APPEND)
            {
                return Err(SysErrorKind::IsADirectory.into());
            }
        }

        FileType::Symlink => {
            panic!("unexpected")
        }
    }
    Ok(())
}

pub fn close(fd: u32) -> KResult<()> {
    let task = task::current_task_or_err()?;
    let file = {
        let mut task = task.lock();
        task.o_files
            .get_mut(fd as usize)
            .unwrap_or(&mut None)
            .take()
    };

    if file.is_none() {
        return Err(SysErrorKind::BadFileDescriptor.into());
    }
    Ok(())
}

#[inline]
pub fn read(fd: u32, buf: &mut [u8]) -> KResult<u64> {
    get_file(fd)?.read(buf)
}

#[inline]
pub fn write(fd: u32, buf: &[u8]) -> KResult<u64> {
    get_file(fd)?.write(buf)
}

#[inline]
pub fn seek(fd: u32, offset: i64, whence: SeekFrom) -> KResult<u64> {
    get_file(fd)?.seek(offset, whence)
}

pub fn rmdir(pathname: &[u8]) -> KResult<()> {
    let path = Path::new(pathname);
    let last_comp = path.components().next_back();
    match last_comp {
        Some(Component::RootDir) => Err(SysErrorKind::NotEmpty.into()),
        Some(Component::CurDir) => Err(SysErrorKind::InvalidArgument.into()),
        Some(Component::ParentDir) => {
            pathname::lookup(pathname)?;
            // we can not remove the directory likes '/home/..'
            Err(SysErrorKind::NotEmpty.into())
        }
        Some(Component::Normal(name)) => {
            let parent = pathname::lookup_parent(pathname)?;
            let child = pathname::ilookup(parent.clone(), name)?;
            if !child.is_directory()? {
                return Err(SysErrorKind::NotADirectory.into());
            }
            Ok(parent.unlink(ValidFileName::try_from(name)?)?)
        }
        None => Err(SysErrorKind::NoSuchFileOrDirectory.into()),
    }
}

pub fn mkdir(pathname: &[u8]) -> KResult<()> {
    let path = Path::new(pathname);
    let last_comp = path.components().next_back();
    match last_comp {
        Some(Component::RootDir) => Err(SysErrorKind::AlreadyExists.into()),
        Some(Component::CurDir) | Some(Component::ParentDir) => {
            if pathname::try_lookup(pathname)?.is_some() {
                Err(SysErrorKind::AlreadyExists.into())
            } else {
                Err(SysErrorKind::NoSuchFileOrDirectory.into())
            }
        }
        Some(Component::Normal(name)) => {
            let parent_inode = pathname::lookup_parent(pathname)?;
            let name = ValidFileName::try_from(name)?;
            parent_inode.create(name, FileType::Directory)?;
            Ok(())
        }
        None => Err(SysErrorKind::NoSuchFileOrDirectory.into()),
    }
}

pub fn unlink(pathname: &[u8]) -> KResult<()> {
    let path = Path::new(pathname);
    let last_comp = path.components().next_back();
    match last_comp {
        Some(Component::RootDir) => Err(SysErrorKind::IsADirectory.into()),
        Some(Component::CurDir) | Some(Component::ParentDir) => {
            // Check whether the pathname exists.
            pathname::lookup(pathname)?;
            Err(SysErrorKind::IsADirectory.into())
        }
        Some(Component::Normal(name)) => {
            let parent = pathname::lookup_parent(pathname)?;
            let child = pathname::ilookup(parent.clone(), name)?;
            child.check_type_is_file()?;
            Ok(parent.unlink(ValidFileName::try_from(name)?)?)
        }
        None => Err(SysErrorKind::NoSuchFileOrDirectory.into()),
    }
}

pub fn get_dirents(fd: u32, dirents: &mut [Dirent]) -> KResult<usize> {
    get_file(fd)?.get_dirents(dirents)
}

pub fn stat(pathname: &[u8], stat: &mut Stat) -> KResult<()> {
    let inode = pathname::lookup(pathname)?;
    *stat = inode.metadata()?;
    Ok(())
}

pub fn cd(pathname: &[u8]) -> KResult<()> {
    let task = task::current_task_or_err()?;
    let inode = pathname::lookup(pathname)?;

    if !inode.is_directory()? {
        return Err(SysErrorKind::NotADirectory.into());
    }

    let old_inode = {
        let mut task = task.lock();
        task.cwd.replace(inode)
    };
    drop(old_inode);
    Ok(())
}

pub fn dup2(src_fd: u32, dst_fd: u32) -> KResult<()> {
    let task = task::current_task_or_err()?;
    {
        let mut task = task.lock();
        let file = task.get_file(src_fd)?;
        task.set_file(dst_fd, file.dup())?;
    }
    Ok(())
}

pub fn sync() -> KResult<()> {
    current_fs().sync()?;
    Ok(())
}

pub fn pipe(fds: &mut [u32; 2]) -> KResult<()> {
    let task = task::current_task_or_err()?;
    let (r_pipe, w_pipe) = SharedPipe::create()?;
    let read_end_file = File::with_pipe(r_pipe, OpenFlags::RDONLY);
    let write_end_file = File::with_pipe(w_pipe, OpenFlags::WRONLY);
    {
        let mut task = task.lock();
        let fd0 = task.allocate_fd_for(read_end_file)?;
        let fd1 = task.allocate_fd_for(write_end_file);
        match fd1 {
            Ok(fd1) => {
                fds[0] = fd0;
                fds[1] = fd1;
                Ok(())
            }
            Err(err) => {
                task.o_files[fd0 as usize] = None;
                Err(err)
            }
        }
    }
}
