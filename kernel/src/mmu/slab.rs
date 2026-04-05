use core::{ptr::NonNull, sync::atomic::AtomicUsize};

use alloc::alloc::Allocator;
use intrusive_collections::SinglyLinkedListLink;

use crate::{
    mmu::{align_up, buddy},
    params::MAX_NCPU,
    sync::{percpu::PerCpu, spin::Spinlock},
};

use super::{
    paddr_to_page,
    types::{Page, PageLinkAdapter, PhysicalPtr, SlabFreeObj, SlabFreeObjLinkAdapter},
    PGSIZE,
};

pub static SLAB_ALLOCATORS_TABLE: Spinlock<heapless::Vec<&'static SlabAllocator, 32>> =
    Spinlock::new("slab_allocators", heapless::Vec::new());

const MAX_EMPTY_SLABS: usize = 8;
const EMPTY_SLABS_BATCH_COUNT: usize = 4;

pub macro define_slab_allocator($name:ident, $slab_name:tt, $object_sz:expr) {
    lazy_static::lazy_static! {
        pub static ref $name: SlabAllocator = SlabAllocator::new($slab_name, $object_sz, 8);
    }
}

define_slab_allocator!(KMALLOC_8, "kalloc-8", 8);
define_slab_allocator!(KMALLOC_16, "kalloc-16", 16);
define_slab_allocator!(KMALLOC_32, "kalloc-32", 32);
define_slab_allocator!(KMALLOC_64, "kalloc-64", 64);
define_slab_allocator!(KMALLOC_96, "kalloc-96", 96);
define_slab_allocator!(KMALLOC_128, "kalloc-128", 128);
define_slab_allocator!(KMALLOC_192, "kalloc-192", 192);
define_slab_allocator!(KMALLOC_256, "kalloc-256", 256);
define_slab_allocator!(KMALLOC_512, "kalloc-512", 512);
define_slab_allocator!(KMALLOC_1024, "kalloc-1024", 1024);
define_slab_allocator!(KMALLOC_2048, "kalloc-2048", 2048);
define_slab_allocator!(KMALLOC_4096, "kalloc-4096", 4096);
define_slab_allocator!(KMALLOC_8192, "kalloc-8192", 8192);

pub fn init() {
    register_slab_allocator(&KMALLOC_8);
    register_slab_allocator(&KMALLOC_16);
    register_slab_allocator(&KMALLOC_32);
    register_slab_allocator(&KMALLOC_64);
    register_slab_allocator(&KMALLOC_96);
    register_slab_allocator(&KMALLOC_128);
    register_slab_allocator(&KMALLOC_192);
    register_slab_allocator(&KMALLOC_256);
    register_slab_allocator(&KMALLOC_512);
    register_slab_allocator(&KMALLOC_1024);
    register_slab_allocator(&KMALLOC_2048);
    register_slab_allocator(&KMALLOC_4096);
    register_slab_allocator(&KMALLOC_8192);
}

pub fn register_slab_allocator(slab_allocator: &'static SlabAllocator) {
    if SLAB_ALLOCATORS_TABLE.lock().push(slab_allocator).is_err() {
        panic!("the slab allocators table is full");
    }
}

fn pgforder_for_slab(object_size: usize, min_obj_cnt: usize) -> usize {
    let order = (object_size * min_obj_cnt)
        .div_ceil(PGSIZE)
        .next_power_of_two()
        .trailing_zeros() as usize;
    order.min(5)
}

// =======================================================================================
//  Slab functions
// =======================================================================================

/// Allocates a new page frame as a slab.
///
/// This function spilts the page frame into many objects and uses a singly
/// linked list links theme.
///
/// Returns the slab descriptor(`struct page`).
#[must_use]
fn slab_new(pgforder: usize, object_sz: usize) -> Option<PhysicalPtr<Page>> {
    let pgf = unsafe { buddy::alloc_by_order(pgforder)? };
    let mut slab = paddr_to_page(pgf.addr().get());

    slab.slab_obj_sz = object_sz as u32;
    slab.slab_free_list =
        intrusive_collections::SinglyLinkedList::new(SlabFreeObjLinkAdapter::new());

    let list = &mut slab.slab_free_list;
    let mut free_nr = 0;

    // Initialize the slab object list.
    let pgframe_sz = PGSIZE * (1 << pgforder);
    let end = pgf.addr().get() + pgframe_sz;
    let mut object = pgf.as_ptr() as *mut u8;
    while object.addr() + object_sz <= end {
        let node = object as *mut SlabFreeObj;
        unsafe {
            node.write_volatile(SlabFreeObj {
                link: intrusive_collections::SinglyLinkedListLink::new(),
            });
            list.push_front(PhysicalPtr::new_unchecked(node));
            object = object.byte_offset(object_sz as isize);
        }
        free_nr += 1;
    }

    slab.slab_nr_free = free_nr;
    slab.slab_num_objs = free_nr;

    Some(slab)
}

