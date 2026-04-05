use core::cmp;

use alloc::sync::Arc;
use log::trace;
use syserr::sys_err;

use crate::{
    arch::{
        self,
        ctx::Trapframe,
        memlayout::{TASK_TRAPFRAME_BASE, USER_BASE_VADDR, USER_END_VADDR},
        pgtable::PageTableImpl,
    },
    error::{KResult, SysErrorKind},
    mmu::{is_valid_phy_addr, vm_area::VmArea},
    proc::cpu::{intr_off_store, intr_restore, mycpu_mut},
    sync::spin::{Once, Spinlock, SpinlockGuard},
    trap::AccessType,
};

use super::{
    buddy::BuddyBox,
    kernel_ld, pg_round_down, pg_round_up,
    pgtable::{EntryPerm, PageTable},
    types::{PageAlignedUsize, PhysicalPtr},
    vm_area::VmAreaList,
    PGSIZE,
};

static KERNEL_VM: Once<VM> = Once::new();

pub fn init() {
    // Lazyly initialize the kernel VM.
    let kvm = KERNEL_VM.get_or_init(|| VM::create_empty().unwrap());

    // This is used to quit from qemu.
    kvm.kmap(0x100000, 0x100000, PGSIZE, EntryPerm::with_rw());

    // Map kernel ELF.
    let kernel_map = [
        (
            kernel_ld::start_of_text(),
            kernel_ld::end_of_text(),
            EntryPerm::READABLE | EntryPerm::EXECUTABLE,
        ),
        (
            kernel_ld::start_of_rodata(),
            kernel_ld::end_of_rodata(),
            EntryPerm::READABLE,
        ),
        (
            kernel_ld::start_of_data(),
            kernel_ld::end_of_data(),
            EntryPerm::READABLE | EntryPerm::WRITABLE,
        ),
        (
            kernel_ld::start_of_bss(),
            kernel_ld::end_of_bss(),
            EntryPerm::READABLE | EntryPerm::WRITABLE,
        ),
    ];

    for (svaddr, evaddr, perm) in kernel_map {
        kvm.kmap(svaddr, svaddr, evaddr - svaddr, perm);
    }

    arch::memory::vm_init(kvm);
}

/// Enable virtual memory.
#[inline]
pub fn kvm_init_hart() {
    switch_vm_to_kernel();
}

#[inline]
pub fn switch_vm_to_kernel() {
    switch_vm(KERNEL_VM.get().unwrap());
}

#[must_use]
#[inline]
pub fn kernel_vm() -> &'static VMInner {
    unsafe {
        // SAFETY: Ths kernel VM is readonly.
        KERNEL_VM
            .get()
            .expect("you should call kernel_vm() after vm::init().")
            .inner
            .get_unchecked()
    }
}

/// Changes the current page table(virtual memory) to the specified VM.
#[inline]
pub fn switch_vm(vm: &VM) {
    let flag = intr_off_store();

    {
        let cpu = unsafe {
            // SAFETY: the intterupt is disabled.
            mycpu_mut()
        };
        if let Some(current_vm) = cpu.current_vm.as_ref() {
            if Arc::ptr_eq(&current_vm.inner, &vm.inner) {
                return;
            }
        }
        cpu.current_vm = Some(vm.clone());
    }

    // SAFETY: The pointer to the page table never be changed.
    let addr = unsafe { BuddyBox::as_ptr(&vm.inner.get_unchecked().pg_table).addr() };

    arch::pgtable::set_pgtable(unsafe { &*(addr as *const PageTableImpl) });
    arch::pgtable::flush_tlb();

    intr_restore(flag);
}

#[derive(Clone)]
pub struct VM {
    inner: Arc<Spinlock<VMInner>>,
}

impl VM {
    /// Creates a virtual memory with empty entries.
    pub(self) fn create_empty() -> KResult<Self> {
        PageTableImpl::try_alloc().map(|pg_table| Self {
            inner: Arc::new(Spinlock::new(
                "vm",
                VMInner {
                    pg_table,
                    user_space_size: 0,
                    area_list: VmAreaList::new(),
                },
            )),
        })
    }

    /// Creates a new virtual memory and copies references from the kernel
    /// vm [`kernel_vm`] to the new vm.
    ///
    /// Returns error if [`Self::create_empty`] fails.
    pub fn with_kernel_vm() -> KResult<Self> {
        let vm = VM::create_empty()?;
        vm.lock().pg_table.copy_from_kernel(&kernel_vm().pg_table);
        Ok(vm)
    }

