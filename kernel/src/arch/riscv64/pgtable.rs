use core::{
    arch::asm,
    marker::PhantomData,
    ops::RangeInclusive,
    ptr::{self, NonNull},
};

use syserr::sys_err;

use crate::{
    error::{KResult, SysErrorKind},
    mmu::{
        buddy::{self, BuddyAlloc, BuddyBox},
        pgtable::{EntryPerm, PageTable, PageTableEntry},
        types::PageAlignedUsize,
        PGSIZE,
    },
};

use super::memlayout::KERNEL_END_VADDR;
use super::memlayout::KERNEL_START_VADDR;

use riscv::register::*;

#[inline(always)]
pub fn set_pgtable(table: &PageTableImpl) {
    unsafe { satp::set(satp::Mode::Sv39, 0, ptr::from_ref(table).addr() / 4096) };
}

#[inline(always)]
pub fn flush_tlb() {
    unsafe { asm!("sfence.vma zero, zero") };
}

const PTE_VALID: u64 = 1;
const PTE_READ: u64 = 1 << 1;
const PTE_WRITE: u64 = 1 << 2;
const PTE_EXEC: u64 = 1 << 3;
const PTE_USER: u64 = 1 << 4;

const FLAGS_MASK: u64 = (1 << 10) - 1;

const RISCV_VA_MASK: usize = (1 << (12 + 9 + 9 + 9)) - 1;

const KERNEL_VM_V2_INDEX_RANGE: RangeInclusive<usize> = RangeInclusive::new(
    PageTableImpl::index_for_va(2, KERNEL_START_VADDR),
    PageTableImpl::index_for_va(2, KERNEL_END_VADDR),
);

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ArchEntryPerm(u64);

impl From<EntryPerm> for ArchEntryPerm {
    fn from(perm: EntryPerm) -> Self {
        let mut flags = 0;
        if perm.is_user() {
            flags |= PTE_USER;
        }
        if perm.is_valid() {
            flags |= PTE_VALID;
        }
        if perm.is_writable() {
            flags |= PTE_WRITE;
        }
        if perm.is_readable() {
            flags |= PTE_READ;
        }
        if perm.is_executable() {
            flags |= PTE_EXEC;
        }
        Self(flags)
    }
}

/// Page Table Entry.
///
/// The bits layout:
///
/// ``` no-rust
/// | Reversed | Physical Page Number | Flags |
/// ^          ^                      ^
/// 63         53                     10
/// ```
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct PageTableEntryImpl(pub u64);

impl PageTableEntryImpl {
    #[inline]
    fn physical_page_num(self) -> u64 {
        self.0 >> 10
    }

    /// Sets the physical address of this entry (must be 4KB aligned).
    #[inline]
    pub fn set_physical_addr(&mut self, addr: u64) {
        assert!(addr % 4096 == 0 && addr != 0);
        // Clear physical page number. Keep flags.
        self.0 &= FLAGS_MASK;
        self.0 |= (addr / 4096) << 10;
    }

    /// Converts this entry (assumed to be a page directory entry) into a
    /// pointer to a page table.
    ///
    /// # Panics
    ///
    /// Panics if this entry does not represent a page table.
    #[inline]
    pub fn as_table(self) -> *mut PageTableImpl {
        debug_assert!(self.is_page_table());
        self.physical_addr() as _
    }

    /// Returns true if the entry is a valid page table (i.e., only `PTE_VALID`
    /// is set).
    #[inline]
    #[must_use]
    pub fn is_page_table(self) -> bool {
        self.0 & (PTE_READ | PTE_WRITE | PTE_EXEC | PTE_VALID) == PTE_VALID
    }

    /// Initializes the entry as a page directory entry pointing to the given
    /// physical address.
    #[inline]
    pub fn make_pde(&mut self, table_addr: u64) {
        // Only PTE_VALID is set, other bits are zero.
        const PDE_FLAGS: u64 = PTE_VALID;
        self.0 = 0;
        self.0 = ((table_addr / 4096) << 10) | PDE_FLAGS;
    }

