use core::ptr;

use alloc::sync::Arc;
use endian_num::{le32, le64};
use num_enum::{IntoPrimitive, TryFromPrimitive};
use redwing_vfs::{
    error::{FsError, FsErrorKind, Result},
    fs_err,
    name::{ValidFileName, ValidLookupName},
};

use crate::{
    consts::{
        block::{BLOCK_SIZE, INVALID_DATA_BLOCK_NO},
        inode::{
            BLK_NUMS_PER_BLOCK, INVALID_INODE_NO, MAX_FILE_SIZE, NINODES_PER_BLOCK,
            NUM_DIRECT_DATA_BLKS,
        },
    },
    dirent::{RawDirent, RAW_DIRENT_SIZE},
    fs::EfsFileSystem,
};

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, TryFromPrimitive, IntoPrimitive)]
#[repr(u32)]
pub enum FileType {
    Unknown = 0,
    Directory = 1,
    File = 2,
    Device = 3,
    Symlink = 4,
}

impl TryFrom<FileType> for rw_ulib_types::fcntl::FileType {
    type Error = FsError;

    fn try_from(value: FileType) -> core::result::Result<Self, Self::Error> {
        match value {
            FileType::File => Ok(Self::RegularFile),
            FileType::Directory => Ok(Self::Directory),
            FileType::Device => Ok(Self::Device),
            FileType::Symlink => Ok(Self::Symlink),
            FileType::Unknown => Err(FsErrorKind::InvalidData.into()),
        }
    }
}

pub struct INode {
    pub(super) fs: Arc<EfsFileSystem>,
    /// The number of inode.
    pub(super) ino: u64,
    /// The number of block that contains the inode.
    pub(super) blk_no: u64,
    /// Byte offset of the inode within the block.
    pub(super) blk_offset: usize,
}

impl INode {
    /// Creates an inode handler.
    pub fn new(fs: Arc<EfsFileSystem>, ino: u64) -> Self {
        let start_blk_no = fs.superblock().inode_start_bno();

        // The number of block that contains the inode.
        let blk_no = start_blk_no + ino / NINODES_PER_BLOCK as u64;
        // The offset of the inode within that block.
        let blk_offset = RAW_INODE_SIZE * (ino as usize % NINODES_PER_BLOCK);
        Self {
            fs,
            ino,
            blk_no,
            blk_offset,
        }
    }

    #[must_use]
    #[inline]
    pub fn inode_no(&self) -> u64 {
        self.ino
    }

    /// Initializes an inode with a given file type.
    pub(crate) fn init_inode(&self, typ: FileType, dev_no: u32) -> Result<()> {
        assert_ne!(typ, FileType::Unknown);

        self.write_raw_inode_with(|inode| {
            if inode.file_type() != FileType::Unknown {
                return Err(fs_err!(
                    FsErrorKind::FileSystemCorruption,
                    "Caller attempts to init a available raw inode."
                ));
            }

            *inode = RawINode {
                typ: u32::from(typ).into(),
                dev_no: dev_no.into(),
                ..RawINode::default()
            };
            Ok(())
        })
    }

    /// Reads the raw inode in the disk.
    pub fn read_raw_inode(&self) -> Result<RawINode> {
        Ok(*self
            .fs
            .get_block(self.blk_no)?
            .read()
            .as_ref_at(self.blk_offset))
    }

