//! A buddy memory allocator for managing physical memory in page-sized blocks.
//!
//! This allocator supports allocation and deallocation of memory blocks whose
//! sizes are powers of two. Internally, it uses a buddy system to expand and
//! merge memory blocks efficiently.
//!
//! The buddy allocator consists of:
//!
//! * `Page`: The page frame(block) representation. See
//!   [`crate::mmu::types::Page`].
//!
//! * `PageCacheList`: A per-cpu cached page list is used to accelarate
//!   allocating/deallocating 4KB-pages.
//!
//! * `FreeArea`: A struct contains a linked list linking all page frames in the
//!   free area. The size of a page frame is `2^order*PGSIZE`.
//!
//! * `BuddyAllocatorImpl`: contains `FreeArea`s and provides algorithm that
//!   allocating/deallocating page frames.
//!
//! This module provides also a [`BuddyAllocatorState`] that is useful for
//! testing, debug and providing the content of the file at `/proc/buddyinfo`.

use core::{fmt::Display, ptr::NonNull};

use alloc::{alloc::Allocator, boxed::Box};
use lazy_static::lazy_static;

use crate::{
    arch::memlayout::{DIRECT_MAPPING_BASE_VADDR, DIRECT_MAPPING_END_VADDR},
    params::MAX_NCPU,
    sync::{percpu::PerCpu, spin::Spinlock},
};

use super::{
    paddr_to_page,
    types::{Page, PageLinkAdapter},
    types::{PageAlignedUsize, PhysicalPtr},
    PGSIZE,
};

pub type BuddyBox<T> = Box<T, BuddyAlloc>;

pub const MAX_ORDER: usize = 9;

const PAGE_CACHE_LIST_LIMIT: usize = 64;
const PAGE_CACHE_LIST_BATCHCOUNT: usize = 16;

lazy_static! {
    static ref PAGE_CACHE_LIST: PerCpu<PageCacheList> = PerCpu::from_fn(|_| PageCacheList::new());
}

lazy_static! {
    static ref BUDDY_ALLOCATOR: Spinlock<BuddyAllocatorImpl> =
        Spinlock::new("buddy", BuddyAllocatorImpl::default());
}

/// Initializes the global buddy allocator.
pub fn init() {
    let mut buddy = BUDDY_ALLOCATOR.lock_irq_save();
    unsafe {
        buddy.init(
            PageAlignedUsize::new_const(DIRECT_MAPPING_BASE_VADDR),
            PageAlignedUsize::new_const(DIRECT_MAPPING_END_VADDR),
        );
    }
}

#[derive(Clone)]
pub struct BuddyAlloc {}

unsafe impl Send for BuddyAlloc {}
unsafe impl Sync for BuddyAlloc {}

unsafe impl Allocator for BuddyAlloc {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<core::ptr::NonNull<[u8]>, alloc::alloc::AllocError> {
        let size = layout.size();
        let order = find_order(size);
        unsafe { alloc_by_order(order).ok_or(alloc::alloc::AllocError {}) }
    }

    unsafe fn deallocate(&self, ptr: core::ptr::NonNull<u8>, layout: core::alloc::Layout) {
        let size = layout.size();
        let order = find_order(size);
        free_by_order(ptr, order);
    }
}

/// Returns the smallest order that can satisfy the given `size`.
///
/// For example:
///
/// ```no_run
/// find_order(4095) // -> order 0 (PGSIZE)
/// find_order(5126) // -> order 1 (PGSIZE*2)
/// find_order(9900) // -> order 2 (PGSIZE*4)
/// ```
#[must_use]
#[inline]
const fn find_order(size: usize) -> usize {
    size.div_ceil(PGSIZE).next_power_of_two().trailing_zeros() as usize
}

/// Allocates a single page(4KB memory).
///
/// # Safety
///
/// Caller must ensure that the returned pointer can be deallocated by
/// [`free_page`].
///
/// The allocated page may not be initialized.
#[must_use = "The allocated page must be used because you need to free it by `free_page` at least."]
#[inline]
pub unsafe fn alloc_page() -> Option<NonNull<[u8]>> {
    alloc_by_order(0)
}

/// Allocates a zero-initialized page. See [`alloc_page`] for more details.
///
/// # Safety
///
/// Caller must ensure that the returned pointer can be deallocated by
/// [`free_page`].
#[must_use = "The allocated page must be used because you need to free it by `free_page` at least."]
#[inline]
pub unsafe fn alloc_zeroed_page() -> Option<NonNull<[u8]>> {
    if let Some(mut ptr) = alloc_page() {
        ptr.as_mut().fill(0);
        Some(ptr)
    } else {
        None
    }
}

