//! The virtual memory layout:
//!
//! ``` table
//! +---------------+---------------+-------------+------------------------------------------+
//! | Start addr    | End addr      | Area Size   | Description                              |
//! +===============+===============+=============+==========================================+
//! | 0x1000_0000   | 0x1000_1000   | 4KB         | UART registers                           |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0x1000_1000   | 0x1000_2000   | 4KB         | Virtio mmio registers                    |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0x8000_0000   | 0xC000_0000   | 1GB         | Kernel code and data, which includes     |
//! |               |               |             | both text and rodata sections for the OS |
//! |               |               |             | kernel                                   |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0xC000_0000   | 0xD000_0000   | 256MB       | RISC-V plic                              |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0xD000_0000   | 0xD800_0000   | 128MB       | Direct maping of all phyical memory      |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0x2_0000_9000 | 0x2_0000_A000 | 4KB         | Task TrapFrame                           |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0x2_0008_0000 | 0x2_2008_0000 | 512MB       | User ELF(code, data, rodata...)          |
//! +---------------+---------------+-------------+------------------------------------------+
//! | 0x3_0000_0000 | 0x3_4000_0000 | 1GB         | User stack and heap                      |
//! +---------------+---------------+-------------+------------------------------------------+
//! ```

use crate::mmu::PGSIZE;

pub const KERNEL_START_VADDR: usize = 0;
pub const KERNEL_END_VADDR: usize = 0x1_4000_0000;

pub const UART_BASE_VADDR: usize = 0x1000_0000;
pub const UART_BASE_PADDR: usize = 0x1000_0000;
pub const UART_END_VADDR: usize = UART_BASE_VADDR + PGSIZE;

pub const VIRTIO_BASE_VADDR: usize = 0x1000_1000;
pub const VIRTIO_BASE_PADDR: usize = 0x1000_1000;
pub const VIRTIO_END_VADDR: usize = VIRTIO_BASE_VADDR + PGSIZE;

pub const KERNEL_ELF_BASE_VADDR: usize = 0x8000_0000;

pub const PLIC_BASE_VADDR: usize = 0xC000_0000;
pub const PLIC_END_VADDR: usize = PLIC_BASE_VADDR + 0x1000_0000;
pub const PLIC_BASE_PADDR: usize = 0xC00_0000;

pub const DIRECT_MAPPING_BASE_VADDR: usize = 0xD000_0000;
pub const DIRECT_MAPPING_SIZE: usize = 128 * 1024 * 1024;
pub const DIRECT_MAPPING_END_VADDR: usize = DIRECT_MAPPING_BASE_VADDR + DIRECT_MAPPING_SIZE;

#[no_mangle]
pub static TASK_TRAPFRAME_BASE: usize = 0x2_0000_9000;
pub const TASK_TRAPFRAME_END: usize = TASK_TRAPFRAME_BASE + PGSIZE;

pub const USER_ELF_BASE_VADDR: usize = 0x2_0008_0000;
pub const USER_ELF_SIZE: usize = 512 * 1024 * 102;
pub const USER_ELF_END_VADDR: usize = USER_ELF_BASE_VADDR + USER_ELF_SIZE;

pub const USER_BASE_VADDR: usize = 0x3_0000_0000;
pub const USER_AREA_SIZE: usize = 1024 * 1024 * 1024;
pub const USER_END_VADDR: usize = USER_BASE_VADDR + USER_AREA_SIZE;

pub const fn is_vmap_area(vaddr: usize) -> bool {
    vaddr >= 0x2_0000_0000
}

pub const fn is_from_user_elf(addr: usize) -> bool {
    addr >= USER_ELF_BASE_VADDR && addr < USER_ELF_END_VADDR
}

pub const fn is_from_user(addr: usize, size: usize) -> bool {
    addr >= USER_ELF_BASE_VADDR
        && if let Some(end_addr) = addr.checked_add(size) {
            end_addr <= USER_END_VADDR
        } else {
            false
        }
}