    pub(crate) fn write_raw_inode_with<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut RawINode) -> Result<T>,
    {
        let block_lock = self.fs.get_block(self.blk_no)?;
        let mut block = block_lock.write();
        let inode = block.as_ref_mut_at::<RawINode>(self.blk_offset);
        f(inode)
    }

    pub(crate) fn read_raw_inode_with<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&RawINode) -> Result<T>,
    {
        let block_lock = self.fs.get_block(self.blk_no)?;
        let block = block_lock.write();
        let inode = block.as_ref_at::<RawINode>(self.blk_offset);
        f(inode)
    }

    /// The `write` function attempts to write the whole `src` buffer to the
    /// file.
    ///
    /// Note: This is provided for internal file system, so you can write a
    /// buffer into a directory.
    ///
    /// # Arguments
    ///
    /// - `offset`: The byte offset to start writing.
    /// - `src`: The data to be written.
    ///
    /// # Returns
    ///
    /// The number of bytes successfully written.
    pub(crate) fn raw_write(&self, offset: u64, src: &[u8]) -> Result<u64> {
        self.fs
            .get_block(self.blk_no)?
            .write()
            .as_ref_mut_at::<RawINode>(self.blk_offset)
            .write(&self.fs, offset, src)
    }

    /// The `read` function attempts to read `dst.len()` bytes from the file to
    /// the `dst` buffer.
    ///
    /// Note: This is provided for internal file system, so you can read data
    /// from a directory.
    ///
    /// # Arguments
    ///
    /// - `offset`: The byte offset to read from.
    /// - `dst`: The buffer to store the read data.
    ///
    /// # Returns
    ///
    /// The number of bytes successfully readed.
    pub(crate) fn raw_read(&self, offset: u64, dst: &mut [u8]) -> Result<u64> {
        self.fs
            .get_block(self.blk_no)?
            .read()
            .as_ref_at::<RawINode>(self.blk_offset)
            .read(&self.fs, offset, dst)
    }

    /// Truncates a file to a new size.
    ///
    /// Note: This is provided for internal file system, so you can truncates
    /// any inode even it is a directory.
    ///
    /// # Arguments
    ///
    /// - `new_size`: The new file size in bytes.
    ///
    /// # Errors
    ///
    /// If the `new_size` is greater than the file size, `truncate` will return
    /// an [`FsError::InvalidInput`] error.
    ///
    /// `truncate` will return an I/O error while reading from or writing to the
    /// file.
    pub(crate) fn raw_truncate(&self, new_size: u64) -> Result<()> {
        let block_lock = self.fs.get_block(self.blk_no)?;
        let mut block = block_lock.write();
        let inode = block.as_ref_at::<RawINode>(self.blk_offset);

        if new_size > inode.size() {
            return Err(fs_err!(
                FsErrorKind::InvalidArgument,
                "the new size is greater than the file size.",
            ));
        }

        if new_size == inode.size() {
            return Ok(()); // No change needed.
        }

        // If shrinking, deallocate unused blocks.
        block
            .as_ref_mut_at::<RawINode>(self.blk_offset)
            .truncate(&self.fs, new_size)
    }

    /// Returns the size of the file content.
    #[inline]
    pub fn size(&self) -> Result<u64> {
        self.read_raw_inode_with(|inode| Ok(inode.size()))
    }

    /// Returns the file type of the inode.
    #[inline]
    pub fn file_type(&self) -> Result<FileType> {
        self.read_raw_inode_with(|inode| Ok(inode.file_type()))
    }
}

/// Implementation of operations for regular file.
impl INode {
    pub fn read(&self, offset: u64, dst: &mut [u8]) -> Result<u64> {
        match self.file_type()? {
            FileType::File => Ok(self.raw_read(offset, dst)?),
            FileType::Unknown => Err(FsErrorKind::InvalidData.into()),
            FileType::Directory => Err(FsErrorKind::IsADirectory.into()),
            FileType::Symlink => Err(FsErrorKind::Unsupported.into()),
            FileType::Device => Err(FsErrorKind::Unsupported.into()),
        }
    }

    pub fn write(&self, offset: u64, src: &[u8]) -> Result<u64> {
        match self.file_type()? {
            FileType::File => Ok(self.raw_write(offset, src)?),
            FileType::Unknown => Err(FsErrorKind::InvalidData.into()),
            FileType::Directory => Err(FsErrorKind::IsADirectory.into()),
            FileType::Symlink => Err(FsErrorKind::Unsupported.into()),
            FileType::Device => Err(FsErrorKind::Unsupported.into()),
        }
    }

