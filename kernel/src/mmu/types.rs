use super::is_valid_phy_addr;
use crate::my_intrusive_adapter;
use core::{
    num::NonZero,
    ops::{self, Deref, DerefMut},
};
use intrusive_collections::{LinkedListAtomicLink, SinglyLinkedListLink};

my_intrusive_adapter!(pub PageLinkAdapter = PhysicalPtr<Page>: Page { link: LinkedListAtomicLink });
my_intrusive_adapter!(pub SlabFreeObjLinkAdapter = PhysicalPtr<SlabFreeObj>: SlabFreeObj { link: SinglyLinkedListLink });

#[repr(C)]
pub struct SlabFreeObj {
    pub(super) link: SinglyLinkedListLink,
}

pub struct Page {
    pub(super) link: LinkedListAtomicLink,

    pub pgn: usize,

    /**
     * ```
     * | is_free: 1 bit | order: 7 bits |
     * ```
     */
    pub(super) buddy_flags: u8,

    // For slab allocator.
    pub(super) slab_obj_sz: u32,
    pub(super) slab_free_list: intrusive_collections::SinglyLinkedList<SlabFreeObjLinkAdapter>,
    pub(super) slab_nr_free: u32,
    pub(super) slab_num_objs: u32,
}

impl Page {
    #[must_use]
    #[inline]
    pub fn new(pgn: usize) -> Self {
        Self {
            link: LinkedListAtomicLink::new(),
            pgn,
            buddy_flags: 0,
            slab_obj_sz: 0,
            slab_free_list: intrusive_collections::SinglyLinkedList::new(
                SlabFreeObjLinkAdapter::new(),
            ),
            slab_nr_free: 0,
            slab_num_objs: 0,
        }
    }

    #[must_use]
    #[inline]
    pub const fn paddr(&self) -> usize {
        self.pgn << 12
    }

    #[must_use]
    #[inline]
    pub const fn is_free(&self) -> bool {
        self.buddy_flags & (1 << 7) != 0
    }

    #[inline]
    pub const fn set_free(&mut self) {
        self.buddy_flags |= 1 << 7;
    }

    #[inline]
    pub const fn clear_free(&mut self) {
        self.buddy_flags &= !(1 << 7);
    }

    #[must_use]
    #[inline]
    pub const fn pgf_order(&self) -> u8 {
        self.buddy_flags & !(1 << 7)
    }

    #[inline]
    pub const fn set_pgf_order(&mut self, pgf_order: u8) {
        self.buddy_flags &= 1 << 7;
        self.buddy_flags |= pgf_order;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageAligned<T>(pub T);

macro_rules! impl_pgaligned_for {
    ($typ:ty) => {
        impl PageAligned<$typ> {
            #[must_use]
            #[inline]
            pub const fn new(val: $typ) -> Option<Self> {
                if val % $crate::mmu::PGSIZE as $typ == 0 {
                    Some(Self(val))
                } else {
                    None
                }
            }

            #[must_use]
            #[inline]
            pub const fn new_const(val: $typ) -> Self {
                assert!(val % $crate::mmu::PGSIZE as $typ == 0);
                Self(val)
            }

            #[must_use]
            #[inline]
            pub const fn round_up(val: $typ) -> Self {
                Self((val + $crate::mmu::PGSIZE as $typ - 1) & !($crate::mmu::PGSIZE as $typ - 1))
            }

            #[must_use]
            #[inline]
            pub const fn round_down(val: $typ) -> Self {
                Self(val & !($crate::mmu::PGSIZE as $typ - 1))
            }
        }

        impl TryFrom<$typ> for PageAligned<$typ> {
            type Error = ();

            fn try_from(value: $typ) -> Result<Self, Self::Error> {
                Self::new(value).ok_or(())
            }
        }
    };
}

impl_pgaligned_for!(usize);
impl_pgaligned_for!(u64);
impl_pgaligned_for!(u32);
impl_pgaligned_for!(u16);

macro_rules! pgaligned_alias {
    ($typ:tt, $name:ident) => {
        pub type $name = PageAligned<$typ>;
    };
}

pgaligned_alias!(usize, PageAlignedUsize);
pgaligned_alias!(u64, PageAlignedU64);
pgaligned_alias!(u32, PageAlignedU32);
pgaligned_alias!(u16, PageAlignedU16);

impl<T> PageAligned<T> {
    /// # Safety
    ///
    /// Caller must sure that the val is page aligned.
    #[must_use]
    #[inline]
    pub unsafe fn new_unchecked(val: T) -> Self {
        Self(val)
    }

    #[must_use]
    #[inline]
    pub fn get(self) -> T {
        self.0
    }
}

impl<T> Deref for PageAligned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for PageAligned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: ops::Add<T, Output = T>> ops::Add<PageAligned<T>> for PageAligned<T> {
    type Output = PageAligned<T>;

    #[inline]
    fn add(self, rhs: PageAligned<T>) -> Self::Output {
        unsafe { PageAligned::new_unchecked(self.0 + rhs.0) }
    }
}

impl<T: ops::Sub<T, Output = T>> ops::Sub<PageAligned<T>> for PageAligned<T> {
    type Output = PageAligned<T>;

    #[inline]
    fn sub(self, rhs: PageAligned<T>) -> Self::Output {
        unsafe { PageAligned::new_unchecked(self.0 - rhs.0) }
    }
}

/// This represents a valid pointer to a memory in a regoin that is directly
/// mapped.
#[derive(Debug)]
#[repr(transparent)]
pub struct PhysicalPtr<T: ?Sized> {
    ptr: *mut T,
}

unsafe impl<T: ?Sized> Send for PhysicalPtr<T> {}

impl<T: ?Sized> PhysicalPtr<T> {
    #[must_use]
    #[inline]
    pub fn new(ptr: *mut T) -> Option<Self> {
        if !is_valid_phy_addr(ptr.addr()) {
            return None;
        }
        Some(Self { ptr })
    }

    /// Creates a physical pointer without checking whether the pointer is a
    /// valid physical address.
    ///
    /// # Safety
    ///
    /// The pointer must be a valid physical address.
    #[must_use]
    #[inline]
    pub unsafe fn new_unchecked(ptr: *mut T) -> Self {
        Self { ptr }
    }

    #[must_use]
    #[inline]
    pub fn addr(&self) -> NonZero<usize> {
        unsafe { NonZero::new_unchecked(self.ptr.addr()) }
    }

    #[must_use]
    #[inline]
    pub const fn as_ref(&self) -> &T {
        unsafe { &*self.ptr }
    }

    #[must_use]
    #[inline]
    pub const fn as_mut(&mut self) -> &mut T {
        unsafe { &mut *self.ptr }
    }

    #[must_use]
    #[inline]
    pub const fn as_ptr(&self) -> *mut T {
        self.ptr
    }
}

impl<T: ?Sized> Clone for PhysicalPtr<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: ?Sized> Copy for PhysicalPtr<T> {}

impl<T: ?Sized> Deref for PhysicalPtr<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T: ?Sized> DerefMut for PhysicalPtr<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}