    pub fn set_perm(&mut self, perm: ArchEntryPerm) {
        self.0 &= !FLAGS_MASK;
        self.0 |= perm.0;
    }
}

impl PageTableEntry for PageTableEntryImpl {
    fn is_valid(&self) -> bool {
        self.0 & PTE_VALID != 0
    }

    fn set_valid(&mut self, is_valid: bool) {
        if is_valid {
            self.0 |= PTE_VALID
        } else {
            self.0 &= !PTE_VALID
        }
    }

    fn is_writable(&self) -> bool {
        self.0 & PTE_WRITE != 0
    }

    fn set_writable(&mut self, writable: bool) {
        if writable {
            self.0 |= PTE_WRITE
        } else {
            self.0 &= !PTE_WRITE
        }
    }

    fn is_readable(&self) -> bool {
        self.0 & PTE_READ != 0
    }

    fn set_readable(&mut self, readable: bool) {
        if readable {
            self.0 |= PTE_READ
        } else {
            self.0 &= !PTE_READ
        }
    }

    fn is_user(&self) -> bool {
        self.0 & PTE_USER != 0
    }

    fn set_user(&mut self, user: bool) {
        if user {
            self.0 |= PTE_USER
        } else {
            self.0 &= !PTE_USER
        }
    }

    fn is_executable(&self) -> bool {
        self.0 & PTE_EXEC != 0
    }

    fn set_executable(&mut self, executable: bool) {
        if executable {
            self.0 |= PTE_EXEC
        } else {
            self.0 &= !PTE_EXEC
        }
    }

    fn clear(&mut self) {
        self.0 = 0;
    }

    fn physical_addr(&self) -> usize {
        (self.physical_page_num() * 4096) as usize
    }
}

/// A page table structure holding an array of page table entries.
#[repr(C)]
pub struct PageTableImpl(pub [PageTableEntryImpl; PGSIZE / 8]);

impl PageTableImpl {
    /// Extract the index from the virtual address for a given page table level.
    ///
    /// # Arguments
    ///
    /// * `level` - Page table level in the range of `[0, 2]`.
    /// * `va` - Virtual address.
    #[must_use]
    #[inline]
    const fn index_for_va(level: usize, va: usize) -> usize {
        const PX_MASK: usize = (1 << 9) - 1;
        // Virtual address
        // | EXT | L2 | L1 | L0 | Offset |
        //   24    9    9    9      12
        let shift = level * 9 + 12;
        (va >> shift) & PX_MASK
    }

    /// Computes the exclusive upper bound index for a virtual address at a
    /// given page table level.
    ///
    /// This function determines the number of entries in a page table level
    /// that the virtual address range covers.
    const fn upper_bound_index_for_va(level: usize, va: usize) -> usize {
        const PX_MASK: usize = (1 << 9) - 1;

        let shift = level * 9 + 12;
        let mut index = (va >> shift) & PX_MASK;

        // If any lower bits are set, the virtual address spills into the next index.
        let mask = (1 << shift) - 1;
        if va & mask != 0 {
            index += 1;
        }

        index
    }

    #[must_use]
    #[inline]
    const fn va_for_index(level: usize, idx: usize) -> usize {
        let shift = level * 9 + 12;
        idx << shift
    }

    /// Returns a mutable reference to the PTE at the given virtual address and
    /// level.
    #[must_use]
    #[inline]
    fn entry_mut_from_va(&mut self, level: usize, va: usize) -> &mut PageTableEntryImpl {
        &mut self.0[Self::index_for_va(level, va)]
    }

    /// Returns a reference to the PTE at the given virtual address and level.
    #[must_use]
    fn entry_from_va(&self, level: usize, va: usize) -> &PageTableEntryImpl {
        &self.0[Self::index_for_va(level, va)]
    }

