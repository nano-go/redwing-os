use core::fmt::Display;

use bitflags::bitflags;
use num_enum::{IntoPrimitive, TryFromPrimitive};

bitflags! {
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct OpenFlags: u32 {
        const RDONLY = 0;
        const WRONLY = 1;
        const RDWR = 2;
        const CREAT = 0o_100;
        const EXCL = 0o_200;
        const TRUNC = 0o_1000;
        const APPEND = 0o_2000;
    }
}

impl OpenFlags {
    pub fn access_mode(self) -> OpenFlags {
        OpenFlags::from_bits_retain(self.bits() & 0b11)
    }

    pub fn writable(self) -> bool {
        self.intersects(Self::WRONLY | Self::RDWR)
    }

    pub fn readable(self) -> bool {
        self.contains(OpenFlags::RDWR) || self.access_mode() == Self::RDONLY
    }

    pub fn is_valid(self) -> bool {
        if self.intersects(OpenFlags::TRUNC | OpenFlags::CREAT | OpenFlags::APPEND)
            && !self.writable()
        {
            return false;
        }
        if self.contains(OpenFlags::EXCL) && !self.contains(OpenFlags::CREAT) {
            return false;
        }
        true
    }
}

impl Display for OpenFlags {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        for (name, _) in self.iter_names() {
            write!(f, "{}, ", name)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum SeekFrom {
    Set = 0,
    Current = 1,
    End = 2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum FileType {
    RegularFile,
    Directory,
    Symlink,
    Device,
}

#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct Dirent {
    pub inode_no: u64,
    pub typ: FileType,
    pub name: [u8; 256],
    pub name_len: usize,
}

impl Default for Dirent {
    fn default() -> Self {
        Self {
            inode_no: 0,
            typ: FileType::RegularFile,
            name: [0; 256],
            name_len: 0,
        }
    }
}

impl Dirent {
    #[must_use]
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    #[inline]
    pub fn with_stat(stat: &Stat, name: &str) -> Self {
        let mut name_bytes = [0; 256];
        name_bytes[..name.len()].copy_from_slice(name.as_bytes());
        Self {
            inode_no: stat.ino,
            typ: stat.typ,
            name: name_bytes,
            name_len: name.len(),
        }
    }

    pub fn set_name(&mut self, name: &str) {
        self.name = [0; 256];
        self.name[..name.len()].copy_from_slice(name.as_bytes());
        self.name_len = name.len();
    }

    #[must_use]
    #[inline]
    pub fn name(&self) -> &str {
        unsafe { str::from_utf8_unchecked(&self.name[..self.name_len]) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[repr(C)]
pub struct Stat {
    pub ino: u64,
    pub dev_no: u32,
    pub typ: FileType,
    pub size: u64,
    pub nlink: u32,
}

impl Default for Stat {
    fn default() -> Self {
        Self {
            ino: 0,
            dev_no: 0,
            typ: FileType::RegularFile,
            size: 0,
            nlink: 0,
        }
    }
}

impl Stat {
    #[must_use]
    pub fn is_dirctory(&self) -> bool {
        self.typ == FileType::Directory
    }

    #[must_use]
    pub fn size(&self) -> u64 {
        self.size
    }
}