    #[inline]
    pub(crate) fn kmap(&self, va: usize, pa: usize, size: usize, perm: EntryPerm) {
        trace!(target: "vm", "map {:#x} to {:#x}, size: {}, {}", va, pa, size, perm);
        self.lock()
            .pg_table
            .map_range(
                PageAlignedUsize::new(va).unwrap(),
                PageAlignedUsize::new(pa).unwrap(),
                PageAlignedUsize::new(size).unwrap(),
                perm,
            )
            .unwrap();
    }

    #[must_use]
    pub fn lock(&self) -> SpinlockGuard<VMInner> {
        self.inner.lock_irq_save()
    }
}

pub struct VMInner {
    pub(crate) pg_table: BuddyBox<PageTableImpl>,
    user_space_size: usize,
    area_list: VmAreaList,
}

impl VMInner {
    /// Copies all user memory contents(including trapframe) from a given VM.
    pub fn copy_user_content_from(&mut self, user_vm: &Self) -> KResult<()> {
        self.dealloc_all_segments();
        self.uvalloc(0, EntryPerm::empty(), false)?;

        for area in user_vm.area_list.iter() {
            unsafe { self.copy_content_from(area.va, area.size, user_vm)? };
            self.area_list.add(*area).unwrap();
        }

        unsafe {
            self.copy_content_from(
                PageAlignedUsize::new_const(USER_BASE_VADDR),
                pg_round_up(user_vm.user_space_size),
                user_vm,
            )?;
        }

        self.user_space_size = user_vm.user_space_size;
        Ok(())
    }

    #[must_use]
    #[inline]
    pub fn user_space_size(&self) -> usize {
        self.user_space_size
    }

    /// Grows the user space memory by exactly one page with the given
    /// permissions.
    ///
    /// This is a convenience wrapper around [`uvalloc`] that expands the user
    /// space by a single page (`PGSIZE`). It is useful for dynamic memory
    /// growth scenarios, such as implementing stack expansion.
    #[inline]
    pub fn uvgrow_page(&mut self, perm: EntryPerm) -> KResult<()> {
        self.uvalloc(self.user_space_size + PGSIZE, perm, false)
    }

    /// Resizes the user space memory region, either growing or shrinking it as
    /// needed.
    ///
    /// This function adjusts the amount of user space memory available to a
    /// program. It aligns both the current and new sizes to the nearest
    /// page boundary, then performs one of the following:
    ///
    /// - **Grow**: Maps additional pages from the old size up to `new_sz`,
    ///   using the given permissions combined with `EntryPerm::USER`.
    /// - **Shrink**: Unmaps pages from `new_sz` up to the previous size.
    ///
    /// The `lazy` flag specifies whether the growing pages are allocated in
    /// lazy(allocated on access).
    ///
    /// This is useful for some system calls such as `exec`, `brk`...
    ///
    /// # See Also
    ///
    /// [`on_page_fault`]: lazily load growing pages.
    ///
    /// [`on_page_fault`]: [`Self::on_page_fault`]
    pub fn uvalloc(&mut self, new_sz: usize, perm: EntryPerm, lazy: bool) -> KResult<()> {
        let old_sz_aligned = pg_round_up(self.user_space_size);
        let new_sz_aligned = pg_round_up(new_sz);

        trace!(target:"vm", "vm: uvalloc, new_sz: {:#x}, old_sz: {:#x}", new_sz, self.user_space_size);

        if *new_sz_aligned > USER_END_VADDR {
            return Err(SysErrorKind::OutOfMemory.into());
        }

        if old_sz_aligned == new_sz_aligned {
            return Ok(());
        }

        let result = if *new_sz_aligned > *old_sz_aligned {
            if lazy {
                // Lazy loading requires the permission is readable and writable.
                // See on_page_fault().
                assert_eq!(perm, EntryPerm::with_rw());
                Ok(())
            } else {
                self.pg_table.vmap(
                    PageAlignedUsize::new_const(USER_BASE_VADDR) + old_sz_aligned,
                    new_sz_aligned - old_sz_aligned,
                    perm | EntryPerm::USER,
                )
            }
        } else {
            self.pg_table.vunmap(
                PageAlignedUsize::new_const(USER_BASE_VADDR) + new_sz_aligned,
                old_sz_aligned - new_sz_aligned,
            );
            Ok(())
        };

        if result.is_ok() {
            self.user_space_size = new_sz;
        }
        result
    }