    pub fn truncate(&self, new_size: u64) -> Result<u64> {
        let raw_inode = self.read_raw_inode()?;
        if raw_inode.nlink() == 0 {
            return Err(FsErrorKind::NoSuchFileOrDirectory.into());
        }

        match raw_inode.file_type() {
            FileType::File => {
                let old_sz = raw_inode.size();
                self.raw_truncate(new_size)?;
                Ok(old_sz)
            }
            FileType::Symlink => Err(FsErrorKind::Unsupported.into()),
            FileType::Device => Err(FsErrorKind::Unsupported.into()),
            FileType::Directory => Err(FsErrorKind::IsADirectory.into()),
            FileType::Unknown => Err(FsErrorKind::InvalidData.into()),
        }
    }
}

/// Implementation of operations for directory.
impl INode {
    /// This is used for creating the root directory.
    pub fn init_root_dir(&self) -> Result<()> {
        self.init_inode(FileType::Directory, 0)?;
        self.link(self, ".")?;
        Ok(())
    }

    fn check_for_create(&self, name: ValidFileName) -> Result<()> {
        let inode = self.read_raw_inode()?;

        if inode.file_type() != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        if inode.nlink() == 0 {
            return Err(FsErrorKind::NoSuchFileOrDirectory.into());
        }

        if self.lookup(name.into())?.is_some() {
            return Err(FsErrorKind::AlreadyExists.into());
        }

        Ok(())
    }

    fn create_with<F>(&self, name: ValidFileName, init_inode: F) -> Result<u64>
    where
        F: FnOnce(INode) -> Result<()>,
    {
        self.check_for_create(name)?;

        let new_ino = self.fs.alloc_inode()?;
        let new_inode = INode::new(self.fs.clone(), new_ino);

        if let Err(err) = init_inode(new_inode) {
            self.fs.dealloc_inode(new_ino)?;
            Err(err)
        } else {
            Ok(new_ino)
        }
    }

    pub fn symlink(&self, name: ValidFileName, linkpath: &[u8]) -> Result<u64> {
        if linkpath.len() >= 64 {
            return Err(FsErrorKind::InvalidArgument.into());
        }

        self.create_with(name, |new_inode| {
            new_inode.init_inode(FileType::File, 0)?;
            let result = new_inode.write(0, linkpath);
            if !matches!(result, Ok(len) if len == linkpath.len() as u64) {
                new_inode.raw_truncate(0)?;
                return Err(result.err().unwrap_or(FsErrorKind::IOError.into()));
            }
            self.link(&new_inode, &name)?;
            Ok(())
        })
    }

    /// Attempts to create a directory named the given `name`.
    pub fn mkdir(&self, name: ValidFileName) -> Result<u64> {
        self.create_with(name, |new_inode| {
            new_inode.init_inode(FileType::Directory, 0)?;
            new_inode.link(&new_inode, ".")?;
            if let Err(err) = new_inode.link(self, "..") {
                new_inode.unlink(&new_inode, ".")?;
                return Err(err);
            }
            self.link(&new_inode, &name)?;
            Ok(())
        })
    }

    pub fn create_file(&self, name: ValidFileName) -> Result<u64> {
        self.create_with(name, |new_inode| {
            new_inode.init_inode(FileType::File, 0)?;
            self.link(&new_inode, &name)?;
            Ok(())
        })
    }

    /// Remove a directory entry with the specified name.
    ///
    /// Note: caller must ensure that the `child` is a child file of this inode.
    pub fn remove(&self, child: &INode, name: ValidFileName) -> Result<()> {
        let parent = self;
        let parent_raw_inode = parent.read_raw_inode()?;

        if parent_raw_inode.file_type() != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        if parent_raw_inode.nlink() == 0 {
            return Err(FsErrorKind::NoSuchFileOrDirectory.into());
        }

        let child_raw_inode = child.read_raw_inode()?;

        if child_raw_inode.nlink() == 0 {
            return Err(FsErrorKind::FileSystemCorruption.into());
        }

        match child_raw_inode.file_type() {
            FileType::Unknown => {
                return Err(FsErrorKind::FileSystemCorruption.into());
            }

            FileType::File | FileType::Symlink | FileType::Device => {
                // A file can be deleted.
            }

            FileType::Directory => {
                if !child.is_empty_dir()? {
                    return Err(FsErrorKind::NotEmpty.into());
                }
                child.unlink(parent, "..")?;
                child.unlink(child, ".")?;
            }
        }

        parent.unlink(child, &name)?;
        Ok(())
    }

