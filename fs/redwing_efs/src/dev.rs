use core::{
    any::Any,
    marker::{Send, Sync},
};

pub trait BlockDevice: Any + Sync + Send {
    fn read_block(&self, blk_no: u64, buf: &mut [u8]) -> Result<(), &'static str>;
    fn write_block(&self, blk_no: u64, buf: &[u8]) -> Result<(), &'static str>;
}

#[cfg(any(feature = "ram-dev", test))]
pub mod ramdev {
    use alloc::vec;
    use alloc::vec::Vec;

    use hashbrown::HashMap;
    use spin::RwLock;

    use super::BlockDevice;

    #[derive(Default)]
    pub struct RamBlockDevice {
        blocks: RwLock<HashMap<u64, Vec<u8>>>,
    }

    impl RamBlockDevice {
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl BlockDevice for RamBlockDevice {
        fn read_block(&self, blk_no: u64, buf: &mut [u8]) -> Result<(), &'static str> {
            let blocks = self.blocks.upgradeable_read();
            if let Some(block) = blocks.get(&blk_no) {
                buf.copy_from_slice(block);
            } else {
                let mut blocks = blocks.upgrade();
                blocks.insert(blk_no, vec![0; buf.len()]);
                buf.fill(0);
            }
            Ok(())
        }

        fn write_block(&self, blk_no: u64, buf: &[u8]) -> Result<(), &'static str> {
            let mut blocks = self.blocks.write();
            if let Some(block) = blocks.get_mut(&blk_no) {
                block.copy_from_slice(buf);
            } else {
                blocks.insert(blk_no, Vec::from(buf));
            }
            Ok(())
        }
    }
}

#[cfg(feature = "stdio-dev")]
pub mod stddev {
    use std::io::{Read, Seek, Write};

    use spin::Mutex;

    use crate::consts::block::BLOCK_SIZE;

    use super::BlockDevice;

    extern crate std;

    #[derive(Debug, Default)]
    pub struct StdioBlockDev<T> {
        io: Mutex<T>,
    }

    impl<T> StdioBlockDev<T> {
        pub fn new(io: T) -> Self {
            Self { io: Mutex::new(io) }
        }
    }

    impl<T: Seek + Read + Write + Send + 'static> BlockDevice for StdioBlockDev<T> {
        fn read_block(&self, blk_no: u64, buf: &mut [u8]) -> Result<(), &'static str> {
            let mut io = self.io.lock();
            let mut read_blk = || -> std::io::Result<()> {
                io.seek(std::io::SeekFrom::Start(blk_no * BLOCK_SIZE as u64))?;
                io.read_exact(buf)?;
                Ok(())
            };
            read_blk().map_err(|_| "io: read error")
        }

        fn write_block(&self, blk_no: u64, buf: &[u8]) -> Result<(), &'static str> {
            let mut io = self.io.lock();
            let mut write_blk = || -> std::io::Result<()> {
                io.seek(std::io::SeekFrom::Start(blk_no * BLOCK_SIZE as u64))?;
                io.write_all(buf)?;
                Ok(())
            };
            write_blk().map_err(|_| "io: write error")
        }
    }
}
