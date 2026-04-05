use redwing_vfs::{VfsINodeOps, VfsINodeRef};
use rw_ulib_types::fcntl::{Dirent, OpenFlags, SeekFrom};

use crate::{
    error::{KResult, SysErrorKind},
    pipe::SharedPipe,
    sync::spin::Spinlock,
};

pub struct File {
    pub writable: bool,
    pub readable: bool,
    kind: FileKind,
}

impl File {
    #[must_use]
    pub fn with_inode(inode: VfsINodeRef, flags: OpenFlags) -> Self {
        Self {
            kind: FileKind::Inode {
                inode,
                offset: Spinlock::new("file_offset", 0),
            },
            writable: flags.writable(),
            readable: flags.readable(),
        }
    }

    #[must_use]
    pub fn with_pipe(pipe: SharedPipe, flags: OpenFlags) -> Self {
        Self {
            kind: FileKind::Pipe(pipe),
            writable: flags.writable(),
            readable: flags.readable(),
        }
    }

    #[must_use]
    pub fn dup(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            readable: self.readable,
            writable: self.writable,
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> KResult<u64> {
        if !self.readable {
            return Err(SysErrorKind::BadFileDescriptor.into());
        }

        self.kind.read(buf)
    }

    pub fn write(&self, buf: &[u8]) -> KResult<u64> {
        if !self.writable {
            return Err(SysErrorKind::BadFileDescriptor.into());
        }
        self.kind.write(buf)
    }

    pub fn seek(&self, offset: i64, whence: SeekFrom) -> KResult<u64> {
        self.kind.seek(offset, whence)
    }

    pub fn get_dirents(&self, dirents: &mut [Dirent]) -> KResult<usize> {
        if !self.readable {
            return Err(SysErrorKind::BadFileDescriptor.into());
        }

        self.kind.get_dirents(dirents)
    }

    pub fn get_inode(&self) -> Option<VfsINodeRef> {
        if let FileKind::Inode {
            inode,
            offset: _offset,
        } = &self.kind
        {
            Some(inode.clone())
        } else {
            None
        }
    }
}

enum FileKind {
    Pipe(SharedPipe),
    Inode {
        inode: VfsINodeRef,
        offset: Spinlock<u64>,
    },
}

impl FileKind {
    pub fn read(&self, buf: &mut [u8]) -> KResult<u64> {
        match self {
            Self::Pipe(pipe) => pipe.read(buf).map(|len| len as u64),
            Self::Inode { inode, offset } => {
                let len = inode.read(*offset.lock(), buf)?;
                *offset.lock() += len;
                Ok(len)
            }
        }
    }

    pub fn write(&self, buf: &[u8]) -> KResult<u64> {
        match self {
            Self::Pipe(pipe) => pipe.write(buf).map(|len| len as u64),
            Self::Inode { inode, offset } => {
                let len = inode.write(*offset.lock(), buf)?;
                *offset.lock() += len;
                Ok(len)
            }
        }
    }

    pub fn seek(&self, offset: i64, whence: SeekFrom) -> KResult<u64> {
        fn calculate_offset(
            cur_pos: u64,
            offset: i64,
            inode: &dyn VfsINodeOps,
            pos: SeekFrom,
        ) -> KResult<u64> {
            Ok(u64::try_from(match pos {
                SeekFrom::Set => offset,
                SeekFrom::Current => i64::try_from(cur_pos)
                    .map_err(|_| SysErrorKind::InvalidArgument)?
                    .checked_add(offset)
                    .ok_or(SysErrorKind::InvalidArgument)?,
                SeekFrom::End => i64::try_from(inode.metadata()?.size)
                    .map_err(|_| SysErrorKind::InvalidArgument)?
                    .checked_add(offset)
                    .ok_or(SysErrorKind::InvalidArgument)?,
            })
            .map_err(|_| SysErrorKind::InvalidArgument)?)
        }

        match self {
            Self::Pipe(_) => Err(SysErrorKind::Unsupported.into()),
            Self::Inode {
                inode,
                offset: f_offset,
            } => {
                let cur_offset = *f_offset.lock();
                let offset = calculate_offset(cur_offset, offset, inode.as_ref(), whence)
                    .map_err(|_| SysErrorKind::InvalidArgument)?;
                *f_offset.lock() = offset;
                Ok(offset)
            }
        }
    }

    pub fn get_dirents(&self, dirents: &mut [Dirent]) -> KResult<usize> {
        match self {
            Self::Pipe(_) => Err(SysErrorKind::Unsupported.into()),
            Self::Inode {
                inode,
                offset: f_offset,
            } => {
                let offset = *f_offset.lock();
                let (bytes_read, len) = inode.get_dirents(offset, dirents)?;
                *f_offset.lock() += bytes_read;
                Ok(len)
            }
        }
    }
}

impl Clone for FileKind {
    fn clone(&self) -> Self {
        match self {
            Self::Pipe(pipe) => Self::Pipe(pipe.clone()),
            Self::Inode { inode, offset } => Self::Inode {
                inode: inode.clone(),
                offset: Spinlock::new("file_offset", *offset.lock()),
            },
        }
    }
}