    /// Walks the page table and returns the PTE at level 0 (final level).
    ///
    /// Allocates intermediate tables as needed.
    ///
    /// # Return
    ///
    /// `None` if allocating a intermediate table is failure.
    fn walk(&mut self, va: usize) -> Option<&mut PageTableEntryImpl> {
        let mut table = self;

        for level in (1..3).rev() {
            let pde = table.entry_mut_from_va(level, va);
            table = if pde.is_valid() {
                unsafe { &mut *pde.as_table() }
            } else if let Ok(new_table) = Self::try_alloc() {
                // attemtps to allocate a intermediate table.
                let raw = BuddyBox::into_raw(new_table);
                pde.make_pde(raw.addr() as u64);
                unsafe { &mut *raw }
            } else {
                // allocating a table is failure.
                return None;
            };
        }

        Some(table.entry_mut_from_va(0, va))
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_range_mut_recursive<F>(
        &mut self,
        level: usize,
        base_va: usize,
        start_va: usize,
        end_va: usize,
        is_first: bool,
        is_last: bool,
        f: &mut F,
    ) -> Result<(), (usize, crate::error::SysError)>
    where
        F: FnMut(usize, &mut PageTableEntryImpl) -> KResult<()>,
    {
        let start_idx = if is_first {
            Self::index_for_va(level, start_va)
        } else {
            0
        };

        let end_idx = if is_last {
            Self::upper_bound_index_for_va(level, end_va)
        } else {
            PGSIZE / 8
        };

        let iter = self.0[start_idx..end_idx]
            .iter_mut()
            .enumerate()
            .map(|(idx, pte)| {
                let base_va = base_va | Self::va_for_index(level, idx + start_idx);
                (idx + start_idx, base_va, pte)
            });

        if level == 0 {
            for (_, cur_va, pte) in iter {
                f(cur_va, pte).map_err(|err| (cur_va, err))?;
            }
            Ok(())
        } else {
            for (idx, base_va, pde) in iter {
                if !pde.is_valid() {
                    // Creates intermediate tables.
                    if let Ok(new_table) = Self::try_alloc() {
                        let ptr = BuddyBox::into_raw(new_table);
                        pde.make_pde(ptr.addr() as u64);
                    } else {
                        // The break address is used for rollback.
                        let brk_addr = if is_first { start_va } else { base_va };
                        return Err((brk_addr, SysErrorKind::OutOfMemory.into()));
                    }
                }

                let table = unsafe { &mut *pde.as_table() };
                table.walk_range_mut_recursive(
                    level - 1,
                    base_va,
                    start_va,
                    end_va,
                    is_first && idx == start_idx,
                    is_last && idx == end_idx - 1,
                    f,
                )?;
            }
            Ok(())
        }
    }

    fn clear_recursive(&mut self, level: usize) {
        fn clear_table(table: &mut [PageTableEntryImpl], level: usize) {
            for pte in table {
                if !pte.is_valid() {
                    continue;
                }
                let paddr = pte.physical_addr();
                let mut table =
                    unsafe { BuddyBox::from_raw_in(paddr as *mut PageTableImpl, BuddyAlloc {}) };
                table.clear_recursive(level - 1);
                pte.clear();
                drop(table);
            }
        }

        if level == 0 {
            return;
        }

        if level == 2 {
            // Without kernel memory
            let s = *KERNEL_VM_V2_INDEX_RANGE.start();
            let e = *KERNEL_VM_V2_INDEX_RANGE.end();
            clear_table(&mut self.0[..s], level);
            clear_table(&mut self.0[e + 1..], level);
        } else {
            clear_table(&mut self.0, level);
        }
    }
}

impl PageTable for PageTableImpl {
    fn try_alloc() -> KResult<BuddyBox<Self>> {
        unsafe { Ok(BuddyBox::<Self>::try_new_zeroed_in(BuddyAlloc {})?.assume_init()) }
    }