fn slab_drop(slab: &mut Page) {
    unsafe {
        buddy::free_by_order(
            NonNull::new(slab.paddr() as *mut u8).unwrap_unchecked(),
            slab.pgf_order() as usize,
        )
    };
}

#[must_use]
unsafe fn slab_allocate(slab: &mut Page) -> Option<NonNull<u8>> {
    if let Some(node) = slab.slab_free_list.pop_front() {
        slab.slab_nr_free -= 1;
        Some(NonNull::new_unchecked(node.as_ptr().cast()))
    } else {
        None
    }
}

unsafe fn slab_deallocate(slab: &mut Page, obj: NonNull<u8>) {
    slab.slab_nr_free += 1;
    slab.slab_free_list
        .push_front(PhysicalPtr::new_unchecked(obj.as_ptr() as *mut SlabFreeObj));
}

#[must_use]
#[inline]
fn slab_is_full(slab: &Page) -> bool {
    slab.slab_free_list.is_empty()
}

#[must_use]
#[inline]
fn slab_is_empty(slab: &Page) -> bool {
    slab.slab_num_objs == slab.slab_nr_free
}

#[must_use]
#[inline]
pub fn slab_contains(slab: &Page, obj: NonNull<u8>) -> bool {
    let pgfaddr = slab.paddr();
    let pgf_sz = PGSIZE * (1 << slab.pgf_order() as usize);
    let addr = obj.addr().get();
    addr >= pgfaddr && (addr + slab.slab_obj_sz as usize) <= pgfaddr + pgf_sz
}

pub struct SlabMemCache {
    /// The page frame order for buddy.
    pgforder: usize,

    /// The size in bytes of an object.
    object_sz: usize,

    /// How many objects in the mem cache.
    num_objs: usize,

    /// How many objects in active.
    active_objs: usize,

    slabs_empty: intrusive_collections::LinkedList<PageLinkAdapter>,
    slabs_full: intrusive_collections::LinkedList<PageLinkAdapter>,
    slabs_partial: intrusive_collections::LinkedList<PageLinkAdapter>,

    slabs_empty_len: usize,
    total_slabs: usize,
}

impl SlabMemCache {
    #[must_use]
    pub fn new(object_sz: usize) -> Self {
        let pgforder = pgforder_for_slab(object_sz, 8);

        Self {
            pgforder,
            object_sz,
            num_objs: 0,
            active_objs: 0,
            slabs_empty: intrusive_collections::LinkedList::new(PageLinkAdapter::new()),
            slabs_full: intrusive_collections::LinkedList::new(PageLinkAdapter::new()),
            slabs_partial: intrusive_collections::LinkedList::new(PageLinkAdapter::new()),
            slabs_empty_len: 0,
            total_slabs: 0,
        }
    }

    unsafe fn allocate(&mut self) -> Option<NonNull<u8>> {
        // 1) try partial list.
        if let Some(slab) = self.slabs_partial.front().get() {
            // Make slab reference mutable.
            //
            // The intrusive linked list does not provides the access to mutable reference.
            let mut slab = paddr_to_page(slab.paddr());

            let obj = slab_allocate(&mut slab).unwrap();
            if slab_is_full(&slab) {
                let slab = self.slabs_partial.pop_front().unwrap();
                self.slabs_full.push_front(slab);
            }
            self.active_objs += 1;
            return Some(obj);
        }

        loop {
            // 2) try empty list
            if let Some(mut slab) = self.slabs_empty.pop_front() {
                self.slabs_empty_len -= 1;
                let obj = slab_allocate(&mut slab).unwrap();
                if slab_is_full(&slab) {
                    self.slabs_full.push_front(slab);
                } else {
                    self.slabs_partial.push_front(slab);
                }
                self.active_objs += 1;
                return Some(obj);
            }

            // 3) allocate new slab and jump to 2)
            let new_slab = slab_new(self.pgforder, self.object_sz)?;
            self.slabs_empty.push_front(new_slab);
            self.slabs_empty_len += 1;
            self.total_slabs += 1;
            self.num_objs += new_slab.slab_num_objs as usize;
        }
    }