/// Frees a previously allocated single page.
///
/// # Safety
///
/// Caller must ensure that the memory of the page is allocated by
/// [`alloc_page`] or [`alloc_zeroed_page`].
#[inline]
pub unsafe fn free_page(ptr: NonNull<u8>) {
    free_by_order(ptr, 0);
}

/// This function attempts to allocate a memory block whose size is `PGSIZE *
/// (2^order)`.
///
/// # Arguments
///
/// * `order`: The order of the memory block to allocate. The size of the block
///   will be `PGSIZE * (1 << order)`. Must be less than `MAX_ORDER`.
///
/// # Safety
///
/// This function is `unsafe` for the following reasons:
/// * The caller must ensure that the returned `NonNull<[u8]>` pointer, if
///   `Some`, is eventually deallocated using `free_by_order` with the *correct*
///   `order` to prevent memory leaks or corruption.
/// * The allocated memory is uninitialized. The caller is responsible for
///   initializing it before use.
/// * This function manipulates raw pointers and global allocator state, which
///   requires careful handling to maintain memory safety.
///
/// # Panics
///
/// Panics if `order` is greater than or equal to `MAX_ORDER`.
#[must_use]
#[inline]
pub unsafe fn alloc_by_order(order: usize) -> Option<NonNull<[u8]>> {
    assert!(order < MAX_ORDER);
    if order == 0 {
        PAGE_CACHE_LIST.lock_irq_save().alloc_page()
    } else {
        BUDDY_ALLOCATOR.lock_irq_save().alloc_by_order(order)
    }
}

/// Frees a previously allocated memory block of a specific order.
///
/// # Arguments
///
/// * `ptr`: A `NonNull<u8>` pointer to the beginning of the memory block to
///   free. This pointer *must* have been previously returned by a call to
///   `alloc_by_order` or `alloc_page` (for order 0) with the corresponding
///   `order`.
/// * `order`: The order of the memory block being freed. This must match the
///   `order` used during its allocation. Must be less than `MAX_ORDER`.
///
/// # Safety
///
/// This function is `unsafe` for the following reasons:
/// * The caller must ensure that `ptr` is a valid pointer to a memory block
///   that was previously allocated by this allocator and is *not* currently in
///   use.
/// * The caller must ensure that the provided `order` precisely matches the
///   `order` of the block when it was allocated. Incorrect `order` will lead to
///   memory corruption (e.g., double-frees, freeing unallocated memory,
///   incorrect merges).
/// * Double-freeing the same block will lead to undefined behavior and likely
///   memory corruption.
/// * This function manipulates raw pointers and global allocator state,
///   requiring careful handling.
///
/// # Panics
///
/// Panics if `order` is greater than or equal to `MAX_ORDER`.
#[inline]
pub unsafe fn free_by_order(ptr: NonNull<u8>, order: usize) {
    assert!(order < MAX_ORDER);
    if order == 0 {
        PAGE_CACHE_LIST.lock_irq_save().dealloc_page(ptr)
    } else {
        BUDDY_ALLOCATOR.lock_irq_save().free_by_order(ptr, order)
    }
}

#[derive(Default)]
struct FreeArea {
    /// An intrusive linked list links all free blocks(page frames).
    ///
    /// See also: [`Page`]
    list: intrusive_collections::LinkedList<PageLinkAdapter>,

    /// The number of free blocks.
    nr_free: usize,
}

impl FreeArea {
    #[inline]
    pub fn push_front(&mut self, mut pgf: PhysicalPtr<Page>) {
        debug_assert!(!pgf.is_free());
        debug_assert!(!pgf.link.is_linked());
        pgf.set_free();
        self.list.push_front(pgf);
        self.nr_free += 1;
    }

    #[inline]
    pub fn pop_front(&mut self) -> Option<PhysicalPtr<Page>> {
        if let Some(mut pgf) = self.list.pop_front() {
            debug_assert!(pgf.is_free());
            pgf.clear_free();
            self.nr_free -= 1;
            Some(pgf)
        } else {
            None
        }
    }

    /// Splits the block at the front of linked list into two sub-blocks.
    #[must_use]
    pub fn expand(&mut self, order: usize) -> Option<(PhysicalPtr<Page>, PhysicalPtr<Page>)> {
        debug_assert_ne!(order, 0);
        let block_size = 1 << (order + 12);
        self.pop_front().map(|pgf| {
            let mut pgf1 = pgf;
            let mut pgf2 = paddr_to_page(pgf.paddr() + block_size / 2);
            pgf1.clear_free();
            pgf2.clear_free();
            (pgf1, pgf2)
        })
    }

