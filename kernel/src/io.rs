use crate::error::KResult;

pub trait Read {
    fn read(&mut self, buf: &mut [u8]) -> KResult<usize>;
}

pub trait Write {
    fn write(&mut self, buf: &[u8]) -> KResult<usize>;
    fn flush(&mut self) -> KResult<()>;
}

pub trait SyncRead: Send + Sync {
    fn read(&self, buf: &mut [u8]) -> KResult<usize>;
}

pub trait SyncWrite: Send + Sync {
    fn write(&self, buf: &[u8]) -> KResult<usize>;
    fn flush(&self) -> KResult<()>;
}