    unsafe fn deallocate(&mut self, obj: NonNull<u8>) {
        // 1) try partial list.
        let mut partial_cursor = self.slabs_partial.front_mut();
        while let Some(slab) = partial_cursor.get() {
            if slab_contains(slab, obj) {
                let mut slab = paddr_to_page(slab.paddr());
                self.active_objs -= 1;
                slab_deallocate(&mut slab, obj);
                if slab_is_empty(&slab) {
                    // If slab is empty, move it to empty list.
                    let slab = partial_cursor.remove().unwrap_unchecked();
                    self.slabs_empty.push_front(slab);
                    self.slabs_empty_len += 1;
                    self.maybe_shrink_empty_slabs();
                }
                return;
            }
            partial_cursor.move_next();
        }

        // 2) try full list.
        let mut full_cursor = self.slabs_full.front_mut();
        while let Some(slab) = full_cursor.get() {
            if slab_contains(slab, obj) {
                self.active_objs -= 1;
                let mut slab = full_cursor.remove().unwrap();
                slab_deallocate(&mut slab, obj);
                if slab_is_empty(&slab) {
                    self.slabs_empty.push_front(slab);
                    self.slabs_empty_len += 1;
                    self.maybe_shrink_empty_slabs();
                } else {
                    self.slabs_partial.push_front(slab);
                }
                return;
            }
            full_cursor.move_next();
        }
    }

    fn maybe_shrink_empty_slabs(&mut self) {
        if self.slabs_empty_len > MAX_EMPTY_SLABS {
            for _ in 0..EMPTY_SLABS_BATCH_COUNT {
                let mut slab = self.slabs_empty.pop_front().unwrap();
                self.num_objs -= slab.slab_num_objs as usize;
                self.slabs_empty_len -= 1;
                self.total_slabs -= 1;
                slab_drop(&mut slab);
            }
        }
    }
}

impl Drop for SlabMemCache {
    fn drop(&mut self) {
        while let Some(mut slab) = self.slabs_full.pop_front() {
            slab_drop(&mut slab);
        }
        while let Some(mut slab) = self.slabs_empty.pop_front() {
            slab_drop(&mut slab);
        }
        while let Some(mut slab) = self.slabs_partial.pop_front() {
            slab_drop(&mut slab);
        }
    }
}

struct ArrayCache {
    limit: usize,
    batchcount: usize,
    list: intrusive_collections::SinglyLinkedList<SlabFreeObjLinkAdapter>,
    len: usize,
}

impl ArrayCache {
    #[must_use]
    pub fn new(limit: usize, batchcount: usize) -> Self {
        Self {
            limit,
            batchcount,
            list: intrusive_collections::SinglyLinkedList::new(SlabFreeObjLinkAdapter::new()),
            len: 0,
        }
    }

    pub fn grow_from_cache(&mut self, cache: &mut SlabMemCache) -> bool {
        let len = self.len;
        for _ in 0..self.batchcount {
            if let Some(obj) = unsafe { cache.allocate() } {
                self.list
                    .push_front(unsafe { PhysicalPtr::new_unchecked(obj.as_ptr().cast()) });
                self.len += 1;
            } else {
                break;
            }
        }
        len != self.len
    }

    pub fn shrink_to_cache(&mut self, cache: &mut SlabMemCache) {
        for _ in 0..self.batchcount {
            if let Some(obj) = self.list.pop_front() {
                self.len -= 1;
                unsafe { cache.deallocate(NonNull::new_unchecked(obj.as_ptr().cast())) };
            } else {
                break;
            }
        }
    }

    pub fn transfer_objects(dst: &mut Self, src: &mut Self, batchcount: usize) {
        for _ in 0..batchcount {
            if let Some(obj) = src.list.pop_front() {
                src.len -= 1;
                dst.list.push_front(obj);
                dst.len += 1;
            } else {
                break;
            }
        }
    }

    #[inline]
    pub fn fetch_obj(&mut self) -> Option<NonNull<u8>> {
        if let Some(ptr) = self.list.pop_front() {
            self.len -= 1;
            Some(unsafe { NonNull::new_unchecked(ptr.as_ptr().cast()) })
        } else {
            None
        }
    }

    #[inline]
    pub fn place_obj(&mut self, obj: NonNull<u8>) {
        let mut node = unsafe { PhysicalPtr::new_unchecked(obj.as_ptr() as *mut SlabFreeObj) };
        node.link = SinglyLinkedListLink::new();
        self.list.push_front(node);
        self.len += 1;
    }
}

pub struct SlabAllocator {
    name: &'static str,
    object_size: usize,
    mem_cache: Spinlock<SlabMemCache>,
    per_cpu_array_cache: PerCpu<ArrayCache>,
    shared_array_cache: Spinlock<ArrayCache>,
}

