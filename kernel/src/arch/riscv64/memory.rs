use crate::mmu::{pgtable::EntryPerm, vm::VM};

use super::memlayout::{
    DIRECT_MAPPING_BASE_VADDR, DIRECT_MAPPING_END_VADDR, PLIC_BASE_PADDR, PLIC_BASE_VADDR,
    PLIC_END_VADDR, UART_BASE_PADDR, UART_BASE_VADDR, UART_END_VADDR, VIRTIO_BASE_PADDR,
    VIRTIO_BASE_VADDR, VIRTIO_END_VADDR,
};

static VMAP_TABLE: [(usize, usize, usize, EntryPerm); 4] = [
    (
        UART_BASE_VADDR,
        UART_END_VADDR,
        UART_BASE_PADDR,
        EntryPerm::with_rw(),
    ),
    (
        VIRTIO_BASE_VADDR,
        VIRTIO_END_VADDR,
        VIRTIO_BASE_PADDR,
        EntryPerm::with_rw(),
    ),
    (
        PLIC_BASE_VADDR,
        PLIC_END_VADDR,
        PLIC_BASE_PADDR,
        EntryPerm::with_rw(),
    ),
    (
        DIRECT_MAPPING_BASE_VADDR,
        DIRECT_MAPPING_END_VADDR,
        DIRECT_MAPPING_BASE_VADDR,
        EntryPerm::with_rw(),
    ),
];

pub fn vm_init(kvm: &VM) {
    for (vbase, vend, paddr, perm) in VMAP_TABLE {
        kvm.kmap(vbase, paddr, vend - vbase, perm);
    }
}
