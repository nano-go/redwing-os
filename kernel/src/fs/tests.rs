use alloc::sync::Arc;
use redwing_efs::{
    config::FsConfig, consts::block::BLOCK_SIZE, dev::ramdev::RamBlockDevice,
    superblock::RawSuperBlock,
};
use rw_ulib_types::fcntl::{Dirent, OpenFlags};

use crate::error::SysErrorKind;

use super::fcntl::*;

const INVALID_PATHS: [&'static str; 1] = ["/contains_unexpected_eof\0"];

const NO_SUCH_FILES: [&'static str; 4] = ["/../..", "/..", "/home/../..", "/home/work/../../.."];

macro_rules! assert_result_kind {
    ($result:expr, $kind:expr) => {
        if !matches!(&$result, Err(err) if err.kind() ==$kind) {
            panic!("should be '{}', but: {}", $kind, $result.unwrap_err());
        }
    };

    ($result:expr, $kind:expr, $( $args:tt )*) => {
        if !matches!(&$result, Err(err) if err.kind() ==$kind) {
            panic!("should be '{}', but: {}, msg: {}", $kind, $result.unwrap_err(), format_args!($( $args )*));
        }
    };

}

macro_rules! assert_for_invalid_path {
    ($result:tt, $path:expr) => {
        assert_result_kind!($result, SysErrorKind::InvalidArgument, "{}", $path)
    };
}

macro_rules! assert_for_no_such {
    ($result:tt, $path:expr) => {
        assert_result_kind!($result, SysErrorKind::NoSuchFileOrDirectory, "{}", $path)
    };

    ($result:tt) => {
        assert_result_kind!($result, SysErrorKind::NoSuchFileOrDirectory)
    };
}

fn mount_test_fs() {
    // Mount a temporary file system at root directory.
    super::mount(
        "/",
        // Tests the inner logical for my file system.
        redwing_efs::vfs_impl::VfsImpl::make(
            Arc::new(RamBlockDevice::new()),
            RawSuperBlock::new(BLOCK_SIZE * 1024, 1024 * 32),
            &FsConfig::new(),
        )
        .unwrap(),
    )
    .unwrap();
    mkdir(b"/home/").unwrap();
    mkdir(b"/home/work/").unwrap();
    mkdir(b"/tmp").unwrap();
}

fn unmoung_test_fs() {
    super::unmount("/").unwrap();
}

fn dirents(fd: u32) -> impl Iterator<Item = Dirent> {
    core::iter::from_fn(move || {
        let mut dirent = [Dirent::default(); 1];
        let len = get_dirents(fd, &mut dirent).unwrap();
        if len == 0 {
            None
        } else {
            Some(dirent[0])
        }
    })
}

#[test_case]
pub fn test_open_invalid_paths() {
    mount_test_fs();

    for path in INVALID_PATHS {
        let result = open(path.as_bytes(), OpenFlags::RDONLY);
        assert_for_invalid_path!(result, path);
    }
    for path in NO_SUCH_FILES {
        let result = open(path.as_bytes(), OpenFlags::CREAT | OpenFlags::RDWR);
        assert_for_no_such!(result, path);
    }

    let result = open(b"/home/..", OpenFlags::CREAT | OpenFlags::RDWR);
    assert_result_kind!(result, SysErrorKind::IsADirectory);

    let result = open(b"/home/foo/", OpenFlags::CREAT | OpenFlags::RDWR);
    assert_result_kind!(result, SysErrorKind::IsADirectory);

    let result = open(b"/home/foo", OpenFlags::RDWR);
    assert_result_kind!(result, SysErrorKind::NoSuchFileOrDirectory);

    unmoung_test_fs();
}

#[test_case]
pub fn test_open_dir_with_invalid_flags() {
    mount_test_fs();

    let cases = [
        // (open path, flags, expected error kind)
        ("/", OpenFlags::CREAT, SysErrorKind::InvalidArgument),
        (
            "/",
            OpenFlags::CREAT | OpenFlags::WRONLY,
            SysErrorKind::IsADirectory,
        ),
        ("/", OpenFlags::TRUNC, SysErrorKind::InvalidArgument),
        (
            "/",
            OpenFlags::TRUNC | OpenFlags::WRONLY,
            SysErrorKind::IsADirectory,
        ),
        ("/home/", OpenFlags::WRONLY, SysErrorKind::IsADirectory),
        ("/home/", OpenFlags::RDWR, SysErrorKind::IsADirectory),
        (
            "./",
            OpenFlags::CREAT | OpenFlags::RDWR,
            SysErrorKind::IsADirectory,
        ),
        (
            "/home/.",
            OpenFlags::CREAT | OpenFlags::RDWR,
            SysErrorKind::IsADirectory,
        ),
        (
            "/abc/.",
            OpenFlags::CREAT | OpenFlags::RDWR,
            SysErrorKind::IsADirectory,
        ),
        (
            "/abc/..",
            OpenFlags::CREAT | OpenFlags::RDWR,
            SysErrorKind::NoSuchFileOrDirectory,
        ),
    ];

    for (path, flags, err_kind) in cases {
        let result = open(path.as_bytes(), flags);
        assert_result_kind!(result, err_kind, "path<{}>, flags<{}>", path, flags);
    }

    unmoung_test_fs();
}

#[test_case]
pub fn test_rmdir_with_invalid_paths() {
    mount_test_fs();

    for path in INVALID_PATHS {
        let result = rmdir(path.as_bytes());
        assert_for_invalid_path!(result, path);
    }

    for path in NO_SUCH_FILES {
        let result = rmdir(path.as_bytes());
        assert_for_no_such!(result, path);
    }

    unmoung_test_fs();
}

#[test_case]
pub fn test_mkdir_with_invalid_paths() {
    mount_test_fs();

    for path in INVALID_PATHS {
        let result = mkdir(path.as_bytes());
        assert_for_invalid_path!(result, path);
    }

    let result = mkdir(b"/../..");
    assert_for_no_such!(result);

    unmoung_test_fs();
}

#[test_case]
pub fn test_mkdir_rmdir_basic() {
    mount_test_fs();

    mkdir(b"/tmp/test").unwrap();
    mkdir(b"/tmp/test/abc").unwrap();

    let fd0 = open(b"/tmp/test/abc/", OpenFlags::RDONLY).unwrap();
    let fd1 = open(b"/tmp/test/", OpenFlags::RDONLY).unwrap();
    close(fd0).unwrap();
    close(fd1).unwrap();

    let result = rmdir(b"/tmp/test");
    assert_result_kind!(result, SysErrorKind::NotEmpty);

    rmdir(b"tmp/test/abc").unwrap();
    rmdir(b"tmp/test/").unwrap();

    assert_result_kind!(
        open(b"/tmp/test/abc", OpenFlags::RDONLY),
        SysErrorKind::NoSuchFileOrDirectory
    );
    assert_result_kind!(
        open(b"/tmp/test/", OpenFlags::RDONLY),
        SysErrorKind::NoSuchFileOrDirectory
    );

    unmoung_test_fs();
}

#[test_case]
pub fn test_rmdir_and_getents() {
    mount_test_fs();

    mkdir(b"/tmp/test").unwrap();
    let fd = open(b"/tmp", OpenFlags::RDONLY).unwrap();
    assert!(dirents(fd).any(|dirent| dirent.name() == "test"));

    rmdir(b"/tmp/test").unwrap();
    assert!(!dirents(fd).any(|dirent| dirent.name() == "test"));
    close(fd).unwrap();

    unmoung_test_fs();
}

#[test_case]
pub fn test_unlink_basic() {
    mount_test_fs();

    let fd0 = open(b"/tmp/tmp0.test", OpenFlags::CREAT | OpenFlags::RDWR).unwrap();
    unlink(b"/tmp/tmp0.test").unwrap();
    close(fd0).unwrap();

    let result = open(b"/tmp/tmp0.test", OpenFlags::RDONLY);
    assert_for_no_such!(result);

    unmoung_test_fs();
}

#[test_case]
pub fn test_unlink_directory() {
    mount_test_fs();

    mkdir(b"/tmp/test").unwrap();
    let result = unlink(b"/tmp/test");
    assert_result_kind!(result, SysErrorKind::IsADirectory);

    unmoung_test_fs();
}

#[test_case]
pub fn test_create_on_existing() {
    mount_test_fs();

    mkdir(b"/tmp/test").unwrap();
    let err = mkdir(b"/tmp/test").unwrap_err();
    assert_eq!(err.kind(), SysErrorKind::AlreadyExists);
    rmdir(b"/tmp/test").unwrap();

    unmoung_test_fs();
}

#[test_case]
pub fn test_getdirents_on_file() {
    mount_test_fs();

    let fd = open(b"/tmp/file.txt", OpenFlags::CREAT | OpenFlags::RDWR).unwrap();
    let mut dirents = [];
    let result = get_dirents(fd, &mut dirents);
    assert_result_kind!(result, SysErrorKind::NotADirectory);
    close(fd).unwrap();

    unmoung_test_fs();
}