impl SlabAllocator {
    #[must_use]
    pub fn new(name: &'static str, object_size: usize, align: usize) -> Self {
        let object_size = align_up(object_size, align);

        if object_size > PGSIZE * 2 {
            panic!("the object size is too large");
        } else if object_size < size_of::<SlabFreeObj>() {
            panic!("the object size is too small");
        }

        fn compute_batchcount_alt(obj_size: usize) -> usize {
            let n = PGSIZE / obj_size;
            (1 + n / 8).clamp(2, 32)
        }

        let percpu_batchcount = compute_batchcount_alt(object_size);
        let percpu_limit = percpu_batchcount * 4;

        let shared_limit = (percpu_limit * MAX_NCPU).clamp(16, 256);
        let shared_batchcount = (percpu_batchcount * MAX_NCPU).clamp(4, 64);

        Self {
            name,
            object_size,
            mem_cache: Spinlock::new("slab_mem_cache", SlabMemCache::new(object_size)),
            per_cpu_array_cache: PerCpu::from_fn(|_| {
                ArrayCache::new(percpu_limit, percpu_batchcount)
            }),
            shared_array_cache: Spinlock::new(
                "slab_shared_array_cache",
                ArrayCache::new(shared_limit, shared_batchcount),
            ),
        }
    }

    #[must_use]
    pub fn with_type<T>() -> Self {
        Self::new(core::any::type_name::<T>(), size_of::<T>(), align_of::<T>())
    }

    #[must_use]
    pub fn with_arc_type<T>() -> Self {
        Self::new(
            core::any::type_name::<T>(),
            size_of::<T>() + size_of::<AtomicUsize>() * 2,
            align_of::<T>(),
        )
    }

    #[must_use]
    pub fn name(&self) -> &str {
        self.name
    }

    #[must_use]
    pub fn info(&self) -> SlabInfo {
        let shared_ac = self.shared_array_cache.lock_irq_save();
        let mem_cahce = self.mem_cache.lock();

        let mut active_objs = mem_cahce.active_objs - shared_ac.len;
        let num_objs = mem_cahce.num_objs;
        let pgforder = mem_cahce.pgforder;
        let objs_per_slab = (1 << pgforder) * PGSIZE / mem_cahce.object_sz;
        let num_slabs = mem_cahce.total_slabs;
        let batchcount = self.per_cpu_array_cache.lock_irq_save().batchcount;

        drop(mem_cahce);
        drop(shared_ac);

        for i in 0..MAX_NCPU {
            // SAFETY: we just access the length of the free list.
            active_objs -= unsafe { self.per_cpu_array_cache.get_by_cpuid(i).len };
        }

        SlabInfo {
            name: self.name,
            object_size: self.object_size,
            num_objs,
            active_objs,
            objs_per_slab,
            pgforder,
            num_slabs,
            batchcount,
        }
    }

    /// Allocates a memory object from the slab cache.
    ///
    /// This function attempts to allocate an object from the current CPU's
    /// local array cache (`per_cpu_array_cache`) for fast allocation. If
    /// the local cache is empty, it fetches a batch of objects from the
    /// shared array cache (`shared_array_cache`), which may in turn refill
    /// itself from the underlying memory cache (`mem_cache`) if necessary.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the returned object is properly deallocated
    /// using [`deallocate_obj`] and not used after being freed. The object
    /// is uninitialized memory and must be properly constructed before use.
    ///
    /// # Returns
    ///
    /// - `Some(ptr)` pointing to the allocated object.
    /// - `None` if allocation fails (e.g., out of memory).
    ///
    /// # Performance
    ///
    /// This design improves performance by using per-CPU and shared caches to
    /// reduce contention and increase locality.
    pub unsafe fn allocate_obj(&self) -> Option<NonNull<u8>> {
        // fetch object from local cpu array cache.
        let mut local_ac = self.per_cpu_array_cache.lock_irq_save();
        if let Some(obj) = local_ac.fetch_obj() {
            return Some(obj);
        }

        let mut shared_ac = self.shared_array_cache.lock();
        if shared_ac.list.is_empty() {
            // allocate objects from mem cache and move theme to shared array cache.
            if !shared_ac.grow_from_cache(&mut self.mem_cache.lock()) {
                return None;
            }
        }

        let batchcount = local_ac.batchcount;
        ArrayCache::transfer_objects(&mut local_ac, &mut shared_ac, batchcount);
        local_ac.fetch_obj()
    }