    pub fn lookup(&self, name: ValidLookupName) -> Result<Option<RawDirent>> {
        if self.file_type()? != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        let mut offset = 0;
        while let Some(dirent) = self.read_dirent_at(offset)? {
            if dirent.is_unused() || name.as_bytes() != dirent.name() {
                offset += RAW_DIRENT_SIZE;
                continue;
            }
            return Ok(Some(dirent));
        }

        Ok(None)
    }

    pub fn get_dirents(
        &self,
        offset: u64,
        dirents: &mut [rw_ulib_types::fcntl::Dirent],
    ) -> Result<(u64, usize)> {
        let inode = self.read_raw_inode()?;
        if inode.file_type() != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        if inode.nlink() == 0 {
            return Err(FsErrorKind::NoSuchFileOrDirectory.into());
        }

        let mut cur_offset = offset;
        let mut idx = 0;
        loop {
            if idx == dirents.len() {
                return Ok((cur_offset - offset, idx));
            }

            let raw_dirent = self.read_dirent_at(cur_offset)?;
            if let Some(raw_dirent) = raw_dirent {
                cur_offset += RAW_DIRENT_SIZE;
                if raw_dirent.is_unused() {
                    continue;
                }
                let mut name = [0_u8; 256];
                name[..raw_dirent.name.len()].copy_from_slice(&raw_dirent.name);
                dirents[idx] = rw_ulib_types::fcntl::Dirent {
                    name,
                    name_len: raw_dirent.name_len as usize,
                    inode_no: raw_dirent.inode_no,
                    typ: raw_dirent.typ.try_into()?,
                };
                idx += 1;
            } else {
                return Ok((cur_offset - offset, idx));
            }
        }
    }

    fn link(&self, child: &INode, name: &str) -> Result<()> {
        let dirent = RawDirent::new(child.ino, child.file_type()?, name);
        self.add_entry(&dirent)?;
        child.write_raw_inode_with(|raw_inode| {
            raw_inode.increase_link();
            Ok(())
        })
    }

    fn unlink(&self, child: &INode, name: &str) -> Result<()> {
        let dirent = self.remove_entry(name.as_bytes())?;
        if let Some(dirent_inode_no) = dirent {
            if dirent_inode_no != child.inode_no() {
                return Err(fs_err!(
                    FsErrorKind::FileSystemCorruption,
                    "unlink: mismatched inode"
                ));
            }

            child.write_raw_inode_with(|raw_inode| {
                raw_inode.decrease_link();
                Ok(())
            })
        } else {
            Err(FsErrorKind::NoSuchFileOrDirectory.into())
        }
    }

    fn add_entry(&self, new_dirent: &RawDirent) -> Result<()> {
        let mut offset = 0;
        let mut empty_dirent_offset = None;
        let name = &new_dirent.name;

        if self.file_type()? != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        while let Some(dirent) = self.read_dirent_at(offset)? {
            if dirent.is_unused() {
                // Try to find an empty entry to store the new dirent.
                empty_dirent_offset = Some(offset);
            } else if name == dirent.name() {
                return Err(FsErrorKind::AlreadyExists.into());
            }

            offset += RAW_DIRENT_SIZE;
        }

        // Write the new dirent into the empty place or append it to the directory.
        let write_offset = empty_dirent_offset.unwrap_or(offset);
        self.write_dirent_at(write_offset, new_dirent)
    }

