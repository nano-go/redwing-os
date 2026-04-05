use alloc::sync::Arc;
use alloc::vec::Vec;
use rand::RngCore;
use redwing_vfs::{error::FsErrorKind, VfsOps};
use rw_ulib_types::fcntl::FileType;

use crate::{
    config::FsConfig,
    consts::{block::BLOCK_SIZE, inode::NUM_DIRECT_DATA_BLKS},
    dev::ramdev::RamBlockDevice,
    superblock::RawSuperBlock,
    vfs_impl::VfsImpl,
};

pub fn ram_fs() -> Arc<VfsImpl> {
    VfsImpl::make(
        Arc::new(RamBlockDevice::new()),
        RawSuperBlock::new(BLOCK_SIZE * 4000, 1024 * 16),
        &FsConfig::new(),
    )
    .unwrap()
}

#[test]
pub fn test_create_dir() {
    let fs = ram_fs();
    let root = fs.root().unwrap();

    let dir_a = root
        .create("dir_a".try_into().unwrap(), FileType::Directory)
        .unwrap();

    let dir_b = root
        .create("dir_b".try_into().unwrap(), FileType::Directory)
        .unwrap();

    let l_dir_a = root
        .try_lookup("dir_a".try_into().unwrap())
        .unwrap()
        .unwrap();

    let l_dir_b = root
        .try_lookup("dir_b".try_into().unwrap())
        .unwrap()
        .unwrap();

    assert_eq!(dir_a.metadata().unwrap(), l_dir_a.metadata().unwrap());
    assert_eq!(dir_b.metadata().unwrap(), l_dir_b.metadata().unwrap());

    let l_root = l_dir_a.lookup("..".try_into().unwrap()).unwrap();
    assert_eq!(l_root.metadata().unwrap(), l_root.metadata().unwrap());

    let dir_a = l_dir_a.lookup(".".try_into().unwrap()).unwrap();
    assert_eq!(dir_a.metadata().unwrap(), l_dir_a.metadata().unwrap());
}

#[test]
pub fn test_remove_dir() {
    let fs = ram_fs();
    let root = fs.root().unwrap();

    let cnt = fs.inner.count_allocated_inodes().unwrap();

    let dir_a = root
        .create("dir_a".try_into().unwrap(), FileType::Directory)
        .unwrap();

    root.unlink("dir_a".try_into().unwrap()).unwrap();
    drop(dir_a);

    assert!(root
        .try_lookup("dir_a".try_into().unwrap())
        .unwrap()
        .is_none());

    assert_eq!(fs.inner.count_allocated_inodes().unwrap(), cnt);
}

#[test]
pub fn test_remove_file() {
    let fs = ram_fs();
    let root = fs.root().unwrap();

    let cnt = fs.inner.count_allocated_inodes().unwrap();

    let file = root
        .create("file".try_into().unwrap(), FileType::RegularFile)
        .unwrap();

    root.unlink("file".try_into().unwrap()).unwrap();
    drop(file);

    assert!(root
        .try_lookup("file".try_into().unwrap())
        .unwrap()
        .is_none());

    assert_eq!(fs.inner.count_allocated_inodes().unwrap(), cnt);
}

#[test]
pub fn test_remove_non_empty_dir() {
    let fs = ram_fs();
    let root = fs.root().unwrap();

    let dir_a = root
        .create("dir_a".try_into().unwrap(), FileType::Directory)
        .unwrap();
    dir_a
        .create("tmp.txt".try_into().unwrap(), FileType::RegularFile)
        .unwrap();

    let result = root.unlink("dir_a".try_into().unwrap());
    assert_eq!(result.unwrap_err().kind, FsErrorKind::NotEmpty);
}

// =============================================================
//
//     Test Read/Write
//
// =============================================================

macro_rules! assert_read {
    ($inode:expr, $offset:expr, $buf:expr, $expected:expr) => {
        $buf.fill(0);
        let len = $inode.read($offset as u64, &mut $buf).unwrap();
        assert_eq!(len, ($buf.len() as u64) - $offset as u64);
        assert_eq!($buf[..len as usize], $expected[$offset..]);
    };
}

fn rand_bytes(len: usize) -> Vec<u8> {
    let mut arr = Vec::with_capacity(len);
    arr.resize(len, 0);
    rand::thread_rng().fill_bytes(&mut arr);
    arr
}