    /// Deallocates a memory object and returns it to the slab cache.
    ///
    /// The object is first returned to the per-CPU array cache. If the local
    /// cache exceeds its configured limit, a batch of objects is
    /// transferred to the shared cache. If the shared cache also exceeds
    /// its limit, excess objects are returned to the underlying memory
    /// cache (`mem_cache`) for reuse.
    ///
    /// # Safety
    ///
    /// - The object `obj` must have been previously allocated by
    ///   [`allocate_obj`].
    /// - It must not be used after this function is called.
    /// - Double-free or invalid pointer input results in undefined behavior.
    ///
    /// # Parameters
    ///
    /// - `obj`: A non-null pointer to the memory object to deallocate.
    ///
    /// # Efficiency
    ///
    /// This tiered caching system (local → shared → global) minimizes lock
    /// contention and maximizes cache locality, especially in multicore
    /// environments.
    pub unsafe fn deallocate_obj(&self, obj: NonNull<u8>) {
        let mut local_ac = self.per_cpu_array_cache.lock_irq_save();
        local_ac.place_obj(obj);
        if local_ac.len < local_ac.limit {
            return;
        }

        let mut shared_ac = self.shared_array_cache.lock();
        let batchcount = local_ac.batchcount;
        ArrayCache::transfer_objects(&mut shared_ac, &mut local_ac, batchcount);
        if shared_ac.len < shared_ac.limit {
            return;
        }

        let mut mem_cache = self.mem_cache.lock();
        shared_ac.shrink_to_cache(&mut mem_cache);
    }
}

unsafe impl Send for SlabAllocator {}
unsafe impl Sync for SlabAllocator {}

unsafe impl Allocator for SlabAllocator {
    fn allocate(
        &self,
        layout: core::alloc::Layout,
    ) -> Result<NonNull<[u8]>, alloc::alloc::AllocError> {
        assert!(
            layout.size() <= self.object_size,
            "layout size: {:#x}({}), object size: {:#x}({})",
            layout.size(),
            layout.size(),
            self.object_size,
            self.object_size
        );
        unsafe {
            let obj = self.allocate_obj().ok_or(alloc::alloc::AllocError {})?;
            Ok(NonNull::new_unchecked(core::slice::from_raw_parts_mut(
                obj.as_ptr().cast(),
                self.object_size,
            )))
        }
    }

    unsafe fn deallocate(&self, ptr: NonNull<u8>, layout: core::alloc::Layout) {
        assert!(
            layout.size() <= self.object_size,
            "layout size: {:#x}({}), object size: {:#x}({})",
            layout.size(),
            layout.size(),
            self.object_size,
            self.object_size
        );
        self.deallocate_obj(ptr.cast());
    }
}

#[derive(Debug)]
pub struct SlabInfo {
    pub name: &'static str,
    pub num_objs: usize,
    pub active_objs: usize,
    pub object_size: usize,
    pub pgforder: usize,
    pub objs_per_slab: usize,
    pub num_slabs: usize,
    pub batchcount: usize,
}

impl SlabInfo {
    #[must_use]
    pub fn pages_per_slab(&self) -> usize {
        1 << self.pgforder
    }
}

#[cfg(test)]
mod tests {

    use alloc::{boxed::Box, collections::vec_deque::VecDeque};

    use crate::mmu::buddy::{BuddyAlloc, BuddyAllocatorState};

    use super::SlabAllocator;

    #[test_case]
    pub fn test_allocator_simple() {
        let state = BuddyAllocatorState::current();
        let slabs = SlabAllocator::with_type::<u128>();
        {
            let mut a = Box::new_in(0_u128, &slabs);
            *a = 64;
            assert_eq!(*a, 64);

            let b = Box::new_in(12_u64, &slabs);
            assert_eq!(*b, 12);
            drop(b);

            let mut c = Box::new_in(12_u128, &slabs);
            *c = 32;
            assert_eq!(*c, 32);
        }
        drop(slabs);
        assert!(!state.is_memory_leaky(&BuddyAllocatorState::current()));
    }

    #[test_case]
    pub fn test_allocate_many_objs() {
        let state = BuddyAllocatorState::current();
        let slabs = SlabAllocator::with_type::<u128>();
        let mut vec = VecDeque::new_in(BuddyAlloc {});

        for i in 0..1024 {
            vec.push_front(Box::new_in(i as u128, &slabs));
        }

        for _ in 0..512 {
            vec.pop_back();
        }

        for i in 0..1024 {
            vec.push_back(Box::new_in(i as u128, &slabs));
        }

        for _ in 0..512 {
            vec.pop_front();
        }

        drop(vec);
        drop(slabs);

        assert!(
            !state.is_memory_leaky(&BuddyAllocatorState::current()),
            "old: {}, current: {}",
            state,
            BuddyAllocatorState::current()
        );
    }
}