    fn map(&mut self, va: usize, pa: usize, perm: EntryPerm) -> KResult<&mut dyn PageTableEntry> {
        assert_eq!(va & !RISCV_VA_MASK, 0);
        let pte = self.walk(va);
        if let Some(pte) = pte {
            pte.set_physical_addr(pa as _);
            pte.apply_perm(perm | EntryPerm::VALID);
            Ok(pte)
        } else {
            Err(sys_err!(
                SysErrorKind::OutOfMemory,
                "could not allocate a PTE"
            ))
        }
    }

    fn try_map(
        &mut self,
        va: usize,
        pa: usize,
        perm: EntryPerm,
    ) -> KResult<Option<&mut dyn PageTableEntry>> {
        assert_eq!(va & !RISCV_VA_MASK, 0);
        let pte = self.walk(va);
        if let Some(pte) = pte {
            if pte.is_valid() {
                return Ok(None);
            }
            pte.set_physical_addr(pa as _);
            pte.apply_perm(perm | EntryPerm::VALID);
            Ok(Some(pte))
        } else {
            Err(sys_err!(
                SysErrorKind::OutOfMemory,
                "could not allocate a PTE"
            ))
        }
    }

    fn unmap(&mut self, va: usize) -> Option<usize> {
        assert_eq!(va & !RISCV_VA_MASK, 0);
        let entry = self.get_entry_mut(va);
        if let Some(entry) = entry {
            let paddr = entry.physical_addr();
            entry.clear();
            Some(paddr)
        } else {
            None
        }
    }

    fn vmap(
        &mut self,
        start_va: PageAlignedUsize,
        size: PageAlignedUsize,
        perm: EntryPerm,
    ) -> KResult<()> {
        let perm = ArchEntryPerm::from(perm | EntryPerm::VALID);
        let result = self.walk_range_mut_recursive(
            2,
            0,
            *start_va,
            *start_va + *size,
            true,
            true,
            &mut |cur_va, entry| {
                if entry.is_valid() {
                    return Err(sys_err!(
                        SysErrorKind::InvalidArgument,
                        "vmap error: the page at virtual address {:#x} is already mapped.",
                        cur_va
                    ));
                }

                if let Some(pa) = unsafe { buddy::alloc_zeroed_page() } {
                    entry.set_physical_addr(pa.as_ptr().addr() as u64);
                    entry.set_perm(perm);
                    Ok(())
                } else {
                    Err(SysErrorKind::OutOfMemory.into())
                }
            },
        );

        if let Err((brk_addr, err)) = result {
            self.vunmap(
                start_va,
                PageAlignedUsize::new(brk_addr).unwrap() - start_va,
            );
            Err(err)
        } else {
            Ok(())
        }
    }

    fn clear(&mut self) {
        self.clear_recursive(2);
    }

    fn get_entry(&self, va: usize) -> Option<&dyn PageTableEntry> {
        assert_eq!(va & !RISCV_VA_MASK, 0);
        let mut pte = self.entry_from_va(2, va);
        for level in (0..2).rev() {
            if !pte.is_valid() {
                return None;
            }
            assert!(pte.is_page_table());
            unsafe {
                let table = pte.as_table();
                pte = (*table).entry_from_va(level, va);
            }
        }
        pte.is_valid().then_some(pte)
    }

    fn get_entry_mut(&mut self, va: usize) -> Option<&mut dyn PageTableEntry> {
        assert_eq!(va & !RISCV_VA_MASK, 0);
        let mut pte = self.entry_mut_from_va(2, va);
        for level in (0..2).rev() {
            if !pte.is_valid() {
                return None;
            }
            unsafe {
                let table = &mut *pte.as_table();
                pte = table.entry_mut_from_va(level, va);
            }
        }
        pte.is_valid().then_some(pte)
    }

