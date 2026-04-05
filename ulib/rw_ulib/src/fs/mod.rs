use crate::{
    error::{wrap_with_result, Error, Result},
    io::Read,
    syscall::api::*,
};
use rw_ulib_types::fcntl::{Dirent, OpenFlags, Stat};

use alloc::{ffi::CString, string::String, vec::Vec};
use path::{Component, Path};
use syserr::SysErrorKind;

mod file;
pub use file::File;

#[derive(PartialEq, Eq, Clone, Copy)]
pub struct FileDiscriptor(pub u32);

pub(crate) fn cstr_path<P>(path: P) -> CString
where
    P: AsRef<Path>,
{
    CString::new(path.as_ref().as_bytes()).expect("CString::new failure")
}

/// Open a file with flags.
///
/// # Safety
///
/// Caller must ensure that the opened file should be closed as it is
/// unnecessary.
pub(self) unsafe fn open_fd<P>(path: P, flags: OpenFlags) -> Result<FileDiscriptor>
where
    P: AsRef<Path>,
{
    let cstr = cstr_path(path);
    let code = sys_open(&cstr, flags);
    let fd = wrap_with_result(code)?;
    Ok(FileDiscriptor(fd as u32))
}

pub fn mkdir<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let cstr = cstr_path(path);
    let code = sys_mkdir(&cstr);
    wrap_with_result(code)?;
    Ok(())
}

pub fn remove_dir<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let cstr = cstr_path(path);
    let code = sys_rmdir(&cstr);
    wrap_with_result(code)?;
    Ok(())
}

pub fn remove_file<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let cstr = cstr_path(path);
    let code = sys_unlink(&cstr);
    wrap_with_result(code)?;
    Ok(())
}

pub fn create_dir<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let cstr = cstr_path(path);
    let code = sys_mkdir(&cstr);
    wrap_with_result(code)?;
    Ok(())
}

pub fn create_dir_all<P>(path: P) -> Result<()>
where
    P: AsRef<Path>,
{
    let path = path.as_ref();
    match create_dir(path) {
        Err(Error::System(SysErrorKind::NoSuchFileOrDirectory)) => {
            if let Some(Component::ParentDir) = path.components().next_back() {
                return Err(Error::System(SysErrorKind::NoSuchFileOrDirectory));
            }
            if let Some(parent) = path.parent() {
                create_dir_all(parent)?;
                create_dir(path)
            } else {
                Err(Error::System(SysErrorKind::NoSuchFileOrDirectory))
            }
        }
        Ok(_) => Ok(()),
        Err(err) => Err(err),
    }
}

pub fn metadata<P>(path: P) -> Result<Stat>
where
    P: AsRef<Path>,
{
    let cstr = cstr_path(path);
    let mut stat = Stat::default();
    let code = sys_stat(&cstr, &mut stat);
    wrap_with_result(code)?;
    Ok(stat)
}

pub fn is_dir<P>(path: P) -> Result<bool>
where
    P: AsRef<Path>,
{
    Ok(metadata(path)?.is_dirctory())
}

pub fn exists<P>(path: P) -> Result<bool>
where
    P: AsRef<Path>,
{
    match metadata(path) {
        Err(Error::System(SysErrorKind::NoSuchFileOrDirectory)) => Ok(false),
        Ok(_) => Ok(true),
        Err(err) => Err(err),
    }
}

pub fn read<P>(path: P) -> Result<Vec<u8>>
where
    P: AsRef<Path>,
{
    let mut file = File::open(path)?;
    let mut buf = Vec::with_capacity(4096);
    file.read_to_end(&mut buf)?;
    Ok(buf)
}

pub fn read_str<P>(path: P) -> Result<String>
where
    P: AsRef<Path>,
{
    let bytes = read(path)?;
    String::from_utf8(bytes).map_err(|_| Error::System(SysErrorKind::NoSuchFileOrDirectory))
}

pub fn read_dir<P>(path: P) -> Result<ReadDirIter>
where
    P: AsRef<Path>,
{
    let dir = File::open(path)?;
    Ok(ReadDirIter::new(dir))
}

pub fn pipe() -> Result<[File; 2]> {
    let mut fds = [0; 2];
    let code = sys_pipe(&mut fds);
    wrap_with_result(code)?;
    Ok([File::with_fd(fds[0]), File::with_fd(fds[1])])
}

pub fn dup2(src_fd: FileDiscriptor, dst_fd: FileDiscriptor) -> Result<()> {
    let code = unsafe { sys_dup2(src_fd.0, dst_fd.0) };
    wrap_with_result(code)?;
    Ok(())
}

pub struct ReadDirIter {
    dir: File,
    buffer: [Dirent; 8],
    len: usize,
    p: usize,
}

impl ReadDirIter {
    #[must_use]
    pub fn new(dir: File) -> Self {
        Self {
            dir,
            buffer: Default::default(),
            len: 0,
            p: 0,
        }
    }
}

impl Iterator for ReadDirIter {
    type Item = Result<Dirent>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.p == self.len {
            self.len = 0;
            self.p = 0;
        }

        if self.len == 0 {
            let code = sys_getdirents(self.dir.raw_fd().0, &mut self.buffer);
            match wrap_with_result(code) {
                Ok(len) => {
                    if len == 0 {
                        return None;
                    }
                    self.len = len;
                }
                Err(err) => return Some(Err(err)),
            }
        }

        let dirent = self.buffer[self.p];
        self.p += 1;
        if dirent.name() == "." || dirent.name() == ".." {
            self.next()
        } else {
            Some(Ok(dirent))
        }
    }
}
