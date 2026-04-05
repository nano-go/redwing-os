use core::alloc::GlobalAlloc;

use linked_list_allocator::LockedHeap;

use crate::{error::wrap_with_result, syscall::api::sys_brk};

#[global_allocator]
static LIB_ALLOCATOR: LibAlloc = LibAlloc;

static HEAP_ALLOCATOR: LockedHeap = LockedHeap::empty();

#[alloc_error_handler]
pub fn handle_alloc_error(layout: core::alloc::Layout) -> ! {
    panic!("Heap allocation error: {:?}", layout)
}

pub fn init() {
    let old_ptr = sbrk(4096 * 32).unwrap();
    unsafe {
        #[allow(static_mut_refs)]
        HEAP_ALLOCATOR.lock().init(old_ptr, 4096 * 32)
    };
}

pub struct LibAlloc;

unsafe impl GlobalAlloc for LibAlloc {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        let ptr = HEAP_ALLOCATOR.alloc(layout);
        while ptr.is_null() {
            if let Err(_) = sbrk(4096 * 32) {
                return core::ptr::null_mut();
            }
            HEAP_ALLOCATOR.lock().extend(4096 * 32);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        HEAP_ALLOCATOR.dealloc(ptr, layout);
    }
}

pub fn sbrk(increment: isize) -> crate::error::Result<*mut u8> {
    let old_ptr = brk(0)?;
    brk(unsafe { old_ptr.offset(increment).addr() })?;
    Ok(old_ptr)
}

pub fn brk(ptr: usize) -> crate::error::Result<*mut u8> {
    let old_ptr = wrap_with_result(unsafe { sys_brk(ptr) })?;
    Ok(old_ptr as *mut u8)
}