    fn range_of_entries_mut(
        &mut self,
        start_va: PageAlignedUsize,
        end_va: PageAlignedUsize,
    ) -> impl Iterator<Item = (usize, &mut dyn PageTableEntry)> {
        assert!(*end_va >= *start_va);
        RangeMut::new(self, *start_va, *end_va)
    }

    fn range_of_entries(
        &self,
        start_va: PageAlignedUsize,
        end_va: PageAlignedUsize,
    ) -> impl Iterator<Item = (usize, &dyn PageTableEntry)> {
        assert!(*end_va >= *start_va);
        Range::new(self, *start_va, *end_va)
    }

    #[inline(always)]
    fn copy_from_kernel(&mut self, kernel: &Self) {
        self.0[KERNEL_VM_V2_INDEX_RANGE].copy_from_slice(&kernel.0[KERNEL_VM_V2_INDEX_RANGE]);
    }
}

struct TableRangeIter<BorrowType: Default> {
    table: NonNull<PageTableImpl>,
    level: usize,

    /// The base virtual address can be used for computing the virtual address
    /// of each PTE.
    base_va: usize,

    cur_idx: usize,
    start_idx: usize,
    // exclusive.
    end_idx: usize,

    /// Is the first at level-2 table?
    is_first: bool,
    /// Is the last at level-2 table?
    is_last: bool,

    _marker: BorrowType,
}

impl<BorrowType: Default> TableRangeIter<BorrowType> {
    pub fn new(
        table: NonNull<PageTableImpl>,
        level: usize,
        base_va: usize,
        start_va: usize,
        end_va: usize,
        is_first: bool,
        is_last: bool,
    ) -> Self {
        let start_idx = if is_first {
            PageTableImpl::index_for_va(level, start_va)
        } else {
            0
        };

        let end_idx = if is_last {
            PageTableImpl::upper_bound_index_for_va(level, end_va)
        } else {
            PGSIZE / 8
        };

        Self {
            table,
            level,
            base_va,
            cur_idx: start_idx,
            start_idx,
            end_idx,
            is_first,
            is_last,
            _marker: BorrowType::default(),
        }
    }
}

impl<BorrowType: Default> Iterator for TableRangeIter<BorrowType> {
    type Item = (usize, usize, NonNull<PageTableEntryImpl>);

    fn next(&mut self) -> Option<Self::Item> {
        let table = unsafe { self.table.as_mut() };

        while self.cur_idx < self.end_idx {
            if table.0[self.cur_idx].is_valid() {
                let elem = (
                    self.cur_idx,
                    self.base_va | PageTableImpl::va_for_index(self.level, self.cur_idx),
                    unsafe { NonNull::new_unchecked(ptr::addr_of_mut!(table.0[self.cur_idx])) },
                );
                self.cur_idx += 1;
                return Some(elem);
            }
            self.cur_idx += 1;
        }

        None
    }
}

/// An iterator over all valid PTEs in a range of virtual memory.
struct InternalRangeIter<BorrowType: Default> {
    table_iters: [TableRangeIter<BorrowType>; 3],
    start_va: usize,
    end_va: usize,
}

impl<BorrowType: Default> InternalRangeIter<BorrowType> {
    pub fn new(pgtable: NonNull<PageTableImpl>, start_va: usize, end_va: usize) -> Self {
        let table_iters = core::array::from_fn(|level| {
            if level == 2 {
                TableRangeIter::new(pgtable, 2, 0, start_va, end_va, true, true)
            } else {
                // A dummy iterator to remove extra condition statements.
                TableRangeIter::new(pgtable, 2, 0, 0, 0, true, true)
            }
        });
        Self {
            table_iters,
            start_va,
            end_va,
        }
    }

