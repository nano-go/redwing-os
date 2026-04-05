use alloc::borrow::Cow;
use alloc::string::String;
use core::mem;

use redwing_vfs::{
    error::{FsErrorKind, Result},
    fs_err,
};

use crate::{
    consts::inode::{FILE_NAME_LEN, INVALID_INODE_NO},
    inode::{FileType, INode},
};

pub const RAW_DIRENT_SIZE: u64 = core::mem::size_of::<RawDirent>() as u64;

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct RawDirent {
    /// Number of inode that the directory entry points to.
    pub inode_no: u64,
    pub typ: FileType,
    pub name_len: u32,
    pub name: [u8; FILE_NAME_LEN],
}

impl RawDirent {
    pub fn new(inode_no: u64, typ: FileType, name: &str) -> Self {
        assert_ne!(typ, FileType::Unknown);
        let mut dirent = Self {
            inode_no,
            typ,
            name_len: name.len() as u32,
            name: [0; FILE_NAME_LEN],
        };
        dirent.name[..name.len()].copy_from_slice(name.as_bytes());
        dirent
    }

    pub(crate) fn read_from(inode: &INode, offset: u64) -> Result<Option<Self>> {
        let mut buf = [0_u8; RAW_DIRENT_SIZE as usize];
        let size = inode.raw_read(offset, &mut buf)?;

        if size == 0 {
            return Ok(None);
        }

        if size != RAW_DIRENT_SIZE {
            return Err(fs_err!(
                FsErrorKind::InvalidData,
                "there is no enough data to read a directory entry",
            ));
        }

        let dirent: RawDirent = unsafe { mem::transmute(buf) };
        if (dirent.name_len as usize) > FILE_NAME_LEN {
            return Err(FsErrorKind::FileNameTooLong.into());
        }

        Ok(Some(dirent))
    }

    pub(crate) fn write_to(&self, inode: &INode, offset: u64) -> Result<()> {
        let dirent: &[u8; RAW_DIRENT_SIZE as usize] = unsafe { mem::transmute(self) };
        let size = inode.raw_write(offset, dirent)?;
        if size != RAW_DIRENT_SIZE {
            inode.raw_truncate(inode.size()? - size)?;
            return Err(fs_err!(
                FsErrorKind::InvalidData,
                "writing a dirent is failure",
            ));
        }
        Ok(())
    }

    #[must_use]
    pub fn name(&self) -> &[u8] {
        &self.name[..self.name_len as usize]
    }

    #[must_use]
    pub fn name_as_utf8(&self) -> Cow<str> {
        String::from_utf8_lossy(self.name())
    }

    #[must_use]
    pub fn is_unused(&self) -> bool {
        self.inode_no == INVALID_INODE_NO
    }
}
