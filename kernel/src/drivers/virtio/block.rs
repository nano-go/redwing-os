//! The virtio block device is a simple virtual block device (ie. disk). Read
//! and write requests (and other exotic requests) are placed in one of its
//! queues, and serviced (probably out of order) by the device except where
//! noted.
//!
//! This mod provides the implementation of the `BlockDevice` trait for `efs`.

use core::{
    array,
    mem::{self},
    pin::Pin,
    ptr::{self},
};

use alloc::sync::Arc;
use log::{error, trace};
use num_enum::TryFromPrimitive;
use redwing_efs::{consts::block::BLOCK_SIZE, dev::BlockDevice};

use crate::{
    mmu::buddy::{BuddyAlloc, BuddyBox},
    sync::{
        condvar::BoolCondvar,
        spin::{Once, Spinlock},
        wait::WaitQueue,
    },
};

use super::{
    features::{block::*, *},
    mmio,
    types::{
        DeviceStatus, VirtioDeviceType, VirtqAvail, VirtqDesc, VirtqDescFlags, VirtqUsed,
        VIRTQ_RING_SIZE,
    },
};

/// Specified queue index of virtio block device, which read/write requests will
/// be tranforrmed.
const QUEUE_INDEX: u32 = 0;

pub static VIRTIO_BLOCK_DEV: Once<Arc<VirtioBlockDevice>> = Once::new();

/// Initialize the virtio block device.
pub fn virtio_block_device_init() {
    if !mmio::is_valid() || mmio::r_device_type() != VirtioDeviceType::Block {
        panic!("could not find virtio block device.");
    }

    let blk_dev = SharedVirtioBlockDevice::new();

    let mut status = DeviceStatus::empty();

    // Reset status.
    mmio::w_status(status);

    // tell device that our OS has found the device and known how to drive it.
    status |= DeviceStatus::ACKNOWLEDGE | DeviceStatus::DRIVER;
    mmio::w_status(status);

    // negotiate features.
    let mut features = mmio::r_device_features();
    features &= !(1 << VIRTIO_BLK_F_RO);
    features &= !(1 << VIRTIO_BLK_F_SCSI);
    features &= !(1 << VIRTIO_BLK_F_CONFIG_WCE);
    features &= !(1 << VIRTIO_BLK_F_MQ);
    features &= !(1 << VIRTIO_F_ANY_LAYOUT);
    features &= !(1 << VIRTIO_F_INDIRECT_DESC);
    features &= !(1 << VIRTIO_F_EVENT_IDX);
    mmio::w_driver_features(features);

    // tell device that feature negotiation is complete.
    status |= DeviceStatus::FEATURES_OK;
    mmio::w_status(status);

    // re-read to ensure that the FEATURES_OK is set.
    status = mmio::r_status();
    if !status.contains(DeviceStatus::FEATURES_OK) {
        panic!("virtio block FEATURES_OK unset.");
    }

    mmio::w_queue_sel(QUEUE_INDEX);
    if mmio::r_queue_ready() != 0 {
        panic!("virtio block should not be ready.");
    }

    let max = mmio::r_queue_size_max();
    if max == 0 {
        panic!("virtio block has no queue.");
    }
    if max < VIRTQ_RING_SIZE as u32 {
        panic!("virtio block max queue too short.");
    }

    // set queue size.
    mmio::w_queue_size(VIRTQ_RING_SIZE as u32);

    mmio::w_queue_desc(BuddyBox::as_ptr(&blk_dev.desc).addr() as u64);
    mmio::w_driver_desc(BuddyBox::as_ptr(&blk_dev.avail).addr() as u64);
    mmio::w_device_desc(BuddyBox::as_ptr(&blk_dev.used).addr() as u64);

    // queue is ready.
    mmio::w_queue_ready(1);

    // tell device that we are completely ready.
    status |= DeviceStatus::DRIVER_OK;
    mmio::w_status(status);
    mmio::w_interrupt_ack(0);

    VIRTIO_BLOCK_DEV.call_once(|| Arc::new(VirtioBlockDevice::new(blk_dev)));
}

pub fn virtio_blk_intr() {
    VIRTIO_BLOCK_DEV.get().unwrap().virtio_blk_intr();
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, TryFromPrimitive)]
#[repr(u32)]
enum VirtioBlkReqType {
    In = 0,
    Out = 1,
    Flush = 4,
    GetId = 8,
    GetLifeTime = 10,
    Discard = 11,
    WriteZeros = 13,
    SecureErase = 14,
}

/// The format of the first descriptor in a disk request.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct VirtioBlkReq {
    pub typ: VirtioBlkReqType,
    pub reserved: u32,

    /// The sector number indicates the offset (multiplied by 512) where the
    /// read or write is to occur. This field is unused and set to 0 for
    /// commands other than read, write and some zone operations.
    pub sector: u64,
}