fn zero_bytes(len: usize) -> Vec<u8> {
    let mut arr = Vec::with_capacity(len);
    arr.resize(len, 0);
    arr
}

#[test]
pub fn test_wr_file_basic() {
    let fs = ram_fs();
    let root = fs.root().unwrap();
    let inode = root
        .create("test".try_into().unwrap(), FileType::RegularFile)
        .unwrap();

    inode.write(0, b"hello_world").unwrap();
    assert_eq!(inode.metadata().unwrap().size, 11);

    let mut buf = [0_u8; 5];
    inode.read(6, &mut buf).unwrap();
    assert_eq!(&buf, b"world");

    inode.write(6, b"rust1").unwrap();
    assert_eq!(inode.metadata().unwrap().size, 11);
    inode.read(6, &mut buf).unwrap();
    assert_eq!(&buf, b"rust1");

    // hello_ruby111
    inode.write(8, b"by111").unwrap();
    assert_eq!(inode.metadata().unwrap().size, 13);
    inode.read(8, &mut buf).unwrap();
    assert_eq!(&buf, b"by111");
    inode.read(6, &mut buf).unwrap();
    assert_eq!(&buf, b"ruby1");
}

#[test]
pub fn test_wr_large_file() {
    const BLOCK_CNT: usize = 3000;
    let data = rand_bytes(BLOCK_CNT * BLOCK_SIZE);

    let fs = ram_fs();
    let root = fs.root().unwrap();
    let inode = root
        .create("test".try_into().unwrap(), FileType::RegularFile)
        .unwrap();

    assert_eq!(inode.write(0, data.as_slice()).unwrap(), data.len() as _);
    assert_eq!(inode.metadata().unwrap().size, data.len() as _);

    let mut buf = zero_bytes(BLOCK_CNT * BLOCK_SIZE);

    assert_read!(inode, 0, buf, data);
    assert_read!(inode, 4095, buf, data);
    assert_read!(inode, 4096, buf, data);
    assert_read!(inode, BLOCK_SIZE * NUM_DIRECT_DATA_BLKS - 1, buf, data);
    assert_read!(inode, BLOCK_SIZE * NUM_DIRECT_DATA_BLKS, buf, data);
    assert_read!(inode, BLOCK_SIZE * 400, buf, data);
    assert_read!(inode, BLOCK_SIZE * 1025 - 1, buf, data);
    assert_read!(inode, BLOCK_SIZE * 2000 - 1, buf, data);
}

#[test]
pub fn test_truncate() {
    const BLOCK_CNT: usize = 100;
    let data = rand_bytes(BLOCK_CNT * BLOCK_SIZE);

    let fs = ram_fs();
    let root = fs.root().unwrap();
    let inode = root
        .create("test".try_into().unwrap(), FileType::RegularFile)
        .unwrap();

    let n = fs.inner.count_allocated_data_blocks().unwrap();

    assert_eq!(inode.write(0, &data).unwrap(), data.len() as u64);

    let mut buf = zero_bytes(BLOCK_CNT * BLOCK_SIZE);

    let len = BLOCK_SIZE * 50;
    inode.truncate(len as u64).unwrap();
    assert_read!(inode, 0, buf[..len], data[..len]);

    let len = BLOCK_SIZE * 14;
    inode.truncate(len as u64).unwrap();
    assert_read!(inode, 0, buf[..len], data[..len]);

    let len = BLOCK_SIZE * 12;
    inode.truncate(len as u64).unwrap();
    assert_read!(inode, 0, buf[..len], data[..len]);

    let len = BLOCK_SIZE * 11;
    inode.truncate(len as u64).unwrap();
    assert_read!(inode, 0, buf[..len], data[..len]);

    let len = 0;
    inode.truncate(len as u64).unwrap();
    assert_read!(inode, 0, buf[..len], data[..len]);

    assert_eq!(n, fs.inner.count_allocated_data_blocks().unwrap());
}

#[test]
pub fn test_with_wrong_offset() {
    let fs = ram_fs();
    let root = fs.root().unwrap();
    let inode = root
        .create("test".try_into().unwrap(), FileType::RegularFile)
        .unwrap();

    let mut buf = [0_u8; BLOCK_SIZE * 4];

    assert_eq!(
        inode.write(10, b"test").unwrap_err().kind,
        FsErrorKind::InvalidArgument
    );

    assert_eq!(
        inode.read(10, &mut buf).unwrap_err().kind,
        FsErrorKind::InvalidArgument
    );
}
