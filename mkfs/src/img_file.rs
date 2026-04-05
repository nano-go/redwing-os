use std::{
    fs::{self, Metadata, OpenOptions},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::Path,
    sync::Arc,
};

use redwing_efs::{
    config::FsConfig,
    consts::block::BLOCK_SIZE,
    dev::{stddev::StdioBlockDev, BlockDevice},
    superblock::RawSuperBlock,
    vfs_impl::VfsImpl,
};

use redwing_vfs::{VfsINodeRef, VfsOps};

use crate::{args::CmdArgs, custom_err, error::Result};

pub const MIN_IMG_FILE_SIZE: u64 = 40 * 1024 * 1024;

pub struct ImageFile {
    fs: Arc<VfsImpl>,
}

impl ImageFile {
    #[must_use]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let dev = Arc::new(StdioBlockDev::new(
            OpenOptions::new().read(true).write(true).open(path)?,
        ));
        Ok(Self {
            fs: VfsImpl::open(dev, &FsConfig::default())?,
        })
    }

    #[must_use]
    pub fn make<P: AsRef<Path>>(path: P, args: &CmdArgs) -> Result<Self> {
        let path = path.as_ref();
        let metadata = fs::metadata(path)?;

        Self::check_arguments(&metadata, args)?;

        let dev = Arc::new(StdioBlockDev::new(
            OpenOptions::new().read(true).write(true).open(path)?,
        ));

        Self::fill_zero(dev.as_ref(), metadata.size())?;

        let superblock = Self::make_super_block(&metadata, args);
        let fs = VfsImpl::make(dev.clone(), superblock, &FsConfig::default())?;
        Ok(Self { fs })
    }

    pub fn check_arguments(metadata: &Metadata, args: &CmdArgs) -> Result<()> {
        let size = metadata.size();
        if size < MIN_IMG_FILE_SIZE {
            return Err(custom_err!("the size of the image file must >= {}.", size));
        }

        if size % 4096 != 0 {
            return Err(custom_err!(
                "The size of the image file is not 4KB-aligned.",
            ));
        }

        if args.inode_size < 1024 * 8 {
            return Err(custom_err!("the inode size is too small."));
        }

        if args.inode_size as u64 >= size / 3 {
            return Err(custom_err!("the inode size is too large"));
        }
        Ok(())
    }

    pub fn make_super_block(metadata: &Metadata, args: &CmdArgs) -> RawSuperBlock {
        let size = metadata.size();
        RawSuperBlock::new(size as usize, args.inode_size)
    }

    pub fn fill_zero(dev: &dyn BlockDevice, size: u64) -> Result<()> {
        let total_blocks = size / BLOCK_SIZE as u64;
        let zeroed_buf = [0; BLOCK_SIZE];
        for bno in 0..total_blocks {
            dev.write_block(bno, &zeroed_buf)
                .map_err(|str| custom_err!("{str}"))?;
        }
        Ok(())
    }

    pub fn make_basic_dirs(&self) -> Result<()> {
        self.mkdir("/home")?;
        self.mkdir("/home/work")?;
        self.mkdir("/bin")?;
        self.mkdir("/usr")?;
        self.mkdir("/tmp")?;
        self.mkdir("/dev")?;
        self.mkdir("/proc")?;
        Ok(())
    }

    fn mkdir<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let path = path.as_ref();
        let parent_inode = self.namei(path.parent().unwrap())?;
        let name = path.file_name().unwrap();
        parent_inode.create(
            name.as_bytes().try_into()?,
            rw_ulib_types::fcntl::FileType::Directory,
        )?;
        Ok(())
    }

    fn namei<P: AsRef<Path>>(&self, path: P) -> Result<VfsINodeRef> {
        assert!(path.as_ref().is_absolute());
        let mut inode = self.fs.root()?;
        for seg in path.as_ref().iter().skip(1) {
            inode = inode.lookup(seg.as_bytes().try_into()?)?;
        }
        Ok(inode)
    }

    pub fn create_file<P: AsRef<Path>>(
        &self,
        parent_path: P,
        file_name: &str,
        data: &[u8],
    ) -> Result<()> {
        let parent_path = parent_path.as_ref();
        let parent_inode = self.namei(parent_path)?;

        let inode = if let Some(inode) = parent_inode.try_lookup(file_name.try_into()?)? {
            inode
        } else {
            parent_inode.create(
                file_name.try_into()?,
                rw_ulib_types::fcntl::FileType::RegularFile,
            )?
        };

        inode.write(0, data)?;
        Ok(())
    }

    pub fn sync_all(&self) -> Result<()> {
        self.fs.sync()?;
        Ok(())
    }
}
