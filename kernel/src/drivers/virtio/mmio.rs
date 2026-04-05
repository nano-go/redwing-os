//! Defines operations for virtio mmio device registers, mapped starting at
//! 0x10001000.
//!
//! See the virtio spec: [virtio-v1.3#4.2]
//!
//! [virtio-v1.3#4.2]: https://docs.oasis-open.org/virtio/virtio/v1.3/csd01/virtio-v1.3-csd01.html#x1-1820002

use crate::{arch::memlayout::VIRTIO_BASE_VADDR, drivers::virtio::types::VirtioDeviceType};

use super::types::DeviceStatus;

/// Checks whether the magic number and the version are both valid.
pub fn is_valid() -> bool {
    r_magic_number() == 0x74726976 && r_version() == 2
}

fn w_reg(offset: usize, value: u32) {
    unsafe {
        *((VIRTIO_BASE_VADDR + offset) as *mut u32) = value;
    }
}

fn r_reg(offset: usize) -> u32 {
    unsafe { *((VIRTIO_BASE_VADDR + offset) as *mut u32) }
}

/// Magic number.
/// It should be 0x74726976 (a Little Endian equivalent of the "virt" string).
pub fn r_magic_number() -> u32 {
    const MAGIC_VALUE_OFFSET: usize = 0x000;
    r_reg(MAGIC_VALUE_OFFSET)
}

/// Device version number.
/// 0x2. Note: Legacy devices used 0x1.
pub fn r_version() -> u32 {
    const VERSION_OFFSET: usize = 0x004;
    r_reg(VERSION_OFFSET)
}

/// Virtio Subsystem Device ID.
pub fn r_device_id() -> u32 {
    const DEVICE_ID_OFFSET: usize = 0x008;
    r_reg(DEVICE_ID_OFFSET)
}

/// Same with [`r_device_id`]. This reads the `Virtio Subsystem Device ID` and
/// converts it into the [`VirtioDeviceType`].
pub fn r_device_type() -> VirtioDeviceType {
    VirtioDeviceType::from(r_device_id())
}

/// Vendor ID register.
pub fn r_vendor_id() -> u32 {
    const VENDOR_ID_OFFSET: usize = 0x00c;
    r_reg(VENDOR_ID_OFFSET)
}

/// Flags representing features the device supports
/// Reading from this register returns 32 consecutive flag bits, the least
/// significant bit depending on the last value written to DeviceFeaturesSel.
/// Access to this register returns bits DeviceFeaturesSel ∗ 32 to
/// (DeviceFeaturesSel ∗ 32) + 31, eg. feature bits 0 to 31 if DeviceFeaturesSel
/// is set to 0 and features bits 32 to 63 if DeviceFeaturesSel is set to 10
pub fn r_device_features() -> u32 {
    const DEVICE_FEATURES_OFFSET: usize = 0x010;
    r_reg(DEVICE_FEATURES_OFFSET)
}

/// Device (host) features word selection.
/// Writing to this register selects a set of 32 device feature bits accessible
/// by reading from DeviceFeatures.
pub fn w_device_features_sel(value: u32) {
    const DEVICE_FEATURES_SEL_OFFSET: usize = 0x014;
    w_reg(DEVICE_FEATURES_SEL_OFFSET, value);
}

/// Flags representing device features understood and activated by the driver.
/// Writing to this register sets 32 consecutive flag bits, the least
/// significant bit depending on the last value written to DriverFeaturesSel.
/// Access to this register sets bits DriverFeaturesSel ∗ 32 to
/// (DriverFeaturesSel ∗ 32) + 31, eg. feature bits 0 to 31 if DriverFeaturesSel
/// is set to 0 and features bits 32 to 63 if DriverFeaturesSel is set to 1.
pub fn w_driver_features(value: u32) {
    const DRIVER_FEATURES_OFFSET: usize = 0x020;
    w_reg(DRIVER_FEATURES_OFFSET, value);
}

/// Activated (guest) features word selection.
/// Writing to this register selects a set of 32 activated feature bits
/// accessible by writing to DriverFeatures.
pub fn w_driver_features_sel(value: u32) {
    const DRIVER_FEATURES_SEL_OFFSET: usize = 0x024;
    w_reg(DRIVER_FEATURES_SEL_OFFSET, value);
}

/// Virtqueue index.
/// Writing to this register selects the virtqueue that the following operations
/// on QueueSizeMax, QueueSize, QueueReady, QueueDescLow, QueueDescHigh,
/// QueueDriverlLow, QueueDriverHigh, QueueDeviceLow, QueueDeviceHigh and
/// QueueReset apply to.
pub fn w_queue_sel(value: u32) {
    const QUEUE_SEL_OFFSET: usize = 0x030;
    w_reg(QUEUE_SEL_OFFSET, value);
}

/// Maximum virtqueue size.
/// Reading from the register returns the maximum size of the queue the device
/// is ready to process or zero (0x0) if the queue is not available. This
/// applies to the queue selected by writing to QueueSel and is allowed only
/// when QueuePFN is set to zero (0x0), so when the queue is not actively used.
/// Note: QueueSizeMax was previously known as QueueNumMax.
pub fn r_queue_size_max() -> u32 {
    const QUEUE_SIZE_MAX_OFFSET: usize = 0x034;
    r_reg(QUEUE_SIZE_MAX_OFFSET)
}