    /// Try to merge the page frame at the front of linked list and its buddy
    /// page frame.
    ///
    /// Return the merged page frame if the buddy page frame is free.
    pub fn merge(&mut self, start_address: usize, order: usize) -> Option<PhysicalPtr<Page>> {
        let head_addr = self.list.front().get()?.paddr();
        let buddy_addr = Self::get_buddy_addr(start_address, head_addr, order);

        let buddy_pgf = paddr_to_page(buddy_addr);
        if buddy_pgf.pgf_order() != order as u8 || !buddy_pgf.is_free() {
            return None;
        }

        let mut pgf1 = paddr_to_page(head_addr);
        let mut pgf2 = buddy_pgf;

        unsafe {
            self.list
                .cursor_mut_from_ptr(pgf1.as_ptr())
                .remove()
                .unwrap();

            self.list
                .cursor_mut_from_ptr(pgf2.as_ptr())
                .remove()
                .unwrap();
        }
        self.nr_free -= 2;

        pgf1.clear_free();
        pgf2.clear_free();

        if pgf1.paddr() < pgf2.paddr() {
            Some(pgf1)
        } else {
            Some(pgf2)
        }
    }

    #[must_use]
    #[inline]
    fn get_buddy_addr(start_address: usize, addr: usize, order: usize) -> usize {
        ((addr - start_address) ^ (1 << (order + 12))) + start_address
    }

    #[must_use]
    #[inline]
    pub fn is_emtpy(&self) -> bool {
        self.list.is_empty()
    }
}

struct BuddyAllocatorImpl {
    /// Free lists for each order of blocks.
    free_area: [FreeArea; MAX_ORDER],

    /// Total size of free memory in bytes.
    free_size: usize,

    /// The start address of the whole block that the allocator managed.
    start_address: usize,
}

impl Default for BuddyAllocatorImpl {
    fn default() -> Self {
        Self {
            free_area: core::array::from_fn(|_| FreeArea::default()),
            free_size: 0,
            start_address: 0,
        }
    }
}

impl BuddyAllocatorImpl {
    /// Initializes the buddy allocator with a range of memory.
    ///
    /// # Safety
    ///
    /// Caller must ensure the range of memory is valid.
    unsafe fn init(&mut self, base_addr: PageAlignedUsize, end_addr: PageAlignedUsize) {
        self.start_address = base_addr.get();

        let mut addr = base_addr.get();
        for order in (0..MAX_ORDER).rev() {
            let size = Self::block_size(order);

            let free_area = &mut self.free_area[order];
            let saddr = addr;

            while (addr + size) <= end_addr.get() {
                let mut pg = paddr_to_page(addr);
                pg.set_pgf_order(order as u8);
                free_area.push_front(pg);
                addr += size;
            }

            self.free_size += addr - saddr;
        }
    }

    #[must_use]
    #[inline]
    const fn block_size(order: usize) -> usize {
        PGSIZE * (1 << order)
    }

    unsafe fn alloc_by_order(&mut self, order: usize) -> Option<NonNull<[u8]>> {
        if order >= self.free_area.len() {
            return None;
        }

        if self.free_area[order].is_emtpy() && !self.expand(order + 1) {
            return None;
        }

        let pgf = self.free_area[order].pop_front().unwrap();
        let pgf_size = Self::block_size(order);
        self.free_size -= pgf_size;

        debug_assert!(!pgf.is_free());
        debug_assert!(!pgf.link.is_linked());

        Some(NonNull::new_unchecked(core::slice::from_raw_parts_mut(
            pgf.paddr() as *mut u8,
            pgf_size,
        )))
    }

    unsafe fn free_by_order(&mut self, ptr: NonNull<u8>, order: usize) {
        if order >= self.free_area.len() {
            panic!("invalid argument: `order`({})", order);
        }

        let mut pgf = paddr_to_page(ptr.addr().get());

        assert!(!pgf.is_free());
        assert!(!pgf.link.is_linked());

        pgf.set_pgf_order(order as u8);
        self.free_area[order].push_front(pgf);
        self.free_size += Self::block_size(order);

        self.merge(order);
    }