    fn remove_entry(&self, name: &[u8]) -> Result<Option<u64>> {
        let inode = self.read_raw_inode()?;

        if inode.file_type() != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        let sz = inode.size();
        let mut offset = 0;

        while let Some(mut dirent) = self.read_dirent_at(offset)? {
            if !dirent.is_unused() && name == dirent.name() {
                let old_ino = dirent.inode_no;
                dirent.inode_no = INVALID_INODE_NO;
                self.write_dirent_at(offset, &dirent)?;
                if offset == sz - RAW_DIRENT_SIZE {
                    self.raw_truncate(offset)?;
                }
                return Ok(Some(old_ino));
            }

            offset += RAW_DIRENT_SIZE;
        }

        Ok(None)
    }

    #[inline]
    pub(crate) fn read_dirent_at(&self, offset: u64) -> Result<Option<RawDirent>> {
        RawDirent::read_from(self, offset)
    }

    #[inline]
    pub(crate) fn write_dirent_at(&self, offset: u64, dirent: &RawDirent) -> Result<()> {
        dirent.write_to(self, offset)
    }

    pub fn is_empty_dir(&self) -> Result<bool> {
        if self.file_type()? != FileType::Directory {
            return Err(FsErrorKind::NotADirectory.into());
        }

        // Skip '.' and '..'
        let mut offset = RAW_DIRENT_SIZE * 2;
        while let Some(dirent) = self.read_dirent_at(offset)? {
            if !dirent.is_unused() {
                return Ok(false);
            }
            offset += RAW_DIRENT_SIZE;
        }

        Ok(true)
    }
}

pub const RAW_INODE_SIZE: usize = core::mem::size_of::<RawINode>();

/// Represents the inode layout stored on disk.
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct RawINode {
    pub typ: le32,
    pub size: le64,
    pub nlink: le32,
    pub dev_no: le32,

    pub direct_blk_ptrs: [le64; NUM_DIRECT_DATA_BLKS],
    pub indirect_blk_ptr: le64,
    pub double_indirect_blk_ptr: le64,
}

impl Default for RawINode {
    fn default() -> Self {
        Self {
            typ: u32::from(FileType::Unknown).into(),
            size: le64::default(),
            nlink: le32::default(),
            dev_no: le32::default(),
            direct_blk_ptrs: [le64::from(INVALID_DATA_BLOCK_NO); NUM_DIRECT_DATA_BLKS],
            indirect_blk_ptr: le64::from(INVALID_DATA_BLOCK_NO),
            double_indirect_blk_ptr: le64::from(INVALID_DATA_BLOCK_NO),
        }
    }
}

impl RawINode {
    #[must_use]
    #[inline]
    pub fn file_type(&self) -> FileType {
        FileType::try_from_primitive(self.typ.to_ne()).unwrap_or(FileType::Unknown)
    }

    #[inline]
    pub fn set_file_type(&mut self, typ: FileType) {
        self.typ = (Into::<u32>::into(typ)).into();
    }

    #[must_use]
    #[inline]
    pub fn size(&self) -> u64 {
        self.size.to_ne()
    }

    #[must_use]
    #[inline]
    pub fn nlink(&self) -> u32 {
        self.nlink.to_ne()
    }

    #[inline]
    pub fn increase_link(&mut self) {
        self.nlink = le32::from(self.nlink.0 + 1);
    }

    #[inline]
    pub fn decrease_link(&mut self) {
        self.nlink = le32::from(self.nlink.0.saturating_sub(1));
    }

    #[must_use]
    #[inline]
    pub fn dev_no(&self) -> u32 {
        self.dev_no.into()
    }

    #[must_use]
    #[inline]
    fn direct_blk_ptrs(&self) -> &[le64] {
        unsafe {
            ptr::read_unaligned(&&raw const self.direct_blk_ptrs)
                .as_ref()
                .unwrap()
        }
    }

    #[must_use]
    #[inline]
    fn direct_blk_ptrs_mut(&mut self) -> &mut [le64] {
        unsafe {
            ptr::read_unaligned(&&raw mut self.direct_blk_ptrs)
                .as_mut()
                .unwrap()
        }
    }

