use core::{fmt, ptr::NonNull};

use bitflags::bitflags;
use syserr::sys_err;

use crate::{
    arch,
    error::{KResult, SysErrorKind},
};

use super::{
    buddy::{self, BuddyBox},
    types::{PageAlignedUsize, PhysicalPtr},
    PGSIZE,
};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct EntryPerm: u32 {
        const VALID = 1 << 0;
        const WRITABLE = 1 << 1;
        const READABLE = 1 << 2;
        const USER = 1 << 3;
        const EXECUTABLE = 1 << 4;
    }
}

impl EntryPerm {
    #[must_use]
    pub const fn with_rw() -> Self {
        Self::from_bits_retain(Self::READABLE.bits() | Self::WRITABLE.bits())
    }

    #[must_use]
    pub const fn with_text() -> Self {
        Self::from_bits_retain(Self::READABLE.bits() | Self::EXECUTABLE.bits())
    }

    #[inline]
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.contains(Self::VALID)
    }

    #[inline]
    #[must_use]
    pub const fn is_writable(self) -> bool {
        self.contains(Self::WRITABLE)
    }

    #[inline]
    #[must_use]
    pub const fn is_readable(self) -> bool {
        self.contains(Self::READABLE)
    }

    #[inline]
    #[must_use]
    pub const fn is_user(self) -> bool {
        self.contains(Self::USER)
    }

    #[inline]
    #[must_use]
    pub const fn is_executable(self) -> bool {
        self.contains(Self::EXECUTABLE)
    }
}

impl fmt::Display for EntryPerm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn write_flag(f: &mut fmt::Formatter<'_>, flag: bool, s: &str) -> fmt::Result {
            if flag {
                write!(f, "{}", s)
            } else {
                write!(f, "-")
            }
        }
        write_flag(f, self.is_user(), "U")?;
        write_flag(f, self.is_executable(), "X")?;
        write_flag(f, self.is_readable(), "R")?;
        write_flag(f, self.is_writable(), "W")?;
        write_flag(f, self.is_valid(), "V")
    }
}

pub trait PageTableEntry {
    fn is_valid(&self) -> bool;
    fn set_valid(&mut self, is_valid: bool);
    fn is_writable(&self) -> bool;
    fn set_writable(&mut self, writable: bool);
    fn is_readable(&self) -> bool;
    fn set_readable(&mut self, readable: bool);
    fn is_user(&self) -> bool;
    fn set_user(&mut self, user: bool);
    fn is_executable(&self) -> bool;
    fn set_executable(&mut self, executable: bool);
    fn physical_addr(&self) -> usize;
    fn clear(&mut self);

    fn apply_perm(&mut self, perm: EntryPerm) {
        self.set_valid(perm.is_valid());
        self.set_writable(perm.is_writable());
        self.set_readable(perm.is_readable());
        self.set_user(perm.is_user());
        self.set_executable(perm.is_executable());
    }

    fn perm(&self) -> EntryPerm {
        let mut perm = EntryPerm::empty();
        if self.is_valid() {
            perm |= EntryPerm::VALID;
        }
        if self.is_writable() {
            perm |= EntryPerm::WRITABLE;
        }
        if self.is_readable() {
            perm |= EntryPerm::READABLE;
        }
        if self.is_user() {
            perm |= EntryPerm::USER;
        }
        if self.is_executable() {
            perm |= EntryPerm::EXECUTABLE;
        }
        perm
    }
}

pub trait PageTable: Sized {
    fn alloc() -> BuddyBox<Self> {
        Self::try_alloc().unwrap()
    }

    fn try_alloc() -> KResult<BuddyBox<Self>>;

    /// Maps a virtual address to a physical address with the PTE permission
    /// `perm`.
    fn map(&mut self, va: usize, pa: usize, perm: EntryPerm) -> KResult<&mut dyn PageTableEntry>;

    /// Attempts to map a virtual address to a physical_addr with the PTE
    /// permission.
    ///
    /// # Return
    ///
    /// [`None`] if the virtual address has already mapped.
    /// [`Some`] the PTE according to the virtual address.
    fn try_map(
        &mut self,
        va: usize,
        pa: usize,
        perm: EntryPerm,
    ) -> KResult<Option<&mut dyn PageTableEntry>>;

    /// Unmaps a virtual address. Return the previously mapped physical address
    /// if the mapping exists.
    fn unmap(&mut self, va: usize) -> Option<usize>;

