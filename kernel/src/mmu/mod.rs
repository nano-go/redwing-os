use core::mem::MaybeUninit;

use types::PhysicalPtr;
use types::{Page, PageAlignedUsize};

use crate::{
    arch::memlayout::{DIRECT_MAPPING_BASE_VADDR, DIRECT_MAPPING_END_VADDR, DIRECT_MAPPING_SIZE},
    params::{KERNEL_STACK_SIZE_PER_CPU, MAX_NCPU},
};

pub mod buddy;
pub mod heap;
pub mod pgtable;
pub mod slab;
pub mod types;
pub mod vm;
pub mod vm_area;

pub const PGSIZE: usize = 4096;

#[repr(C, align(4096))]
pub struct KernelStackAligned(pub [u8; KERNEL_STACK_SIZE_PER_CPU * MAX_NCPU]);

/// This provides stacks for initializing everything in kernel.
///
/// These stacks are also used by schedulers.
#[no_mangle]
pub static mut KERNEL_STACK: KernelStackAligned =
    KernelStackAligned([0; KERNEL_STACK_SIZE_PER_CPU * MAX_NCPU]);

/// Pages describe all physical pages in direct mapping memory area.
pub static mut PAGES: MaybeUninit<[Page; DIRECT_MAPPING_SIZE / PGSIZE]> = MaybeUninit::zeroed();

pub fn mmu_init() {
    for paddr in (DIRECT_MAPPING_BASE_VADDR..DIRECT_MAPPING_END_VADDR).step_by(PGSIZE) {
        let pg = paddr_to_page(paddr);
        unsafe { pg.as_ptr().write_volatile(Page::new(paddr / PGSIZE)) };
    }
    heap::init();
    buddy::init();
    slab::init();
    vm::init();
}

#[must_use]
#[inline]
pub fn paddr_to_page(paddr: usize) -> PhysicalPtr<Page> {
    debug_assert!(paddr % PGSIZE == 0);
    #[allow(static_mut_refs)]
    unsafe {
        PhysicalPtr::new_unchecked(
            &mut (*PAGES.as_mut_ptr())[(paddr - DIRECT_MAPPING_BASE_VADDR) >> 12],
        )
    }
}

#[must_use]
#[inline]
pub const fn align_up(val: usize, align: usize) -> usize {
    debug_assert!(align.is_power_of_two());
    (val + (align - 1)) & !(align - 1)
}

#[must_use]
#[inline]
pub const fn align_to_word(val: usize) -> usize {
    align_up(val, core::mem::size_of::<usize>())
}

#[inline]
#[must_use]
pub const fn pg_round_up(addr: usize) -> PageAlignedUsize {
    PageAlignedUsize::new_const((addr + PGSIZE - 1) & !(PGSIZE - 1))
}

#[inline]
#[must_use]
pub const fn pg_round_down(addr: usize) -> PageAlignedUsize {
    PageAlignedUsize::new_const(addr & !(PGSIZE - 1))
}

#[inline]
#[must_use]
pub const fn is_valid_phy_page_addr(addr: usize) -> bool {
    addr % PGSIZE == 0 && is_valid_phy_addr(addr)
}

#[inline]
#[must_use]
pub const fn is_valid_phy_addr(addr: usize) -> bool {
    addr >= DIRECT_MAPPING_BASE_VADDR && addr < DIRECT_MAPPING_END_VADDR
}

pub mod kernel_ld {
    //! This module provides accessing symbols where are defined in 'kernel.ld'.
    use core::ptr::addr_of;

    use crate::arch::memlayout::KERNEL_ELF_BASE_VADDR;

    extern "C" {
        static text_end: *mut u8;
        static rodata_begin: *mut u8;
        static rodata_end: *mut u8;
        static data_begin: *mut u8;
        static data_end: *mut u8;
        static bss_begin: *mut u8;
        static bss_end: *mut u8;
    }

    #[inline]
    #[must_use]
    pub fn start_of_text() -> usize {
        KERNEL_ELF_BASE_VADDR
    }

    #[inline]
    #[must_use]
    pub fn end_of_text() -> usize {
        addr_of!(text_end) as usize
    }

    #[inline]
    #[must_use]
    pub fn start_of_rodata() -> usize {
        addr_of!(rodata_begin) as usize
    }

    #[inline]
    #[must_use]
    pub fn end_of_rodata() -> usize {
        addr_of!(rodata_end) as usize
    }

    #[inline]
    #[must_use]
    pub fn start_of_data() -> usize {
        addr_of!(data_begin) as usize
    }

    #[inline]
    #[must_use]
    pub fn end_of_data() -> usize {
        addr_of!(data_end) as usize
    }

    #[inline]
    #[must_use]
    pub fn start_of_bss() -> usize {
        addr_of!(bss_begin) as usize
    }

    #[inline]
    #[must_use]
    pub fn end_of_bss() -> usize {
        addr_of!(bss_end) as usize
    }
}
