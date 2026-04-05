use core::fmt::{self, Arguments};

use super::error::Result;
use alloc::{string::String, vec::Vec};
use buffer::{BufferedReader, BufferedWriter};
use spin::Mutex;

use crate::{
    error,
    fs::{self, File, FileDiscriptor},
};

pub mod buffer;
pub mod ioctl;
pub mod print;

pub static STDIN: Stdin = Stdin::new();
pub static STDOUT: Stdout = Stdout::new();

#[must_use]
pub fn stdin() -> &'static Stdin {
    &STDIN
}

#[must_use]
pub fn stdout() -> &'static Stdout {
    &STDOUT
}

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize>;

    fn read_extact(&mut self, buf: &mut [u8]) -> Result<()> {
        let len = self.read(buf)?;
        if len != buf.len() {
            Err(error::Error::ReadExact)
        } else {
            Ok(())
        }
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> Result<()> {
        let mut tmp_buf = [0; 4096];
        loop {
            let len = self.read(&mut tmp_buf)?;
            if len == 0 {
                break;
            }
            buf.extend_from_slice(&tmp_buf[..len]);
        }
        Ok(())
    }
}

impl<T: Read> Read for &mut T {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        (self as &mut T).read(buf)
    }
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> Result<usize>;

    fn write_extact(&mut self, buf: &mut [u8]) -> Result<()> {
        let len = self.write(buf)?;
        if len != buf.len() {
            Err(error::Error::ReadExact)
        } else {
            Ok(())
        }
    }

    fn flush(&mut self) -> Result<()>;
}

impl<T: Write> Write for &mut T {
    fn write(&mut self, buf: &[u8]) -> Result<usize> {
        (self as &mut T).write(buf)
    }

    fn flush(&mut self) -> Result<()> {
        (self as &mut T).flush()
    }
}

pub struct Stdin {
    file: Mutex<BufferedReader<File>>,
}

impl Stdin {
    #[must_use]
    pub(self) const fn new() -> Self {
        Self {
            file: Mutex::new(BufferedReader::new(File::with_fd(0))),
        }
    }

    pub fn as_raw_fd(&self) -> FileDiscriptor {
        self.file.lock().inner().raw_fd()
    }

    pub fn replace_with(&self, file: &File) -> Result<()> {
        let fd = self.as_raw_fd();
        fs::dup2(file.raw_fd(), fd)
    }

    pub fn read_line(&self) -> Result<String> {
        let mut f = self.file.lock();
        let mut line = String::new();

        loop {
            let mut buf = [0; 1024];
            let len = f.read(&mut buf)?;
            if len == 0 {
                return Ok(line);
            }
            line.push_str(str::from_utf8(&buf[..len]).unwrap());
            let last = buf[len - 1];
            if last == b'\n' || last == 0 {
                return Ok(line);
            }
        }
    }
}

pub struct Stdout {
    inner: Mutex<StdoutInner>,
}

pub struct StdoutInner {
    file: BufferedWriter<File>,
    fmt_error: Option<crate::error::Error>,
}

impl fmt::Write for StdoutInner {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        if let Err(err) = self.file.write(s.as_bytes()) {
            self.fmt_error = Some(err);
            Err(fmt::Error)
        } else {
            Ok(())
        }
    }
}

impl Stdout {
    #[must_use]
    pub(self) const fn new() -> Self {
        Self {
            inner: Mutex::new(StdoutInner {
                file: BufferedWriter::new(File::with_fd(1)),
                fmt_error: None,
            }),
        }
    }

    pub fn as_raw_fd(&self) -> FileDiscriptor {
        self.inner.lock().file.inner().raw_fd()
    }

    pub fn replace_with(&self, file: &File) -> Result<()> {
        let fd = self.as_raw_fd();
        fs::dup2(file.raw_fd(), fd)
    }

    /// Print to standard out with arguments. This is useful for print macros.
    ///
    /// # Panics
    ///
    /// Panics when `print_fmt_err` returns an error.
    pub fn print_fmt<'a>(&self, args: Arguments<'a>) {
        self.print_fmt_err(args).expect("print_fmt error");
    }

    pub fn print_fmt_err<'a>(&self, args: Arguments<'a>) -> Result<()> {
        let mut inner = self.inner.lock();
        let error = fmt::Write::write_fmt(&mut *inner, args);
        if error.is_err() {
            Err(inner.fmt_error.take().unwrap())
        } else {
            inner.file.flush_buffer()?;
            Ok(())
        }
    }
}