/// Virtqueue size.
/// Queue size is the number of elements in the queue. Writing to this register
/// notifies the device what size of the queue the driver will use. This applies
/// to the queue selected by writing to QueueSel. Note: QueueSize was previously
/// known as QueueNum.
pub fn w_queue_size(value: u32) {
    const QUEUE_SIZE_OFFSET: usize = 0x038;
    w_reg(QUEUE_SIZE_OFFSET, value);
}

/// Reads from the queue ready register.
/// Notifies the device that the selected queue is ready for use.
pub fn r_queue_ready() -> u32 {
    const QUEUE_READY_OFFSET: usize = 0x044;
    r_reg(QUEUE_READY_OFFSET)
}

/// Writes to the queue ready register.
/// Notifies the device that the selected queue is ready for use.
pub fn w_queue_ready(value: u32) {
    const QUEUE_READY_OFFSET: usize = 0x044;
    w_reg(QUEUE_READY_OFFSET, value);
}

/// Queue notifier
/// Writing a value to this register notifies the device that there are new
/// buffers to process in a queue.
/// When VIRTIO_F_NOTIFICATION_DATA has not been negotiated, the value written
/// is the queue index. When VIRTIO_F_NOTIFICATION_DATA has been negotiated, the
/// Notification data value has the following format:
///
/// ``` no_rust
/// le32 {
///     vq_index: 16; /* previously known as vqn */
///     next_off : 15;
///     next_wrap : 1;
/// };
/// ```
pub fn w_queue_notify(value: u32) {
    const QUEUE_NOTIFY: usize = 0x050;
    w_reg(QUEUE_NOTIFY, value);
}

/// Interrupt status.
/// Reading from this register returns a bit mask of events that caused the
/// device interrupt to be asserted. The following events are possible:
///
/// Used Buffer Notification
/// - bit 0 - the interrupt was asserted because the device has used a buffer in
///   at least one of the active virtqueues.
///
/// Configuration Change Notification
/// - bit 1 - the interrupt was asserted because the configuration of the device
///   has changed.
pub fn r_interrupt_status() -> u32 {
    const INTERRUPT_STATUS_OFFSET: usize = 0x060;
    r_reg(INTERRUPT_STATUS_OFFSET)
}

/// Interrupt acknowledge
/// Writing a value with bits set as defined in InterruptStatus to this register
/// notifies the device that events causing the interrupt have been handled.
pub fn w_interrupt_ack(value: u32) {
    const INTERRUPT_ACK_OFFSET: usize = 0x064;
    w_reg(INTERRUPT_ACK_OFFSET, value);
}

/// Reading from status register returns the current device status flags.
pub fn r_status() -> DeviceStatus {
    const STATUS_OFFSET: usize = 0x070;
    DeviceStatus::from_bits(r_reg(STATUS_OFFSET)).unwrap()
}

/// Writing non-zero values to this register sets the status flags, indicating
/// the driver progress. Writing zero (0x0) to this register triggers a device
/// reset.
pub fn w_status(value: DeviceStatus) {
    const STATUS_OFFSET: usize = 0x070;
    w_reg(STATUS_OFFSET, value.bits());
}

/// Writing to the virtqueue's descriptor 64 bit long physical address notifies
/// the the device about location of the Descriptor Area of the queue selected
/// by writing to QueueSel register.
pub fn w_queue_desc(addr: u64) {
    w_queue_desc_low(addr as u32);
    w_queue_desc_high((addr >> 32) as u32);
}

/// For the lower 32 bits of [`w_queue_desc`]
pub fn w_queue_desc_low(value: u32) {
    const QUEUE_DESC_LOW: usize = 0x80;
    w_reg(QUEUE_DESC_LOW, value);
}

/// For the higher 32 bits of [`w_queue_desc`]
pub fn w_queue_desc_high(value: u32) {
    const QUEUE_DESC_HIGH: usize = 0x84;
    w_reg(QUEUE_DESC_HIGH, value);
}

/// Writing to the virtqueue's descriptor 64 bit long physical address notifies
/// the the device about location of the Driver Area of the queue selected by
/// writing to QueueSel register.
pub fn w_driver_desc(value: u64) {
    w_driver_desc_low(value as u32);
    w_driver_desc_high((value >> 32) as u32);
}

/// For the lower 32 bits of [`w_driver_desc`]
pub fn w_driver_desc_low(value: u32) {
    const QUEUE_DRIVER_LOW: usize = 0x90;
    w_reg(QUEUE_DRIVER_LOW, value);
}

/// For the higher 32 bits of [`w_driver_desc`]
pub fn w_driver_desc_high(value: u32) {
    const QUEUE_DRIVER_HIGH: usize = 0x94;
    w_reg(QUEUE_DRIVER_HIGH, value);
}

/// Writing to the virtqueue's descriptor 64 bit long physical address notifies
/// the the device about location of the Device Area of the queue selected by
/// writing to QueueSel register.
pub fn w_device_desc(addr: u64) {
    w_device_desc_low(addr as u32);
    w_device_desc_high((addr >> 32) as u32);
}

/// For the lower 32 bits of [`w_device_desc`]
pub fn w_device_desc_low(value: u32) {
    const QUEUE_DEVICE_LOW: usize = 0xa0;
    w_reg(QUEUE_DEVICE_LOW, value);
}

/// For the higher 32 bits of [`w_device_desc`]
pub fn w_device_desc_high(value: u32) {
    const QUEUE_DEVICE_HIGH: usize = 0xa4;
    w_reg(QUEUE_DEVICE_HIGH, value);
}
