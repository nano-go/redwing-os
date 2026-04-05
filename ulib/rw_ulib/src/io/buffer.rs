use core::cmp;

use alloc::vec::Vec;

use super::{Read, Write};

pub struct BufferedReader<T> {
    inner: T,
    buffer: Vec<u8>,
    read_pos: usize,
    capacity: usize,
}

impl<T> BufferedReader<T> {
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self::with_capacity(inner, 4096)
    }

    #[must_use]
    pub const fn with_capacity(inner: T, capacity: usize) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            read_pos: 0,
            capacity,
        }
    }

    pub const fn inner(&self) -> &T {
        &self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    fn read_from_buffer(&mut self, buf: &mut [u8]) -> usize {
        if self.buffer.capacity() == 0 {
            self.buffer = Vec::with_capacity(self.capacity);
        }
        let bytes_read = cmp::min(buf.len(), self.buffer.len() - self.read_pos);
        buf[..bytes_read].copy_from_slice(&self.buffer[self.read_pos..self.read_pos + bytes_read]);
        self.read_pos += bytes_read;
        bytes_read
    }
}

impl<T: Read> BufferedReader<T> {
    pub fn refill_buffer(&mut self) -> crate::error::Result<usize> {
        if self.buffer.capacity() == 0 {
            self.buffer = Vec::with_capacity(self.capacity);
        }

        self.buffer.clear();
        self.read_pos = 0;
        unsafe {
            self.buffer.set_len(self.buffer.capacity());
            let len = self.inner.read(&mut self.buffer)?;
            self.buffer.set_len(len);
            Ok(len)
        }
    }
}

impl<T: Read> Read for BufferedReader<T> {
    fn read(&mut self, mut buf: &mut [u8]) -> crate::error::Result<usize> {
        let mut len = self.read_from_buffer(buf);
        if len == buf.len() {
            return Ok(len);
        }
        buf = &mut buf[len..];
        if self.refill_buffer()? == 0 {
            return Ok(0);
        }
        len += self.read_from_buffer(buf);
        Ok(len)
    }
}

pub struct BufferedWriter<T> {
    inner: T,
    buffer: Vec<u8>,
    capacity: usize,
}

impl<T> BufferedWriter<T> {
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self::with_capacity(inner, 4096)
    }

    #[must_use]
    pub const fn with_capacity(inner: T, capacity: usize) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
            capacity,
        }
    }

    pub const fn inner(&self) -> &T {
        &self.inner
    }

    pub fn into_inner(self) -> T {
        self.inner
    }

    fn write_to_buffer(&mut self, buf: &[u8]) -> usize {
        if self.buffer.capacity() == 0 {
            self.buffer = Vec::with_capacity(self.capacity);
        }
        let bytes_write = cmp::min(buf.len(), self.buffer.capacity() - self.buffer.len());
        self.buffer.extend_from_slice(&buf[..bytes_write]);
        bytes_write
    }
}

impl<T: Write> BufferedWriter<T> {
    pub fn flush_buffer(&mut self) -> crate::error::Result<usize> {
        if self.buffer.is_empty() {
            return Ok(0);
        }
        let len = self.inner.write(&self.buffer.as_slice())?;
        self.buffer.drain(..len);
        Ok(len)
    }
}

impl<T: Write> Write for BufferedWriter<T> {
    fn write(&mut self, mut buf: &[u8]) -> crate::error::Result<usize> {
        let mut len = 0;
        loop {
            let bytes_write = self.write_to_buffer(buf);
            len += bytes_write;
            if bytes_write == buf.len() {
                return Ok(len);
            }
            buf = &buf[bytes_write..];
            if self.flush_buffer()? == 0 {
                return Ok(len);
            }
        }
    }

    fn flush(&mut self) -> crate::error::Result<()> {
        self.flush_buffer()?;
        self.inner.flush()?;
        Ok(())
    }
}