impl Default for VirtioBlkReq {
    fn default() -> Self {
        Self {
            typ: VirtioBlkReqType::In,
            reserved: 0,
            sector: 0,
        }
    }
}

#[derive(Clone)]
struct Info {
    status: u8,
    wait_for_buf: Arc<BoolCondvar>,
}

impl Info {
    #[must_use]
    pub fn new() -> Self {
        Self {
            status: 0,
            wait_for_buf: Arc::new(BoolCondvar::new_debug("virtio_block", true)),
        }
    }

    pub fn reset(&mut self) {
        self.status = 0xff;
        self.wait_for_buf.reset();
    }
}

struct SharedVirtioBlockDevice {
    pub desc: BuddyBox<[VirtqDesc; VIRTQ_RING_SIZE]>,
    pub avail: BuddyBox<VirtqAvail>,
    pub used: BuddyBox<VirtqUsed>,
    pub used_idx: u16,
    pub free: [bool; VIRTQ_RING_SIZE],
    pub ops: [VirtioBlkReq; VIRTQ_RING_SIZE],
    pub info: [Info; VIRTQ_RING_SIZE],
}

unsafe impl Send for SharedVirtioBlockDevice {}

impl SharedVirtioBlockDevice {
    pub fn new() -> Self {
        Self {
            desc: BuddyBox::new_in([VirtqDesc::empty(); VIRTQ_RING_SIZE], BuddyAlloc {}),
            avail: BuddyBox::new_in(VirtqAvail::default(), BuddyAlloc {}),
            used: BuddyBox::new_in(VirtqUsed::default(), BuddyAlloc {}),
            used_idx: 0,
            free: [true; VIRTQ_RING_SIZE],
            ops: [VirtioBlkReq::default(); VIRTQ_RING_SIZE],
            info: array::from_fn(|_| Info::new()),
        }
    }

    /// Find a free discriptor, mark it non-free and return its index.
    pub fn alloc_desc(&mut self) -> Option<usize> {
        for (idx, is_free) in self.free.iter_mut().enumerate() {
            if *is_free {
                *is_free = false;
                return Some(idx);
            }
        }
        None
    }

    /// Mark a descriptor at `index` free.
    pub fn free_desc(&mut self, index: usize) {
        if index >= self.free.len() {
            panic!("free desc index out of bound: index {}", index);
        }
        if self.free[index] {
            panic!("free a freed desc.");
        }
        self.free[index] = true;
        self.desc[index] = VirtqDesc::empty();
    }

    /// Allocate three deacriptors. This is used to create a chain of
    /// descriptors for a read/write command.
    pub fn alloc3_desc(&mut self) -> Option<[usize; 3]> {
        let mut idx = [0; 3];
        for i in 0..3 {
            if let Some(desc_idx) = self.alloc_desc() {
                idx[i] = desc_idx;
            } else {
                for allocated_desc in &idx[..i] {
                    self.free_desc(*allocated_desc);
                }
                return None;
            }
        }
        Some(idx)
    }

    pub fn free_chain(&mut self, mut header_desc_idx: usize) {
        loop {
            let desc = self.desc[header_desc_idx];
            self.free_desc(header_desc_idx);

            if desc.flags.contains(VirtqDescFlags::NEXT) {
                header_desc_idx = desc.next as usize;
            } else {
                break;
            }
        }
    }

    /// Write the descriptor at `desc_idx` that holds the `write/read` request.
    pub fn setup_req_desc(&mut self, desc_idx: usize, next_idx: usize, sector: u64, write: bool) {
        let req = &mut self.ops[desc_idx];
        req.typ = if write {
            VirtioBlkReqType::Out
        } else {
            VirtioBlkReqType::In
        };
        req.reserved = 0;
        req.sector = sector;

        let req_addr = ptr::addr_of!(*req).addr() as u64;
        let desc = &mut self.desc[desc_idx];
        desc.addr = req_addr;
        desc.len = mem::size_of::<VirtioBlkReq>() as u32;
        desc.flags = VirtqDescFlags::NEXT;
        desc.next = next_idx as u16;
    }

    /// Write the descriptor at `desc_idx` that holds the readable/writable
    /// buffer.
    pub fn setup_data_desc(&mut self, desc_idx: usize, next_idx: usize, buf: &[u8], write: bool) {
        let buffer_addr = ptr::addr_of!(*buf).addr() as u64;
        let desc = &mut self.desc[desc_idx];
        desc.addr = buffer_addr;
        desc.len = BLOCK_SIZE as u32;
        desc.flags = if write {
            // The device reads the buffer.
            VirtqDescFlags::empty()
        } else {
            // The device prepares the data and then writes it into the buffer.
            VirtqDescFlags::WRITE
        };
        desc.flags |= VirtqDescFlags::NEXT;
        desc.next = next_idx as u16;
    }