    pub fn next_recursive(
        &mut self,
        level: usize,
    ) -> Option<(usize, usize, NonNull<PageTableEntryImpl>)> {
        let iter = &mut self.table_iters[level];
        if let Some(entry) = iter.next() {
            return Some(entry);
        }

        if level == 2 {
            return None;
        }

        if let Some((idx, base_va, pde)) = self.next_recursive(level + 1) {
            let up_iter = &self.table_iters[level + 1];
            let is_first = up_iter.is_first && up_iter.start_idx == idx;
            let is_last = up_iter.is_last && up_iter.end_idx - 1 == idx;
            let pgtable = unsafe { NonNull::new_unchecked(pde.as_ref().as_table()) };
            self.table_iters[level] = TableRangeIter::new(
                pgtable,
                level,
                base_va,
                self.start_va,
                self.end_va,
                is_first,
                is_last,
            );
            self.next_recursive(level)
        } else {
            None
        }
    }
}

impl<BorrowType: Default> Iterator for InternalRangeIter<BorrowType> {
    type Item = (usize, NonNull<PageTableEntryImpl>);

    fn next(&mut self) -> Option<Self::Item> {
        self.next_recursive(0).map(|(_, va, entry)| (va, entry))
    }
}

pub struct Range<'a> {
    iter: InternalRangeIter<PhantomData<&'a ()>>,
}

impl<'a> Range<'a> {
    fn new(pgtable: &PageTableImpl, start_va: usize, end_va: usize) -> Self {
        Self {
            iter: InternalRangeIter::new(
                unsafe { NonNull::new_unchecked(ptr::addr_of!(*pgtable) as *mut _) },
                start_va,
                end_va,
            ),
        }
    }
}

impl<'a> Iterator for Range<'a> {
    type Item = (usize, &'a dyn PageTableEntry);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|(va, entry)| (va, unsafe { entry.as_ref() as _ }))
    }
}

pub struct RangeMut<'a> {
    iter: InternalRangeIter<PhantomData<&'a mut ()>>,
}

impl<'a> RangeMut<'a> {
    fn new(pgtable: &mut PageTableImpl, start_va: usize, end_va: usize) -> Self {
        Self {
            iter: InternalRangeIter::new(
                unsafe { NonNull::new_unchecked(ptr::from_mut(pgtable)) },
                start_va,
                end_va,
            ),
        }
    }
}

impl<'a> Iterator for RangeMut<'a> {
    type Item = (usize, &'a mut dyn PageTableEntry);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter
            .next()
            .map(|(va, mut entry)| (va, unsafe { entry.as_mut() as _ }))
    }
}

#[cfg(test)]
pub mod tests {

    use crate::mmu::{
        buddy::BuddyAllocatorState,
        pgtable::{EntryPerm, PageTable},
        types::PageAlignedUsize,
        PGSIZE,
    };

    use super::PageTableImpl;

    #[test_case]
    pub fn test_map_unmap() {
        let mut table = PageTableImpl::try_alloc().unwrap();
        let vaddr = 0x3f887ac000;
        let paddr = 0x496fade000;

        table.map(vaddr, paddr, EntryPerm::with_rw()).unwrap();

        let entry = table.get_entry(vaddr).unwrap();
        assert!(entry.is_valid());
        assert!(entry.is_writable());
        assert!(entry.is_readable());

        assert_eq!(table.unmap(vaddr), Some(paddr));
        assert!(table.get_entry(vaddr).is_none());
        table.clear();
        drop(table);
    }

    #[test_case]
    pub fn test_clear() {
        let state = BuddyAllocatorState::current();
        let map_table = [
            (0x2_124f_e000_usize, 0x1000_e000_usize),
            (0x1_ffff_1000, 0x2000_2000),
            (0xf_ffff_a000, 0x3000_2000),
            (0x1_f000_4000, 0x2000),
            (0x1_f355_9000, 0x3000),
        ];

        let mut table = PageTableImpl::try_alloc().unwrap();

        for (vaddr, paddr) in map_table {
            table.map(vaddr, paddr, EntryPerm::with_rw()).unwrap();
        }

        for (vaddr, _) in map_table {
            let entry = table.get_entry(vaddr).unwrap();
            assert!(entry.is_valid());
            assert!(entry.is_writable());
            assert!(entry.is_readable());
        }

        table.clear();

        for (vaddr, _) in map_table {
            let entry = table.get_entry(vaddr);
            assert!(entry.is_none(), "{:#x}", vaddr);
        }

        drop(table);
        assert!(!BuddyAllocatorState::current().is_memory_leaky(&state));
    }