    #[must_use]
    #[inline]
    fn indirect_blk_ptr_mut(&mut self) -> &mut le64 {
        unsafe {
            ptr::read_unaligned(&&raw mut self.indirect_blk_ptr)
                .as_mut()
                .unwrap()
        }
    }

    #[must_use]
    #[inline]
    fn double_indirect_blk_ptr_mut(&mut self) -> &mut le64 {
        unsafe {
            ptr::read_unaligned(&&raw mut self.double_indirect_blk_ptr)
                .as_mut()
                .unwrap()
        }
    }

    /// Maps a file offset to a data block.
    ///
    /// # Returns
    ///
    /// The number of the data block that contains data byte at the offset in
    /// file content.
    #[inline]
    fn bmap(&self, fs: &EfsFileSystem, offset: u64) -> Result<u64> {
        self.bmap_walk(fs, offset, |data_blk_no| Ok(data_blk_no.to_ne()))
    }

    /// Likes `bmap` but this will allocate a new data block if the located data
    /// block does not exist.
    #[inline]
    fn bmap_and_alloc_if_need(&mut self, fs: &EfsFileSystem, offset: u64) -> Result<u64> {
        self.bmap_walk_mut(fs, offset, true, |data_blk_no| {
            if data_blk_no.to_ne() == INVALID_DATA_BLOCK_NO {
                *data_blk_no = fs.alloc_data_block()?.into();
            }
            Ok(data_blk_no.to_ne())
        })
    }

    fn bmap_walk<F, T>(&self, fs: &EfsFileSystem, offset: u64, f: F) -> Result<T>
    where
        F: FnOnce(le64) -> Result<T>,
    {
        let mut idx = (offset / BLOCK_SIZE as u64) as usize;
        let direct = self.direct_blk_ptrs();
        if idx < direct.len() {
            return Self::bmap0(fs, direct, 0, idx, f);
        }

        idx -= direct.len();
        if idx < BLK_NUMS_PER_BLOCK {
            return Self::bmap_from_blk(fs, self.indirect_blk_ptr.into(), 0, idx, f);
        }

        idx -= BLK_NUMS_PER_BLOCK;
        Self::bmap_from_blk(fs, self.double_indirect_blk_ptr.into(), 1, idx, f)
    }

