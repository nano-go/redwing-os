use path::Path;
use rw_ulib_types::fcntl::OpenFlags;

use crate::{
    error::{wrap_with_result, Result},
    io::{self},
    syscall::api::{sys_close, sys_read, sys_sync, sys_write},
};

use super::FileDiscriptor;

pub struct File {
    fd: FileDiscriptor,
}

impl File {
    #[must_use]
    pub(crate) const fn with_fd(fd: u32) -> Self {
        Self {
            fd: FileDiscriptor(fd),
        }
    }

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        Ok(Self {
            fd: unsafe { super::open_fd(path, OpenFlags::RDONLY) }?,
        })
    }

    pub fn with_flags<P: AsRef<Path>>(path: P, flags: OpenFlags) -> Result<Self> {
        Ok(Self {
            fd: unsafe { super::open_fd(path, flags) }?,
        })
    }

    #[must_use]
    pub fn raw_fd(&self) -> FileDiscriptor {
        self.fd
    }
}

impl Drop for File {
    fn drop(&mut self) {
        unsafe { sys_close(self.fd.0) };
    }
}

impl io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        let code = sys_read(self.fd.0, buf);
        let bytes_read = wrap_with_result(code)?;
        Ok(bytes_read)
    }
}

impl io::Write for File {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        let code = sys_write(self.fd.0, buf);
        let bytes_write = wrap_with_result(code)?;
        Ok(bytes_write)
    }

    fn flush(&mut self) -> Result<()> {
        wrap_with_result(unsafe { sys_sync() })?;
        Ok(())
    }
}