    fn merge(&mut self, order: usize) {
        for order in order..(self.free_area.len() - 1) {
            if let Some(mut pgf) = self.free_area[order].merge(self.start_address, order) {
                pgf.set_pgf_order(order as u8 + 1);
                self.free_area[order + 1].push_front(pgf);
            } else {
                break;
            }
        }
    }

    /// Attempts to recursively split a larger block down to the desired
    /// `order`.
    #[must_use]
    fn expand(&mut self, order: usize) -> bool {
        assert!(order != 0);
        if order >= self.free_area.len() {
            return false;
        }

        loop {
            if let Some((mut block1, mut block2)) = self.free_area[order].expand(order) {
                block1.set_pgf_order(order as u8 - 1);
                block2.set_pgf_order(order as u8 - 1);
                self.free_area[order - 1].push_front(block2);
                self.free_area[order - 1].push_front(block1);
                return true;
            }

            if self.expand(order + 1) {
                continue;
            }

            return false;
        }
    }
}

#[derive(Default)]
struct PageCacheList {
    cache_list: intrusive_collections::LinkedList<PageLinkAdapter>,
    cache_list_len: usize,
}

impl PageCacheList {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache_list: intrusive_collections::LinkedList::new(PageLinkAdapter::new()),
            cache_list_len: 0,
        }
    }

    #[must_use]
    pub unsafe fn alloc_page(&mut self) -> Option<NonNull<[u8]>> {
        if let Some(page) = self.cache_list.pop_front() {
            self.cache_list_len -= 1;
            return Some(NonNull::new_unchecked(core::slice::from_raw_parts_mut(
                page.paddr() as *mut u8,
                PGSIZE,
            )));
        }

        let old_len = self.cache_list_len;

        for _ in 0..PAGE_CACHE_LIST_BATCHCOUNT {
            if let Some(pg_ptr) = BUDDY_ALLOCATOR.lock_irq_save().alloc_by_order(0) {
                let pg = paddr_to_page(pg_ptr.addr().get());
                self.cache_list.push_front(pg);
                self.cache_list_len += 1;
            } else {
                break;
            }
        }

        if old_len != self.cache_list_len {
            self.alloc_page()
        } else {
            None
        }
    }

    pub unsafe fn dealloc_page(&mut self, ptr: NonNull<u8>) {
        let pg = paddr_to_page(ptr.addr().get());
        self.cache_list.push_front(pg);
        self.cache_list_len += 1;

        if self.cache_list_len < PAGE_CACHE_LIST_LIMIT {
            return;
        }

        for _ in 0..PAGE_CACHE_LIST_BATCHCOUNT {
            // SAFETY: the cache_list_len accurately reflects the number of elements.
            let pg = unsafe { self.cache_list.pop_back().unwrap_unchecked() };
            self.cache_list_len -= 1;
            BUDDY_ALLOCATOR
                .lock_irq_save()
                .free_by_order(NonNull::new_unchecked(pg.paddr() as *mut u8), 0);
        }
    }
}

/// Snapshot of the allocator's state used for testing and debugging.
#[derive(Debug)]
pub struct BuddyAllocatorState {
    free_size: usize,
    free_blocks: [usize; MAX_ORDER],
    cached_pages: usize,
}

impl BuddyAllocatorState {
    /// Returns the current allocator state.
    #[must_use]
    pub fn current() -> Self {
        let allocator = BUDDY_ALLOCATOR.lock();

        let mut free_blocks = [0; MAX_ORDER];
        #[allow(clippy::needless_range_loop)]
        for i in 0..MAX_ORDER {
            free_blocks[i] = allocator.free_area[i].nr_free;
        }

        let mut free_size = allocator.free_size;
        let mut cached_pages = 0;
        for i in 0..MAX_NCPU {
            // SAFETY: we just access the length of cache list.
            cached_pages += unsafe { PAGE_CACHE_LIST.get_by_cpuid(i).cache_list_len };
        }
        free_size += cached_pages * PGSIZE;

        Self {
            free_size,
            free_blocks,
            cached_pages,
        }
    }

    /// Compares two states to detect memory leaks.
    #[must_use]
    pub fn is_memory_leaky(&self, state: &Self) -> bool {
        self.free_size != state.free_size
    }
}

impl Display for BuddyAllocatorState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        writeln!(f, "free size: {}\n", self.free_size)?;
        writeln!(f, "cached_pages: {}", self.cached_pages)?;
        for (order, blocks) in self.free_blocks.iter().enumerate() {
            writeln!(f, "order {order}: {blocks}")?;
        }
        Ok(())
    }
}