    /// This is called by 'brk' system call.
    pub fn brk(&mut self, new_brk_ptr: usize) -> KResult<usize> {
        let old_brk_ptr = USER_BASE_VADDR + self.user_space_size;
        if new_brk_ptr == 0 {
            return Ok(old_brk_ptr);
        }

        if !(USER_BASE_VADDR..=USER_END_VADDR).contains(&new_brk_ptr) {
            return Err(SysErrorKind::InvalidArgument.into());
        }

        let new_sz = new_brk_ptr - USER_BASE_VADDR;
        self.uvalloc(new_sz, EntryPerm::with_rw(), true)?;

        Ok(old_brk_ptr)
    }

    /// Allocates a page at [`TASK_TRAPFRAME_BASE`] in this VM.
    ///
    /// # Why need to map?
    ///
    /// We can easily known the base address of the trapframe of the current
    /// task when an intterupt or exception occurs.
    ///
    /// For example:
    ///
    /// ``` asm
    /// .trap_handler_asm:
    /// # Loads the base address of the trap frame to the register a0.
    /// li a0, TASK_TRAPFRAME_BASE
    ///
    /// # Save registers
    /// sd sp, a0(0)
    /// sd a1, a0(8)
    /// sd a2, a0(16)
    /// # and more registers...
    /// ```
    pub fn alloc_trapframe(&mut self, trapframe: Trapframe) -> KResult<PhysicalPtr<Trapframe>> {
        self.dealloc_segment(PageAlignedUsize::new_const(TASK_TRAPFRAME_BASE));
        self.alloc_segment(
            PageAlignedUsize::new_const(TASK_TRAPFRAME_BASE),
            PageAlignedUsize::new_const(PGSIZE),
            EntryPerm::with_rw(),
        )?;

        let mut ptr = self.trapframe();
        *ptr = trapframe;
        Ok(ptr)
    }

    /// Returns a pointer(can be accessed directly in kernel) to the trapframe.
    ///
    /// # Panics
    ///
    /// You must allocate the trapframe by [`Self::alloc_trapframe`] before
    /// this.
    #[must_use]
    #[inline]
    pub fn trapframe(&self) -> PhysicalPtr<Trapframe> {
        unsafe {
            let paddr = self.va2pa(TASK_TRAPFRAME_BASE).unwrap();
            PhysicalPtr::new_unchecked(paddr as *mut Trapframe)
        }
    }

    /// Allocates a new virtual memory segment(area) in the VM.
    ///
    /// This can be used to allocate user ELF sections(exec system call).
    pub fn alloc_segment(
        &mut self,
        va: PageAlignedUsize,
        size: PageAlignedUsize,
        perm: EntryPerm,
    ) -> KResult<()> {
        trace!(
            target: "vm",
            "alloc section: [{:#x}..{:#x}], perm: {}",
            *va,
            *va + *size,
            perm
        );
        self.pg_table.vmap(va, size, perm)?;
        self.area_list.add(VmArea { va, size, perm }).unwrap();
        Ok(())
    }

    pub fn dealloc_segment(&mut self, va: PageAlignedUsize) -> bool {
        if let Some(area) = self.area_list.remove_by_va(va) {
            self.pg_table.vunmap(area.va, area.size);
            true
        } else {
            false
        }
    }

    pub fn dealloc_all_segments(&mut self) {
        let area_list = core::mem::take(&mut self.area_list);
        for area in area_list {
            self.pg_table.vunmap(area.va, area.size);
        }
    }

    /// Deeply copy the range `[va..+=size]` of the memory from `src` to `self`.
    ///
    /// # Safety
    ///
    /// Caller must ensure that the range of memory does not exists in `self`
    /// VM.
    unsafe fn copy_content_from(
        &mut self,
        start_va: PageAlignedUsize,
        size: PageAlignedUsize,
        src: &VMInner,
    ) -> KResult<()> {
        for (va, entry) in src.pg_table.range_of_entries(start_va, start_va + size) {
            // SAFETY: the va(pte according to) must be page aligned.
            let va = unsafe { PageAlignedUsize::new_unchecked(va) };
            match self.pg_table.vmap_page(va, entry.perm()) {
                Ok(dst_paddr) => {
                    let src_paddr = entry.physical_addr() as *mut u8;
                    unsafe {
                        dst_paddr
                            .as_ptr()
                            .copy_from_nonoverlapping(src_paddr, PGSIZE);
                    }
                }

                Err(err) => {
                    // rollback.
                    self.pg_table.vunmap(start_va, va - start_va);
                    return Err(err);
                }
            }
        }
        Ok(())
    }

