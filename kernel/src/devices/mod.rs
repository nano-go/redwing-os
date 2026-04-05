use core::any::Any;

use alloc::{sync::Arc, vec::Vec};
use hashbrown::HashMap;

use crate::{
    error::{KResult, SysErrorKind},
    sync::spin::{Once, Spinlock},
};

pub mod block;
// pub mod terminal;
pub mod terminal;
pub mod tty;

pub const NULL_DEV_NO: u32 = 2;
pub const ZERO_DEV_NO: u32 = 3;

pub static DEVICE_TABLE: Once<Spinlock<HashMap<u32, Arc<dyn Device>>>> = Once::new();

pub fn dev_init() {
    DEVICE_TABLE.call_once(|| Spinlock::new("dev_table", HashMap::new()));
    dev_register(Arc::new(NullDev {}));
    dev_register(Arc::new(ZeroDev {}));

    tty::init();
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceInfo {
    pub device_no: u32,
    pub name: &'static str,
    pub file_name: &'static str,
}

pub trait Device: Any + Send + Sync + 'static {
    fn info(&self) -> DeviceInfo;
    fn dev_read(&self, offset: u64, buf: &mut [u8]) -> KResult<u64>;
    fn dev_write(&self, offset: u64, buf: &[u8]) -> KResult<u64>;
}

impl Device for Arc<dyn Device> {
    fn info(&self) -> DeviceInfo {
        self.as_ref().info()
    }

    fn dev_read(&self, offset: u64, buf: &mut [u8]) -> KResult<u64> {
        self.as_ref().dev_read(offset, buf)
    }

    fn dev_write(&self, offset: u64, buf: &[u8]) -> KResult<u64> {
        self.as_ref().dev_write(offset, buf)
    }
}

pub fn dev_register(device: Arc<dyn Device>) {
    let dev_no = device.info().device_no;
    let old = DEVICE_TABLE.get().unwrap().lock().insert(dev_no, device);
    if let Some(old) = old {
        panic!(
            "The device no {dev_no} already has been registered. the replaced dev: {:?}",
            old.info(),
        )
    }
}

pub fn get_device(device_no: u32) -> KResult<Arc<dyn Device>> {
    DEVICE_TABLE
        .get()
        .unwrap()
        .lock()
        .get(&device_no)
        .cloned()
        .ok_or_else(|| SysErrorKind::NoSuchDev.into())
}

#[must_use]
pub fn get_all_devices() -> Vec<Arc<dyn Device>> {
    DEVICE_TABLE
        .get()
        .unwrap()
        .lock()
        .values()
        .map(Arc::clone)
        .collect()
}

pub struct NullDev;

impl Device for NullDev {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            device_no: NULL_DEV_NO,
            name: "null",
            file_name: "null",
        }
    }

    fn dev_read(&self, _offset: u64, _buf: &mut [u8]) -> KResult<u64> {
        Ok(0)
    }

    fn dev_write(&self, _offset: u64, _buf: &[u8]) -> KResult<u64> {
        Ok(0)
    }
}

pub struct ZeroDev;

impl Device for ZeroDev {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            device_no: ZERO_DEV_NO,
            name: "zero",
            file_name: "zero",
        }
    }

    fn dev_read(&self, _offset: u64, buf: &mut [u8]) -> KResult<u64> {
        buf.fill(0);
        Ok(buf.len() as u64)
    }

    fn dev_write(&self, _offset: u64, buf: &[u8]) -> KResult<u64> {
        Ok(buf.len() as u64)
    }
}