    #[test_case]
    pub fn test_iter_range() {
        let state = BuddyAllocatorState::current();

        let mut table = PageTableImpl::try_alloc().unwrap();

        let test_addresses = [
            (0x2_4753_3000_usize, 0x10_0000_usize),
            (0x2_0000_0000, 0x1_0000),
            (0x1_ffff_0000, 0x4_0000),
            (0x1_ffff_e000, 0xf000),
            (0x2_fbef_0000, 0x13_0000),
            (0x1_0000_0000, 0),
        ];

        for (va, size) in test_addresses {
            let va = PageAlignedUsize::new(va).unwrap();
            let size = PageAlignedUsize::new(size).unwrap();

            table.map_range(va, va, size, EntryPerm::with_rw()).unwrap();

            let mut iter = table.range_of_entries(va, va + size);
            for expected_va in (*va..*va + *size).step_by(PGSIZE) {
                let (va, entry) = iter.next().unwrap();
                assert_eq!(va, expected_va);
                assert_eq!(entry.physical_addr(), va);
            }
            drop(iter);

            let mut iter = table.range_of_entries_mut(va, va + size);
            for expected_va in (*va..*va + *size).step_by(PGSIZE) {
                let (va, entry) = iter.next().unwrap();
                assert_eq!(va, expected_va);
                assert_eq!(entry.physical_addr(), va);
            }
            drop(iter);
        }

        table.clear();
        drop(table);

        assert!(!BuddyAllocatorState::current().is_memory_leaky(&state));
    }

    #[test_case]
    pub fn test_iter_range_empty() {
        let state = BuddyAllocatorState::current();

        let va = PageAlignedUsize::new(0x0).unwrap();
        let size = PageAlignedUsize::new(0x0).unwrap();

        let mut table = PageTableImpl::try_alloc().unwrap();
        table.map_range(va, va, size, EntryPerm::with_rw()).unwrap();

        let iter = table.range_of_entries(va, va + size);
        assert_eq!(iter.count(), 0);

        let iter = table.range_of_entries_mut(va, va + size);
        assert_eq!(iter.count(), 0);

        table.clear();
        drop(table);

        assert!(!BuddyAllocatorState::current().is_memory_leaky(&state));
    }

    #[test_case]
    pub fn test_vmap_vunmap() {
        let state = BuddyAllocatorState::current();

        let mut table = PageTableImpl::try_alloc().unwrap();

        let test_addresses = [
            (0x2_4753_3000_usize, 0x10_0000_usize),
            (0x2_0000_0000, 0x1_0000),
            (0x1_ffff_0000, 0x4_0000),
            (0x1_ffff_e000, 0xf000),
            (0x2_fbef_0000, 0x13_0000),
            (0x1_0000_0000, 0),
        ];

        for (va, size) in test_addresses {
            let va = PageAlignedUsize::new(va).unwrap();
            let size = PageAlignedUsize::new(size).unwrap();

            table.vmap(va, size, EntryPerm::with_text()).unwrap();

            for va in (*va..*va + *size).step_by(PGSIZE) {
                let entry = table.get_entry(va.try_into().unwrap()).unwrap();
                assert_eq!(entry.perm(), EntryPerm::with_text() | EntryPerm::VALID);
            }

            table.vunmap(va, size);

            for va in (*va..*va + *size).step_by(PGSIZE) {
                let entry = table.get_entry(va.try_into().unwrap());
                assert!(entry.is_none());
            }
        }

        table.clear();
        drop(table);

        assert!(!BuddyAllocatorState::current().is_memory_leaky(&state));
    }
}
