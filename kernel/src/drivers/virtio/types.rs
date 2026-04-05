use bitflags::bitflags;
use num_enum::FromPrimitive;

/// Enum representing the different types of VirtIO devices.
#[derive(Debug, Copy, Clone, PartialEq, Eq, FromPrimitive)]
#[repr(u32)]
pub enum VirtioDeviceType {
    Reserved = 0,
    Network = 1,
    Block = 2,
    Console = 3,
    EntropySource = 4,
    MemoryBallooningTraditional = 5,
    IoMemory = 6,
    Rpmsg = 7,
    ScsiHost = 8,
    NinePTransport = 9,
    Mac80211Wlan = 10,
    RprocSerial = 11,
    Caif = 12,
    MemoryBalloon = 13,
    Gpu = 16,
    TimerClock = 17,
    Input = 18,
    Socket = 19,
    Crypto = 20,
    SignalDistributionModule = 21,
    Pstore = 22,
    Iommu = 23,
    Memory = 24,
    Sound = 25,
    FileSystem = 26,
    Pmem = 27,
    Rpmb = 28,
    Mac80211Hwsim = 29,
    VideoEncoder = 30,
    VideoDecoder = 31,
    Scmi = 32,
    NitroSecureModule = 33,
    I2cAdapter = 34,
    Watchdog = 35,
    Can = 36,
    ParameterServer = 38,
    AudioPolicy = 39,
    Bluetooth = 40,
    Gpio = 41,
    Rdma = 42,
    Camera = 43,
    Ism = 44,
    SpiMaster = 45,
    #[num_enum(default)]
    Unknown,
}

bitflags! {
    #[derive(Clone, Copy)]
    pub struct DeviceStatus: u32 {
        /// Indicates that the guest OS has found the device and recognized it as a valid virtio
        /// device.
        const ACKNOWLEDGE = 1;

        /// Indicates that the guest OS knows how to drive the device.
        const DRIVER = 2;

        /// Indicates that something went wrong in the guest, and it has given up on the device.
        /// This could be an internal error, or the driver didn’t like the device for some reason,
        /// or even a fatal error during device operation.
        const FAILED = 128;

        /// Indicates that the driver has acknowledged all the features it understands, and feature
        /// negotiation is complete.
        const FEATURES_OK = 8;

        /// Indicates that the driver is set up and ready to drive the device.
        const DRIVER_OK = 4;

        /// Indicates that the device has experienced an error from which it can’t recover.
        const DEVICE_NEEDS_RESET = 64;
    }
}

/// The descriptor table refers to the buffers the driver is using for the
/// device. `addr` is a physical address, and the buffers can be chained via
/// `next`. Each descriptor describes a buffer which is read-only for the device
/// (“device-readable”) or write-only for the device (“device-writable”), but a
/// chain of descriptors can contain both device-readable and device-writable
/// buffers.
///
/// The actual contents of the memory offered to the device depends on
/// the device type. Most common is to begin the data with a header (containing
/// little-endian fields) for the device to read, and postfix it with a status
/// tailer for the device to write.
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct VirtqDesc {
    pub addr: u64,
    pub len: u32,
    pub flags: VirtqDescFlags,

    /// Next field if flags & NEXT
    pub next: u16,
}

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct VirtqDescFlags: u16 {
        /// This marks a buffer as continuing via the next field.
        const NEXT = 1;
        /// This marks a buffer as device write-only (otherwise device read-only).
        const WRITE = 2;
        /// This means the buffer contains a list of buffer descriptors.
        const INDIRECT = 4;
    }
}

impl VirtqDesc {
    /// Returns an empty descriptor. This function can be used to reset a
    /// descriptor.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            addr: 0,
            len: 0,
            flags: VirtqDescFlags::empty(),
            next: 0,
        }
    }
}

pub const VIRTQ_RING_SIZE: usize = 8;

/// The driver uses the available ring to offer buffers to the device: each ring
/// entry refers to the head of a descriptor chain. It is only written by the
/// driver and read by the device.
#[derive(Debug, Default)]
#[repr(C)]
pub struct VirtqAvail {
    /// If this is 1, the intterrupt disables.
    pub flags: u16,

    /// idx field indicates where the driver would put the next descriptor entry
    /// in the ring (modulo the queue size). This starts at 0, and
    /// increases.
    pub idx: u16,

    /// Descriptor numbers of chain heads.
    pub ring: [u16; VIRTQ_RING_SIZE],

    /// Only if VIRTIO_F_EVENT_IDX
    pub used_event: u16,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtqUsed {
    pub flags: u16,
    pub idx: u16,

    /// The used ring is where the device returns buffers once it is done with
    /// them: it is only written to by the device, and read by the driver.
    pub ring: [VirtqUsedElem; VIRTQ_RING_SIZE],

    /// Only if VIRTIO_F_EVENT_IDX
    pub avail_event: u16,
}

#[derive(Debug, Clone, Copy, Default)]
#[repr(C)]
pub struct VirtqUsedElem {
    /// Index of start of used descriptor chain.
    ///
    /// This indicates the head entry of the descriptor chain describing the
    /// buffer.
    pub id: u32,

    /// The number of bytes written into the device writable portion of the
    /// buffer described by the descriptor chain.
    pub len: u32,
}