    fn bmap_from_blk<F, T>(
        fs: &EfsFileSystem,
        blk_no: u64,
        level: u32,
        idx: usize,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(le64) -> Result<T>,
    {
        Self::bmap0(
            fs,
            // Read block as table contains block numbers.
            fs.get_block(blk_no)?
                .read()
                .as_ref_at::<[le64; BLK_NUMS_PER_BLOCK]>(0),
            level,
            idx,
            f,
        )
    }

    fn bmap0<F, T>(fs: &EfsFileSystem, table: &[le64], level: u32, idx: usize, f: F) -> Result<T>
    where
        F: FnOnce(le64) -> Result<T>,
    {
        if level == 0 {
            if idx >= table.len() {
                return Err(FsErrorKind::FileTooLarge.into());
            }

            let data_no = table[idx];
            if data_no.to_ne() == INVALID_DATA_BLOCK_NO {
                return Err(FsErrorKind::InvalidArgument.into());
            }

            f(data_no)
        } else {
            let blk_nums_per_table = BLK_NUMS_PER_BLOCK.pow(level);
            let next_level_table_idx = idx / blk_nums_per_table;
            let idx = idx % blk_nums_per_table;

            if next_level_table_idx >= table.len() {
                return Err(FsErrorKind::FileTooLarge.into());
            }

            let blk_no = table[next_level_table_idx].to_ne();
            if blk_no == INVALID_DATA_BLOCK_NO {
                return Err(FsErrorKind::InvalidArgument.into());
            }
            Self::bmap_from_blk(fs, blk_no, level - 1, idx, f)
        }
    }

    fn bmap_walk_mut<F, T>(
        &mut self,
        fs: &EfsFileSystem,
        offset: u64,
        alloc: bool,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut le64) -> Result<T>,
    {
        let mut data_blk_idx = (offset / BLOCK_SIZE as u64) as usize;
        let direct = self.direct_blk_ptrs_mut();
        if data_blk_idx < direct.len() {
            return Self::bmap0_mut(fs, direct, 0, data_blk_idx, alloc, f);
        }

        data_blk_idx -= direct.len();
        if data_blk_idx < BLK_NUMS_PER_BLOCK {
            return Self::bmap_mut_from_blk(
                fs,
                self.indirect_blk_ptr_mut(),
                0,
                data_blk_idx,
                alloc,
                f,
            );
        }

        data_blk_idx -= BLK_NUMS_PER_BLOCK;
        Self::bmap_mut_from_blk(
            fs,
            self.double_indirect_blk_ptr_mut(),
            1,
            data_blk_idx,
            alloc,
            f,
        )
    }

    fn bmap_mut_from_blk<F, T>(
        fs: &EfsFileSystem,
        table_blk_no: &mut le64,
        level: u32,
        idx: usize,
        alloc: bool,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut le64) -> Result<T>,
    {
        if table_blk_no.to_ne() == INVALID_DATA_BLOCK_NO {
            if !alloc {
                return Err(FsErrorKind::InvalidArgument.into());
            }
            *table_blk_no = fs.alloc_data_block()?.into();
        }

        let table_block_lock = fs.get_block(table_blk_no.to_ne())?;
        let mut table_block = table_block_lock.write();
        Self::bmap0_mut(
            fs,
            table_block.as_ref_mut_at::<[le64; BLK_NUMS_PER_BLOCK]>(0),
            level,
            idx,
            alloc,
            f,
        )
    }

    fn bmap0_mut<F, T>(
        fs: &EfsFileSystem,
        table: &mut [le64],
        level: u32,
        idx: usize,
        alloc: bool,
        f: F,
    ) -> Result<T>
    where
        F: FnOnce(&mut le64) -> Result<T>,
    {
        if level == 0 {
            if idx >= table.len() {
                return Err(FsErrorKind::FileTooLarge.into());
            }
            f(&mut table[idx])
        } else {
            let blk_nums_per_table = BLK_NUMS_PER_BLOCK.pow(level);
            let next_level_table_idx = idx / blk_nums_per_table;
            let idx = idx % blk_nums_per_table;

            if next_level_table_idx >= table.len() {
                return Err(FsErrorKind::FileTooLarge.into());
            }
            let blk_no = &mut table[next_level_table_idx];
            Self::bmap_mut_from_blk(fs, blk_no, level - 1, idx, alloc, f)
        }
    }

    pub(crate) fn write(
        &mut self,
        fs: &EfsFileSystem,
        mut offset: u64,
        mut src: &[u8],
    ) -> Result<u64> {
        if offset > self.size() {
            return Err(FsErrorKind::InvalidArgument.into());
        }

        match self.file_type() {
            FileType::File | FileType::Symlink | FileType::Directory => (),
            FileType::Unknown => {
                return Err(fs_err!(
                    FsErrorKind::InvalidData,
                    "attempts to write data to an unknown inode.",
                ))
            }
            FileType::Device => {
                return Err(fs_err!(
                    FsErrorKind::InvalidData,
                    "attempts to write data a device inode.",
                ))
            }
        }

        // How many bytes were written.
        let mut write_len = 0_u64;

        while !src.is_empty() {
            if offset >= MAX_FILE_SIZE {
                return Ok(write_len);
            }

            let data_bno = self.bmap_and_alloc_if_need(fs, offset)?;
            // Offset within the data block.
            let b_offset = (offset % BLOCK_SIZE as u64) as usize;

            // Write `len` bytes.
            let len = usize::min(BLOCK_SIZE - b_offset, src.len());

            {
                fs.get_block(data_bno)?
                    .write()
                    .as_slice_mut_at(b_offset, len)
                    .copy_from_slice(&src[..len]);
            }

            src = &src[len..];
            write_len += len as u64;
            offset += len as u64;
            self.size = u64::max(self.size(), offset).into();
        }

        Ok(write_len)
    }

    pub(crate) fn read(
        &self,
        fs: &EfsFileSystem,
        mut offset: u64,
        mut dst: &mut [u8],
    ) -> Result<u64> {
        if offset > self.size() {
            return Err(FsErrorKind::InvalidArgument.into());
        }

        match self.file_type() {
            FileType::File | FileType::Symlink | FileType::Directory => (),
            FileType::Unknown => {
                return Err(fs_err!(
                    FsErrorKind::InvalidData,
                    "attempts to read data to an unknown inode.",
                ))
            }
            FileType::Device => {
                return Err(fs_err!(
                    FsErrorKind::InvalidData,
                    "attempts to read data a device inode.",
                ))
            }
        }

        // How many bytes were readed.
        let mut read_len = 0;
        let remaining_size = self.size() - offset;

        // Until the dst buffer is full or no data to read.
        while !dst.is_empty() && read_len < remaining_size {
            if offset >= MAX_FILE_SIZE {
                return Ok(read_len);
            }
            let data_bno = self.bmap(fs, offset)?;
            // Offset within the data block.
            let b_offset = (offset % BLOCK_SIZE as u64) as usize;

            // Read `len` bytes.
            let len = (BLOCK_SIZE - b_offset)
                .min(dst.len())
                .min((remaining_size - read_len) as usize);

            {
                let block = fs.get_block(data_bno)?;
                dst[..len].copy_from_slice(block.read().as_slice_at(b_offset, len));
            }

            dst = &mut dst[len..];
            read_len += len as u64;
            offset += len as u64;
        }

        Ok(read_len)
    }

    fn truncate(&mut self, fs: &EfsFileSystem, new_size: u64) -> Result<()> {
        assert!(self.size() > new_size && self.size() != 0);
        let blk_idx = new_size.div_ceil(BLOCK_SIZE as u64);
        self.dealloc_data_blks_from(fs, blk_idx)?;
        self.size = new_size.into();
        Ok(())
    }

    fn dealloc_data_blks_from(
        &mut self,
        fs: &EfsFileSystem,
        start_data_blk_idx: u64,
    ) -> Result<()> {
        if self.size() == 0 {
            return Ok(());
        }

        let last_data_blk_idx = self.size().saturating_sub(1) / BLOCK_SIZE as u64;

        for blk_idx in (start_data_blk_idx..=last_data_blk_idx).rev() {
            let offset = blk_idx * BLOCK_SIZE as u64;

            let result = self.bmap_walk_mut(fs, offset, false, |data_blk_no| {
                if data_blk_no.to_ne() == INVALID_DATA_BLOCK_NO {
                    Err(FsErrorKind::InvalidArgument.into())
                } else {
                    fs.dealloc_data_block(data_blk_no.to_ne())?;
                    *data_blk_no = INVALID_DATA_BLOCK_NO.into();
                    Ok(())
                }
            });

            if let Err(err) = result {
                if err.kind == FsErrorKind::InvalidArgument {
                    break;
                }
                return Err(err);
            }

            self.size = le64::from(self.size.0.saturating_sub(BLOCK_SIZE as u64));
        }

        if start_data_blk_idx <= (NUM_DIRECT_DATA_BLKS + BLK_NUMS_PER_BLOCK) as u64 {
            if self.double_indirect_blk_ptr.to_ne() != INVALID_DATA_BLOCK_NO {
                fs.dealloc_data_block(self.double_indirect_blk_ptr.to_ne())?;
            }

            if start_data_blk_idx <= NUM_DIRECT_DATA_BLKS as u64
                && self.indirect_blk_ptr.0 != INVALID_DATA_BLOCK_NO
            {
                fs.dealloc_data_block(self.indirect_blk_ptr.into())?;
            }
        }

        Ok(())
    }
}
