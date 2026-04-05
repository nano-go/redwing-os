/// This feature indicates that the device accepts arbitrary descriptor layouts
pub const VIRTIO_F_ANY_LAYOUT: u32 = 27;

/// Negotiating this feature indicates that the driver can use descriptors with
/// the VIRTQ_DESC_F_INDIRECT flag set.
pub const VIRTIO_F_INDIRECT_DESC: u32 = 28;

/// This feature enables the used_event and the avail_event fields.
pub const VIRTIO_F_EVENT_IDX: u32 = 29;

pub mod block {
    /// Maximum size of any single segment is in size_max.
    pub const VIRTIO_BLK_F_SIZE_MAX: u32 = 1;

    /// Maximum number of segments in a request is in seg_max.
    pub const VIRTIO_BLK_F_SEG_MAX: u32 = 2;

    /// Disk-style geometry specified in geometry.
    pub const VIRTIO_BLK_F_GEOMETRY: u32 = 4;

    /// Device is read-only.
    pub const VIRTIO_BLK_F_RO: u32 = 5;

    /// Block size of disk is in blk_size.
    pub const VIRTIO_BLK_F_BLK_SIZE: u32 = 6;

    /// Cache flush command support.
    pub const VIRTIO_BLK_F_FLUSH: u32 = 9;

    /// Device exports information on optimal I/O alignment.
    pub const VIRTIO_BLK_F_TOPOLOGY: u32 = 10;

    /// Device can toggle its cache between writeback and writethrough modes.
    pub const VIRTIO_BLK_F_CONFIG_WCE: u32 = 11;

    /// Device supports multiqueue.
    pub const VIRTIO_BLK_F_MQ: u32 = 12;

    /// Device can support discard command, maximum discard sectors size in
    /// max_discard_sectors and maximum discard segment number in
    /// max_discard_seg.
    pub const VIRTIO_BLK_F_DISCARD: u32 = 13;

    /// Device can support write zeroes command, maximum write zeroes sectors
    /// size in max_write_zeroes_sectors and maximum write zeroes segment
    /// number in max_write_zeroes_seg.
    pub const VIRTIO_BLK_F_WRITE_ZEROES: u32 = 14;

    /// Device supports providing storage lifetime information.
    pub const VIRTIO_BLK_F_LIFETIME: u32 = 15;

    /// Device supports secure erase command, maximum erase sectors count in
    /// max_secure_erase_sectors and maximum erase segment number in
    /// max_secure_erase_seg.
    pub const VIRTIO_BLK_F_SECURE_ERASE: u32 = 16;

    /// Device is a Zoned Block Device, that is, a device that follows the zoned
    /// storage device behavior that is also supported by industry standards
    /// such as the T10 Zoned Block Command standard (ZBC r05) or the
    /// NVMe(TM) NVM Express Zoned Namespace Command Set Specification 1.1b
    /// (ZNS). For brevity, these standard documents are referred as "ZBD
    /// standards" from this point on in the text.
    pub const VIRTIO_BLK_F_ZONED: u32 = 17;

    /// Device supports scsi packet commands.
    pub const VIRTIO_BLK_F_SCSI: u32 = 7;
}