    /// Returns the physical address that the `va` maps to, or [`None`] if the
    /// associated PTE is not present.
    #[must_use]
    #[inline]
    pub fn va2pa(&self, va: usize) -> Option<usize> {
        self.pg_table
            .get_entry(va)
            .map(|entry| entry.physical_addr())
    }

    pub fn write(&self, mut va: usize, mut bytes: &[u8]) -> KResult<usize> {
        const PGSIZE_MASK: usize = (1 << 12) - 1;
        while !bytes.is_empty() {
            let pa = self.va2pa(va & !PGSIZE_MASK).ok_or_else(|| {
                sys_err!(
                    syserr::SysErrorKind::InvalidArgument,
                    "Vm::write(): the page at {:#x} is not mapped.",
                    va & !PGSIZE
                )
            })?;

            if !is_valid_phy_addr(pa) {
                return Err(sys_err!(
                    syserr::SysErrorKind::InvalidArgument, 
                    "Vm::write(): the mapped physical address can not be accessiable from kernel: {:#x}", 
                    pa
                ))
            }
            let offset = va & PGSIZE_MASK;
            let remainning = PGSIZE - offset;
            let bytes_cp = cmp::min(bytes.len(), remainning);

            // SAFETY: we can access these bytes from kernel.
            unsafe { ((pa + offset) as *mut u8).copy_from(bytes.as_ptr(), bytes_cp) };

            va += bytes_cp;
            bytes = &bytes[bytes_cp..];
        }
        Ok(va + bytes.len())
    }

    /// This is called by trap when a task accesses invalid page. If the address
    /// falls with in the dynamic user space and no page is currently
    /// mapped, this it allocates and maps a new page with `writable` and
    /// `readable` permissions.
    ///
    /// This is used in lazy allocation schemes for [`Self::uvalloc`].
    pub fn on_page_fault(&mut self, addr: usize, _access: AccessType) -> KResult<()> {
        if (USER_BASE_VADDR..USER_BASE_VADDR + self.user_space_size).contains(&addr) {
            let pg_vaddr = pg_round_down(addr);
            let entry = self.pg_table.get_entry(*pg_vaddr);
            if entry.is_some() {
                // permission deny.
                return Err(SysErrorKind::Fault.into());
            }
            log::trace!(
                target: "vm",
                "vm: on_page_fault: user size {:#x}, fault addr {:#x}",
                self.user_space_size,
                addr,
            );
            self.pg_table
                .vmap_page(pg_vaddr, EntryPerm::with_rw() | EntryPerm::USER)?;
            Ok(())
        } else {
            log::trace!(
                target: "vm",
                "vm: on_page_fault: out of user memory: user size {:#x}, fault addr {:#x}",
                self.user_space_size,
                addr,
            );
            Err(SysErrorKind::OutOfMemory.into())
        }
    }
}

impl Drop for VMInner {
    fn drop(&mut self) {
        self.dealloc_all_segments();

        self.pg_table.vunmap(
            PageAlignedUsize::new_const(USER_BASE_VADDR),
            pg_round_up(self.user_space_size),
        );

        // Deallocate some intermediate tables.
        self.pg_table.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::VM;
    use crate::{
        arch::{ctx::Trapframe, memlayout::USER_ELF_BASE_VADDR},
        mmu::{buddy::BuddyAllocatorState, pgtable::EntryPerm, types::PageAlignedUsize, PGSIZE},
    };

    #[test_case]
    pub fn test_memory_leak() {
        let state = BuddyAllocatorState::current();
        let vm = VM::create_empty().unwrap();
        {
            let mut vm = vm.lock();
            vm.alloc_trapframe(Trapframe::default()).unwrap();
            vm.alloc_segment(
                PageAlignedUsize::new_const(USER_ELF_BASE_VADDR),
                PageAlignedUsize::new_const(PGSIZE * 4),
                EntryPerm::with_rw(),
            )
            .unwrap();
            vm.uvalloc(PGSIZE * 3, EntryPerm::with_rw(), false).unwrap();
        }
        let copy_vm = VM::with_kernel_vm().unwrap();
        copy_vm.lock().copy_user_content_from(&vm.lock()).unwrap();
        drop(vm);
        drop(copy_vm);
        assert!(!state.is_memory_leaky(&BuddyAllocatorState::current()));
    }
}