    fn map_range(
        &mut self,
        va: PageAlignedUsize,
        pa: PageAlignedUsize,
        size: PageAlignedUsize,
        perm: EntryPerm,
    ) -> KResult<()> {
        let start_va = *va;

        let mut va = *va;
        let mut pa = *pa;
        let cnt = *size / PGSIZE;
        for i in 0..cnt {
            if let Err(e) = self.map(va, pa, perm) {
                // rollback.
                self.unmap_range(
                    PageAlignedUsize::new(start_va).unwrap(),
                    PageAlignedUsize::new(PGSIZE * i).unwrap(),
                );
                return Err(e);
            }
            va += PGSIZE;
            pa += PGSIZE;
        }

        Ok(())
    }

    fn unmap_range(&mut self, va: PageAlignedUsize, size: PageAlignedUsize) {
        for (_, pte) in self.range_of_entries_mut(va, va + size) {
            pte.clear();
        }
    }

    /// Maps the given virtual address `va` to a new physical page which is
    /// allocated by `buddy`.
    ///
    /// Returns a pointer to the physical page.
    fn vmap_page(&mut self, va: PageAlignedUsize, perm: EntryPerm) -> KResult<PhysicalPtr<u8>> {
        if !arch::memlayout::is_vmap_area(*va) {
            panic!("va is not from vmap area: {:#x}", *va);
        }
        if let Some(pa) = unsafe { buddy::alloc_zeroed_page() } {
            let pte = self.try_map(*va, pa.addr().get(), perm);
            match pte {
                Ok(Some(_)) => Ok(unsafe { PhysicalPtr::new_unchecked(pa.as_ptr() as *mut u8) }),
                Ok(None) => {
                    unsafe { buddy::free_page(pa.cast()) };
                    Err(sys_err!(
                        SysErrorKind::InvalidArgument,
                        "vmap page error: the page at virtual address {:#x} is already mapped.",
                        *va
                    ))
                }
                Err(err) => {
                    unsafe { buddy::free_page(pa.cast()) };
                    Err(err)
                }
            }
        } else {
            Err(sys_err!(
                SysErrorKind::OutOfMemory,
                "could not allocate a physical page for vmap_page.",
            ))
        }
    }

    /// Maps a range of virtual memory by allocating physical pages.
    fn vmap(
        &mut self,
        start_va: PageAlignedUsize,
        size: PageAlignedUsize,
        perm: EntryPerm,
    ) -> KResult<()> {
        for va in (*start_va..*start_va + *size).step_by(PGSIZE) {
            let va = unsafe { PageAlignedUsize::new_unchecked(va) };
            if let Err(e) = self.vmap_page(va, perm) {
                // rollback.
                self.vunmap(start_va, va - start_va);
                return Err(e);
            }
        }
        Ok(())
    }

    /// Unmaps a range of virtual pages and frees their associated physical
    /// pages if exist.
    fn vunmap(&mut self, va: PageAlignedUsize, size: PageAlignedUsize) {
        for (_, pte) in self.range_of_entries_mut(va, va + size) {
            // for va in (*va..*va + *size).step_by(PGSIZE) {
            /*let Some(pte) = self.get_entry_mut(va) else {
                continue;
            };*/
            let paddr = pte.physical_addr();
            pte.clear();
            unsafe { buddy::free_page(NonNull::new_unchecked(paddr as *mut u8)) };
        }
    }

    /// Returns a reference to the PTE associated with the virtual address `va`
    /// or `None` if the PTE is not present.
    fn get_entry(&self, va: usize) -> Option<&dyn PageTableEntry>;

    /// Returns a mutable reference to the PTE associated with the virtual
    /// address `va` or `None` if the PTE is not present.
    fn get_entry_mut(&mut self, va: usize) -> Option<&mut dyn PageTableEntry>;

    fn range_of_entries_mut(
        &mut self,
        start_va: PageAlignedUsize,
        end_va: PageAlignedUsize,
    ) -> impl Iterator<Item = (usize, &mut dyn PageTableEntry)>;

    fn range_of_entries(
        &self,
        start_va: PageAlignedUsize,
        end_va: PageAlignedUsize,
    ) -> impl Iterator<Item = (usize, &dyn PageTableEntry)>;

    fn copy_from_kernel(&mut self, kernel: &Self);

    fn clear(&mut self);
}
