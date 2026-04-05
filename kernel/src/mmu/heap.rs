use core::{
    alloc::{GlobalAlloc, Layout},
    cmp,
    ptr::{self, NonNull},
};

use alloc::alloc::Allocator;
use lazy_static::lazy_static;
use linked_list_allocator::LockedHeap;

use crate::params::KERNEL_HEAP_SIZE;

// Includes KMALLOC_xxx
use super::slab::*;

static mut KERNEL_HEAP_SPACE: [u8; KERNEL_HEAP_SIZE] = [0; KERNEL_HEAP_SIZE];

/// Older heap allocator, now a helper for allocating objects larger than 8192.
static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

pub fn init() {
    unsafe {
        #[allow(static_mut_refs)]
        HEAP_ALLOCATOR
            .lock()
            .init(KERNEL_HEAP_SPACE.as_mut_ptr(), KERNEL_HEAP_SIZE)
    };
}

lazy_static! {
    #[rustfmt::skip]
    static ref SLAB_ALLOCATORS: [&'static SlabAllocator; 30] = [
        &KMALLOC_8   /*   8 */, &KMALLOC_16  /*  16 */, &KMALLOC_32  /*  24 */,
        &KMALLOC_32  /*  32 */, &KMALLOC_64  /*  40 */, &KMALLOC_64  /*  48 */,
        &KMALLOC_64  /*  56 */, &KMALLOC_64  /*  64 */, &KMALLOC_96  /*  72 */,
        &KMALLOC_96  /*  80 */, &KMALLOC_96  /*  88 */, &KMALLOC_96  /*  96 */,
        &KMALLOC_128 /* 104 */, &KMALLOC_128 /* 112 */, &KMALLOC_128 /* 120 */,
        &KMALLOC_128 /* 128 */, &KMALLOC_192 /* 136 */, &KMALLOC_192 /* 144 */,
        &KMALLOC_192 /* 152 */, &KMALLOC_192 /* 160 */, &KMALLOC_192 /* 168 */,
        &KMALLOC_192 /* 176 */, &KMALLOC_192 /* 184 */, &KMALLOC_192 /* 192 */,

        // For larger objects.
        &KMALLOC_256, &KMALLOC_512, &KMALLOC_1024,
        &KMALLOC_2048, &KMALLOC_4096,&KMALLOC_8192
    ];
}

fn slab_allocator_for_sz(sz: usize) -> &'static SlabAllocator {
    if sz <= 192 {
        return SLAB_ALLOCATORS[(sz - 1) / 8];
    }

    let idx = (sz.next_power_of_two() >> 8).trailing_zeros();
    SLAB_ALLOCATORS[24 + idx as usize]
}

#[global_allocator]
pub static GLOBAL_HEAP_ALLOCATOR: GlobalHeapAllocator = GlobalHeapAllocator {};

pub struct GlobalHeapAllocator;

unsafe impl GlobalAlloc for GlobalHeapAllocator {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        if layout.size() > 8192 {
            return HEAP_ALLOCATOR.alloc(layout);
        }
        slab_allocator_for_sz(layout.size())
            .allocate(layout)
            .map(|ptr| ptr.as_ptr().cast())
            .unwrap_or(ptr::null_mut())
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        if layout.size() > 8192 {
            HEAP_ALLOCATOR.dealloc(ptr, layout);
            return;
        }
        slab_allocator_for_sz(layout.size()).deallocate(NonNull::new_unchecked(ptr), layout);
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: core::alloc::Layout,
        new_size: usize,
    ) -> *mut u8 {
        let new_layout = unsafe { Layout::from_size_align_unchecked(new_size, layout.align()) };

        if layout.size() > 8192 {
            if new_layout.size() > 8192 {
                return HEAP_ALLOCATOR.realloc(ptr, layout, new_size);
            }

            let new_ptr = self.alloc(new_layout);
            if !new_ptr.is_null() {
                ptr::copy_nonoverlapping(ptr, new_ptr, cmp::min(layout.size(), new_size));
                HEAP_ALLOCATOR.dealloc(ptr, layout);
            }
            return new_ptr;
        }

        let slab0 = slab_allocator_for_sz(layout.size());
        if new_layout.size() > 8192 {
            let new_ptr = self.alloc(new_layout);
            if !new_ptr.is_null() {
                ptr::copy_nonoverlapping(ptr, new_ptr, cmp::min(layout.size(), new_size));
                slab0.deallocate(NonNull::new_unchecked(ptr), layout);
            }
            return new_ptr;
        }

        let slab1 = slab_allocator_for_sz(new_layout.size());
        if ptr::eq(slab0, slab1) {
            return ptr;
        }

        if let Ok(new_ptr) = slab1.allocate(new_layout) {
            let new_ptr = new_ptr.as_ptr() as *mut u8;
            unsafe {
                ptr::copy_nonoverlapping(ptr, new_ptr, cmp::min(layout.size(), new_size));
                slab0.deallocate(NonNull::new_unchecked(ptr), layout);
            }
            new_ptr
        } else {
            ptr::null_mut()
        }
    }
}

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error: {:?}", layout)
}

#[cfg(test)]
mod tests {
    use super::slab_allocator_for_sz;

    use crate::mmu::slab::*;

    macro_rules! assert_slab {
        ($left:expr, $right:expr) => {
            let _left_slab = $left;
            let _right_slab = $right;
            assert!(
                core::ptr::eq(_left_slab, _right_slab),
                "slab left {}, slab right {}",
                _left_slab.name(),
                _right_slab.name()
            );
        };
    }

    #[test_case]
    pub fn test_slab_allocator_for_sz() {
        assert_slab!(slab_allocator_for_sz(2), &*KMALLOC_8);
        assert_slab!(slab_allocator_for_sz(86), &*KMALLOC_96);
        assert_slab!(slab_allocator_for_sz(128), &*KMALLOC_128);
        assert_slab!(slab_allocator_for_sz(129), &*KMALLOC_192);
        assert_slab!(slab_allocator_for_sz(175), &*KMALLOC_192);
        assert_slab!(slab_allocator_for_sz(192), &*KMALLOC_192);
        assert_slab!(slab_allocator_for_sz(193), &*KMALLOC_256);
        assert_slab!(slab_allocator_for_sz(256), &*KMALLOC_256);
        assert_slab!(slab_allocator_for_sz(258), &*KMALLOC_512);
        assert_slab!(slab_allocator_for_sz(512), &*KMALLOC_512);
        assert_slab!(slab_allocator_for_sz(520), &*KMALLOC_1024);
        assert_slab!(slab_allocator_for_sz(1024), &*KMALLOC_1024);
        assert_slab!(slab_allocator_for_sz(1025), &*KMALLOC_2048);
        assert_slab!(slab_allocator_for_sz(1900), &*KMALLOC_2048);
        assert_slab!(slab_allocator_for_sz(2048), &*KMALLOC_2048);
        assert_slab!(slab_allocator_for_sz(3000), &*KMALLOC_4096);
        assert_slab!(slab_allocator_for_sz(4096), &*KMALLOC_4096);
        assert_slab!(slab_allocator_for_sz(8000), &*KMALLOC_8192);
        assert_slab!(slab_allocator_for_sz(8192), &*KMALLOC_8192);
    }
}
