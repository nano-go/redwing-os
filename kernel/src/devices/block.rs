use alloc::sync::Arc;

use crate::drivers::virtio::block::{VirtioBlockDevice, VIRTIO_BLOCK_DEV};

pub type BlockDeviceImpl = VirtioBlockDevice;

pub fn get_block_device() -> Arc<BlockDeviceImpl> {
    VIRTIO_BLOCK_DEV.get().unwrap().clone()
}