    /// Write the descriptor that holds the status (device writes) of this
    /// command.
    ///
    /// The descriptor should be the last of a chain.
    pub fn setup_status_desc(&mut self, desc_idx: usize, head_idx: usize) {
        let info = &mut self.info[head_idx];
        info.reset();
        let status_addr = ptr::addr_of!(info.status).addr() as u64;
        let desc = &mut self.desc[desc_idx];
        desc.addr = status_addr;
        desc.len = 1;
        desc.flags = VirtqDescFlags::WRITE;
        desc.next = 0;
    }

    /// In order to read/write a block, we need to send a read/write command to
    /// the VIRTIO device. A command consists of a chain of descriptors.
    pub fn send_rw_command(
        &mut self,
        desc_idx: [usize; 3],
        block_id: usize,
        buf: &[u8],
        write: bool,
    ) -> Info {
        let sector_id = block_id * (BLOCK_SIZE / 512);

        let [idx0, idx1, idx2] = desc_idx;

        self.setup_req_desc(idx0, idx1, sector_id as u64, write);
        self.setup_data_desc(idx1, idx2, buf, write);
        self.setup_status_desc(idx2, idx0);

        {
            let avail = &mut *self.avail;
            // tell device `idx0` as header in our chain of descriptors.
            avail.ring[avail.idx as usize % VIRTQ_RING_SIZE] = idx0 as u16;
            // tell the device that another avail ring entry is available.
            avail.idx += 1;
        }

        let info = self.info[idx0].clone();

        trace!("virtio block: queue notify. ->");
        mmio::w_queue_notify(QUEUE_INDEX);

        info
    }
}

/// The implementation of the virtio block device.
pub struct VirtioBlockDevice {
    shared: Pin<Arc<Spinlock<SharedVirtioBlockDevice>>>,
    wait_for_alloc: WaitQueue,
}

impl VirtioBlockDevice {
    #[must_use]
    fn new(shared: SharedVirtioBlockDevice) -> Self {
        Self {
            shared: Pin::new(Arc::new(Spinlock::new("virtio block", shared))),
            wait_for_alloc: WaitQueue::with_name("wait_for_alloc"),
        }
    }

    /// Read from or write to the block at `block_id` to/from the buffer.
    ///
    /// # Safety
    ///
    /// Caller must ensure that the `buf` is muttable if the `write` flag is
    /// `false`(means read).
    unsafe fn disk_rw(&self, block_id: usize, buf: &[u8], write: bool) {
        // Allocate three descriptors for this request chain.
        let desc_idx = loop {
            let mut blk_dev = self.shared.lock_irq_save();
            if let Some(idx) = blk_dev.alloc3_desc() {
                break idx;
            } else {
                drop(blk_dev); // release lock.
                self.wait_for_alloc.wait();
            }
        };

        let info = {
            let mut blk_dev = self.shared.lock_irq_save();
            blk_dev.send_rw_command(desc_idx, block_id, buf, write)
        };

        // Wait for the device to say the request has handled.
        // See virtio_blk_intr.
        trace!("virtio block: wait a buffer.");
        info.wait_for_buf.wait();

        {
            let mut blk_dev = self.shared.lock_irq_save();
            blk_dev.free_chain(desc_idx[0]);
            self.wait_for_alloc.signal_all();
        }

        trace!("virtio block: disk_rw end. ->");
    }

    fn virtio_blk_intr(&self) {
        trace!("virtio block: intr occured.");

        let mut blk_dev = self.shared.lock_irq_save();
        mmio::w_interrupt_ack(mmio::r_interrupt_status() & 0x3);

        // The device incraments blk_dev.used().idx when it adds an entry to the used
        // ring.
        while blk_dev.used.idx != blk_dev.used_idx {
            trace!("virtio block: handle buffer.");
            let id = blk_dev.used.ring[blk_dev.used_idx as usize % VIRTQ_RING_SIZE].id;

            let info = &blk_dev.info[id as usize];
            if info.status != 0 {
                error!("virtio block: invalid status: {:#x}", info.status);
            }

            // Tell task the request has handled.
            info.wait_for_buf.notify();
            trace!("virtio block: handle buffer end.");

            blk_dev.used_idx += 1;
        }

        self.wait_for_alloc.signal_all();
    }
}

impl BlockDevice for VirtioBlockDevice {
    fn read_block(&self, blk_no: u64, buf: &mut [u8]) -> Result<(), &'static str> {
        unsafe { self.disk_rw(blk_no as usize, buf, false) };
        Ok(())
    }

    fn write_block(&self, blk_no: u64, buf: &[u8]) -> Result<(), &'static str> {
        unsafe { self.disk_rw(blk_no as usize, buf, true) };
        Ok(())
    }
}
